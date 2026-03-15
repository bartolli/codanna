//! Git repository operations for profile fetching
//!
//! Delegates to crate::git which uses system git binary.

use super::error::{ProfileError, ProfileResult};
use crate::git::GitError;
use std::path::Path;

fn map_git_error(e: GitError, operation: &str) -> ProfileError {
    let detail = match &e {
        GitError::CommandFailed { .. } => e.message(),
        _ => format!("{operation}: {}", e.message()),
    };
    ProfileError::GitOperationFailed { operation: detail }
}

/// Clone a repository with shallow depth. Returns commit SHA.
pub fn clone_repository(
    repo_url: &str,
    target_dir: &Path,
    git_ref: Option<&str>,
) -> ProfileResult<String> {
    crate::git::clone_repository(repo_url, target_dir, git_ref)
        .map_err(|e| map_git_error(e, "clone"))
}

/// Resolve a git reference to a commit SHA without cloning.
pub fn resolve_reference(repo_url: &str, git_ref: &str) -> ProfileResult<String> {
    crate::git::resolve_reference(repo_url, git_ref)
        .map_err(|e| map_git_error(e, "resolve reference"))
}
