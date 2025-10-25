//! Profile file installation logic
//!
//! Plugin reference: src/plugins/mod.rs:871-940 (check_file_conflicts)

use super::error::{ProfileError, ProfileResult};
use super::lockfile::ProfileLockfile;
use std::path::{Path, PathBuf};

/// Installation result: (installed files, sidecar files)
/// - installed: List of relative file paths that were installed
/// - sidecars: List of (intended_path, actual_sidecar_path) tuples
pub type InstallResult = (Vec<String>, Vec<(String, String)>);

/// Generate sidecar filename for conflicting files
///
/// Pattern: {stem}.{provider}.{ext}
/// Examples:
/// - CLAUDE.md + "codanna" → CLAUDE.codanna.md
/// - .gitignore + "codanna" → .gitignore.codanna
/// - docker-compose.yml + "codanna" → docker-compose.codanna.yml
pub fn generate_sidecar_path(original: &Path, provider: &str) -> PathBuf {
    let file_name = original
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let parent = original.parent();

    let sidecar_name =
        if file_name.starts_with('.') && file_name.chars().filter(|&c| c == '.').count() == 1 {
            // Dotfile without extension: .gitignore → .gitignore.codanna
            format!("{file_name}.{provider}")
        } else if let Some(dot_pos) = file_name.find('.') {
            // Has extension: split on first dot
            let (stem, ext) = file_name.split_at(dot_pos);
            format!("{stem}.{provider}{ext}")
        } else {
            // No extension
            format!("{file_name}.{provider}")
        };

    if let Some(p) = parent {
        p.join(sidecar_name)
    } else {
        PathBuf::from(sidecar_name)
    }
}

/// Check for file conflicts before profile installation
///
/// Verifies that no files will be overwritten unless they belong to the same profile
/// or force mode is enabled. This prevents accidental overwrites of files from other
/// profiles or untracked files.
///
/// Plugin reference: src/plugins/mod.rs:871-940
pub fn check_profile_conflicts(
    workspace: &Path,
    profile_name: &str,
    files: &[String],
    force: bool,
) -> ProfileResult<()> {
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    let lockfile = ProfileLockfile::load(&lockfile_path)?;

    for file_path in files {
        let dest = workspace.join(file_path);

        // Only check if destination already exists
        if dest.exists() {
            // Resolve owner from lockfile
            match lockfile.find_file_owner(file_path) {
                Some(owner) if owner != profile_name && !force => {
                    // File owned by different profile
                    return Err(ProfileError::FileConflict {
                        path: file_path.to_string(),
                        owner: owner.to_string(),
                    });
                }
                None if !force => {
                    // File exists but not tracked (unknown owner)
                    return Err(ProfileError::FileConflict {
                        path: file_path.to_string(),
                        owner: "unknown".to_string(),
                    });
                }
                _ => {
                    // Same owner or force mode enabled - allow overwrite
                }
            }
        }
    }

    Ok(())
}

/// Handles file installation for profiles
#[derive(Debug, Clone)]
pub struct ProfileInstaller;

impl ProfileInstaller {
    /// Create a new installer
    pub fn new() -> Self {
        Self
    }

    /// Install files from source to destination
    ///
    /// Conflict resolution based on ownership and force flag:
    /// - Same profile owns file: Overwrite (upgrade)
    /// - Different profile/unknown owner WITHOUT force: Error
    /// - Different profile/unknown owner WITH force: Create sidecar {stem}.{provider}.{ext}
    ///
    /// Returns tuple: (installed_files, sidecar_files)
    /// - installed_files: Successfully installed to intended paths
    /// - sidecar_files: (intended_path, sidecar_path) for conflicts
    pub fn install_files(
        &self,
        source_dir: &Path,
        dest_dir: &Path,
        files: &[String],
        profile_name: &str,
        provider_name: &str,
        lockfile: &ProfileLockfile,
        force: bool,
    ) -> ProfileResult<InstallResult> {
        let mut installed = Vec::new();
        let mut sidecars = Vec::new();

        for file_path in files {
            let source_path = source_dir.join(file_path);

            // Skip if source doesn't exist
            if !source_path.exists() {
                continue;
            }

            let dest_path = dest_dir.join(file_path);

            // Check if destination exists and who owns it
            let use_sidecar = if dest_path.exists() {
                match lockfile.find_file_owner(file_path) {
                    Some(owner) if owner == profile_name => {
                        // We own it - overwrite (upgrade)
                        false
                    }
                    Some(owner) => {
                        // Different profile owns it
                        if !force {
                            return Err(ProfileError::FileConflict {
                                path: file_path.clone(),
                                owner: owner.to_string(),
                            });
                        }
                        // Force enabled - use sidecar
                        true
                    }
                    None => {
                        // Unknown owner
                        if !force {
                            return Err(ProfileError::FileConflict {
                                path: file_path.clone(),
                                owner: "unknown".to_string(),
                            });
                        }
                        // Force enabled - use sidecar
                        true
                    }
                }
            } else {
                false // Doesn't exist - install normally
            };

            let (final_path, relative_path) = if use_sidecar {
                // Generate sidecar path
                let sidecar_path = generate_sidecar_path(&dest_path, provider_name);
                let sidecar_relative = generate_sidecar_path(Path::new(file_path), provider_name);
                (sidecar_path, sidecar_relative.to_string_lossy().to_string())
            } else {
                (dest_path.clone(), file_path.clone())
            };

            // Create parent directory if needed
            if let Some(parent) = final_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Copy file
            std::fs::copy(&source_path, &final_path)?;

            if use_sidecar {
                sidecars.push((file_path.clone(), relative_path.clone()));
                installed.push(relative_path);
            } else {
                installed.push(file_path.clone());
            }
        }

        Ok((installed, sidecars))
    }
}

impl Default for ProfileInstaller {
    fn default() -> Self {
        Self::new()
    }
}
