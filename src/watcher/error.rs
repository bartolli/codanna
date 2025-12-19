//! Error types for the unified watcher system.

use std::path::PathBuf;
use thiserror::Error;

/// Errors from watcher operations.
#[derive(Error, Debug)]
pub enum WatchError {
    #[error("Failed to initialize watcher: {reason}")]
    InitFailed { reason: String },

    #[error("Cannot watch path {path}: {reason}")]
    PathWatchFailed { path: PathBuf, reason: String },

    #[error("File system event error: {details}")]
    EventError { details: String },

    #[error("Handler '{handler}' failed for {path}: {reason}")]
    HandlerFailed {
        handler: String,
        path: PathBuf,
        reason: String,
    },

    #[error("Failed to load config: {reason}")]
    ConfigError { reason: String },

    #[error("Channel closed unexpectedly")]
    ChannelClosed,
}

impl From<notify::Error> for WatchError {
    fn from(e: notify::Error) -> Self {
        WatchError::InitFailed {
            reason: e.to_string(),
        }
    }
}
