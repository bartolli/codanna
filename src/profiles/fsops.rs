//! File system operations for profile installation

use super::error::{ProfileError, ProfileResult};
use super::lockfile::ProfileLockEntry;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Calculate SHA-256 integrity hash for a set of files
///
/// Returns a hex string representing the combined hash of all file contents.
/// Files are hashed in order with a newline separator between each file.
///
/// Plugin reference: src/plugins/fsops.rs:174-190
pub fn calculate_integrity(file_paths: &[String]) -> ProfileResult<String> {
    let mut hasher = Sha256::new();

    for file_path in file_paths {
        let path = Path::new(file_path);
        if path.exists() {
            let content = std::fs::read(path)?;
            hasher.update(&content);
            hasher.update(b"\n"); // Separator between files
        }
    }

    let result = hasher.finalize();
    Ok(hex::encode(result))
}

/// Collect all files from a profile directory
///
/// Recursively walks the profile directory and returns relative paths to all files.
/// Excludes:
/// - Directories
/// - .git directories and their contents
/// - profile.json manifest file
///
/// Plugin reference: src/plugins/fsops.rs:47-105 (copy_plugin_payload)
pub fn collect_all_files(profile_dir: &Path) -> ProfileResult<Vec<String>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(profile_dir) {
        let entry = entry.map_err(|e| ProfileError::IoError(std::io::Error::other(e)))?;

        // Skip directories
        if entry.file_type().is_dir() {
            continue;
        }

        // Get relative path
        let relative = entry
            .path()
            .strip_prefix(profile_dir)
            .expect("walkdir entry should be under profile_dir");

        // Skip .git directories
        if relative.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }

        // Skip profile.json manifest
        let normalized = relative.to_string_lossy().replace('\\', "/");
        if normalized == "profile.json" {
            continue;
        }

        files.push(normalized);
    }

    Ok(files)
}

/// Remove profile files and clean up empty directories
///
/// Safely removes all files in the list. If a file doesn't exist, it's skipped.
/// After removing each file, attempts to remove its parent directory if empty.
///
/// Plugin reference: src/plugins/fsops.rs:107-121
pub fn remove_profile_files(file_list: &[String]) -> ProfileResult<()> {
    for file_path in file_list {
        let path = Path::new(file_path);
        if path.exists() {
            std::fs::remove_file(path)?;
        }

        // Try to remove parent directory if empty
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir(parent); // Ignore errors if not empty
        }
    }

    Ok(())
}

/// Copy profile files to workspace with conflict detection
///
/// Copies files from source to destination, checking for conflicts.
/// Returns a list of absolute paths to all successfully copied files.
///
/// Plugin reference: src/plugins/fsops.rs:9-44
pub fn copy_profile_files(
    source_dir: &Path,
    dest_dir: &Path,
    file_list: &[String],
    force: bool,
    conflict_owner: impl Fn(&Path) -> Option<String>,
) -> ProfileResult<Vec<String>> {
    let mut copied_files = Vec::new();

    for file_path in file_list {
        let source_path = source_dir.join(file_path);
        let dest_path = dest_dir.join(file_path);

        // Skip if source doesn't exist
        if !source_path.exists() {
            continue;
        }

        // Check for conflicts
        if dest_path.exists() && !force {
            let owner = conflict_owner(&dest_path).unwrap_or_else(|| "unknown".to_string());
            return Err(ProfileError::FileConflict {
                path: file_path.to_string(),
                owner,
            });
        }

        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Copy file
        std::fs::copy(&source_path, &dest_path)?;

        // Store absolute path with normalized separators
        let absolute_path = dest_path
            .canonicalize()
            .unwrap_or(dest_path)
            .to_string_lossy()
            .replace('\\', "/");
        copied_files.push(absolute_path);
    }

    Ok(copied_files)
}

/// Backup of an existing profile for rollback support
///
/// Stores all file contents and lockfile entry in memory to enable
/// rollback if profile installation fails.
///
/// Plugin reference: src/plugins/mod.rs:51-56 (ExistingPluginBackup)
#[derive(Debug, Clone)]
pub struct ProfileBackup {
    /// Lockfile entry for the profile being backed up
    pub entry: ProfileLockEntry,
    /// File paths and their contents: (absolute_path, content)
    pub files: Vec<(PathBuf, Vec<u8>)>,
}

/// Create a backup of an existing profile before modification
///
/// Reads all files tracked in the lockfile entry and stores their contents
/// in memory for potential rollback. Files that don't exist are skipped.
///
/// Plugin reference: src/plugins/mod.rs:458-483 (backup_existing_plugin)
pub fn backup_profile(workspace: &Path, entry: &ProfileLockEntry) -> ProfileResult<ProfileBackup> {
    let mut files = Vec::new();

    for relative in &entry.files {
        let absolute = workspace.join(relative);
        if absolute.exists() {
            let data = fs::read(&absolute)?;
            files.push((absolute, data));
        }
    }

    Ok(ProfileBackup {
        entry: entry.clone(),
        files,
    })
}

/// Restore a profile from backup
///
/// Writes all backed-up files back to their original locations.
/// Creates parent directories as needed.
///
/// Plugin reference: src/plugins/mod.rs:485-514 (restore_existing_plugin)
pub fn restore_profile(backup: &ProfileBackup) -> ProfileResult<()> {
    for (path, data) in &backup.files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, data)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_calculate_integrity_single_file() {
        let temp = tempdir().unwrap();
        let file1 = temp.path().join("test1.txt");
        fs::write(&file1, "test content").unwrap();

        let files = vec![file1.to_string_lossy().to_string()];
        let integrity = calculate_integrity(&files).unwrap();

        // Should produce consistent hash
        assert_eq!(integrity.len(), 64); // SHA-256 is 64 hex chars
    }

    #[test]
    fn test_calculate_integrity_multiple_files() {
        let temp = tempdir().unwrap();
        let file1 = temp.path().join("test1.txt");
        let file2 = temp.path().join("test2.txt");

        fs::write(&file1, "content 1").unwrap();
        fs::write(&file2, "content 2").unwrap();

        let files = vec![
            file1.to_string_lossy().to_string(),
            file2.to_string_lossy().to_string(),
        ];
        let integrity1 = calculate_integrity(&files).unwrap();

        // Same files should produce same hash
        let integrity2 = calculate_integrity(&files).unwrap();
        assert_eq!(integrity1, integrity2);

        // Different order should produce different hash (order matters)
        let files_reversed = vec![
            file2.to_string_lossy().to_string(),
            file1.to_string_lossy().to_string(),
        ];
        let integrity3 = calculate_integrity(&files_reversed).unwrap();
        assert_ne!(integrity1, integrity3);
    }

    #[test]
    fn test_calculate_integrity_missing_file() {
        let temp = tempdir().unwrap();
        let file1 = temp.path().join("exists.txt");
        let file2 = temp.path().join("missing.txt");

        fs::write(&file1, "content").unwrap();

        let files = vec![
            file1.to_string_lossy().to_string(),
            file2.to_string_lossy().to_string(),
        ];

        // Should succeed and skip missing file
        let integrity = calculate_integrity(&files).unwrap();
        assert_eq!(integrity.len(), 64);
    }

    #[test]
    fn test_remove_profile_files() {
        let temp = tempdir().unwrap();
        let file1 = temp.path().join("test1.txt");
        let file2 = temp.path().join("subdir/test2.txt");

        fs::write(&file1, "content 1").unwrap();
        fs::create_dir_all(temp.path().join("subdir")).unwrap();
        fs::write(&file2, "content 2").unwrap();

        let files = vec![
            file1.to_string_lossy().to_string(),
            file2.to_string_lossy().to_string(),
        ];

        remove_profile_files(&files).unwrap();

        assert!(!file1.exists());
        assert!(!file2.exists());
    }

    #[test]
    fn test_remove_profile_files_missing() {
        // Should not error on missing files
        let files = vec!["/nonexistent/file.txt".to_string()];
        let result = remove_profile_files(&files);
        assert!(result.is_ok());
    }

    #[test]
    fn test_copy_profile_files_single() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");

        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("test.txt"), "test content").unwrap();

        let files = vec!["test.txt".to_string()];
        let copied = copy_profile_files(&source, &dest, &files, false, |_| None).unwrap();

        assert_eq!(copied.len(), 1);
        assert!(dest.join("test.txt").exists());

        let content = fs::read_to_string(dest.join("test.txt")).unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_copy_profile_files_with_subdirs() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");

        fs::create_dir_all(source.join("subdir")).unwrap();
        fs::write(source.join("subdir/test.txt"), "nested content").unwrap();

        let files = vec!["subdir/test.txt".to_string()];
        let copied = copy_profile_files(&source, &dest, &files, false, |_| None).unwrap();

        assert_eq!(copied.len(), 1);
        assert!(dest.join("subdir/test.txt").exists());
    }

    #[test]
    fn test_copy_profile_files_skip_missing() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");

        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("exists.txt"), "content").unwrap();

        let files = vec!["exists.txt".to_string(), "missing.txt".to_string()];
        let copied = copy_profile_files(&source, &dest, &files, false, |_| None).unwrap();

        // Should only copy the file that exists
        assert_eq!(copied.len(), 1);
        assert!(dest.join("exists.txt").exists());
        assert!(!dest.join("missing.txt").exists());
    }

    #[test]
    fn test_copy_profile_files_conflict_detection() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let dest = temp.path().join("dest");

        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dest).unwrap();

        fs::write(source.join("test.txt"), "source content").unwrap();
        fs::write(dest.join("test.txt"), "existing content").unwrap();

        let files = vec!["test.txt".to_string()];

        // Should error without force
        let result = copy_profile_files(&source, &dest, &files, false, |_| {
            Some("other-profile".to_string())
        });
        assert!(result.is_err());
        match result {
            Err(ProfileError::FileConflict { path, owner }) => {
                assert_eq!(path, "test.txt");
                assert_eq!(owner, "other-profile");
            }
            _ => panic!("Expected FileConflict error"),
        }

        // Should succeed with force
        let copied = copy_profile_files(&source, &dest, &files, true, |_| {
            Some("other-profile".to_string())
        })
        .unwrap();
        assert_eq!(copied.len(), 1);

        // Verify content was overwritten
        let content = fs::read_to_string(dest.join("test.txt")).unwrap();
        assert_eq!(content, "source content");
    }

    #[test]
    fn test_backup_profile() {
        let temp = tempdir().unwrap();
        let workspace = temp.path();

        // Create test files
        fs::write(workspace.join("test1.txt"), "content 1").unwrap();
        fs::create_dir_all(workspace.join("subdir")).unwrap();
        fs::write(workspace.join("subdir/test2.txt"), "content 2").unwrap();

        let entry = ProfileLockEntry {
            name: "test-profile".to_string(),
            version: "1.0.0".to_string(),
            installed_at: "2025-01-11".to_string(),
            files: vec!["test1.txt".to_string(), "subdir/test2.txt".to_string()],
            integrity: "abc123".to_string(),
            commit: None,
            provider_id: None,
            source: None,
        };

        let backup = backup_profile(workspace, &entry).unwrap();

        assert_eq!(backup.entry.name, "test-profile");
        assert_eq!(backup.files.len(), 2);

        // Verify file contents were backed up
        let (path1, data1) = &backup.files[0];
        assert!(path1.ends_with("test1.txt"));
        assert_eq!(data1, b"content 1");

        let (path2, data2) = &backup.files[1];
        assert!(path2.ends_with("test2.txt"));
        assert_eq!(data2, b"content 2");
    }

    #[test]
    fn test_backup_profile_missing_files() {
        let temp = tempdir().unwrap();
        let workspace = temp.path();

        // Create only one of two files
        fs::write(workspace.join("exists.txt"), "content").unwrap();

        let entry = ProfileLockEntry {
            name: "test-profile".to_string(),
            version: "1.0.0".to_string(),
            installed_at: "2025-01-11".to_string(),
            files: vec!["exists.txt".to_string(), "missing.txt".to_string()],
            integrity: "abc123".to_string(),
            commit: None,
            provider_id: None,
            source: None,
        };

        let backup = backup_profile(workspace, &entry).unwrap();

        // Should only backup the file that exists
        assert_eq!(backup.files.len(), 1);
        let (path, data) = &backup.files[0];
        assert!(path.ends_with("exists.txt"));
        assert_eq!(data, b"content");
    }

    #[test]
    fn test_restore_profile() {
        let temp = tempdir().unwrap();
        let workspace = temp.path();

        // Create original files
        fs::write(workspace.join("test1.txt"), "original 1").unwrap();
        fs::create_dir_all(workspace.join("subdir")).unwrap();
        fs::write(workspace.join("subdir/test2.txt"), "original 2").unwrap();

        let entry = ProfileLockEntry {
            name: "test-profile".to_string(),
            version: "1.0.0".to_string(),
            installed_at: "2025-01-11".to_string(),
            files: vec!["test1.txt".to_string(), "subdir/test2.txt".to_string()],
            integrity: "abc123".to_string(),
            commit: None,
            provider_id: None,
            source: None,
        };

        // Create backup
        let backup = backup_profile(workspace, &entry).unwrap();

        // Modify files
        fs::write(workspace.join("test1.txt"), "modified 1").unwrap();
        fs::write(workspace.join("subdir/test2.txt"), "modified 2").unwrap();

        // Restore from backup
        restore_profile(&backup).unwrap();

        // Verify files were restored
        let content1 = fs::read_to_string(workspace.join("test1.txt")).unwrap();
        assert_eq!(content1, "original 1");

        let content2 = fs::read_to_string(workspace.join("subdir/test2.txt")).unwrap();
        assert_eq!(content2, "original 2");
    }

    #[test]
    fn test_restore_profile_creates_directories() {
        let temp = tempdir().unwrap();
        let workspace = temp.path();

        // Create file with subdirectory
        fs::create_dir_all(workspace.join("deep/nested")).unwrap();
        fs::write(workspace.join("deep/nested/file.txt"), "content").unwrap();

        let entry = ProfileLockEntry {
            name: "test-profile".to_string(),
            version: "1.0.0".to_string(),
            installed_at: "2025-01-11".to_string(),
            files: vec!["deep/nested/file.txt".to_string()],
            integrity: "abc123".to_string(),
            commit: None,
            provider_id: None,
            source: None,
        };

        // Create backup
        let backup = backup_profile(workspace, &entry).unwrap();

        // Remove everything
        fs::remove_dir_all(workspace.join("deep")).unwrap();
        assert!(!workspace.join("deep/nested/file.txt").exists());

        // Restore should recreate directories
        restore_profile(&backup).unwrap();
        assert!(workspace.join("deep/nested/file.txt").exists());

        let content = fs::read_to_string(workspace.join("deep/nested/file.txt")).unwrap();
        assert_eq!(content, "content");
    }

    #[test]
    fn test_backup_and_restore_roundtrip() {
        let temp = tempdir().unwrap();
        let workspace = temp.path();

        // Create multiple files
        fs::write(workspace.join("file1.txt"), "data 1").unwrap();
        fs::write(workspace.join("file2.txt"), "data 2").unwrap();
        fs::create_dir_all(workspace.join("dir")).unwrap();
        fs::write(workspace.join("dir/file3.txt"), "data 3").unwrap();

        let entry = ProfileLockEntry {
            name: "profile".to_string(),
            version: "1.0.0".to_string(),
            installed_at: "2025-01-11".to_string(),
            files: vec![
                "file1.txt".to_string(),
                "file2.txt".to_string(),
                "dir/file3.txt".to_string(),
            ],
            integrity: "xyz789".to_string(),
            commit: None,
            provider_id: None,
            source: None,
        };

        // Backup
        let backup = backup_profile(workspace, &entry).unwrap();

        // Corrupt all files
        fs::write(workspace.join("file1.txt"), "corrupted").unwrap();
        fs::write(workspace.join("file2.txt"), "corrupted").unwrap();
        fs::write(workspace.join("dir/file3.txt"), "corrupted").unwrap();

        // Restore
        restore_profile(&backup).unwrap();

        // Verify all files restored correctly
        assert_eq!(
            fs::read_to_string(workspace.join("file1.txt")).unwrap(),
            "data 1"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("file2.txt")).unwrap(),
            "data 2"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("dir/file3.txt")).unwrap(),
            "data 3"
        );
    }
}
