//! File watcher for index hot-reloading
//!
//! This module provides functionality to watch the index file for changes
//! and automatically reload it without restarting the MCP server.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use super::{
    CodeIntelligenceServer,
    notifications::{FileChangeEvent, NotificationBroadcaster},
};
use crate::{IndexPersistence, Settings, SimpleIndexer};

/// Watches the index file and reloads it when changes are detected
pub struct IndexWatcher {
    index_path: PathBuf,
    indexer: Arc<RwLock<SimpleIndexer>>,
    settings: Arc<Settings>,
    persistence: IndexPersistence,
    last_modified: Option<SystemTime>,
    /// Last modification time of document store state.json
    last_doc_modified: Option<SystemTime>,
    check_interval: Duration,
    mcp_server: Option<Arc<CodeIntelligenceServer>>,
    broadcaster: Option<Arc<NotificationBroadcaster>>,
}

impl IndexWatcher {
    /// Create a new index watcher
    pub fn new(
        indexer: Arc<RwLock<SimpleIndexer>>,
        settings: Arc<Settings>,
        check_interval: Duration,
    ) -> Self {
        let index_path = settings.index_path.clone();
        let persistence = IndexPersistence::new(index_path.clone());

        // Get initial modification time of the actual index metadata file
        let meta_file_path = index_path.join("tantivy").join("meta.json");
        let last_modified = std::fs::metadata(&meta_file_path)
            .ok()
            .and_then(|meta| meta.modified().ok());

        // Get initial modification time of document store state.json
        let doc_state_path = index_path.join("documents").join("state.json");
        let last_doc_modified = std::fs::metadata(&doc_state_path)
            .ok()
            .and_then(|meta| meta.modified().ok());

        Self {
            index_path,
            indexer,
            settings,
            persistence,
            last_modified,
            last_doc_modified,
            check_interval,
            mcp_server: None,
            broadcaster: None,
        }
    }

    /// Set the MCP server to send notifications
    pub fn with_mcp_server(mut self, server: Arc<CodeIntelligenceServer>) -> Self {
        self.mcp_server = Some(server);
        self
    }

    /// Set the notification broadcaster
    pub fn with_broadcaster(mut self, broadcaster: Arc<NotificationBroadcaster>) -> Self {
        self.broadcaster = Some(broadcaster);
        self
    }

    /// Start watching for index changes
    pub async fn watch(mut self) {
        let mut ticker = interval(self.check_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // Started message is logged by the server, just begin polling

        loop {
            ticker.tick().await;

            if let Err(e) = self.check_and_reload().await {
                error!("Error checking/reloading index: {}", e);
            }
        }
    }

    /// Check if the index file has been modified and reload if necessary
    /// Also checks for source file changes when file_watch is enabled
    async fn check_and_reload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // First, check for source file changes if file watching is enabled
        if self.settings.file_watch.enabled {
            self.check_and_reindex_source_files().await?;
        }

        // Check for document store changes (state.json modified externally)
        self.check_document_changes();

        // Check if file exists
        if !self.persistence.exists() {
            debug!("Index file does not exist at {:?}", self.index_path);
            return Ok(());
        }

        // Get current modification time of the actual index metadata file
        let meta_file_path = self.index_path.join("tantivy").join("meta.json");
        let metadata = std::fs::metadata(&meta_file_path)?;
        let current_modified = metadata.modified()?;

        // Check if file has been modified
        let should_reload = match self.last_modified {
            Some(last) => current_modified > last,
            None => true, // First check after file creation
        };

        if !should_reload {
            debug!("Index file unchanged");
            return Ok(());
        }

        crate::log_event!(
            "index-watcher",
            "reloading",
            "{}",
            self.index_path.display()
        );

        // Load the new index
        match self.persistence.load_with_settings(self.settings.clone()) {
            Ok(new_indexer) => {
                // Get write lock and replace the indexer
                let mut indexer_guard = self.indexer.write().await;
                *indexer_guard = new_indexer;

                // Update last modified time
                self.last_modified = Some(current_modified);

                // Ensure semantic search stays attached after hot reloads
                let mut restored_semantic = false;
                if !indexer_guard.has_semantic_search() {
                    let semantic_path = self.index_path.join("semantic");
                    let metadata_exists = semantic_path.join("metadata.json").exists();
                    if metadata_exists {
                        match indexer_guard.load_semantic_search(&semantic_path) {
                            Ok(true) => {
                                restored_semantic = true;
                            }
                            Ok(false) => {
                                crate::debug_event!(
                                    "index-watcher",
                                    "semantic metadata present but reload returned false"
                                );
                            }
                            Err(e) => {
                                warn!(
                                    "Warning: Failed to reload semantic search after index update: {}",
                                    e
                                );
                            }
                        }
                    } else {
                        crate::debug_event!(
                            "index-watcher",
                            "semantic metadata missing",
                            "{}",
                            semantic_path.display()
                        );
                    }
                }

                let symbol_count = indexer_guard.symbol_count();
                let has_semantic = indexer_guard.has_semantic_search();
                if restored_semantic {
                    match indexer_guard.semantic_search_embedding_count() {
                        Ok(count) => {
                            crate::debug_event!(
                                "index-watcher",
                                "restored semantic",
                                "{count} embeddings"
                            );
                        }
                        Err(e) => {
                            crate::debug_event!(
                                "index-watcher",
                                "restored semantic",
                                "failed to count: {e}"
                            );
                        }
                    }
                }
                crate::log_event!("index-watcher", "reloaded", "{symbol_count} symbols");
                crate::debug_event!("index-watcher", "semantic search", "{has_semantic}");

                // Send notification that index was reloaded
                if let Some(ref broadcaster) = self.broadcaster {
                    broadcaster.send(FileChangeEvent::IndexReloaded);
                    crate::debug_event!("index-watcher", "broadcast", "IndexReloaded");
                }

                Ok(())
            }
            Err(e) => {
                warn!("Failed to reload index: {}", e);
                Err(Box::new(std::io::Error::other(format!(
                    "Failed to reload index: {e}"
                ))))
            }
        }
    }

    /// Check if document store state.json has changed (documents indexed externally)
    fn check_document_changes(&mut self) {
        let doc_state_path = self.index_path.join("documents").join("state.json");

        // Get current modification time
        let current_modified = match std::fs::metadata(&doc_state_path) {
            Ok(meta) => match meta.modified() {
                Ok(time) => time,
                Err(_) => return,
            },
            Err(_) => return, // File doesn't exist
        };

        // Check if changed
        let changed = match self.last_doc_modified {
            Some(last) => current_modified > last,
            None => true, // First check
        };

        if changed {
            self.last_doc_modified = Some(current_modified);
            info!("Document store changed, notifying watchers");

            // Send IndexReloaded to refresh document handler's watched files
            if let Some(ref broadcaster) = self.broadcaster {
                broadcaster.send(FileChangeEvent::IndexReloaded);
            }
        }
    }

    /// Check source files for changes and re-index if needed
    async fn check_and_reindex_source_files(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Get list of indexed files
        let indexed_paths = {
            let indexer = self.indexer.read().await;
            indexer.get_all_indexed_paths()
        };

        if indexed_paths.is_empty() {
            return Ok(());
        }

        // Check each file's modification time
        let index_modified_time = {
            let meta_file_path = self.index_path.join("tantivy").join("meta.json");
            std::fs::metadata(&meta_file_path)
                .ok()
                .and_then(|m| m.modified().ok())
        };

        let mut files_to_reindex = Vec::new();

        for path in &indexed_paths {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(file_modified) = metadata.modified() {
                    // Check if file was modified after the index
                    if let Some(index_time) = index_modified_time {
                        if file_modified > index_time {
                            files_to_reindex.push(path.clone());
                        }
                    }
                }
            }
        }

        // Re-index changed files
        if !files_to_reindex.is_empty() {
            info!(
                "Found {} modified source files, re-indexing...",
                files_to_reindex.len()
            );

            let mut indexer = self.indexer.write().await;
            let mut reindexed_count = 0;

            for path in files_to_reindex {
                debug!("Re-indexing: {:?}", path);
                match indexer.index_file(&path) {
                    Ok(result) => {
                        use crate::IndexingResult;
                        match result {
                            IndexingResult::Indexed(_) => {
                                reindexed_count += 1;
                                debug!("  ✓ Re-indexed successfully");

                                // Send notification if MCP server is available
                                if let Some(ref server) = self.mcp_server {
                                    let path_str = path.display().to_string();
                                    let server_clone = server.clone();
                                    tokio::spawn(async move {
                                        server_clone.notify_file_reindexed(&path_str).await;
                                    });
                                }
                            }
                            IndexingResult::Cached(_) => {
                                debug!("  - File unchanged (hash match)");
                            }
                        }
                    }
                    Err(e) => {
                        warn!("  ✗ Failed to re-index {:?}: {}", path, e);
                    }
                }
            }

            if reindexed_count > 0 {
                info!("Re-indexed {} files successfully", reindexed_count);

                // Persist the updated index
                let persistence = IndexPersistence::new(self.index_path.clone());
                if let Err(e) = persistence.save(&indexer) {
                    error!("Failed to persist index after re-indexing: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get current index statistics
    pub async fn get_stats(&self) -> IndexStats {
        let indexer = self.indexer.read().await;
        IndexStats {
            symbol_count: indexer.symbol_count(),
            last_modified: self.last_modified,
            index_path: self.index_path.clone(),
        }
    }
}

/// Statistics about the watched index
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub symbol_count: usize,
    pub last_modified: Option<SystemTime>,
    pub index_path: PathBuf,
}
