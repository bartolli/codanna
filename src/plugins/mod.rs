//! プラグイン管理システム
//!
//! Claude Code プラグインのインストール、更新、管理機能を提供します。
//! Gitベースのマーケットプレイスからプラグインを取得できます。
//!
//! # 主要な機能
//!
//! - プラグインのインストールと更新
//! - マーケットプレイスからの解決
//! - ロックファイル管理
//! - プラグインのマージ
//!
//! # 使用例
//!
//! ```no_run
//! // use codanna::plugins::marketplace::MarketplaceResolver;
//! // let resolver = MarketplaceResolver::new();
//! // resolver.install_plugin("my-plugin");
//! ```

pub mod error;
pub mod fsops;
pub mod lockfile;
pub mod marketplace;
pub mod merger;
pub mod plugin;
pub mod resolver;

use crate::Settings;
use chrono::Utc;
use error::{PluginError, PluginResult};
use fsops::{calculate_dest_path, calculate_integrity, copy_plugin_files, copy_plugin_payload};
use lockfile::{LockfilePluginSource, PluginLockEntry, PluginLockfile};
use marketplace::{MarketplaceManifest, ResolvedPluginSource};
use plugin::{HookSpec, PathSpec, PluginManifest};
use resolver::{clone_repository, extract_subdirectory};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct WorkspacePaths {
    root: PathBuf,
    commands_dir: PathBuf,
    agents_dir: PathBuf,
    hooks_dir: PathBuf,
    scripts_dir: PathBuf,
    plugins_dir: PathBuf,
    lockfile_path: PathBuf,
    mcp_path: PathBuf,
}

struct PreparedPlugin {
    plugin_dir: TempDir,
    manifest: PluginManifest,
    component_files: Vec<String>,
    commit_sha: String,
    mcp_servers: Option<Value>,
    source: LockfilePluginSource,
}

struct ExistingPluginBackup {
    entry: PluginLockEntry,
    files: Vec<(PathBuf, Vec<u8>)>,
    mcp_content: Option<Vec<u8>>,
    mcp_existed: bool,
}

impl WorkspacePaths {
    fn for_root(root: PathBuf) -> Self {
        let claude_dir = root.join(".claude");
        let commands_dir = claude_dir.join("commands");
        let agents_dir = claude_dir.join("agents");
        let hooks_dir = claude_dir.join("hooks");
        let scripts_dir = claude_dir.join("scripts");
        let plugins_dir = claude_dir.join("plugins");
        let lockfile_path = root.join(".codanna/plugins/lockfile.json");
        let mcp_path = root.join(".mcp.json");

        Self {
            root,
            commands_dir,
            agents_dir,
            hooks_dir,
            scripts_dir,
            plugins_dir,
            lockfile_path,
            mcp_path,
        }
    }
}

/// Install a plugin from a marketplace
pub fn add_plugin(
    settings: &Settings,
    marketplace_url: &str,
    plugin_name: &str,
    git_ref: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<(), PluginError> {
    let workspace_root = resolve_workspace_root(settings)?;
    let paths = WorkspacePaths::for_root(workspace_root.clone());

    let mut lockfile = load_lockfile(&paths)?;
    let previous_entry = lockfile.get_plugin(plugin_name).cloned();
    if let Some(ref existing) = previous_entry {
        if !force {
            return Err(PluginError::AlreadyInstalled {
                name: plugin_name.to_string(),
                version: existing.version.clone(),
            });
        }
    }

    let plan = prepare_plugin(
        &paths,
        &lockfile,
        plugin_name,
        marketplace_url,
        git_ref,
        force,
        previous_entry.as_ref(),
    )?;

    if dry_run {
        println!("DRY RUN: Would install plugin '{plugin_name}' from {marketplace_url}");
        if let Some(r) = git_ref {
            println!("  Using ref: {r}");
        }
        if force {
            println!("  Force mode: would overwrite conflicts");
        }
        println!("  Target workspace: {}", paths.root.display());
        print_dry_run_summary(&plan);
        return Ok(());
    }

    ensure_workspace_layout(&paths)?;

    let entry = execute_install_with_plan(
        &paths,
        &mut lockfile,
        plugin_name,
        marketplace_url,
        force,
        settings.debug,
        None,
        plan,
    )?;

    println!(
        "Plugin '{plugin_name}' installed into {} (commit {})",
        paths.root.display(),
        entry.commit
    );
    Ok(())
}

/// Remove an installed plugin
pub fn remove_plugin(
    settings: &Settings,
    plugin_name: &str,
    force: bool,
    dry_run: bool,
) -> Result<(), PluginError> {
    let workspace_root = resolve_workspace_root(settings)?;
    let paths = WorkspacePaths::for_root(workspace_root.clone());

    if dry_run {
        println!("DRY RUN: Would remove plugin '{plugin_name}'");
        if force {
            println!("  Force mode: would ignore dependencies");
        }
        println!("  Target workspace: {}", paths.root.display());
        return Ok(());
    }

    let mut lockfile = load_lockfile(&paths)?;

    let entry = match lockfile.get_plugin(plugin_name) {
        Some(entry) => entry.clone(),
        None => {
            return Err(PluginError::NotInstalled {
                name: plugin_name.to_string(),
            });
        }
    };

    // TODO: Consider dependency graph when available. For now we ignore `force`.

    uninstall_plugin(&paths, &mut lockfile, plugin_name, &entry)?;
    save_lockfile(&paths, &lockfile)?;

    println!(
        "Removed plugin '{plugin_name}' from {}",
        paths.root.display()
    );
    Ok(())
}

/// Update an installed plugin
pub fn update_plugin(
    settings: &Settings,
    plugin_name: &str,
    git_ref: Option<&str>,
    force: bool,
    dry_run: bool,
) -> Result<(), PluginError> {
    let workspace_root = resolve_workspace_root(settings)?;
    let paths = WorkspacePaths::for_root(workspace_root.clone());
    let mut lockfile = load_lockfile(&paths)?;
    let existing =
        lockfile
            .get_plugin(plugin_name)
            .cloned()
            .ok_or_else(|| PluginError::NotInstalled {
                name: plugin_name.to_string(),
            })?;

    let remote_commit = if force {
        None
    } else {
        resolve_remote_commit(&existing, git_ref)
    };

    if !force {
        if let Some(commit) = remote_commit.as_ref() {
            if commit == &existing.commit {
                if dry_run {
                    println!(
                        "DRY RUN: Would update plugin '{plugin_name}' (already at commit {commit})"
                    );
                    println!("  Target workspace: {}", paths.root.display());
                    return Ok(());
                }

                match verify_entry(&paths, &existing, false) {
                    Ok(_) => {
                        println!("Plugin '{plugin_name}' already up to date (commit {commit})");
                        return Ok(());
                    }
                    Err(err) => {
                        if settings.debug {
                            eprintln!(
                                "DEBUG: existing install failed verification, reinstalling: {err}"
                            );
                        }
                    }
                }
            }
        }
    }

    let plan = prepare_plugin(
        &paths,
        &lockfile,
        plugin_name,
        &existing.marketplace_url,
        git_ref,
        force,
        Some(&existing),
    )?;

    if dry_run {
        println!("DRY RUN: Would update plugin '{plugin_name}'");
        if let Some(r) = git_ref {
            println!("  To ref: {r}");
        }
        if force {
            println!("  Force mode: would overwrite local changes");
        }
        println!("  Target workspace: {}", paths.root.display());
        print_dry_run_summary(&plan);
        return Ok(());
    }

    if !force && plan.commit_sha == existing.commit {
        match verify_entry(&paths, &existing, false) {
            Ok(_) => {
                println!(
                    "Plugin '{plugin_name}' already up to date (commit {})",
                    existing.commit
                );
                return Ok(());
            }
            Err(err) => {
                if settings.debug {
                    eprintln!("DEBUG: existing install failed verification, reinstalling: {err}");
                }
            }
        }
    }

    ensure_workspace_layout(&paths)?;

    let entry = execute_install_with_plan(
        &paths,
        &mut lockfile,
        plugin_name,
        &existing.marketplace_url,
        force,
        settings.debug,
        Some(existing.clone()),
        plan,
    )?;

    println!(
        "Plugin '{plugin_name}' updated in {} ({} -> {})",
        paths.root.display(),
        existing.commit,
        entry.commit
    );
    Ok(())
}

/// List installed plugins
pub fn list_plugins(settings: &Settings, verbose: bool, json: bool) -> Result<(), PluginError> {
    let workspace_root = resolve_workspace_root(settings)?;
    let paths = WorkspacePaths::for_root(workspace_root.clone());
    let lockfile = load_lockfile(&paths)?;

    let mut entries: Vec<_> = lockfile.plugins.values().cloned().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    if json {
        let payload = serde_json::json!({
            "workspace": paths.root,
            "plugins": entries,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if entries.is_empty() {
        println!("No plugins installed in workspace {}", paths.root.display());
        if verbose {
            println!("\nUse 'codanna plugin add <marketplace> <plugin>' to install a plugin");
        }
    } else {
        println!("Plugins in workspace {}:", paths.root.display());
        for entry in entries {
            println!(
                "  - {} @ {} (commit {})",
                entry.name, entry.version, entry.commit
            );
            if verbose {
                println!("    source: {}", entry.marketplace_url);
                println!("    files: {}", entry.files.len());
            }
        }
    }
    Ok(())
}

fn resolve_remote_commit(existing: &PluginLockEntry, override_ref: Option<&str>) -> Option<String> {
    match existing.source.as_ref() {
        Some(LockfilePluginSource::Git { url, git_ref, .. }) => {
            let reference = override_ref.or(git_ref.as_deref()).unwrap_or("HEAD");
            resolver::resolve_reference(url, reference).ok()
        }
        Some(LockfilePluginSource::MarketplacePath { .. }) | None => {
            let reference = override_ref.unwrap_or("HEAD");
            resolver::resolve_reference(&existing.marketplace_url, reference).ok()
        }
    }
}

/// Verify integrity of a specific plugin
pub fn verify_plugin(
    settings: &Settings,
    plugin_name: &str,
    verbose: bool,
) -> Result<(), PluginError> {
    let workspace_root = resolve_workspace_root(settings)?;
    let paths = WorkspacePaths::for_root(workspace_root.clone());
    let lockfile = load_lockfile(&paths)?;

    let entry = match lockfile.get_plugin(plugin_name) {
        Some(entry) => entry,
        None => {
            return Err(PluginError::NotInstalled {
                name: plugin_name.to_string(),
            });
        }
    };

    if verbose {
        println!(
            "Verifying plugin '{plugin_name}' in workspace {}...",
            paths.root.display()
        );
        println!("  Stored integrity: {}", entry.integrity);
    }

    verify_entry(&paths, entry, verbose)?;

    println!("Plugin '{plugin_name}' verified successfully");
    Ok(())
}

/// Verify all installed plugins
pub fn verify_all_plugins(settings: &Settings, verbose: bool) -> Result<(), PluginError> {
    let workspace_root = resolve_workspace_root(settings)?;
    let paths = WorkspacePaths::for_root(workspace_root.clone());
    let lockfile = load_lockfile(&paths)?;

    if lockfile.plugins.is_empty() {
        if verbose {
            println!("No plugins installed in workspace {}", paths.root.display());
        }
        return Ok(());
    }

    for entry in lockfile.plugins.values() {
        verify_entry(&paths, entry, verbose)?;
    }

    println!("All plugins verified successfully");
    Ok(())
}

fn ensure_workspace_layout(paths: &WorkspacePaths) -> PluginResult<()> {
    for sub in [
        &paths.commands_dir,
        &paths.agents_dir,
        &paths.hooks_dir,
        &paths.scripts_dir,
        &paths.plugins_dir,
    ] {
        fs::create_dir_all(sub)?;
    }
    fs::create_dir_all(paths.lockfile_path.parent().unwrap())?;
    Ok(())
}

fn load_lockfile(paths: &WorkspacePaths) -> PluginResult<PluginLockfile> {
    PluginLockfile::load(&paths.lockfile_path)
}

fn save_lockfile(paths: &WorkspacePaths, lockfile: &PluginLockfile) -> PluginResult<()> {
    lockfile.save(&paths.lockfile_path)
}

fn to_absolute_paths(paths: &WorkspacePaths, files: &[String]) -> Vec<String> {
    files
        .iter()
        .map(|relative| {
            paths
                .root
                .join(relative)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn resolve_file_owner(
    paths: &WorkspacePaths,
    lockfile: &PluginLockfile,
    path: &Path,
) -> Option<String> {
    let relative = path
        .strip_prefix(&paths.root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    lockfile
        .find_file_owner(&relative)
        .map(|name| name.to_string())
}

fn backup_existing_plugin(
    paths: &WorkspacePaths,
    entry: &PluginLockEntry,
) -> PluginResult<ExistingPluginBackup> {
    let mut files = Vec::new();
    for relative in &entry.files {
        let absolute = paths.root.join(relative);
        if absolute.exists() {
            let data = fs::read(&absolute)?;
            files.push((absolute, data));
        }
    }

    let (mcp_content, mcp_existed) = if paths.mcp_path.exists() {
        (Some(fs::read(&paths.mcp_path)?), true)
    } else {
        (None, false)
    };

    Ok(ExistingPluginBackup {
        entry: entry.clone(),
        files,
        mcp_content,
        mcp_existed,
    })
}

fn restore_existing_plugin(
    paths: &WorkspacePaths,
    backup: &ExistingPluginBackup,
) -> PluginResult<()> {
    for (path, data) in &backup.files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, data)?;
    }

    match (&backup.mcp_content, backup.mcp_existed) {
        (Some(content), _) => {
            if let Some(parent) = paths.mcp_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&paths.mcp_path, content)?;
        }
        (None, true) => {
            // If the file previously existed but we could not read it, leave as-is
        }
        (None, false) => {
            if paths.mcp_path.exists() {
                let _ = fs::remove_file(&paths.mcp_path);
            }
        }
    }

    Ok(())
}

fn verify_entry(
    paths: &WorkspacePaths,
    entry: &PluginLockEntry,
    verbose: bool,
) -> PluginResult<()> {
    let filtered: Vec<String> = entry
        .files
        .iter()
        .filter(|path| path.as_str() != ".mcp.json")
        .cloned()
        .collect();
    let absolute_files = to_absolute_paths(paths, &filtered);
    let actual = calculate_integrity(&absolute_files)?;

    if actual != entry.integrity {
        return Err(PluginError::IntegrityCheckFailed {
            plugin: entry.name.clone(),
            expected: entry.integrity.clone(),
            actual,
        });
    }

    if verbose {
        println!(
            "  Integrity OK for '{}' ({} files)",
            entry.name,
            entry.files.len()
        );
    }

    if !entry.mcp_keys.is_empty() && paths.mcp_path.exists() {
        let content = fs::read_to_string(&paths.mcp_path)?;
        let json: Value = serde_json::from_str(&content)?;
        let servers = json
            .get("mcpServers")
            .and_then(|value| value.as_object())
            .cloned()
            .unwrap_or_default();

        for key in &entry.mcp_keys {
            if !servers.contains_key(key) {
                return Err(PluginError::IntegrityCheckFailed {
                    plugin: entry.name.clone(),
                    expected: format!("mcp server '{key}' present"),
                    actual: "missing".to_string(),
                });
            }
        }

        if verbose {
            println!(
                "  MCP servers verified for '{}': {:?}",
                entry.name, entry.mcp_keys
            );
        }
    }

    Ok(())
}

fn uninstall_plugin(
    paths: &WorkspacePaths,
    lockfile: &mut PluginLockfile,
    plugin_name: &str,
    entry: &PluginLockEntry,
) -> PluginResult<()> {
    let mut absolute_files = to_absolute_paths(paths, &entry.files);
    absolute_files.retain(|path| Path::new(path) != paths.mcp_path);
    fsops::remove_plugin_files(&absolute_files)?;

    cleanup_plugin_dirs(paths, plugin_name);

    if !entry.mcp_keys.is_empty() {
        merger::remove_mcp_servers(&paths.mcp_path, &entry.mcp_keys)?;
    }

    lockfile.remove_plugin(plugin_name);
    Ok(())
}

fn collect_component_files(
    plugin_root: &Path,
    manifest: &PluginManifest,
) -> PluginResult<Vec<String>> {
    let mut files = HashSet::new();

    add_directory_files(plugin_root, "commands", &mut files)?;
    if let Some(spec) = &manifest.commands {
        add_spec_paths(plugin_root, spec, &mut files)?;
    }

    add_directory_files(plugin_root, "agents", &mut files)?;
    if let Some(spec) = &manifest.agents {
        add_spec_paths(plugin_root, spec, &mut files)?;
    }

    add_directory_files(plugin_root, "hooks", &mut files)?;
    if let Some(HookSpec::Path(path)) = &manifest.hooks {
        add_single_path(plugin_root, path, &mut files)?;
    }

    add_directory_files(plugin_root, "scripts", &mut files)?;
    if let Some(spec) = &manifest.scripts {
        add_spec_paths(plugin_root, spec, &mut files)?;
    }

    let mut list: Vec<_> = files.into_iter().collect();
    list.sort();
    Ok(list)
}

fn add_directory_files(
    plugin_root: &Path,
    directory: &str,
    files: &mut HashSet<String>,
) -> PluginResult<()> {
    let dir_path = plugin_root.join(directory);
    if !dir_path.exists() {
        return Ok(());
    }

    for file in collect_files_for_path(plugin_root, &dir_path)? {
        files.insert(file);
    }
    Ok(())
}

fn add_spec_paths(
    plugin_root: &Path,
    spec: &PathSpec,
    files: &mut HashSet<String>,
) -> PluginResult<()> {
    match spec {
        PathSpec::Single(path) => add_single_path(plugin_root, path, files)?,
        PathSpec::Multiple(paths) => {
            for path in paths {
                add_single_path(plugin_root, path, files)?;
            }
        }
    }
    Ok(())
}

fn add_single_path(
    plugin_root: &Path,
    path: &str,
    files: &mut HashSet<String>,
) -> PluginResult<()> {
    let sanitized = sanitize_manifest_path(path);
    if sanitized == "." {
        return Err(PluginError::InvalidPluginManifest {
            reason: format!("Referenced path '{path}' must not point to plugin root"),
        });
    }
    let full_path = plugin_root.join(&sanitized);
    if !full_path.exists() {
        return Err(PluginError::InvalidPluginManifest {
            reason: format!("Referenced path '{path}' does not exist"),
        });
    }

    for file in collect_files_for_path(plugin_root, &full_path)? {
        files.insert(file);
    }
    Ok(())
}

fn collect_files_for_path(base: &Path, target: &Path) -> PluginResult<Vec<String>> {
    if target.is_file() {
        let rel = target.strip_prefix(base).unwrap_or(target).to_path_buf();
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        return Ok(vec![rel_str]);
    }

    if target.is_dir() {
        let mut files = Vec::new();
        for entry in WalkDir::new(target).into_iter() {
            let entry = entry.map_err(|e| PluginError::IoError(io::Error::other(e)))?;
            if entry.file_type().is_dir() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(base)
                .unwrap_or(entry.path())
                .to_path_buf();
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
        return Ok(files);
    }

    Ok(Vec::new())
}

fn sanitize_manifest_path(path: &str) -> String {
    let trimmed = path.trim();
    let without_prefix = trimmed.trim_start_matches("./");
    if without_prefix.is_empty() {
        ".".to_string()
    } else {
        without_prefix.to_string()
    }
}

fn load_plugin_mcp(plugin_root: &Path, manifest: &PluginManifest) -> PluginResult<Option<Value>> {
    if let Some(spec) = &manifest.mcp_servers {
        return merger::load_plugin_mcp_servers(plugin_root, spec).map(Some);
    }

    let default_mcp = plugin_root.join(".mcp.json");
    if default_mcp.exists() {
        let content = fs::read_to_string(&default_mcp)?;
        let json: Value = serde_json::from_str(&content)?;
        if let Some(servers) = json.get("mcpServers") {
            if servers.is_object() {
                return Ok(Some(servers.clone()));
            }
        }
    }

    Ok(None)
}

fn normalize_paths(workspace_root: &Path, files: Vec<String>) -> Vec<String> {
    let mut unique = HashSet::new();
    for file in files {
        let path = PathBuf::from(&file);
        let rel = path
            .strip_prefix(workspace_root)
            .unwrap_or(&path)
            .to_path_buf();
        unique.insert(rel.to_string_lossy().replace('\\', "/"));
    }
    let mut list: Vec<_> = unique.into_iter().collect();
    list.sort();
    list
}

fn resolve_workspace_root(settings: &Settings) -> Result<PathBuf, PluginError> {
    if let Some(root) = &settings.workspace_root {
        if root.is_absolute() {
            Ok(root.clone())
        } else {
            let cwd = std::env::current_dir()?;
            Ok(cwd.join(root))
        }
    } else {
        Ok(std::env::current_dir()?)
    }
}

fn prepare_plugin(
    paths: &WorkspacePaths,
    lockfile: &PluginLockfile,
    plugin_name: &str,
    marketplace_url: &str,
    git_ref: Option<&str>,
    force: bool,
    previous_entry: Option<&PluginLockEntry>,
) -> Result<PreparedPlugin, PluginError> {
    let marketplace_dir = tempdir()?;
    let commit_sha = clone_repository(marketplace_url, marketplace_dir.path(), git_ref)?;

    let marketplace_manifest_path = marketplace_dir
        .path()
        .join(".claude-plugin/marketplace.json");
    let marketplace_manifest = MarketplaceManifest::from_file(&marketplace_manifest_path)?;
    let plugin_entry = marketplace_manifest
        .find_plugin(plugin_name)
        .ok_or_else(|| PluginError::PluginNotFound {
            name: plugin_name.to_string(),
        })?;

    let resolved_source = plugin_entry.resolve_source(marketplace_manifest.metadata.as_ref())?;
    let mut effective_commit = commit_sha.clone();

    let (plugin_dir, source_for_lockfile) = match &resolved_source {
        ResolvedPluginSource::MarketplacePath { relative } => {
            let plugin_dir = tempdir()?;
            extract_subdirectory(marketplace_dir.path(), relative, plugin_dir.path())?;
            (
                plugin_dir,
                LockfilePluginSource::MarketplacePath {
                    relative: relative.clone(),
                },
            )
        }
        ResolvedPluginSource::Git {
            url,
            git_ref,
            subdir,
        } => {
            let repo_dir = tempdir()?;
            let repo_commit = resolver::clone_repository(url, repo_dir.path(), git_ref.as_deref())?;
            effective_commit = repo_commit;
            let plugin_dir = if let Some(path) = subdir {
                let plugin_dir = tempdir()?;
                extract_subdirectory(repo_dir.path(), path, plugin_dir.path())?;
                plugin_dir
            } else {
                repo_dir
            };
            (
                plugin_dir,
                LockfilePluginSource::Git {
                    url: url.clone(),
                    git_ref: git_ref.clone(),
                    subdir: subdir.clone(),
                },
            )
        }
    };

    let plugin_manifest_path = plugin_dir.path().join(".claude-plugin/plugin.json");
    let manifest = if plugin_manifest_path.exists() {
        PluginManifest::from_file(&plugin_manifest_path)?
    } else if plugin_entry.strict {
        return Err(PluginError::InvalidPluginManifest {
            reason: format!(
                "Plugin '{plugin_name}' requires .claude-plugin/plugin.json but none was found"
            ),
        });
    } else {
        plugin_entry.to_plugin_manifest()?
    };
    let component_files = collect_component_files(plugin_dir.path(), &manifest)?;

    check_file_conflicts(
        paths,
        lockfile,
        plugin_name,
        &component_files,
        plugin_dir.path(),
        force,
    )?;

    let mcp_servers = load_plugin_mcp(plugin_dir.path(), &manifest)?;

    if let Some(servers) = &mcp_servers {
        let allowed_keys: HashSet<String> = previous_entry
            .map(|entry| entry.mcp_keys.iter().cloned().collect())
            .unwrap_or_default();
        merger::check_mcp_conflicts(&paths.mcp_path, servers, force, &allowed_keys)?;
    }

    Ok(PreparedPlugin {
        plugin_dir,
        manifest,
        component_files,
        commit_sha: effective_commit,
        mcp_servers,
        source: source_for_lockfile,
    })
}

fn check_file_conflicts(
    paths: &WorkspacePaths,
    lockfile: &PluginLockfile,
    plugin_name: &str,
    component_files: &[String],
    plugin_dir: &Path,
    force: bool,
) -> PluginResult<()> {
    for relative in component_files {
        let dest = calculate_dest_path(&paths.root, plugin_name, relative);
        if dest.exists() {
            match resolve_file_owner(paths, lockfile, &dest) {
                Some(owner) if owner != plugin_name && !force => {
                    return Err(PluginError::FileConflict { path: dest, owner });
                }
                None if !force => {
                    return Err(PluginError::FileConflict {
                        path: dest,
                        owner: "unknown".to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    for entry in WalkDir::new(plugin_dir).into_iter() {
        let entry = entry.map_err(|e| PluginError::IoError(io::Error::other(e)))?;
        if entry.file_type().is_dir() {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(plugin_dir)
            .expect("walkdir entry should be under plugin root");

        if relative.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }

        let relative_str = relative.to_string_lossy().replace('\\', "/");
        if component_files.contains(&relative_str)
            || relative_str.starts_with("commands/")
            || relative_str.starts_with("agents/")
            || relative_str.starts_with("hooks/")
            || relative_str.starts_with("scripts/")
        {
            continue;
        }

        let dest = paths.plugins_dir.join(plugin_name).join(relative);
        if dest.exists() {
            match resolve_file_owner(paths, lockfile, &dest) {
                Some(owner) if owner != plugin_name && !force => {
                    return Err(PluginError::FileConflict { path: dest, owner });
                }
                None if !force => {
                    return Err(PluginError::FileConflict {
                        path: dest,
                        owner: "unknown".to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn print_dry_run_summary(plan: &PreparedPlugin) {
    let command_count = plan
        .component_files
        .iter()
        .filter(|p| p.starts_with("commands/"))
        .count();
    let agent_count = plan
        .component_files
        .iter()
        .filter(|p| p.starts_with("agents/"))
        .count();
    let hook_count = plan
        .component_files
        .iter()
        .filter(|p| p.starts_with("hooks/"))
        .count();
    let script_count = plan
        .component_files
        .iter()
        .filter(|p| p.starts_with("scripts/"))
        .count();

    println!("  Commit: {}", plan.commit_sha);
    println!("  Commands: {command_count}");
    println!("  Agents: {agent_count}");
    println!("  Hooks: {hook_count}");
    println!("  Scripts: {script_count}");
}

fn execute_install_with_plan(
    paths: &WorkspacePaths,
    lockfile: &mut PluginLockfile,
    plugin_name: &str,
    marketplace_url: &str,
    force: bool,
    debug: bool,
    previous_entry: Option<PluginLockEntry>,
    plan: PreparedPlugin,
) -> Result<PluginLockEntry, PluginError> {
    if debug {
        eprintln!(
            "DEBUG: component files for plugin '{}': {:?}",
            plugin_name, plan.component_files
        );
    }

    let mut copied_files: Vec<String> = Vec::new();
    let mut mcp_backup: Option<merger::McpMergeOutcome> = None;
    let mut previous_backup: Option<ExistingPluginBackup> = None;

    if let Some(prev) = previous_entry.as_ref() {
        previous_backup = Some(backup_existing_plugin(paths, prev)?);
        uninstall_plugin(paths, lockfile, plugin_name, prev)?;
    }

    let component_paths = copy_plugin_files(
        plan.plugin_dir.path(),
        &paths.root,
        plugin_name,
        &plan.component_files,
        force,
        |path| resolve_file_owner(paths, lockfile, path),
    );

    match component_paths {
        Ok(paths_copied) => {
            copied_files.extend(paths_copied);
        }
        Err(e) => {
            rollback_install(
                paths,
                lockfile,
                plugin_name,
                &copied_files,
                &mcp_backup,
                previous_backup.as_ref(),
            )?;
            return Err(e);
        }
    }

    let payload_paths = copy_plugin_payload(
        plan.plugin_dir.path(),
        &paths.root,
        plugin_name,
        force,
        |path| resolve_file_owner(paths, lockfile, path),
        &plan.component_files,
    );

    match payload_paths {
        Ok(paths_copied) => {
            copied_files.extend(paths_copied);
        }
        Err(e) => {
            rollback_install(
                paths,
                lockfile,
                plugin_name,
                &copied_files,
                &mcp_backup,
                previous_backup.as_ref(),
            )?;
            return Err(e);
        }
    }

    let mut added_keys = Vec::new();
    if let Some(servers) = plan.mcp_servers.as_ref() {
        match merger::merge_mcp_servers(&paths.mcp_path, servers, force) {
            Ok(outcome) => {
                added_keys = outcome.added_keys.clone();
                mcp_backup = Some(outcome);
            }
            Err(e) => {
                rollback_install(
                    paths,
                    lockfile,
                    plugin_name,
                    &copied_files,
                    &mcp_backup,
                    previous_backup.as_ref(),
                )?;
                return Err(e);
            }
        }
    }

    let normalized_files = normalize_paths(&paths.root, copied_files.clone());
    let normalized_files: Vec<_> = normalized_files
        .into_iter()
        .filter(|path| path != ".mcp.json")
        .collect();
    let integrity_inputs = to_absolute_paths(paths, &normalized_files);
    let integrity = calculate_integrity(&integrity_inputs)?;
    let timestamp = Utc::now().to_rfc3339();

    let entry = PluginLockEntry {
        name: plugin_name.to_string(),
        version: plan.manifest.version.clone(),
        commit: plan.commit_sha.clone(),
        marketplace_url: marketplace_url.to_string(),
        installed_at: timestamp.clone(),
        updated_at: timestamp,
        integrity,
        files: normalized_files,
        mcp_keys: added_keys,
        source: Some(plan.source.clone()),
    };

    lockfile.add_plugin(entry.clone());
    if let Err(e) = save_lockfile(paths, lockfile) {
        lockfile.remove_plugin(plugin_name);
        rollback_install(
            paths,
            lockfile,
            plugin_name,
            &copied_files,
            &mcp_backup,
            previous_backup.as_ref(),
        )?;
        return Err(e);
    }

    Ok(entry)
}

fn rollback_install(
    paths: &WorkspacePaths,
    lockfile: &mut PluginLockfile,
    plugin_name: &str,
    copied_files: &[String],
    mcp_backup: &Option<merger::McpMergeOutcome>,
    previous_backup: Option<&ExistingPluginBackup>,
) -> PluginResult<()> {
    if !copied_files.is_empty() {
        fsops::remove_plugin_files(copied_files)?;
    }

    if let Some(previous) = previous_backup {
        restore_existing_plugin(paths, previous)?;
        lockfile.add_plugin(previous.entry.clone());
        save_lockfile(paths, lockfile)?;
    } else if let Some(backup) = mcp_backup {
        if backup.file_existed {
            if let Some(content) = &backup.previous_content {
                fs::write(&paths.mcp_path, content)?;
            }
        } else if paths.mcp_path.exists() {
            let _ = fs::remove_file(&paths.mcp_path);
        }
    }

    if previous_backup.is_none() {
        cleanup_plugin_dirs(paths, plugin_name);
    }
    Ok(())
}

fn cleanup_plugin_dirs(paths: &WorkspacePaths, plugin_name: &str) {
    let payload_dir = paths.plugins_dir.join(plugin_name);
    if payload_dir.exists() {
        let _ = fs::remove_dir_all(&payload_dir);
    }

    let script_dir = paths.scripts_dir.join(plugin_name);
    if script_dir.exists() {
        let _ = fs::remove_dir_all(&script_dir);
    }
}
