//! Directory management commands (add-dir, remove-dir, list-dirs).

use std::path::{Path, PathBuf};

use crate::config::Settings;

/// Reason why a path was skipped during add operation.
#[derive(Debug)]
pub enum SkipReason {
    /// Path is covered by an existing indexed parent directory.
    CoveredBy(PathBuf),
    /// Path is already present in indexed paths.
    AlreadyPresent,
    /// Path is a file (files are indexed but not persisted to settings).
    FileNotPersisted,
}

/// Represents a path that was skipped during add operation.
#[derive(Debug)]
pub struct SkippedPath {
    pub path: PathBuf,
    pub reason: SkipReason,
}

/// Add paths to settings, returning (updated settings, added paths, skipped paths).
///
/// For `add-dir` (strict=true): Returns error if path already exists.
/// For `index` (strict=false): Silently skips existing paths (idempotent).
pub fn add_paths_to_settings(
    paths: &[PathBuf],
    config_path: &Path,
    strict: bool,
) -> Result<(Settings, Vec<PathBuf>, Vec<SkippedPath>), String> {
    // Load settings from file
    let mut settings = Settings::load_from(config_path)
        .map_err(|e| format!("Error loading configuration: {e}"))?;

    let mut added_paths = Vec::new();
    let mut skipped_paths = Vec::new();

    // Add each path (Settings::add_indexed_path handles deduplication)
    for path in paths {
        if path.is_file() {
            if strict {
                return Err(format!(
                    "Path must be a directory (got file): {}",
                    path.display()
                ));
            }
            skipped_paths.push(SkippedPath {
                path: path.clone(),
                reason: SkipReason::FileNotPersisted,
            });
            continue;
        }

        match settings.add_indexed_path(path.clone()) {
            Ok(_) => {
                added_paths.push(path.clone());
            }
            Err(e) if e.contains("already indexed") => {
                // Path already exists
                if strict {
                    return Err(e);
                }
                // index is idempotent - report and skip
                let reason = path.canonicalize().ok().and_then(|canonical| {
                    settings
                        .indexing
                        .indexed_paths
                        .iter()
                        .find(|existing| canonical.starts_with(existing.as_path()))
                        .map(|existing| SkipReason::CoveredBy(existing.clone()))
                });
                skipped_paths.push(SkippedPath {
                    path: path.clone(),
                    reason: reason.unwrap_or(SkipReason::AlreadyPresent),
                });
            }
            Err(e) => {
                // Other errors (invalid path, etc.) always propagate
                return Err(format!("Error adding path {}: {e}", path.display()));
            }
        }
    }

    // Save only if we added new paths
    if !added_paths.is_empty() {
        settings
            .save(config_path)
            .map_err(|e| format!("Error saving configuration: {e}"))?;
    }

    Ok((settings, added_paths, skipped_paths))
}

/// Run add-dir command.
pub fn run_add_dir(path: PathBuf, cli_config: Option<&Path>) {
    let config_path = resolve_config_path(cli_config);

    match add_paths_to_settings(std::slice::from_ref(&path), &config_path, false) {
        Ok((settings, added_paths, skipped_paths)) => {
            if !added_paths.is_empty() {
                println!("Added directory to indexed paths: {}", path.display());
                println!("Configuration saved to: {}", config_path.display());
            } else if !skipped_paths.is_empty() {
                println!("Directory already in indexed paths: {}", path.display());
            }
            println!("\nCurrent indexed paths:");
            for indexed_path in &settings.indexing.indexed_paths {
                println!("  - {}", indexed_path.display());
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

/// Run remove-dir command.
pub fn run_remove_dir(path: PathBuf, cli_config: Option<&Path>) {
    let config_path = resolve_config_path(cli_config);

    let mut settings = Settings::load_from(&config_path).unwrap_or_else(|e| {
        eprintln!("Error loading configuration: {e}");
        std::process::exit(1);
    });

    match settings.remove_indexed_path(&path) {
        Ok(_) => {
            println!("Removed directory from indexed paths: {}", path.display());

            if let Err(e) = settings.save(&config_path) {
                eprintln!("Error saving configuration: {e}");
                std::process::exit(1);
            }

            println!("Configuration saved to: {}", config_path.display());

            if settings.indexing.indexed_paths.is_empty() {
                println!("\nNo indexed paths configured.");
            } else {
                println!("\nRemaining indexed paths:");
                for indexed_path in &settings.indexing.indexed_paths {
                    println!("  - {}", indexed_path.display());
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

/// Run list-dirs command.
pub fn run_list_dirs(config: &Settings) {
    println!("Indexed directories:");
    if config.indexing.indexed_paths.is_empty() {
        println!("  (none configured)");
        println!("\nTo add directories: codanna add-dir <path>");
    } else {
        for path in &config.indexing.indexed_paths {
            println!("  - {}", path.display());
        }
    }
}

fn resolve_config_path(cli_config: Option<&Path>) -> PathBuf {
    if let Some(custom_path) = cli_config {
        custom_path.to_path_buf()
    } else {
        Settings::find_workspace_config().unwrap_or_else(|| {
            eprintln!("Error: No configuration file found. Run 'codanna init' first.");
            std::process::exit(1);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_add_paths_to_settings_records_skipped_paths() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        let parent = temp_dir.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();

        let settings = Settings::default();
        settings
            .save(&config_path)
            .expect("failed to write initial config");

        // Add parent to config
        let (settings, added, skipped) =
            add_paths_to_settings(std::slice::from_ref(&parent), &config_path, false)
                .expect("parent addition should succeed");
        assert_eq!(added.len(), 1);
        assert!(skipped.is_empty());
        settings
            .save(&config_path)
            .expect("failed to persist updated config");

        // Attempt to add child - should be skipped and report parent coverage
        let (_, added_again, skipped_paths) =
            add_paths_to_settings(std::slice::from_ref(&child), &config_path, false)
                .expect("child addition should be skipped gracefully");
        assert!(added_again.is_empty(), "child path should not be added");
        assert_eq!(skipped_paths.len(), 1);
        let skipped = &skipped_paths[0];
        assert_eq!(skipped.path, child);
        let parent_canonical = parent.canonicalize().unwrap();
        match &skipped.reason {
            SkipReason::CoveredBy(p) => assert_eq!(
                p, &parent_canonical,
                "Expected skipped path to report coverage by parent"
            ),
            other => panic!("Unexpected skip reason: {other:?}"),
        }
    }

    #[test]
    fn test_add_paths_to_settings_skips_files_without_persisting() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        let file_path = temp_dir.path().join("single.rs");
        fs::write(&file_path, "fn main() {}\n").unwrap();

        Settings::default()
            .save(&config_path)
            .expect("failed to write initial config");

        let (settings, added, skipped) =
            add_paths_to_settings(std::slice::from_ref(&file_path), &config_path, false)
                .expect("file addition should succeed");
        assert!(added.is_empty(), "file should not be persisted in config");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].path, file_path);
        assert!(matches!(skipped[0].reason, SkipReason::FileNotPersisted));

        let reloaded = Settings::load_from(&config_path).expect("config reload failed");
        assert!(
            reloaded.indexing.indexed_paths.is_empty(),
            "file path should not be stored in indexed_paths"
        );
        drop(settings); // ensure no unused warnings
    }
}
