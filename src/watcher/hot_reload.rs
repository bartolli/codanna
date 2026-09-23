//! Hot-reload watcher for external index changes.
//!
//! Polls for changes to the index made by external processes (CI/CD, other terminals)
//! and hot-reloads them without restarting the server.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::indexing::facade::IndexFacade;
use crate::mcp::notifications::{FileChangeEvent, NotificationBroadcaster};
use crate::{IndexPersistence, Settings};

/// Watches for external index changes and hot-reloads them.
///
/// This watcher polls `meta.json` and `state.json` to detect when the index
/// is modified by external processes (e.g., `codanna index` in another terminal,
/// CI/CD pipelines). It does NOT watch source files - that's handled by UnifiedWatcher.
pub struct HotReloadWatcher {
    index_path: PathBuf,
    facade: Arc<RwLock<IndexFacade>>,
    settings: Arc<Settings>,
    persistence: IndexPersistence,
    last_modified: Option<SystemTime>,
    last_doc_modified: Option<SystemTime>,
    check_interval: Duration,
    broadcaster: Option<Arc<NotificationBroadcaster>>,
}

impl HotReloadWatcher {
    /// Create a new hot-reload watcher.
    pub fn new(
        facade: Arc<RwLock<IndexFacade>>,
        settings: Arc<Settings>,
        check_interval: Duration,
    ) -> Self {
        let index_path = settings.index_path.clone();
        let persistence = IndexPersistence::new(index_path.clone());

        // Get initial modification time of the index metadata file
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
            facade,
            settings,
            persistence,
            last_modified,
            last_doc_modified,
            check_interval,
            broadcaster: None,
        }
    }

    /// Set the notification broadcaster.
    pub fn with_broadcaster(mut self, broadcaster: Arc<NotificationBroadcaster>) -> Self {
        self.broadcaster = Some(broadcaster);
        self
    }

    /// Start watching for external index changes.
    pub async fn watch(mut self) {
        let mut ticker = interval(self.check_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            if let Err(e) = self.check_and_reload().await {
                tracing::error!("Error checking/reloading index: {e}");
            }
        }
    }

    /// Check if the index has been modified externally and reload if necessary.
    async fn check_and_reload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Check for document store changes (state.json modified externally)
        self.check_document_changes();

        // Check if index file exists
        if !self.persistence.exists() {
            debug!("Index file does not exist at {:?}", self.index_path);
            return Ok(());
        }

        // Get current modification time of the index metadata file
        let meta_file_path = self.index_path.join("tantivy").join("meta.json");
        let metadata = std::fs::metadata(&meta_file_path)?;
        let current_modified = metadata.modified()?;

        // Check if file has been modified
        let should_reload = match self.last_modified {
            Some(last) => current_modified > last,
            None => true,
        };

        if !should_reload {
            tracing::trace!("Index file unchanged");
            return Ok(());
        }

        crate::log_event!(
            "hot-reload",
            "reloading",
            "{}",
            crate::parsing::paths::render_absolute_path(&self.index_path).display()
        );

        // Lock before loading: a commit through the active facade between
        // the load and the swap would be missing from the replacement, whose
        // stale id counters then reissue ids already in the index.
        let mut facade_guard = self.facade.write().await;
        match self.persistence.load_facade(self.settings.clone()) {
            Ok(new_facade) => {
                *facade_guard = new_facade;

                // Update last modified time
                self.last_modified = Some(current_modified);

                // Ensure semantic search stays attached after hot reloads
                let mut restored_semantic = false;
                if !facade_guard.has_semantic_search() && !facade_guard.is_semantic_incompatible() {
                    let semantic_path = self.index_path.join("semantic");
                    let metadata_exists = semantic_path.join("metadata.json").exists();
                    if metadata_exists {
                        match facade_guard.load_semantic_search(&semantic_path) {
                            Ok(true) => {
                                restored_semantic = true;
                            }
                            Ok(false) => {
                                crate::debug_event!(
                                    "hot-reload",
                                    "semantic metadata present but reload returned false"
                                );
                            }
                            Err(crate::IndexError::SemanticSearch(
                                crate::semantic::SemanticSearchError::DimensionMismatch {
                                    ref suggestion,
                                    ..
                                },
                            )) => {
                                warn!(
                                    "Semantic index dimension mismatch after hot-reload: {suggestion}. \
                                     Semantic search disabled until re-indexed with --force."
                                );
                            }
                            Err(e) => {
                                warn!("Failed to reload semantic search after index update: {e}");
                            }
                        }
                    } else {
                        crate::debug_event!(
                            "hot-reload",
                            "semantic metadata missing",
                            "{}",
                            crate::parsing::paths::render_absolute_path(&semantic_path).display()
                        );
                    }
                }

                let symbol_count = facade_guard.symbol_count();
                let has_semantic = facade_guard.has_semantic_search();
                if restored_semantic {
                    let count = facade_guard.semantic_search_embedding_count();
                    crate::debug_event!("hot-reload", "restored semantic", "{count} embeddings");
                }
                crate::log_event!("hot-reload", "reloaded", "{symbol_count} symbols");
                crate::debug_event!("hot-reload", "semantic search", "{has_semantic}");

                // Send notification that index was reloaded
                if let Some(ref broadcaster) = self.broadcaster {
                    broadcaster.send(FileChangeEvent::IndexReloaded);
                    crate::debug_event!("hot-reload", "broadcast", "IndexReloaded");
                }

                Ok(())
            }
            Err(e) => {
                warn!("Failed to reload index: {e}");
                Err(Box::new(std::io::Error::other(format!(
                    "Failed to reload index: {e}"
                ))))
            }
        }
    }

    /// Check if document store state.json has changed (documents indexed externally).
    fn check_document_changes(&mut self) {
        let doc_state_path = self.index_path.join("documents").join("state.json");

        // Get current modification time
        let current_modified = match std::fs::metadata(&doc_state_path) {
            Ok(meta) => match meta.modified() {
                Ok(time) => time,
                Err(_) => return,
            },
            Err(_) => return,
        };

        // Check if changed
        let changed = match self.last_doc_modified {
            Some(last) => current_modified > last,
            None => true,
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

    /// Get current index statistics.
    pub async fn get_stats(&self) -> IndexStats {
        let indexer = self.facade.read().await;
        IndexStats {
            symbol_count: indexer.symbol_count(),
            last_modified: self.last_modified,
            index_path: self.index_path.clone(),
        }
    }
}

/// Statistics about the watched index.
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub symbol_count: usize,
    pub last_modified: Option<SystemTime>,
    pub index_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn write_pair(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(format!("{name}.py"));
        std::fs::write(
            &path,
            format!("def {name}_helper():\n    return 1\n\n\ndef {name}():\n    return {name}_helper()\n"),
        )
        .unwrap();
        path
    }

    // A same-process commit that lands while the reload waits for the
    // facade must be in the replacement. A replacement loaded before that
    // commit carries stale id counters, and the next index reuses ids.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reload_waiting_on_a_writer_includes_its_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("src");
        std::fs::create_dir_all(&root).unwrap();
        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings.add_indexed_path(root.clone()).unwrap();
        let settings = Arc::new(settings);
        let mut facade = IndexFacade::new(Arc::clone(&settings)).unwrap();
        facade.index_file(write_pair(&root, "a")).unwrap();
        let facade = Arc::new(RwLock::new(facade));
        let mut watcher =
            HotReloadWatcher::new(Arc::clone(&facade), settings, Duration::from_secs(3600));
        watcher.last_modified = None;

        let mut writer = facade.write().await;
        let reload =
            tokio::spawn(
                async move { watcher.check_and_reload().await.map_err(|e| e.to_string()) },
            );
        // Long enough for a reload that loads before locking to finish its
        // load; a reload that locks first is still waiting.
        tokio::time::sleep(Duration::from_millis(500)).await;
        writer.index_file(write_pair(&root, "b")).unwrap();
        drop(writer);
        reload.await.unwrap().unwrap();
        facade
            .write()
            .await
            .index_file(write_pair(&root, "c"))
            .unwrap();

        let facade = facade.read().await;
        let symbols = facade.get_all_symbols();
        let ids: HashSet<_> = symbols.iter().map(|s| s.id).collect();
        assert_eq!(
            ids.len(),
            symbols.len(),
            "duplicate symbol ids: {symbols:?}"
        );
        for name in ["a", "b", "c"] {
            let caller = facade.find_symbols_by_name(name, None);
            assert_eq!(caller.len(), 1, "{name}");
            let callees: Vec<String> = facade
                .get_called_functions(caller[0].id)
                .iter()
                .map(|s| s.name.to_string())
                .collect();
            assert_eq!(callees, vec![format!("{name}_helper")], "{name}");
        }
    }
}
