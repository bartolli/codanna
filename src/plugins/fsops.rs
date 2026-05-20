//! File system operations for plugin installation

use super::error::{PluginError, PluginResult};
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Copy plugin files to destination with conflict detection
pub fn copy_plugin_files(
    source_dir: &Path,
    dest_dir: &Path,
    plugin_name: &str,
    file_list: &[String],
    force: bool,
    conflict_owner: impl Fn(&Path) -> Option<String>,
) -> PluginResult<Vec<String>> {
    let mut copied_files = Vec::new();

    for file_path in file_list {
        let source_path = source_dir.join(file_path);
        let dest_path = calculate_dest_path(dest_dir, plugin_name, file_path);

        // Check for conflicts
        if dest_path.exists() && !force {
            let owner = conflict_owner(&dest_path).unwrap_or_else(|| "unknown".to_string());
            return Err(PluginError::FileConflict {
                path: dest_path,
                owner,
            });
        }

        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Copy file
        std::fs::copy(&source_path, &dest_path)?;
        let dest_str = dest_path.to_string_lossy().replace('\\', "/");
        copied_files.push(dest_str);
    }

    Ok(copied_files)
}

/// Copy entire plugin payload into the namespaced plugin directory
pub fn copy_plugin_payload(
    source_dir: &Path,
    dest_dir: &Path,
    plugin_name: &str,
    force: bool,
    conflict_owner: impl Fn(&Path) -> Option<String>,
    already_copied: &[String],
) -> PluginResult<Vec<String>> {
    let mut copied_files = Vec::new();
    let plugin_dest_root = dest_dir.join(".claude/plugins").join(plugin_name);
    let already: std::collections::HashSet<_> = already_copied.iter().cloned().collect();

    for entry in WalkDir::new(source_dir).into_iter() {
        let entry = entry.map_err(|e| PluginError::IoError(io::Error::other(e)))?;
        if entry.file_type().is_dir() {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(source_dir)
            .expect("walkdir entry should be under source");

        if relative.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }

        let normalized = relative.to_string_lossy().replace('\\', "/");
        if already.contains(&normalized)
            || normalized.starts_with("commands/")
            || normalized.starts_with("agents/")
            || normalized.starts_with("hooks/")
            || normalized.starts_with("scripts/")
        {
            continue;
        }

        let dest_path = plugin_dest_root.join(relative);

        if dest_path.exists() && !force {
            let owner = conflict_owner(&dest_path).unwrap_or_else(|| "unknown".to_string());
            return Err(PluginError::FileConflict {
                path: dest_path,
                owner,
            });
        }

        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::copy(entry.path(), &dest_path)?;
        let dest_str = dest_path.to_string_lossy().replace('\\', "/");
        copied_files.push(dest_str);
    }

    Ok(copied_files)
}

/// Remove plugin files and clean up empty directories
pub fn remove_plugin_files(file_list: &[String]) -> PluginResult<()> {
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

/// Calculate destination path for a plugin file
pub(crate) fn calculate_dest_path(dest_dir: &Path, plugin_name: &str, file_path: &str) -> PathBuf {
    let trimmed = file_path.trim_start_matches("./");
    let path = Path::new(trimmed);

    if path.starts_with("commands") {
        let relative = path.strip_prefix("commands").unwrap_or(Path::new(""));
        dest_dir
            .join(".claude/commands")
            .join(plugin_name)
            .join(relative)
    } else if path.starts_with("agents") {
        let relative = path.strip_prefix("agents").unwrap_or(Path::new(""));
        dest_dir
            .join(".claude/agents")
            .join(plugin_name)
            .join(relative)
    } else if path.starts_with("hooks") {
        let relative = path.strip_prefix("hooks").unwrap_or(Path::new(""));
        dest_dir
            .join(".claude/hooks")
            .join(plugin_name)
            .join(relative)
    } else if path.starts_with("scripts") {
        let relative = path.strip_prefix("scripts").unwrap_or(Path::new(""));
        dest_dir
            .join(".claude/scripts")
            .join(plugin_name)
            .join(relative)
    } else {
        dest_dir
            .join(".claude/plugins")
            .join(plugin_name)
            .join(path)
    }
}

/// Verify file integrity using checksum
pub fn verify_file_integrity(file_path: &Path, expected_checksum: &str) -> PluginResult<bool> {
    use sha2::{Digest, Sha256};

    let content = std::fs::read(file_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();
    let checksum = hex::encode(result);

    Ok(checksum == expected_checksum)
}

/// Calculate checksum for a set of files
pub fn calculate_integrity(file_paths: &[String]) -> PluginResult<String> {
    use sha2::{Digest, Sha256};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_calculate_dest_path() {
        let dest_dir = Path::new("/project");
        let plugin_name = "test-plugin";

        let cmd_path = calculate_dest_path(dest_dir, plugin_name, "commands/test.md");
        assert_eq!(
            cmd_path,
            PathBuf::from("/project/.claude/commands/test-plugin/test.md")
        );

        let agent_path = calculate_dest_path(dest_dir, plugin_name, "agents/helper.md");
        assert_eq!(
            agent_path,
            PathBuf::from("/project/.claude/agents/test-plugin/helper.md")
        );

        let nested_cmd_path =
            calculate_dest_path(dest_dir, plugin_name, "commands/utils/run/report.md");
        assert_eq!(
            nested_cmd_path,
            PathBuf::from("/project/.claude/commands/test-plugin/utils/run/report.md")
        );
    }

    #[test]
    fn test_copy_and_remove_files() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let source_dir = temp_dir.path().join("source");
        let dest_dir = temp_dir.path().join("dest");

        // Create source files
        fs::create_dir_all(source_dir.join("commands"))?;
        fs::write(source_dir.join("commands/test.md"), "test content")?;

        // Copy files
        let files = vec!["commands/test.md".to_string()];
        let copied =
            copy_plugin_files(&source_dir, &dest_dir, "test-plugin", &files, false, |_| {
                None
            })?;

        assert_eq!(copied.len(), 1);
        assert!(
            dest_dir
                .join(".claude/commands/test-plugin/test.md")
                .exists()
        );

        // Remove files
        remove_plugin_files(&copied)?;
        assert!(
            !dest_dir
                .join(".claude/commands/test-plugin/test.md")
                .exists()
        );

        Ok(())
    }
}
