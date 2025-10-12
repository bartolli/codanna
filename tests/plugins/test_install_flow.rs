use std::fs;
use std::path::Path;

use codanna::plugins::error::PluginError;
use codanna::{Settings, plugins};
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
    settings.debug = true;
    settings
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
            ".claude/plugins/codanna-cc-plugin/scripts/context-provider.js",
        );
        assert_file_exists(
            workspace,
            ".claude/plugins/codanna-cc-plugin/scripts/formatters/symbol.js",
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
        assert!(!workspace.join(".claude/plugins/codanna-cc-plugin").exists());

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
