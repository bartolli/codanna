//! Error types for profile system operations

use crate::io::exit_code::ExitCode;
use std::io;
use thiserror::Error;

/// Errors that can occur during profile operations
#[derive(Error, Debug)]
pub enum ProfileError {
    #[error(
        "Invalid profile manifest: {reason}\nSuggestion: Ensure the profile.json file follows the schema"
    )]
    InvalidManifest { reason: String },

    #[error(
        "File conflict: {path} is owned by {owner}\nSuggestion: Use --force to install alongside as {{filename}}.{{provider}}.{{ext}}"
    )]
    FileConflict { path: String, owner: String },

    #[error("{}", format_multiple_conflicts(.conflicts))]
    MultipleFileConflicts { conflicts: Vec<(String, String)> },

    #[error(
        "Profile '{profile}' failed integrity check\n  Expected: {expected}\n  Actual: {actual}\nSuggestion: Try removing and reinstalling the profile"
    )]
    IntegrityCheckFailed {
        profile: String,
        expected: String,
        actual: String,
    },

    #[error(
        "Profile '{name}' is already installed (version {version})\nSuggestion: Use --force to reinstall"
    )]
    AlreadyInstalled { name: String, version: String },

    #[error(
        "Profile '{name}' is not installed\nSuggestion: Use 'codanna profile install' to install it first"
    )]
    NotInstalled { name: String },

    #[error(
        "Provider '{provider}' not found\nSuggestion: Use 'codanna profile provider list' to see registered providers"
    )]
    ProviderNotFound { provider: String },

    #[error(
        "Profile '{profile}' not found in provider '{provider}'\nSuggestion: Check available profiles with 'codanna profile provider list --verbose'"
    )]
    ProfileNotFoundInProvider { profile: String, provider: String },

    #[error(
        "Profile '{profile}' not found in any registered provider\nSuggestion: Register a provider with 'codanna profile provider add <source>'"
    )]
    ProfileNotFoundInAnyProvider { profile: String },

    #[error(
        "Git operation failed: {operation}\nSuggestion: Check network connection and repository permissions"
    )]
    GitOperationFailed { operation: String },

    #[error("JSON parsing error: {0}\nSuggestion: Ensure the JSON file is well-formed")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}\nSuggestion: Check file permissions and disk space")]
    IoError(#[from] io::Error),
}

/// Result type for profile operations
pub type ProfileResult<T> = Result<T, ProfileError>;

/// Format multiple file conflicts into a user-friendly message
fn format_multiple_conflicts(conflicts: &[(String, String)]) -> String {
    let mut msg = String::from("File conflicts detected:\n\n");

    for (path, owner) in conflicts {
        let owner_display = if owner == "unknown" {
            "exists (not tracked by any profile)".to_string()
        } else {
            format!("owned by profile '{owner}'")
        };
        msg.push_str(&format!("  {path} - {owner_display}\n"));
    }

    msg.push_str("\nUse --force to install profile-scoped versions alongside existing files.");
    msg.push_str("\nYour original files will not be affected.");
    msg
}

impl ProfileError {
    /// Map profile errors to CLI exit codes for consistent UX.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            ProfileError::InvalidManifest { .. } | ProfileError::JsonError(_) => {
                ExitCode::ConfigError
            }
            ProfileError::FileConflict { .. }
            | ProfileError::MultipleFileConflicts { .. }
            | ProfileError::IntegrityCheckFailed { .. }
            | ProfileError::AlreadyInstalled { .. } => ExitCode::BlockingError,
            ProfileError::NotInstalled { .. }
            | ProfileError::ProviderNotFound { .. }
            | ProfileError::ProfileNotFoundInProvider { .. }
            | ProfileError::ProfileNotFoundInAnyProvider { .. } => ExitCode::NotFound,
            ProfileError::GitOperationFailed { .. } | ProfileError::IoError(_) => {
                ExitCode::GeneralError
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages_include_suggestions() {
        let err = ProfileError::NotInstalled {
            name: "test-profile".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Suggestion:"));
        assert!(msg.contains("test-profile"));
    }

    #[test]
    fn test_file_conflict_error() {
        let err = ProfileError::FileConflict {
            path: ".claude/test.md".to_string(),
            owner: "other-profile".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains(".claude/test.md"));
        assert!(msg.contains("other-profile"));
        assert!(msg.contains("--force"));
    }

    #[test]
    fn test_integrity_check_failed() {
        let err = ProfileError::IntegrityCheckFailed {
            profile: "claude".to_string(),
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("claude"));
        assert!(msg.contains("abc123"));
        assert!(msg.contains("def456"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "test");
        let profile_err: ProfileError = io_err.into();
        assert!(matches!(profile_err, ProfileError::IoError(_)));
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(
            ProfileError::NotInstalled {
                name: "test".to_string()
            }
            .exit_code(),
            ExitCode::NotFound
        );
        assert_eq!(
            ProfileError::FileConflict {
                path: "test".to_string(),
                owner: "other".to_string()
            }
            .exit_code(),
            ExitCode::BlockingError
        );
        assert_eq!(
            ProfileError::InvalidManifest {
                reason: "test".to_string()
            }
            .exit_code(),
            ExitCode::ConfigError
        );
    }
}
