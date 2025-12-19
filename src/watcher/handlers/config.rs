//! Handler for configuration file changes.
//!
//! Watches settings.toml and triggers directory indexing when indexed_paths changes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::config::Settings;
use crate::watcher::{WatchAction, WatchError, WatchHandler};

/// Handler for configuration file changes.
///
/// Watches settings.toml and detects changes to indexed_paths.
/// Returns ReloadConfig action with added/removed directories.
pub struct ConfigFileHandler {
    /// Path to settings.toml.
    settings_path: PathBuf,
    /// Last known indexed_paths for diffing.
    last_indexed_paths: RwLock<HashSet<PathBuf>>,
}

impl ConfigFileHandler {
    /// Create a new config file handler.
    pub fn new(settings_path: PathBuf) -> Result<Self, WatchError> {
        // Load initial indexed_paths
        let config = Settings::load_from(&settings_path).map_err(|e| WatchError::ConfigError {
            reason: format!("Failed to load config: {e}"),
        })?;

        let initial_paths: HashSet<PathBuf> = config.indexing.indexed_paths.into_iter().collect();

        Ok(Self {
            settings_path,
            last_indexed_paths: RwLock::new(initial_paths),
        })
    }

    /// Compute diff between current and previous indexed_paths.
    async fn compute_diff(&self) -> Result<(Vec<PathBuf>, Vec<PathBuf>), WatchError> {
        // Reload config
        let new_config =
            Settings::load_from(&self.settings_path).map_err(|e| WatchError::ConfigError {
                reason: format!("Failed to reload config: {e}"),
            })?;

        let new_paths: HashSet<PathBuf> = new_config.indexing.indexed_paths.into_iter().collect();

        let last_paths = self.last_indexed_paths.read().await;

        // Compute added and removed
        let added: Vec<PathBuf> = new_paths.difference(&last_paths).cloned().collect();
        let removed: Vec<PathBuf> = last_paths.difference(&new_paths).cloned().collect();

        // Update stored paths if there were changes
        if !added.is_empty() || !removed.is_empty() {
            drop(last_paths);
            let mut write_lock = self.last_indexed_paths.write().await;
            *write_lock = new_paths;
        }

        Ok((added, removed))
    }
}

#[async_trait]
impl WatchHandler for ConfigFileHandler {
    fn name(&self) -> &str {
        "config"
    }

    fn matches(&self, path: &Path) -> bool {
        // Only match the exact settings file
        path == self.settings_path
    }

    async fn tracked_paths(&self) -> Vec<PathBuf> {
        // Only track the single settings file
        vec![self.settings_path.clone()]
    }

    async fn on_modify(&self, _path: &Path) -> Result<WatchAction, WatchError> {
        // Small delay to ensure file write is complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let (added, removed) = self.compute_diff().await?;

        if added.is_empty() && removed.is_empty() {
            // indexed_paths unchanged
            return Ok(WatchAction::None);
        }

        Ok(WatchAction::ReloadConfig { added, removed })
    }

    async fn on_delete(&self, _path: &Path) -> Result<WatchAction, WatchError> {
        // Config file deleted - nothing we can do
        // Log a warning but don't crash
        eprintln!(
            "Warning: Config file {} was deleted",
            self.settings_path.display()
        );
        Ok(WatchAction::None)
    }
}
