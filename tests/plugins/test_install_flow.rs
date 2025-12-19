use std::fs;
use std::path::Path;

use codanna::plugins::error::PluginError;
use codanna::{Settings, plugins};
use git2::{IndexAddOption, Repository, Signature};
use serde_json::Value;
use tempfile::TempDir;

fn with_temp_workspace<F>(test: F)
where
    F: FnOnce(&Path),
{
    let temp_dir = TempDir::new().expect("create temp dir");
    test(temp_dir.path());
}

fn assert_file_exists(workspace: &Path, relative: &str) {
    let full_path = workspace.join(relative);
    assert!(
        full_path.exists(),
        "expected file '{}' to exist",
        full_path.display()
    );
}

fn read_json(workspace: &Path, relative: &str) -> Value {
    let full_path = workspace.join(relative);
    let content = fs::read_to_string(&full_path).unwrap_or_else(|e| {
        panic!("Failed to read {}: {}", full_path.display(), e);
    });
    serde_json::from_str(&content).unwrap_or_else(|e| {
        panic!("Failed to parse {} as JSON: {}", full_path.display(), e);
    })
}

fn load_workspace_settings(workspace: &Path) -> Settings {
    let config_dir = workspace.join(".codanna");
    fs::create_dir_all(&config_dir).expect("create .codanna directory");

    let settings_path = config_dir.join("settings.toml");
    if !settings_path.exists() {
        // Minimal settings file; defaults fill in the rest
        fs::write(&settings_path, b"index_path = \"index\"\n").expect("write settings.toml");
    }

    let mut settings = Settings::load_from(&settings_path).expect("load workspace settings");
    settings.workspace_root = Some(workspace.to_path_buf());
    settings
}

fn create_marketplace_repo(
    workspace: &Path,
    repo_name: &str,
    plugin_name: &str,
    plugin_manifest: &str,
    extra_files: &[(&str, &str)],
) -> String {
    let repo_path = workspace.join(repo_name);
    let plugin_root = repo_path.join("plugin");
    let marketplace_dir = repo_path.join(".claude-plugin");
    let plugin_manifest_dir = plugin_root.join(".claude-plugin");

    fs::create_dir_all(&plugin_manifest_dir).expect("create plugin manifest dir");
    fs::create_dir_all(&marketplace_dir).expect("create marketplace dir");

    let marketplace_json = format!(
        r#"{{
    "name": "{repo_name}",
    "owner": {{"name": "Test"}},
    "plugins": [
        {{
            "name": "{plugin_name}",
            "source": "./plugin",
            "description": "Test plugin for integration failures"
        }}
    ]
}}"#
    );
    fs::write(marketplace_dir.join("marketplace.json"), marketplace_json)
        .expect("write marketplace manifest");
    fs::write(plugin_manifest_dir.join("plugin.json"), plugin_manifest)
        .expect("write plugin manifest");

    for (relative, content) in extra_files {
        let file_path = plugin_root.join(relative);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("create parent directories for extra file");
        }
        fs::write(&file_path, content).expect("write extra file");
    }

    let repo = Repository::init(&repo_path).expect("init git repo");
    let mut index = repo.index().expect("load git index");
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .expect("stage files");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_id).expect("find tree");
    let sig = Signature::now("Test", "test@example.com").expect("signature");
    repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
        .expect("commit repository");

    repo_path.to_str().unwrap().to_string()
}

#[test]
fn add_plugin_installs_codanna_cc_plugin() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";
        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("plugin installation should succeed");

        assert_file_exists(workspace, ".claude/commands/codanna-cc-plugin/ask.md");
        assert_file_exists(workspace, ".claude/commands/codanna-cc-plugin/find.md");
        assert_file_exists(
            workspace,
            ".claude/scripts/codanna-cc-plugin/context-provider.js",
        );
        assert_file_exists(
            workspace,
            ".claude/scripts/codanna-cc-plugin/formatters/symbol.js",
        );

        // Ensure .mcp.json merged entries
        let mcp = read_json(workspace, ".mcp.json");
        assert!(
            mcp["mcpServers"]["codanna"].is_object(),
            "codanna server missing"
        );
        assert!(
            mcp["mcpServers"]["codanna-sse"].is_object(),
            "codanna-sse server missing"
        );

        // Lockfile should have plugin entry with tracked files
        let lockfile = read_json(workspace, ".codanna/plugins/lockfile.json");
        let entry = &lockfile["plugins"]["codanna-cc-plugin"];
        assert_eq!(entry["name"], "codanna-cc-plugin");
        assert!(
            entry["files"]
                .as_array()
                .expect("files should be array")
                .iter()
                .any(|f| f == ".claude/commands/codanna-cc-plugin/ask.md"),
            "lockfile should track installed files"
        );

        // Workspace index unaffected (sanity check)
        assert!(
            !workspace.join("index").exists(),
            "plugin installation should not create index directory"
        );
    });
}

#[test]
fn dry_run_does_not_modify_workspace() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";
        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            true,
        )
        .expect("dry run should succeed");

        assert!(
            !workspace.join(".claude").exists(),
            "dry-run install must not create .claude"
        );
        assert!(
            !workspace.join(".codanna/plugins/lockfile.json").exists(),
            "dry-run install must not create lockfile"
        );
        assert!(
            !workspace.join(".mcp.json").exists(),
            "dry-run install must not merge MCP config"
        );
    });
}

#[test]
fn remove_plugin_cleans_files_and_lockfile() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";

        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        plugins::remove_plugin(&settings, "codanna-cc-plugin", false, false)
            .expect("remove succeeds");

        assert!(
            !workspace
                .join(".claude/commands/codanna-cc-plugin/ask.md")
                .exists()
        );
        assert!(!workspace.join(".claude/scripts/codanna-cc-plugin").exists());

        let lockfile = read_json(workspace, ".codanna/plugins/lockfile.json");
        assert!(
            lockfile
                .get("plugins")
                .and_then(|plugins| plugins.get("codanna-cc-plugin"))
                .is_none()
        );

        if workspace.join(".mcp.json").exists() {
            let mcp = read_json(workspace, ".mcp.json");
            let missing = |key: &str| {
                mcp.get("mcpServers")
                    .and_then(|servers| servers.get(key))
                    .is_none()
            };
            assert!(missing("codanna"));
            assert!(missing("codanna-sse"));
        }
    });
}

#[test]
fn verify_plugin_detects_tampering() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";

        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        plugins::verify_plugin(&settings, "codanna-cc-plugin", false)
            .expect("verification succeeds before tampering");

        let target_file = workspace.join(".claude/commands/codanna-cc-plugin/ask.md");
        fs::write(&target_file, "tampered content").expect("tamper file");

        let err = plugins::verify_plugin(&settings, "codanna-cc-plugin", false)
            .expect_err("verification should fail after tampering");

        assert!(matches!(err, PluginError::IntegrityCheckFailed { .. }));
    });
}

#[test]
fn list_plugins_reports_state() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";

        plugins::list_plugins(&settings, false, true).expect("list succeeds when empty");

        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        plugins::list_plugins(&settings, true, false).expect("list succeeds after install");
    });
}

#[test]
fn update_plugin_restores_integrity() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";

        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        let target_file = workspace.join(".claude/commands/codanna-cc-plugin/ask.md");
        let original = fs::read_to_string(&target_file).expect("read original command");
        fs::write(&target_file, "tampered content").expect("tamper file");

        plugins::update_plugin(&settings, "codanna-cc-plugin", None, true, false)
            .expect("update succeeds");

        let restored = fs::read_to_string(&target_file).expect("read restored command");
        assert_eq!(restored, original);

        plugins::verify_plugin(&settings, "codanna-cc-plugin", false)
            .expect("verification passes after update");
    });
}

#[test]
fn update_plugin_dry_run_does_not_modify_workspace() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";

        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        let lockfile_before = read_json(workspace, ".codanna/plugins/lockfile.json");
        let commit_before = lockfile_before["plugins"]["codanna-cc-plugin"]["commit"]
            .as_str()
            .unwrap()
            .to_string();

        plugins::update_plugin(&settings, "codanna-cc-plugin", None, false, true)
            .expect("dry-run update succeeds");

        let lockfile_after = read_json(workspace, ".codanna/plugins/lockfile.json");
        let commit_after = lockfile_after["plugins"]["codanna-cc-plugin"]["commit"]
            .as_str()
            .unwrap()
            .to_string();

        assert_eq!(commit_before, commit_after);
    });
}

#[test]
fn update_plugin_requires_installation() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let err = plugins::update_plugin(&settings, "codanna-cc-plugin", None, false, false)
            .expect_err("update should fail when plugin missing");

        assert!(matches!(err, PluginError::NotInstalled { .. }));
    });
}

#[test]
fn install_fails_on_invalid_manifest() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let repo_url = create_marketplace_repo(
            workspace,
            "invalid_manifest_repo",
            "invalid-plugin",
            r#"{
    "name": "invalid-plugin",
    "version": "0.1.0",
    "description": "Invalid plugin",
    "author": { "name": "Test" },
    "commands": "invalid-path"
}"#,
            &[],
        );

        let err = plugins::add_plugin(&settings, &repo_url, "invalid-plugin", None, false, false)
            .expect_err("expected invalid manifest error");
        match err {
            PluginError::InvalidPluginManifest { .. } => {}
            other => panic!("unexpected error: {other}"),
        }
    });
}

#[test]
fn install_fails_on_mcp_conflict() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace = "https://github.com/bartolli/codanna-cc-plugin.git";

        plugins::add_plugin(
            &settings,
            marketplace,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        let repo_url = create_marketplace_repo(
            workspace,
            "conflict_repo",
            "conflict-plugin",
            r#"{
    "name": "conflict-plugin",
    "version": "0.1.0",
    "description": "Conflicting MCP server",
    "author": { "name": "Test" },
    "commands": "./commands/conflict.md",
    "mcpServers": "./.mcp.json"
}"#,
            &[
                (
                    "commands/conflict.md",
                    "# Conflict Command\n\nThis command should not install.",
                ),
                (
                    ".mcp.json",
                    r#"{
    "mcpServers": {
        "codanna": {
            "command": "echo",
            "args": ["conflict"]
        }
    }
}"#,
                ),
            ],
        );

        let err = plugins::add_plugin(&settings, &repo_url, "conflict-plugin", None, false, false)
            .expect_err("expected MCP conflict");
        match err {
            PluginError::McpServerConflict { key } if key == "codanna" => {}
            other => panic!("unexpected error: {other}"),
        }

        assert!(
            !workspace.join(".claude/commands/conflict-plugin").exists(),
            "conflicting plugin should not leave installed commands"
        );

        let lockfile = read_json(workspace, ".codanna/plugins/lockfile.json");
        assert!(
            lockfile["plugins"].get("conflict-plugin").is_none(),
            "lockfile should not record failed plugin"
        );
    });
}
