//! Git repository operations for plugin fetching
//!
//! Delegates to crate::git which uses system git binary.

use super::error::{PluginError, PluginResult};
use crate::git::GitError;
use std::path::Path;

fn map_git_error(e: GitError, operation: &str) -> PluginError {
    let detail = match &e {
        GitError::CommandFailed { .. } => e.message(),
        _ => format!("{operation}: {}", e.message()),
    };
    PluginError::GitOperationFailed { operation: detail }
}

/// Clone a repository with shallow depth. Returns commit SHA.
pub fn clone_repository(
    repo_url: &str,
    target_dir: &Path,
    git_ref: Option<&str>,
) -> PluginResult<String> {
    crate::git::clone_repository(repo_url, target_dir, git_ref)
        .map_err(|e| map_git_error(e, "clone"))
}

/// Resolve a git reference to a commit SHA without cloning.
pub fn resolve_reference(repo_url: &str, git_ref: &str) -> PluginResult<String> {
    crate::git::resolve_reference(repo_url, git_ref).map_err(|e| match e {
        GitError::ReferenceNotFound { ref_name, .. } => PluginError::InvalidReference {
            ref_name,
            reason: "Reference not found in repository".to_string(),
        },
        other => map_git_error(other, "resolve reference"),
    })
}

/// Extract a subdirectory from a cloned repository
pub fn extract_subdirectory(repo_dir: &Path, subdir: &str, target_dir: &Path) -> PluginResult<()> {
    let source_dir = repo_dir.join(subdir);

    if !source_dir.exists() {
        return Err(PluginError::PluginNotFound {
            name: subdir.to_string(),
        });
    }

    // Create target directory
    std::fs::create_dir_all(target_dir)?;

    // Copy subdirectory contents
    copy_dir_contents(&source_dir, target_dir)?;

    Ok(())
}

/// Recursively copy directory contents
fn copy_dir_contents(source: &Path, dest: &Path) -> PluginResult<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let source_path = entry.path();
        let file_name = entry.file_name();
        let dest_path = dest.join(&file_name);

        if file_type.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir_contents(&source_path, &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&source_path, &dest_path)?;
        } else if file_type.is_symlink() {
            // Dereference symlink and copy target content
            let metadata = std::fs::metadata(&source_path)?;
            if metadata.is_dir() {
                std::fs::create_dir_all(&dest_path)?;
                copy_dir_contents(&source_path, &dest_path)?;
            } else {
                std::fs::copy(&source_path, &dest_path)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    #[ignore] // Requires network
    fn test_resolve_reference() {
        // Test resolving a tag in a public repo
        let result = resolve_reference("https://github.com/rust-lang/rust.git", "1.0.0");
        assert!(result.is_ok());

        // Test invalid reference
        let result = resolve_reference("https://github.com/rust-lang/rust.git", "nonexistent-ref");
        assert!(result.is_err());
    }

    #[test]
    #[ignore] // Requires network
    fn test_clone_repository() {
        let temp_dir = tempdir().unwrap();
        let clone_path = temp_dir.path().join("test-repo");

        // Clone a small public repo
        let result = clone_repository(
            "https://github.com/rust-lang/rustlings.git",
            &clone_path,
            Some("main"),
        );

        assert!(result.is_ok());
        assert!(clone_path.exists());
        assert!(clone_path.join(".git").exists());

        // Verify we got a commit SHA
        let sha = result.unwrap();
        assert_eq!(sha.len(), 40); // Git SHA is 40 hex chars
    }

    #[test]
    fn test_extract_subdirectory() {
        let temp_dir = tempdir().unwrap();
        let source_dir = temp_dir.path().join("source");
        let target_dir = temp_dir.path().join("target");

        // Create test structure
        std::fs::create_dir_all(source_dir.join("subdir")).unwrap();
        std::fs::write(source_dir.join("subdir/file.txt"), "test content").unwrap();

        // Extract subdirectory
        let result = extract_subdirectory(&source_dir, "subdir", &target_dir);
        assert!(result.is_ok());
        assert!(target_dir.join("file.txt").exists());

        // Test non-existent subdirectory
        let result = extract_subdirectory(&source_dir, "nonexistent", &target_dir);
        assert!(matches!(result, Err(PluginError::PluginNotFound { .. })));
    }

    #[test]
    #[cfg(unix)]
    fn test_extract_subdirectory_follows_symlinks() {
        let temp_dir = tempdir().unwrap();
        let source_dir = temp_dir.path().join("source");
        let target_dir = temp_dir.path().join("target");

        // Create a file and a symlink to it
        std::fs::create_dir_all(source_dir.join("subdir")).unwrap();
        std::fs::write(source_dir.join("subdir/real.txt"), "real content").unwrap();
        std::os::unix::fs::symlink(
            source_dir.join("subdir/real.txt"),
            source_dir.join("subdir/link.txt"),
        )
        .unwrap();

        let result = extract_subdirectory(&source_dir, "subdir", &target_dir);
        assert!(result.is_ok());
        assert!(target_dir.join("real.txt").exists());
        assert!(target_dir.join("link.txt").exists());

        // Verify the symlink target was copied (not the symlink itself)
        let content = std::fs::read_to_string(target_dir.join("link.txt")).unwrap();
        assert_eq!(content, "real content");
        assert!(!target_dir.join("link.txt").is_symlink());
    }
}
