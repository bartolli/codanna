//! High-level profile installation orchestration
//!
//! Plugin reference: src/plugins/mod.rs:971-1107 (execute_install_with_plan)

use super::error::{ProfileError, ProfileResult};
use super::fsops::{
    ProfileBackup, backup_profile, calculate_integrity, collect_all_files, restore_profile,
};
use super::installer::{self, ProfileInstaller};
use super::lockfile::{ProfileLockEntry, ProfileLockfile};
use super::manifest::ProfileManifest;
use super::project::ProfilesConfig;
use super::provider_registry::ProviderSource;
use std::path::Path;

/// Rollback failures are collected, not swallowed: a failed restore means
/// the user's backed-up files were not returned to the workspace, and a
/// failed lockfile save leaves disk state claiming the install succeeded.
fn fail_with_rollback(
    primary: ProfileError,
    backup: Option<&ProfileBackup>,
    lockfile_rollback: Option<(&mut ProfileLockfile, &Path, &str)>,
) -> ProfileError {
    let mut failures = Vec::new();

    if let Some((lockfile, path, profile_name)) = lockfile_rollback {
        lockfile.remove_profile(profile_name);
        if let Err(e) = lockfile.save(path) {
            failures.push(format!("lockfile rollback save failed: {e}"));
        }
    }

    if let Some(b) = backup {
        if let Err(e) = restore_profile(b) {
            failures.push(format!("backup restore failed: {e}"));
        }
    }

    if failures.is_empty() {
        return primary;
    }
    for failure in &failures {
        eprintln!("Warning: {failure}");
    }
    ProfileError::RollbackFailed {
        primary: Box::new(primary),
        failures,
    }
}

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
/// - `commit`: Optional git commit SHA if installed from git source
/// - `provider_id`: Optional provider ID for team config tracking
///
/// # Dependencies
/// - ProfileManifest: Profile definition with files to install
/// - ProfileInstaller: Handles file copying
/// - ProfilesConfig: Team contract tracking which profiles are active
/// - ProfileLockfile: Tracks installed files for integrity checking
///
/// Plugin reference: src/plugins/mod.rs:971-1107
pub fn install_profile(
    profile_name: &str,
    profiles_dir: &Path,
    workspace: &Path,
    force: bool,
    commit: Option<String>,
    provider_id: Option<&str>,
    source: Option<ProviderSource>,
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

    // 3. Determine files to install
    // If manifest.files is empty, install all files from profile directory
    let files_to_install = if manifest.files.is_empty() {
        collect_all_files(&profile_dir)?
    } else {
        manifest.files.clone()
    };

    // 4. Pre-flight check: Validate ALL conflicts before copying ANY files
    // This ensures atomic behavior - we fail fast before touching the filesystem
    installer::check_all_conflicts(workspace, &files_to_install, profile_name, &lockfile, force)?;

    // 5. Install files (conflicts already validated, safe to proceed)
    let installer = ProfileInstaller::new();
    let provider_name = manifest.provider_name();
    let (installed_files, sidecars) = match installer.install_files(
        &profile_dir,
        workspace,
        &files_to_install,
        profile_name,
        provider_name,
        &lockfile,
        force,
    ) {
        Ok(result) => result,
        Err(e) => {
            return Err(fail_with_rollback(e, backup.as_ref(), None));
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
            return Err(fail_with_rollback(e, backup.as_ref(), None));
        }
    };

    // 6. Create lockfile entry
    let entry = ProfileLockEntry {
        name: profile_name.to_string(),
        version: manifest.version.clone(),
        installed_at: current_timestamp(),
        files: installed_files,
        integrity,
        commit,
        provider_id: provider_id.map(String::from),
        source,
    };

    // 7. Update lockfile (with rollback on error)
    lockfile.add_profile(entry);
    if let Err(e) = lockfile.save(&lockfile_path) {
        // The failed save is not retried; only in-memory removal + restore.
        lockfile.remove_profile(profile_name);
        return Err(fail_with_rollback(e, backup.as_ref(), None));
    }

    // 8. Update team profiles configuration (with rollback on error)
    let profiles_config_path = workspace.join(".codanna/profiles.json");
    let mut profiles_config = ProfilesConfig::load(&profiles_config_path)?;

    // Build profile reference (name@provider or just name)
    let profile_ref = if let Some(provider) = provider_id {
        format!("{profile_name}@{provider}")
    } else {
        profile_name.to_string()
    };

    profiles_config.add_profile(&profile_ref);

    if let Err(e) = profiles_config.save(&profiles_config_path) {
        return Err(fail_with_rollback(
            e,
            backup.as_ref(),
            Some((&mut lockfile, &lockfile_path, profile_name)),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: profiles.json save failure (step 8) must roll back the
    // persisted lockfile and surface the error instead of swallowing the
    // rollback outcome.
    #[test]
    #[cfg(unix)]
    fn profiles_config_save_failure_rolls_back_lockfile() {
        use std::os::unix::fs::PermissionsExt;

        let profiles_dir = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();

        let pdir = profiles_dir.path().join("demo");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("profile.json"),
            r#"{"name":"demo","version":"0.1.0","files":["hello.md"]}"#,
        )
        .unwrap();
        std::fs::write(pdir.join("hello.md"), "hi").unwrap();

        // Read-only profiles.json forces the step-8 save to fail
        let codanna_dir = workspace.path().join(".codanna");
        std::fs::create_dir_all(&codanna_dir).unwrap();
        let profiles_json = codanna_dir.join("profiles.json");
        std::fs::write(&profiles_json, r#"{"version":1,"profiles":[]}"#).unwrap();
        let mut perms = std::fs::metadata(&profiles_json).unwrap().permissions();
        perms.set_mode(0o444);
        std::fs::set_permissions(&profiles_json, perms).unwrap();

        let result = install_profile(
            "demo",
            profiles_dir.path(),
            workspace.path(),
            false,
            None,
            None,
            None,
        );
        assert!(result.is_err());

        // Rollback persisted: lockfile no longer claims the profile
        let lockfile =
            ProfileLockfile::load(&workspace.path().join(".codanna/profiles.lock.json")).unwrap();
        assert!(lockfile.get_profile("demo").is_none());
    }
}
