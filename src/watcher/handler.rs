//! Handler trait and action types for the unified watcher.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::WatchError;

/// Actions returned by handlers for the UnifiedWatcher to execute.
#[derive(Debug, Clone)]
pub enum WatchAction {
    /// Re-index a code file. `created` marks a first-time file (unknown
    /// to the index at event time): the broadcast becomes `FileCreated`,
    /// which the notification lanes map to `list_changed`.
    ReindexCode { path: PathBuf, created: bool },

    /// Re-index a document file.
    ReindexDocument { path: PathBuf },

    /// Remove a code file from the index.
    RemoveCode { path: PathBuf },

    /// Remove a document from the store.
    RemoveDocument { path: PathBuf },

    /// Configuration changed - index new directories.
    ReloadConfig {
        added: Vec<PathBuf>,
        removed: Vec<PathBuf>,
    },

    /// No action needed (e.g., file unchanged).
    None,
}

/// Trait for handlers that process file change events.
///
/// Handlers declare which paths they care about and return actions
/// for the UnifiedWatcher to execute.
#[async_trait]
pub trait WatchHandler: Send + Sync {
    /// Handler name for logging.
    fn name(&self) -> &str;

    /// Check if this handler should process events for the given path.
    fn matches(&self, path: &Path) -> bool;

    /// Get all paths this handler is currently tracking.
    ///
    /// Used at startup to compute which directories to watch.
    async fn tracked_paths(&self) -> Vec<PathBuf>;

    /// Root directories this handler owns.
    ///
    /// Watched directly, and created directories under them extend the
    /// watch set. Default: none.
    async fn watch_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Whether a batch incremental sync over a watch root subsumes this
    /// handler's per-file actions for paths under that root. Removal
    /// waves batch-sync such roots so discovery can pair renames; other
    /// handlers keep per-file semantics. Default: no.
    fn covered_by_batch_sync(&self) -> bool {
        false
    }

    /// Handle a file modification event (called after debouncing).
    async fn on_modify(&self, path: &Path) -> Result<WatchAction, WatchError>;

    /// Handle a file deletion event (called immediately, no debouncing).
    async fn on_delete(&self, path: &Path) -> Result<WatchAction, WatchError>;

    /// Refresh the handler's tracked paths from its source.
    ///
    /// Called when the index is reloaded externally.
    async fn refresh_paths(&self) -> Result<(), WatchError> {
        Ok(())
    }
}
