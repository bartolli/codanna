//! Handler for code file changes.
//!
//! Watches indexed source code files and triggers re-indexing on change.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::SimpleIndexer;
use crate::watcher::{WatchAction, WatchError, WatchHandler};

/// Handler for code file changes.
///
/// Tracks files that are in the code index and returns reindex/remove
/// actions when they change.
pub struct CodeFileHandler {
    /// Shared reference to the indexer.
    indexer: Arc<RwLock<SimpleIndexer>>,
    /// Cached set of indexed paths for fast lookup.
    cached_paths: RwLock<HashSet<PathBuf>>,
    /// Workspace root for path resolution.
    workspace_root: PathBuf,
}

impl CodeFileHandler {
    /// Create a new code file handler.
    pub fn new(indexer: Arc<RwLock<SimpleIndexer>>, workspace_root: PathBuf) -> Self {
        Self {
            indexer,
            cached_paths: RwLock::new(HashSet::new()),
            workspace_root,
        }
    }

    /// Initialize the cached paths from the indexer.
    pub async fn init_cache(&self) {
        let indexer = self.indexer.read().await;
        let paths: HashSet<PathBuf> = indexer
            .get_all_indexed_paths()
            .into_iter()
            .map(|p| self.to_absolute(&p))
            .collect();

        let mut cache = self.cached_paths.write().await;
        *cache = paths;
    }

    /// Convert a path to absolute using workspace root.
    fn to_absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
    }

    /// Convert an absolute path to relative for the indexer.
    fn to_relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.workspace_root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[async_trait]
impl WatchHandler for CodeFileHandler {
    fn name(&self) -> &str {
        "code"
    }

    fn matches(&self, path: &Path) -> bool {
        // Use cached paths for O(1) lookup
        if let Ok(cache) = self.cached_paths.try_read() {
            cache.contains(path)
        } else {
            // Cache locked, fall back to false
            // This is safe because we'll catch the event on retry
            false
        }
    }

    async fn tracked_paths(&self) -> Vec<PathBuf> {
        let indexer = self.indexer.read().await;
        indexer
            .get_all_indexed_paths()
            .into_iter()
            .map(|p| self.to_absolute(&p))
            .collect()
    }

    async fn on_modify(&self, path: &Path) -> Result<WatchAction, WatchError> {
        // Return action for UnifiedWatcher to execute
        Ok(WatchAction::ReindexCode {
            path: self.to_relative(path),
        })
    }

    async fn on_delete(&self, path: &Path) -> Result<WatchAction, WatchError> {
        // Remove from cache
        {
            let mut cache = self.cached_paths.write().await;
            cache.remove(path);
        }

        Ok(WatchAction::RemoveCode {
            path: self.to_relative(path),
        })
    }

    async fn refresh_paths(&self) -> Result<(), WatchError> {
        self.init_cache().await;
        Ok(())
    }
}
