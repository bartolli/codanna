//! Handler for document file changes.
//!
//! Watches indexed document files and triggers re-indexing on change.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::documents::DocumentStore;
use crate::watcher::{WatchAction, WatchError, WatchHandler};

/// Handler for document file changes.
///
/// Tracks files that are in the document index and returns reindex/remove
/// actions when they change.
pub struct DocumentFileHandler {
    /// Shared reference to the document store.
    store: Arc<RwLock<DocumentStore>>,
    /// Cached set of indexed paths for fast lookup.
    cached_paths: RwLock<HashSet<PathBuf>>,
    /// Workspace root for path resolution.
    workspace_root: PathBuf,
}

impl DocumentFileHandler {
    /// Create a new document file handler.
    pub fn new(store: Arc<RwLock<DocumentStore>>, workspace_root: PathBuf) -> Self {
        Self {
            store,
            cached_paths: RwLock::new(HashSet::new()),
            workspace_root,
        }
    }

    /// Initialize the cached paths from the store.
    pub async fn init_cache(&self) {
        let store = self.store.read().await;
        let paths: HashSet<PathBuf> = store
            .get_indexed_paths()
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
}

#[async_trait]
impl WatchHandler for DocumentFileHandler {
    fn name(&self) -> &str {
        "document"
    }

    fn matches(&self, path: &Path) -> bool {
        if let Ok(cache) = self.cached_paths.try_read() {
            cache.contains(path)
        } else {
            false
        }
    }

    async fn tracked_paths(&self) -> Vec<PathBuf> {
        let store = self.store.read().await;
        store
            .get_indexed_paths()
            .into_iter()
            .map(|p| self.to_absolute(&p))
            .collect()
    }

    async fn on_modify(&self, path: &Path) -> Result<WatchAction, WatchError> {
        // DocumentStore.file_states uses absolute paths, so pass absolute
        Ok(WatchAction::ReindexDocument {
            path: path.to_path_buf(),
        })
    }

    async fn on_delete(&self, path: &Path) -> Result<WatchAction, WatchError> {
        // Remove from cache
        {
            let mut cache = self.cached_paths.write().await;
            cache.remove(path);
        }

        // DocumentStore.file_states uses absolute paths, so pass absolute
        Ok(WatchAction::RemoveDocument {
            path: path.to_path_buf(),
        })
    }

    async fn refresh_paths(&self) -> Result<(), WatchError> {
        self.init_cache().await;
        Ok(())
    }
}
