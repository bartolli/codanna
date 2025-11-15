//! プロファイルシステム
//!
//! プロジェクト初期化のためのプロファイルシステムを提供します。
//! テンプレートベースのプロジェクトセットアップを可能にします。
//!
//! # 主要な機能
//!
//! - プロファイルのインストールと管理
//! - プロジェクトマニフェストの処理
//! - ロックファイル管理
//! - ローカルオーバーライド
//!
//! # 使用例
//!
//! ```no_run
//! // use codanna::profiles::installer::ProfileInstaller;
//! // let installer = ProfileInstaller::new();
//! // installer.install_profile("typescript-project");
//! ```

pub mod commands;
pub mod error;
pub mod fsops;
pub mod git;
pub mod installer;
pub mod local;
pub mod lockfile;
pub mod manifest;
pub mod orchestrator;
pub mod project;
pub mod provider;
pub mod provider_registry;
pub mod reference;
pub mod resolver;
pub mod source_resolver;
pub mod template;
pub mod variables;
pub mod verification;

use error::ProfileResult;
use orchestrator::install_profile;
use provider::ProviderManifest;
use provider_registry::{ProviderRegistry, ProviderSource};
use reference::ProfileReference;
use source_resolver::resolve_profile_source;
use std::path::{Path, PathBuf};

/// Get the default profiles directory
/// Returns ~/.codanna/profiles/
pub fn profiles_dir() -> PathBuf {
    crate::init::global_dir().join("profiles")
}

/// Get the provider registry path
/// Returns ~/.codanna/providers.json
pub fn provider_registry_path() -> PathBuf {
    crate::init::global_dir().join("providers.json")
}

/// Initialize a profile to the current workspace
///
/// This is the public API for the `codanna profile init` command.
pub fn init_profile(profile_name: &str, source: Option<&Path>, force: bool) -> ProfileResult<()> {
    let workspace = std::env::current_dir()?;
    let profiles_dir = source.map(|p| p.to_path_buf()).unwrap_or_else(profiles_dir);

    if force {
        println!(
            "Installing profile '{profile_name}' with --force (will overwrite conflicting files)..."
        );
    } else {
        println!("Installing profile '{profile_name}' to workspace...");
    }

    install_profile(
        profile_name,
        &profiles_dir,
        &workspace,
        force,
        None,
        None,
        None,
    )?;

    println!("\nProfile '{profile_name}' installed successfully");
    if force {
        println!("  Note: Conflicting files handled with --force");
        println!("  Use 'codanna profile verify {profile_name}' to check integrity");
    }

    Ok(())
}

/// Add a provider to the global registry
///
/// This is the public API for the `codanna profile provider add` command.
pub fn add_provider(source: &str, provider_id: Option<&str>) -> ProfileResult<()> {
    let registry_path = provider_registry_path();
    let mut registry = ProviderRegistry::load(&registry_path)?;

    // Parse source (GitHub shorthand, git URL, or local path)
    let provider_source = ProviderSource::parse(source);

    // Determine provider ID (user-specified or derive from source)
    let id = provider_id
        .map(String::from)
        .unwrap_or_else(|| derive_provider_id(&provider_source));

    // Check if already registered
    if registry.get_provider(&id).is_some() {
        println!("Provider '{id}' is already registered");
        println!("Use --force to update or remove it first");
        return Ok(());
    }

    // For now, we'll load the manifest from local path
    // TODO: Clone git repos to temp directory and load manifest
    let manifest = load_provider_manifest(&provider_source)?;

    // Add to registry
    registry.add_provider(id.clone(), &manifest, provider_source);
    registry.save(&registry_path)?;

    println!(
        "Added provider '{id}' ({} profiles available)",
        manifest.profiles.len()
    );
    for profile in &manifest.profiles {
        println!("  - {}", profile.name);
    }

    Ok(())
}

/// Remove a provider from the global registry
///
/// This is the public API for the `codanna profile provider remove` command.
pub fn remove_provider(provider_id: &str) -> ProfileResult<()> {
    let registry_path = provider_registry_path();
    let mut registry = ProviderRegistry::load(&registry_path)?;

    if registry.remove_provider(provider_id) {
        registry.save(&registry_path)?;
        println!("Removed provider '{provider_id}'");
    } else {
        println!("Provider '{provider_id}' not found");
    }

    Ok(())
}

/// List registered providers
///
/// This is the public API for the `codanna profile provider list` command.
pub fn list_providers(verbose: bool) -> ProfileResult<()> {
    let registry_path = provider_registry_path();
    let registry = ProviderRegistry::load(&registry_path)?;

    if registry.providers.is_empty() {
        println!("No providers registered");
        println!("\nAdd a provider with:");
        println!("  codanna profile provider add <source>");
        return Ok(());
    }

    println!("Registered providers:");
    for (id, provider) in &registry.providers {
        println!("\n{id}:");
        println!("  Name: {}", provider.name);
        match &provider.source {
            ProviderSource::Github { repo } => println!("  Source: github:{repo}"),
            ProviderSource::Url { url } => println!("  Source: {url}"),
            ProviderSource::Local { path } => println!("  Source: {path}"),
        }
        if verbose {
            println!("  Profiles ({}):", provider.profiles.len());
            for (name, info) in &provider.profiles {
                print!("    - {name} ({})", info.version);
                if let Some(desc) = &info.description {
                    print!(": {desc}");
                }
                println!();
            }
        } else {
            println!("  Profiles: {}", provider.profiles.len());
        }
    }

    Ok(())
}

/// Derive a provider ID from the source
fn derive_provider_id(source: &ProviderSource) -> String {
    match source {
        ProviderSource::Github { repo } => {
            // Extract last part: "codanna/claude-provider" → "claude-provider"
            repo.split('/').next_back().unwrap_or(repo).to_string()
        }
        ProviderSource::Url { url } => {
            // Extract repo name from URL
            url.trim_end_matches(".git")
                .split('/')
                .next_back()
                .unwrap_or("provider")
                .to_string()
        }
        ProviderSource::Local { path } => {
            // Use directory name
            Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("local")
                .to_string()
        }
    }
}

/// Load provider manifest from source
fn load_provider_manifest(source: &ProviderSource) -> ProfileResult<ProviderManifest> {
    match source {
        ProviderSource::Local { path } => {
            let manifest_path = Path::new(path).join(".codanna-profile/provider.json");
            ProviderManifest::from_file(&manifest_path)
        }
        ProviderSource::Github { repo } => {
            let url = format!("https://github.com/{repo}.git");
            load_manifest_from_git(&url)
        }
        ProviderSource::Url { url } => load_manifest_from_git(url),
    }
}

/// Load provider manifest from git repository
fn load_manifest_from_git(url: &str) -> ProfileResult<ProviderManifest> {
    let temp_dir = tempfile::tempdir()?;
    git::clone_repository(url, temp_dir.path(), None)?;
    let manifest_path = temp_dir.path().join(".codanna-profile/provider.json");
    ProviderManifest::from_file(&manifest_path)
}

/// Verify integrity of a specific profile
///
/// This is the public API for the `codanna profile verify` command.
pub fn verify_profile(profile_name: &str, verbose: bool) -> ProfileResult<()> {
    let workspace = std::env::current_dir()?;
    verification::verify_profile(&workspace, profile_name, verbose)
}

/// Verify all installed profiles
///
/// This is the public API for the `codanna profile verify --all` command.
pub fn verify_all_profiles(verbose: bool) -> ProfileResult<()> {
    let workspace = std::env::current_dir()?;
    verification::verify_all_profiles(&workspace, verbose)
}

/// List available profiles from all providers
///
/// This is the public API for the `codanna profile list` command.
pub fn list_profiles(verbose: bool, json: bool) -> ProfileResult<()> {
    use provider_registry::ProviderRegistry;

    let registry_path = provider_registry_path();
    let registry = ProviderRegistry::load(&registry_path)?;

    if registry.providers.is_empty() {
        println!("No providers registered");
        println!("\nAdd a provider with:");
        println!("  codanna profile provider add <source>");
        return Ok(());
    }

    if json {
        // JSON output
        let output = serde_json::to_string_pretty(&registry.providers)?;
        println!("{output}");
        return Ok(());
    }

    // Human-readable output
    println!("Available profiles:\n");

    for (provider_id, provider) in &registry.providers {
        println!("From provider '{provider_id}':");

        if provider.profiles.is_empty() {
            println!("  (no profiles)");
        } else {
            for (profile_name, profile_info) in &provider.profiles {
                print!("  - {profile_name} (v{})", profile_info.version);
                if verbose {
                    if let Some(desc) = &profile_info.description {
                        print!(": {desc}");
                    }
                }
                println!();
            }
        }
        println!();
    }

    Ok(())
}

/// Show status of installed profiles
///
/// This is the public API for the `codanna profile status` command.
pub fn show_status(verbose: bool) -> ProfileResult<()> {
    use lockfile::ProfileLockfile;
    use project::ProfilesConfig;

    let workspace = std::env::current_dir()?;
    let profiles_config_path = workspace.join(".codanna/profiles.json");
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");

    // Check for team configuration
    if profiles_config_path.exists() {
        let profiles_config = ProfilesConfig::load(&profiles_config_path)?;

        if !profiles_config.is_empty() {
            // Load global registry to check which providers are registered
            let registry_path = provider_registry_path();
            let registry = ProviderRegistry::load(&registry_path)?;

            // Get required providers
            let required_providers = profiles_config.get_required_provider_ids();

            // Check which providers are missing
            let missing_providers: Vec<String> = required_providers
                .iter()
                .filter(|id| registry.get_provider(id).is_none())
                .cloned()
                .collect();

            // Check which profiles are missing
            let lockfile = lockfile::ProfileLockfile::load(&lockfile_path).unwrap_or_default();
            let missing_profiles: Vec<String> = profiles_config
                .profiles
                .iter()
                .filter(|profile_ref| {
                    let reference = ProfileReference::parse(profile_ref);
                    lockfile.get_profile(&reference.profile).is_none()
                })
                .cloned()
                .collect();

            // Show team config detection if there are missing providers or profiles
            if !missing_providers.is_empty() || !missing_profiles.is_empty() {
                println!("Team profile configuration detected at .codanna/profiles.json");
                if !missing_providers.is_empty() {
                    println!("  Missing providers: {}", missing_providers.join(", "));
                }
                if !missing_profiles.is_empty() {
                    println!("  Missing profiles: {}", missing_profiles.join(", "));
                }
                println!();
                println!("Run 'codanna profile sync' to register providers and install profiles");
                println!();
            }
        }
    }

    // Check if lockfile exists
    if !lockfile_path.exists() {
        println!("No profiles installed");
        return Ok(());
    }

    // Load lockfile
    let lockfile = ProfileLockfile::load(&lockfile_path)?;

    if lockfile.profiles.is_empty() {
        println!("No profiles installed");
        return Ok(());
    }

    println!("Installed profiles ({}):", lockfile.profiles.len());
    println!();

    for (name, entry) in &lockfile.profiles {
        println!("  {} (v{})", name, entry.version);
        if verbose {
            println!("    Installed: {}", entry.installed_at);
            if let Some(commit) = &entry.commit {
                println!("    Commit: {}", &commit[..8]);
            }
            println!("    Files: {}", entry.files.len());
            for file in &entry.files {
                println!("      - {file}");
            }
            println!("    Integrity: {}", &entry.integrity[..16]);
            println!();
        }
    }

    Ok(())
}

/// Sync team configuration
///
/// This is the public API for the `codanna profile sync` command.
pub fn sync_team_config(force: bool) -> ProfileResult<()> {
    use project::ProfilesConfig;

    let workspace = std::env::current_dir()?;
    let profiles_config_path = workspace.join(".codanna/profiles.json");

    // Check if team config exists
    if !profiles_config_path.exists() {
        println!("No team configuration found at .codanna/profiles.json");
        println!("Nothing to sync");
        return Ok(());
    }

    // Load team config
    let profiles_config = ProfilesConfig::load(&profiles_config_path)?;

    if profiles_config.is_empty() {
        println!("Team configuration is empty");
        println!("Nothing to sync");
        return Ok(());
    }

    println!("Syncing team configuration...");
    println!();

    // Load global registry
    let registry_path = provider_registry_path();
    let mut registry = ProviderRegistry::load(&registry_path)?;

    // 1. Register extraKnownProviders
    for (provider_id, extra_provider) in &profiles_config.extra_known_providers {
        if registry.get_provider(provider_id).is_some() {
            println!("Provider '{provider_id}' already registered, skipping");
            continue;
        }

        println!("Registering provider '{provider_id}'...");

        // Load provider manifest
        let manifest = load_provider_manifest(&extra_provider.source)?;

        // Add to registry
        registry.add_provider(
            provider_id.clone(),
            &manifest,
            extra_provider.source.clone(),
        );

        println!(
            "  Registered provider '{provider_id}' with {} profiles",
            manifest.profiles.len()
        );
    }

    // Save updated registry
    registry.save(&registry_path)?;

    println!();

    // 2. Install required profiles
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    let lockfile = lockfile::ProfileLockfile::load(&lockfile_path).unwrap_or_default();

    for profile_ref in &profiles_config.profiles {
        let reference = ProfileReference::parse(profile_ref);

        // Check if already installed
        if lockfile.get_profile(&reference.profile).is_some() {
            println!(
                "Profile '{}' already installed, skipping",
                reference.profile
            );
            continue;
        }

        println!("Installing profile '{}'...", reference.profile);

        // Install the profile
        if let Err(e) = install_profile_from_registry(profile_ref, force) {
            eprintln!("  Error installing '{}': {e}", reference.profile);
            eprintln!("  Continuing with remaining profiles...");
        } else {
            println!("  Installed '{}'", reference.profile);
        }
    }

    println!();
    println!("Sync complete!");

    Ok(())
}

/// Remove an installed profile
///
/// This is the public API for the `codanna profile remove` command.
pub fn remove_profile(profile_name: &str, verbose: bool) -> ProfileResult<()> {
    use lockfile::ProfileLockfile;

    let workspace = std::env::current_dir()?;
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");

    // Load lockfile
    let mut lockfile = ProfileLockfile::load(&lockfile_path)?;

    // Find profile entry
    let entry =
        lockfile
            .get_profile(profile_name)
            .ok_or_else(|| error::ProfileError::NotInstalled {
                name: profile_name.to_string(),
            })?;

    if verbose {
        println!("Removing profile '{profile_name}'...");
        println!("  Files to remove: {}", entry.files.len());
    }

    // Delete all tracked files
    let mut removed_count = 0;
    let mut failed_removals = Vec::new();

    for file_path in &entry.files {
        let full_path = workspace.join(file_path);

        if verbose {
            println!("  Removing: {file_path}");
        }

        if full_path.exists() {
            match std::fs::remove_file(&full_path) {
                Ok(_) => {
                    removed_count += 1;

                    // Try to remove parent directory if empty (same as plugins)
                    if let Some(parent) = full_path.parent() {
                        let _ = std::fs::remove_dir(parent); // Ignore errors if not empty
                    }
                }
                Err(e) => {
                    eprintln!("  Warning: Failed to remove {file_path}: {e}");
                    failed_removals.push(file_path.clone());
                }
            }
        } else if verbose {
            println!("    (file not found, skipping)");
        }
    }

    // Remove profile from lockfile
    lockfile.remove_profile(profile_name);

    // If lockfile is now empty, delete it
    if lockfile.profiles.is_empty() {
        if verbose {
            println!("  Lockfile is now empty, removing it");
        }
        std::fs::remove_file(&lockfile_path)?;
        println!("\nProfile '{profile_name}' removed successfully");
        println!("  Files removed: {removed_count}");
        if !failed_removals.is_empty() {
            println!("  Failed removals: {}", failed_removals.len());
        }
    } else {
        // Save updated lockfile
        lockfile.save(&lockfile_path)?;
        println!("\nProfile '{profile_name}' removed successfully");
        println!("  Files removed: {removed_count}");
        if !failed_removals.is_empty() {
            println!("  Failed removals: {}", failed_removals.len());
        }
        if verbose {
            println!("  Remaining profiles: {}", lockfile.profiles.len());
        }
    }

    Ok(())
}

/// Install profile from provider registry
///
/// Supports syntax:
/// - "myprofile" - searches all providers for profile
/// - "myprofile@provider" - installs from specific provider
///
/// This is the public API for registry-based installation.
pub fn install_profile_from_registry(profile_ref: &str, force: bool) -> ProfileResult<()> {
    let workspace = std::env::current_dir()?;

    // 1. Parse profile reference
    let reference = ProfileReference::parse(profile_ref);

    // 2. Load provider registry
    let registry_path = provider_registry_path();
    let registry = ProviderRegistry::load(&registry_path)?;

    if registry.providers.is_empty() {
        return Err(error::ProfileError::InvalidManifest {
            reason: "No providers registered. Add a provider first:\n  codanna profile provider add <source>".to_string(),
        });
    }

    // 3. Find provider (need both ID and provider data)
    let (provider_id, provider) = match &reference.provider {
        Some(id) => {
            // Specific provider requested
            let p = registry.get_provider(id).ok_or_else(|| {
                error::ProfileError::InvalidManifest {
                    reason: format!(
                        "Provider '{id}' not found\nUse 'codanna profile provider list' to see registered providers"
                    ),
                }
            })?;
            (id.as_str(), p)
        }
        None => {
            // Search all providers for profile
            registry
                .find_provider_with_id(&reference.profile)
                .ok_or_else(|| error::ProfileError::InvalidManifest {
                    reason: format!(
                        "Profile '{}' not found in any registered provider\nUse 'codanna profile provider list --verbose' to see available profiles",
                        reference.profile
                    ),
                })?
        }
    };

    // 4. Verify profile exists in provider
    if !provider.profiles.contains_key(&reference.profile) {
        return Err(error::ProfileError::InvalidManifest {
            reason: format!(
                "Profile '{}' not found in provider '{}'\nAvailable profiles: {}",
                reference.profile,
                provider.name,
                provider
                    .profiles
                    .keys()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    // 5. Resolve profile source
    println!(
        "Resolving profile '{}' from provider '{}'...",
        reference.profile, provider.name
    );
    let resolved = resolve_profile_source(&provider.source, &reference.profile)?;
    let profile_dir = resolved.profile_dir(&reference.profile);

    if !profile_dir.exists() {
        return Err(error::ProfileError::InvalidManifest {
            reason: format!("Profile directory not found: {}", profile_dir.display()),
        });
    }

    // 6. Install using atomic installer
    if force {
        println!(
            "Installing profile '{}' from provider '{}' with --force...",
            reference.profile, provider.name
        );
    } else {
        println!(
            "Installing profile '{}' from provider '{}'...",
            reference.profile, provider.name
        );
    }

    // Get commit SHA if from git source
    let commit = resolved.commit().map(String::from);

    install_profile(
        &reference.profile,
        profile_dir.parent().unwrap(),
        &workspace,
        force,
        commit,
        Some(provider_id),
        Some(provider.source.clone()),
    )?;

    println!("\nProfile '{}' installed successfully", reference.profile);
    if force {
        println!("  Note: Conflicting files handled with --force");
        println!(
            "  Use 'codanna profile verify {}' to check integrity",
            reference.profile
        );
    }

    Ok(())
}

/// Update an installed profile from its provider
///
/// This is the public API for the `codanna profile update` command.
pub fn update_profile(profile_name: &str, force: bool) -> ProfileResult<()> {
    let workspace = std::env::current_dir()?;
    let lockfile_path = workspace.join(".codanna/profiles.lock.json");
    let lockfile = lockfile::ProfileLockfile::load(&lockfile_path)?;

    // Get existing profile entry
    let existing =
        lockfile
            .get_profile(profile_name)
            .ok_or_else(|| error::ProfileError::NotInstalled {
                name: profile_name.to_string(),
            })?;

    // Check if profile has a commit (git source)
    let existing_commit =
        existing
            .commit
            .as_ref()
            .ok_or_else(|| error::ProfileError::InvalidManifest {
                reason: format!(
                    "Profile '{profile_name}' was installed from local source and cannot be updated"
                ),
            })?;

    // Load provider registry to get source URL
    let registry_path = provider_registry_path();
    let registry = ProviderRegistry::load(&registry_path)?;

    // Find which provider has this profile
    let provider = registry
        .find_provider_for_profile(profile_name)
        .ok_or_else(|| error::ProfileError::ProfileNotFoundInAnyProvider {
            profile: profile_name.to_string(),
        })?;

    // Get the repository URL from provider source
    let repo_url = match &provider.source {
        ProviderSource::Github { repo } => format!("https://github.com/{repo}.git"),
        ProviderSource::Url { url } => url.clone(),
        ProviderSource::Local { .. } => {
            return Err(error::ProfileError::InvalidManifest {
                reason: "Cannot update profile from local provider".to_string(),
            });
        }
    };

    // Resolve remote HEAD commit
    let remote_commit = if force {
        // Force update: skip checking remote
        None
    } else {
        Some(git::resolve_reference(&repo_url, "HEAD")?)
    };

    // Check if already up to date
    if !force {
        if let Some(ref remote) = remote_commit {
            if remote == existing_commit {
                // Verify integrity before declaring up-to-date
                match verification::verify_profile(&workspace, profile_name, false) {
                    Ok(()) => {
                        println!(
                            "Profile '{profile_name}' already up to date (commit {})",
                            &existing_commit[..8]
                        );
                        return Ok(());
                    }
                    Err(_) => {
                        println!(
                            "Profile '{profile_name}' integrity check failed, reinstalling..."
                        );
                    }
                }
            } else {
                println!(
                    "Updating profile '{profile_name}' from {} to {}",
                    &existing_commit[..8],
                    &remote[..8]
                );
            }
        }
    }

    // Perform update by reinstalling with force
    install_profile_from_registry(profile_name, true)?;

    if let Some(ref remote) = remote_commit {
        println!(
            "\nProfile '{profile_name}' updated to commit {}",
            &remote[..8]
        );
    } else {
        println!("\nProfile '{profile_name}' updated (force reinstall)");
    }

    Ok(())
}
