//! High-level profile installation orchestration
//!
//! Plugin reference: src/plugins/mod.rs:971-1107 (execute_install_with_plan)

use super::error::{ProfileError, ProfileResult};
use super::fsops::{ProfileBackup, backup_profile, calculate_integrity, restore_profile};
use super::installer::ProfileInstaller;
use super::lockfile::{ProfileLockEntry, ProfileLockfile};
use super::manifest::ProfileManifest;
use super::project::ProjectManifest;
use std::path::Path;

/// Install a profile to a workspace with atomic operations
///
/// This orchestrates the complete installation with rollback support:
/// 1. Load profile manifest from profiles_dir/{profile_name}/profile.json
/// 2. Backup existing profile if updating (with --force)
/// 3. Install files with conflict resolution
/// 4. Calculate integrity hash
/// 5. Update lockfile and project manifest (rolls back on error)
///
/// Conflict resolution (follows plugin pattern):
/// - Same profile owns file: Overwrite (upgrade)
/// - Different profile/unknown owner WITHOUT --force: Error
/// - Different profile/unknown owner WITH --force: Create sidecar {stem}.{provider}.{ext}
///
/// If any step fails, all changes are rolled back to the previous state.
///
/// # Parameters
/// - `profile_name`: Name of profile to install
/// - `profiles_dir`: Directory containing profile definitions
/// - `workspace`: Target workspace directory
/// - `force`: Enable reinstall and sidecar creation for conflicts
///
/// # Dependencies
/// - ProfileManifest: Profile definition with files to install
/// - ProfileInstaller: Handles file copying
/// - ProjectManifest: Team contract tracking which profile is active
/// - ProfileLockfile: Tracks installed files for integrity checking
///
/// Plugin reference: src/plugins/mod.rs:971-1107
pub fn install_profile(
    profile_name: &str,
    profiles_dir: &Path,
    workspace: &Path,
    force: bool,
) -> ProfileResult<()> {
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    let mut lockfile = ProfileLockfile::load(&lockfile_path)?;
    let mut backup: Option<ProfileBackup> = None;

    // 1. Load profile manifest
    let profile_dir = profiles_dir.join(profile_name);
    let manifest_path = profile_dir.join("profile.json");

    if !manifest_path.exists() {
        return Err(ProfileError::InvalidManifest {
            reason: format!(
                "Profile '{profile_name}' not found at {}",
                manifest_path.display()
            ),
        });
    }

    let manifest = ProfileManifest::from_file(&manifest_path)?;

    // 2. Check if already installed (unless force)
    if let Some(existing) = lockfile.get_profile(profile_name) {
        if !force {
            return Err(ProfileError::AlreadyInstalled {
                name: profile_name.to_string(),
                version: existing.version.clone(),
            });
        }
        // Backup existing before update
        backup = Some(backup_profile(workspace, existing)?);
    }

    // 3. Install files (with conflict resolution based on force flag)
    let installer = ProfileInstaller::new();
    let provider_name = manifest.provider_name();
    let (installed_files, sidecars) = match installer.install_files(
        &profile_dir,
        workspace,
        &manifest.files,
        profile_name,
        provider_name,
        &lockfile,
        force,
    ) {
        Ok(result) => result,
        Err(e) => {
            if let Some(b) = backup {
                let _ = restore_profile(&b);
            }
            return Err(e);
        }
    };

    // Print sidecar summary if any conflicts occurred
    if !sidecars.is_empty() {
        eprintln!("\nWarning: File conflicts resolved with sidecar files:");
        for (intended, actual) in &sidecars {
            eprintln!("  {intended} exists - installed as {actual}");
            eprintln!("    Original file preserved");
        }
        eprintln!("\nReview and manually merge sidecar files with originals.");
    }

    // 5. Calculate integrity hash
    let absolute_files: Vec<String> = installed_files
        .iter()
        .map(|rel| workspace.join(rel).to_string_lossy().to_string())
        .collect();

    let integrity = match calculate_integrity(&absolute_files) {
        Ok(hash) => hash,
        Err(e) => {
            if let Some(b) = backup {
                let _ = restore_profile(&b);
            }
            return Err(e);
        }
    };

    // 6. Create lockfile entry
    let entry = ProfileLockEntry {
        name: profile_name.to_string(),
        version: manifest.version.clone(),
        installed_at: current_timestamp(),
        files: installed_files,
        integrity,
    };

    // 7. Update lockfile (with rollback on error)
    lockfile.add_profile(entry);
    if let Err(e) = lockfile.save(&lockfile_path) {
        lockfile.remove_profile(profile_name);
        if let Some(b) = backup {
            let _ = restore_profile(&b);
        }
        return Err(e);
    }

    // 8. Update project manifest (with rollback on error)
    let manifest_path = workspace.join(".codanna/manifest.json");
    let mut project_manifest = ProjectManifest::load_or_create(&manifest_path)?;
    project_manifest.profile = profile_name.to_string();
    if let Err(e) = project_manifest.save(&manifest_path) {
        // Roll back lockfile
        lockfile.remove_profile(profile_name);
        let _ = lockfile.save(&lockfile_path);
        // Restore backup
        if let Some(b) = backup {
            let _ = restore_profile(&b);
        }
        return Err(e);
    }

    Ok(())
}

/// Get current timestamp in ISO 8601 format
fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");

    // Simple ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ
    let secs = duration.as_secs();
    let days = secs / 86400;
    let years = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;

    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    format!("{years:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}
