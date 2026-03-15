//! Error types for plugin management operations

use crate::io::exit_code::ExitCode;
use std::{io, path::PathBuf};
use thiserror::Error;

/// Errors that can occur during plugin operations
#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Marketplace not found: {url}\nSuggestion: Check the URL is correct and accessible")]
    MarketplaceNotFound { url: String },

    #[error(
        "Plugin '{name}' not found in marketplace\nSuggestion: Use 'codanna plugin list' to see available plugins"
    )]
    PluginNotFound { name: String },

    #[error(
        "Invalid marketplace manifest: {reason}\nSuggestion: Ensure the marketplace.json file follows Claude's schema"
    )]
    InvalidMarketplaceManifest { reason: String },

    #[error(
        "Invalid plugin manifest: {reason}\nSuggestion: Ensure the plugin.json file follows Claude's schema"
    )]
    InvalidPluginManifest { reason: String },

    #[error(
        "Git operation failed: {operation}\nSuggestion: Check network connection and repository permissions"
    )]
    GitOperationFailed { operation: String },

    #[error(
        "File conflict: {path} already exists and belongs to plugin '{owner}'\nSuggestion: Use --force to overwrite or remove the conflicting plugin first"
    )]
    FileConflict { path: PathBuf, owner: String },

    #[error(
        "Integrity check failed for plugin '{plugin}': expected {expected}, got {actual}\nSuggestion: Try removing and reinstalling the plugin"
    )]
    IntegrityCheckFailed {
        plugin: String,
        expected: String,
        actual: String,
    },

    #[error(
        "Lockfile corrupted or invalid\nSuggestion: Remove .codanna/plugins/lockfile.json and reinstall plugins"
    )]
    LockfileCorrupted,

    #[error(
        "Permission denied accessing {path}\nSuggestion: Check file permissions and ensure codanna has write access"
    )]
    PermissionDenied { path: PathBuf },

    #[error(
        "Plugin '{name}' is already installed at version {version}\nSuggestion: Use 'codanna plugin update' to change versions"
    )]
    AlreadyInstalled { name: String, version: String },

    #[error(
        "Plugin '{name}' is not installed\nSuggestion: Use 'codanna plugin add' to install it first"
    )]
    NotInstalled { name: String },

    #[error(
        "Cannot remove plugin '{name}': other plugins depend on it: {dependents:?}\nSuggestion: Remove dependent plugins first or use --force"
    )]
    HasDependents {
        name: String,
        dependents: Vec<String>,
    },

    #[error(
        "MCP server conflict: key '{key}' already defined\nSuggestion: Resolve the conflict manually in .mcp.json or use --force"
    )]
    McpServerConflict { key: String },

    #[error("Missing required argument: {0}\nSuggestion: Provide the {0} argument")]
    MissingArgument(String),

    #[error("Invalid reference '{ref_name}': {reason}\nSuggestion: Use a valid branch name or tag")]
    InvalidReference { ref_name: String, reason: String },

    #[error("Network error: {0}\nSuggestion: Check your internet connection and try again")]
    NetworkError(String),

    #[error("IO error: {0}\nSuggestion: Check file permissions and disk space")]
    IoError(#[from] io::Error),

    #[error("JSON parsing error: {0}\nSuggestion: Ensure the JSON file is well-formed")]
    JsonError(#[from] serde_json::Error),

    #[error(
        "Plugin '{name}' has local modifications\nSuggestion: Use --force to overwrite changes or backup modified files"
    )]
    LocalModifications { name: String },

    #[error("Dry run completed successfully\nNo changes were made to the system")]
    DryRunSuccess,
}

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

impl PluginError {
    /// Map plugin errors to CLI exit codes for consistent UX.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            PluginError::MarketplaceNotFound { .. }
            | PluginError::PluginNotFound { .. }
            | PluginError::NotInstalled { .. } => ExitCode::NotFound,
            PluginError::InvalidMarketplaceManifest { .. }
            | PluginError::InvalidPluginManifest { .. }
            | PluginError::JsonError(_)
            | PluginError::MissingArgument(_)
            | PluginError::LockfileCorrupted => ExitCode::ConfigError,
            PluginError::FileConflict { .. }
            | PluginError::IntegrityCheckFailed { .. }
            | PluginError::HasDependents { .. }
            | PluginError::McpServerConflict { .. }
            | PluginError::LocalModifications { .. } => ExitCode::BlockingError,
            PluginError::PermissionDenied { .. }
            | PluginError::IoError(_)
            | PluginError::GitOperationFailed { .. }
            | PluginError::NetworkError(_)
            | PluginError::InvalidReference { .. } => ExitCode::GeneralError,
            PluginError::AlreadyInstalled { .. } => ExitCode::UnsupportedOperation,
            PluginError::DryRunSuccess => ExitCode::Success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages_include_suggestions() {
        let err = PluginError::PluginNotFound {
            name: "test-plugin".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Suggestion:"));
        assert!(msg.contains("test-plugin"));
    }

    #[test]
    fn test_file_conflict_error() {
        let err = PluginError::FileConflict {
            path: PathBuf::from(".claude/commands/test.md"),
            owner: "other-plugin".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains(".claude/commands/test.md"));
        assert!(msg.contains("other-plugin"));
        assert!(msg.contains("--force"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "test");
        let plugin_err: PluginError = io_err.into();
        assert!(matches!(plugin_err, PluginError::IoError(_)));
    }
}
