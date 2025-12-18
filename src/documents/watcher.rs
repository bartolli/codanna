//! File system watcher for automatic re-indexing of document changes.
//!
//! Watches indexed document files and triggers re-indexing when they change.
//! Based on the same pattern as the code FileSystemWatcher but for documents.

use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, sleep};

use super::config::ChunkingConfig;
use super::store::DocumentStore;

/// Watches indexed document files for changes and triggers re-indexing.
///
/// Key behavior:
/// - Only watches files that are in the document index
/// - Watches parent directories but only processes events for indexed files
/// - Re-indexes modified files
/// - Removes deleted files from index
/// - Uses debouncing to prevent excessive re-indexing
pub struct DocumentWatcher {
    /// Reference to the document store (shared with MCP server).
    document_store: Arc<RwLock<DocumentStore>>,
    /// Chunking configuration for re-indexing.
    chunking_config: ChunkingConfig,
    /// How long to wait before processing changes (milliseconds).
    debounce_ms: u64,
    /// Channel receiver for file events.
    event_rx: mpsc::Receiver<notify::Result<Event>>,
    /// The actual file watcher (kept alive by storing it).
    _watcher: notify::RecommendedWatcher,
    /// Debug flag for verbose output.
    debug: bool,
}

impl DocumentWatcher {
    /// Create a new document watcher.
    ///
    /// # Arguments
    /// * `document_store` - Shared reference to the document store
    /// * `chunking_config` - Configuration for chunking when re-indexing
    /// * `debounce_ms` - Milliseconds to wait before processing changes
    /// * `debug` - Enable verbose output
    pub fn new(
        document_store: Arc<RwLock<DocumentStore>>,
        chunking_config: ChunkingConfig,
        debounce_ms: u64,
        debug: bool,
    ) -> Result<Self, notify::Error> {
        // Create channel for events
        let (tx, rx) = mpsc::channel(100);

        // Create the notify watcher with our channel
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let _ = tx.blocking_send(res);
        })?;

        Ok(Self {
            document_store,
            chunking_config,
            debounce_ms,
            event_rx: rx,
            _watcher: watcher,
            debug,
        })
    }

    /// Get the list of files that are currently indexed.
    async fn get_indexed_paths(&self) -> Vec<PathBuf> {
        let store = self.document_store.read().await;
        store.get_indexed_paths()
    }

    /// Compute unique parent directories to watch.
    fn compute_watch_dirs(paths: &[PathBuf]) -> HashSet<PathBuf> {
        paths
            .iter()
            .filter_map(|p| p.parent().map(|parent| parent.to_path_buf()))
            .collect()
    }

    /// Start watching document files for changes.
    ///
    /// This method:
    /// 1. Gets list of indexed document files
    /// 2. Sets up watching on their parent directories
    /// 3. Processes file events with debouncing
    pub async fn watch(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get list of indexed files
        let indexed_paths = self.get_indexed_paths().await;

        if indexed_paths.is_empty() {
            eprintln!("Document watcher: No indexed documents to watch");
            // Continue anyway - maybe docs will be indexed later
        } else {
            eprintln!(
                "Document watcher: Monitoring {} indexed documents",
                indexed_paths.len()
            );
        }

        // Get workspace root
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Compute directories to watch
        let watch_dirs = Self::compute_watch_dirs(&indexed_paths);

        if !watch_dirs.is_empty() && self.debug {
            eprintln!(
                "Document watcher: Watching {} directories",
                watch_dirs.len()
            );
        }

        // Start watching directories
        for dir in &watch_dirs {
            let watch_path = if dir.is_absolute() {
                dir.clone()
            } else {
                workspace_root.join(dir)
            };

            match self
                ._watcher
                .watch(&watch_path, RecursiveMode::NonRecursive)
            {
                Ok(_) => {
                    if self.debug {
                        eprintln!("  Watching: {}", watch_path.display());
                    }
                }
                Err(e) => {
                    eprintln!("  Warning: Failed to watch {}: {}", watch_path.display(), e);
                }
            }
        }

        // Convert paths to absolute for lookup
        let mut indexed_set: HashSet<PathBuf> = indexed_paths
            .into_iter()
            .map(|p| {
                if p.is_absolute() {
                    p
                } else {
                    workspace_root.join(&p)
                }
            })
            .collect();

        // Debouncing state
        let mut pending_changes: HashMap<PathBuf, Instant> = HashMap::new();
        let debounce_duration = Duration::from_millis(self.debounce_ms);

        eprintln!("Document watcher started");

        loop {
            let timeout = sleep(Duration::from_millis(100));
            tokio::pin!(timeout);

            tokio::select! {
                // Handle incoming file events
                Some(res) = self.event_rx.recv() => {
                    match res {
                        Ok(event) => {
                            for path in &event.paths {
                                if indexed_set.contains(path) {
                                    match event.kind {
                                        EventKind::Modify(_) => {
                                            pending_changes.insert(path.clone(), Instant::now());
                                        }
                                        EventKind::Remove(_) => {
                                            // File deleted - remove from index immediately
                                            eprintln!("Document deleted: {}", path.display());

                                            let relative_path = path
                                                .strip_prefix(&workspace_root)
                                                .map(|p| p.to_path_buf())
                                                .unwrap_or_else(|_| path.clone());

                                            let mut store = self.document_store.write().await;
                                            match store.remove_file(&relative_path) {
                                                Ok(true) => {
                                                    eprintln!("  Removed from index");
                                                    indexed_set.remove(path);
                                                }
                                                Ok(false) => {
                                                    if self.debug {
                                                        eprintln!("  Was not in index");
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("  Failed to remove: {e}");
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Document watch error: {e}");
                        }
                    }
                }

                // Process debounced changes
                _ = &mut timeout => {
                    let now = Instant::now();
                    let mut files_to_process = Vec::new();

                    pending_changes.retain(|path, last_change| {
                        if now.duration_since(*last_change) >= debounce_duration {
                            files_to_process.push(path.clone());
                            false
                        } else {
                            true
                        }
                    });

                    for path in files_to_process {
                        eprintln!("Document changed: {}", path.display());

                        let relative_path = path
                            .strip_prefix(&workspace_root)
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|_| path.clone());

                        let mut store = self.document_store.write().await;
                        match store.reindex_file(&relative_path, &self.chunking_config) {
                            Ok(Some(chunks)) => {
                                eprintln!("  Re-indexed ({chunks} chunks)");
                            }
                            Ok(None) => {
                                if self.debug {
                                    eprintln!("  Not in index, skipped");
                                }
                            }
                            Err(e) => {
                                eprintln!("  Re-index failed: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
}
