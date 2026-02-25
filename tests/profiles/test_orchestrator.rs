//! Tests for profile installation orchestration

use codanna::profiles::error::ProfileError;
use codanna::profiles::lockfile::ProfileLockfile;
use codanna::profiles::orchestrator::install_profile;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_install_profile_creates_structure() {
    let temp = tempdir().unwrap();

    // Create profile source
    let profiles_dir = temp.path().join("profiles");
    let claude_dir = profiles_dir.join("claude");
    fs::create_dir_all(&claude_dir).unwrap();

    // Create profile.json
    let manifest_json = r#"{
        "name": "claude",
        "version": "1.0.0",
        "files": ["CLAUDE.md"]
    }"#;
    fs::write(claude_dir.join("profile.json"), manifest_json).unwrap();

    // Create file to install
    fs::write(claude_dir.join("CLAUDE.md"), "# Claude").unwrap();

    // Workspace
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Install
    install_profile("claude", &profiles_dir, &workspace, false, None, None, None).unwrap();

    // Verify file installed
    assert!(workspace.join("CLAUDE.md").exists());

    // Verify lockfile created
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    assert!(lockfile_path.exists());

    // Verify profile entry in lockfile
    let lockfile = ProfileLockfile::load(&lockfile_path).unwrap();
    assert!(lockfile.get_profile("claude").is_some());
}

#[test]
fn test_install_profile_not_found() {
    let temp = tempdir().unwrap();
    let profiles_dir = temp.path().join("profiles");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&profiles_dir).unwrap();
    fs::create_dir_all(&workspace).unwrap();

    let result = install_profile(
        "nonexistent",
        &profiles_dir,
        &workspace,
        false,
        None,
        None,
        None,
    );
    assert!(result.is_err());
}

#[test]
fn test_install_profile_updates_manifest() {
    let temp = tempdir().unwrap();

    // Setup profile
    let profiles_dir = temp.path().join("profiles");
    let claude_dir = profiles_dir.join("claude");
    fs::create_dir_all(&claude_dir).unwrap();

    let manifest_json = r#"{
        "name": "claude",
        "version": "1.0.0",
        "files": []
    }"#;
    fs::write(claude_dir.join("profile.json"), manifest_json).unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Install
    install_profile("claude", &profiles_dir, &workspace, false, None, None, None).unwrap();

    // Verify lockfile content
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    let lockfile = ProfileLockfile::load(&lockfile_path).unwrap();

    // Verify profile entry exists with correct name
    let entry = lockfile.get_profile("claude").unwrap();
    assert_eq!(entry.name, "claude");
    assert_eq!(entry.version, "1.0.0");
}

#[test]
fn test_install_profile_already_installed() {
    let temp = tempdir().unwrap();

    // Setup profile
    let profiles_dir = temp.path().join("profiles");
    let claude_dir = profiles_dir.join("claude");
    fs::create_dir_all(&claude_dir).unwrap();

    let manifest_json = r#"{
        "name": "claude",
        "version": "1.0.0",
        "files": ["CLAUDE.md"]
    }"#;
    fs::write(claude_dir.join("profile.json"), manifest_json).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), "# Claude").unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Install first time
    install_profile("claude", &profiles_dir, &workspace, false, None, None, None).unwrap();

    // Try to install again without force
    let result = install_profile("claude", &profiles_dir, &workspace, false, None, None, None);
    assert!(result.is_err());
    match result {
        Err(ProfileError::AlreadyInstalled { name, version }) => {
            assert_eq!(name, "claude");
            assert_eq!(version, "1.0.0");
        }
        _ => panic!("Expected AlreadyInstalled error"),
    }
}

#[test]
fn test_install_profile_with_force() {
    let temp = tempdir().unwrap();

    // Setup profile
    let profiles_dir = temp.path().join("profiles");
    let claude_dir = profiles_dir.join("claude");
    fs::create_dir_all(&claude_dir).unwrap();

    let manifest_json = r#"{
        "name": "claude",
        "version": "1.0.0",
        "files": ["CLAUDE.md"]
    }"#;
    fs::write(claude_dir.join("profile.json"), manifest_json).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), "# Original").unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Install first time
    install_profile("claude", &profiles_dir, &workspace, false, None, None, None).unwrap();
    let content1 = fs::read_to_string(workspace.join("CLAUDE.md")).unwrap();
    assert_eq!(content1, "# Original");

    // Update source file
    fs::write(claude_dir.join("CLAUDE.md"), "# Updated").unwrap();

    // Install again with force
    install_profile("claude", &profiles_dir, &workspace, true, None, None, None).unwrap();
    let content2 = fs::read_to_string(workspace.join("CLAUDE.md")).unwrap();
    assert_eq!(content2, "# Updated");
}

#[test]
fn test_install_profile_calculates_integrity() {
    let temp = tempdir().unwrap();

    // Setup profile
    let profiles_dir = temp.path().join("profiles");
    let claude_dir = profiles_dir.join("claude");
    fs::create_dir_all(&claude_dir).unwrap();

    let manifest_json = r#"{
        "name": "claude",
        "version": "1.0.0",
        "files": ["CLAUDE.md"]
    }"#;
    fs::write(claude_dir.join("profile.json"), manifest_json).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), "# Claude").unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Install
    install_profile("claude", &profiles_dir, &workspace, false, None, None, None).unwrap();

    // Verify integrity was calculated
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    let lockfile = ProfileLockfile::load(&lockfile_path).unwrap();
    let entry = lockfile.get_profile("claude").unwrap();

    // Should have non-empty integrity hash
    assert!(!entry.integrity.is_empty());
    assert_eq!(entry.integrity.len(), 64); // SHA-256 hex = 64 chars
}

#[test]
fn test_install_profile_conflict_creates_sidecar() {
    let temp = tempdir().unwrap();

    // Setup two profiles that conflict
    let profiles_dir = temp.path().join("profiles");

    // Profile A
    let profile_a = profiles_dir.join("profile-a");
    fs::create_dir_all(&profile_a).unwrap();
    fs::write(
        profile_a.join("profile.json"),
        r#"{"name": "profile-a", "version": "1.0.0", "files": ["CLAUDE.md"]}"#,
    )
    .unwrap();
    fs::write(profile_a.join("CLAUDE.md"), "# Profile A").unwrap();

    // Profile B (conflicts with A)
    let profile_b = profiles_dir.join("profile-b");
    fs::create_dir_all(&profile_b).unwrap();
    fs::write(
        profile_b.join("profile.json"),
        r#"{"name": "profile-b", "version": "1.0.0", "files": ["CLAUDE.md"]}"#,
    )
    .unwrap();
    fs::write(profile_b.join("CLAUDE.md"), "# Profile B").unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    // Install profile A
    install_profile(
        "profile-a",
        &profiles_dir,
        &workspace,
        false,
        None,
        None,
        None,
    )
    .unwrap();

    // Install profile B with --force (should create sidecar for conflict)
    install_profile(
        "profile-b",
        &profiles_dir,
        &workspace,
        true,
        None,
        None,
        None,
    )
    .unwrap();

    // Verify profile A file preserved
    let content_a = fs::read_to_string(workspace.join("CLAUDE.md")).unwrap();
    assert_eq!(content_a, "# Profile A");

    // Verify profile B created sidecar
    let sidecar_path = workspace.join("CLAUDE.profile-b.md");
    assert!(sidecar_path.exists());
    let content_b = fs::read_to_string(&sidecar_path).unwrap();
    assert_eq!(content_b, "# Profile B");
}

#[cfg(unix)]
#[test]
fn test_install_profile_supports_symlinked_directory() {
    let temp = tempdir().unwrap();

    let profiles_dir = temp.path().join("profiles");
    let profile_dir = profiles_dir.join("symlink-profile");
    fs::create_dir_all(profile_dir.join("skills")).unwrap();

    fs::write(
        profile_dir.join("profile.json"),
        r#"{"name": "symlink-profile", "version": "1.0.0", "files": []}"#,
    )
    .unwrap();

    let shared_skill_dir = temp.path().join("shared-skills").join("sample-skill");
    fs::create_dir_all(&shared_skill_dir).unwrap();
    fs::write(shared_skill_dir.join("SKILL.md"), "# Sample Skill").unwrap();

    std::os::unix::fs::symlink(
        &shared_skill_dir,
        profile_dir.join("skills").join("sample-skill"),
    )
    .unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    install_profile(
        "symlink-profile",
        &profiles_dir,
        &workspace,
        false,
        None,
        None,
        None,
    )
    .unwrap();

    let installed_skill = workspace.join("skills/sample-skill/SKILL.md");
    assert!(installed_skill.exists());
    assert_eq!(
        fs::read_to_string(installed_skill).unwrap(),
        "# Sample Skill"
    );

    let lockfile = ProfileLockfile::load(&workspace.join(".codanna/profiles.lock.json")).unwrap();
    let entry = lockfile.get_profile("symlink-profile").unwrap();
    assert!(
        entry
            .files
            .contains(&"skills/sample-skill/SKILL.md".to_string())
    );
}

#[cfg(unix)]
#[test]
fn test_force_reinstall_symlinked_directory_overwrites_owned_files() {
    let temp = tempdir().unwrap();

    let profiles_dir = temp.path().join("profiles");
    let profile_dir = profiles_dir.join("symlink-profile");
    fs::create_dir_all(profile_dir.join("skills")).unwrap();

    fs::write(
        profile_dir.join("profile.json"),
        r#"{"name": "symlink-profile", "version": "1.0.0", "files": []}"#,
    )
    .unwrap();

    let shared_skill_dir = temp.path().join("shared-skills").join("sample-skill");
    fs::create_dir_all(&shared_skill_dir).unwrap();
    let skill_file = shared_skill_dir.join("SKILL.md");
    fs::write(&skill_file, "# V1").unwrap();

    std::os::unix::fs::symlink(
        &shared_skill_dir,
        profile_dir.join("skills").join("sample-skill"),
    )
    .unwrap();

    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();

    install_profile(
        "symlink-profile",
        &profiles_dir,
        &workspace,
        false,
        None,
        None,
        None,
    )
    .unwrap();

    fs::write(&skill_file, "# V2").unwrap();
    install_profile(
        "symlink-profile",
        &profiles_dir,
        &workspace,
        true,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(workspace.join("skills/sample-skill/SKILL.md")).unwrap(),
        "# V2"
    );
    assert!(
        !workspace
            .join("skills/sample-skill.symlink-profile/SKILL.md")
            .exists()
    );
}
