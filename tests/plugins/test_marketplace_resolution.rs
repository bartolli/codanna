use std::fs;
use std::path::{Path, PathBuf};

use codanna::{Settings, plugins};
use git2::{IndexAddOption, Repository, Signature};
use tempfile::TempDir;

fn with_temp_workspace<F>(test: F)
where
    F: FnOnce(&Path),
{
    let temp_dir = TempDir::new().expect("create temp dir");
    test(temp_dir.path());
}

fn load_workspace_settings(workspace: &Path) -> Settings {
    let config_dir = workspace.join(".codanna");
    fs::create_dir_all(&config_dir).expect("create .codanna directory");

    let settings_path = config_dir.join("settings.toml");
    if !settings_path.exists() {
        fs::write(&settings_path, b"index_path = \"index\"\n").expect("write settings.toml");
    }

    let mut settings = Settings::load_from(&settings_path).expect("load workspace settings");
    settings.workspace_root = Some(workspace.to_path_buf());
    settings
}

fn init_git_repo(path: &Path) {
    let repo = Repository::init(path).expect("init git repo");
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
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, contents).expect("write file");
}

fn path_to_file_url(path: &Path) -> String {
    // Convert path to forward slashes for cross-platform compatibility
    let path_str = path.display().to_string().replace('\\', "/");

    // file:// URLs require three slashes total on all platforms:
    // - Unix:    file:///path/to/repo
    // - Windows: file:///C:/path/to/repo
    if path_str.starts_with('/') {
        format!("file://{path_str}")
    } else {
        format!("file:///{path_str}")
    }
}

fn create_marketplace_with_plugin_root(workspace: &Path) -> String {
    let repo_path = workspace.join("marketplace");
    let marketplace_dir = repo_path.join(".claude-plugin");
    let plugin_dir = repo_path.join("plugins/codanna-cc-plugin");
    let plugin_manifest_dir = plugin_dir.join(".claude-plugin");

    write_file(
        &marketplace_dir.join("marketplace.json"),
        r#"{
  "name": "codanna-market",
  "owner": { "name": "Test" },
  "metadata": { "pluginRoot": "./plugins" },
  "plugins": [
    {
      "name": "codanna-cc-plugin",
      "source": "./codanna-cc-plugin",
      "description": "Codanna plugin in subdirectory"
    }
  ]
}"#,
    );

    write_file(
        &plugin_manifest_dir.join("plugin.json"),
        r#"{
  "name": "codanna-cc-plugin",
  "version": "1.0.0",
  "description": "Codanna integration",
  "author": {"name": "Test"},
  "commands": ["./commands/ask.md"]
}"#,
    );

    write_file(
        &plugin_dir.join("commands/ask.md"),
        "# Ask\n\nUse Codanna semantic search.",
    );

    init_git_repo(&repo_path);
    repo_path.to_string_lossy().to_string()
}

fn create_external_plugin_repo(base: &Path, name: &str) -> PathBuf {
    let repo_path = base.join(name);
    write_file(
        &repo_path.join(".claude-plugin/plugin.json"),
        r#"{
  "name": "external-plugin",
  "version": "0.1.0",
  "description": "External source plugin",
  "author": {"name": "Test"},
  "commands": ["./commands/external.md"]
}"#,
    );
    write_file(
        &repo_path.join("commands/external.md"),
        "# External Command\n\nRuns external workflow.",
    );
    init_git_repo(&repo_path);
    repo_path
}

#[test]
fn install_uses_marketplace_plugin_root() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let repo_url = create_marketplace_with_plugin_root(workspace);

        plugins::add_plugin(
            &settings,
            &repo_url,
            "codanna-cc-plugin",
            None,
            false,
            false,
        )
        .expect("install succeeds");

        assert!(
            workspace
                .join(".claude/commands/codanna-cc-plugin/ask.md")
                .exists(),
            "plugin command should be copied from marketplace subdirectory"
        );
    });
}

#[test]
fn install_supports_git_source_object() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace_repo = workspace.join("catalog");
        let marketplace_dir = marketplace_repo.join(".claude-plugin");
        fs::create_dir_all(&marketplace_dir).expect("create marketplace dir");

        let plugin_repo = create_external_plugin_repo(workspace, "external-source");
        let file_url = path_to_file_url(&plugin_repo);

        let marketplace_json = format!(
            r#"{{
  "name": "catalog",
  "owner": {{"name": "Test"}},
  "plugins": [
    {{
      "name": "external-plugin",
      "source": {{
        "source": "git",
        "url": "{file_url}"
      }},
      "description": "External plugin source"
    }}
  ]
}}"#
        );

        write_file(&marketplace_dir.join("marketplace.json"), &marketplace_json);
        init_git_repo(&marketplace_repo);
        let repo_url = marketplace_repo.to_string_lossy().to_string();

        plugins::add_plugin(&settings, &repo_url, "external-plugin", None, false, false)
            .expect("install succeeds");

        assert!(
            workspace
                .join(".claude/commands/external-plugin/external.md")
                .exists(),
            "external plugin command should be copied from git source"
        );
    });
}

#[test]
fn update_external_plugin_detects_up_to_date() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let marketplace_repo = workspace.join("catalog-update");
        let marketplace_dir = marketplace_repo.join(".claude-plugin");
        fs::create_dir_all(&marketplace_dir).expect("create marketplace dir");

        let plugin_repo = create_external_plugin_repo(workspace, "external-update-source");
        let file_url = path_to_file_url(&plugin_repo);

        let marketplace_json = format!(
            r#"{{
  "name": "catalog",
  "owner": {{"name": "Test"}},
  "plugins": [
    {{
      "name": "external-plugin",
      "source": {{
        "source": "git",
        "url": "{file_url}"
      }}
    }}
  ]
}}"#
        );

        write_file(&marketplace_dir.join("marketplace.json"), &marketplace_json);
        init_git_repo(&marketplace_repo);
        let repo_url = marketplace_repo.to_string_lossy().to_string();

        plugins::add_plugin(&settings, &repo_url, "external-plugin", None, false, false)
            .expect("install succeeds");

        let lockfile_path = workspace.join(".codanna/plugins/lockfile.json");
        let before: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&lockfile_path).expect("read lockfile"))
                .expect("parse lockfile");
        let before_timestamp = before["plugins"]["external-plugin"]["updated_at"]
            .as_str()
            .expect("updated_at string")
            .to_string();

        plugins::update_plugin(&settings, "external-plugin", None, false, false)
            .expect("update succeeds");

        let after: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&lockfile_path).expect("read lockfile"))
                .expect("parse lockfile");
        let after_timestamp = after["plugins"]["external-plugin"]["updated_at"]
            .as_str()
            .expect("updated_at string")
            .to_string();

        assert_eq!(
            before_timestamp, after_timestamp,
            "update should be skipped when external source commit matches"
        );
    });
}

#[test]
fn install_allows_strict_false_without_plugin_manifest() {
    with_temp_workspace(|workspace| {
        let settings = load_workspace_settings(workspace);
        let repo_path = workspace.join("strict-false-market");
        let marketplace_dir = repo_path.join(".claude-plugin");
        fs::create_dir_all(&marketplace_dir).expect("create marketplace dir");

        let plugin_dir = repo_path.join("plugins/loose-plugin");
        write_file(
            &plugin_dir.join("commands/loose.md"),
            "# Loose Command\n\nMarketplace-driven manifest.",
        );

        write_file(
            &marketplace_dir.join("marketplace.json"),
            r#"{
  "name": "strict-test",
  "owner": { "name": "Test" },
  "metadata": { "pluginRoot": "./plugins" },
  "plugins": [
    {
      "name": "loose-plugin",
      "source": "./loose-plugin",
      "description": "Provided entirely by marketplace",
      "commands": ["./commands/loose.md"],
      "strict": false
    }
  ]
}"#,
        );

        init_git_repo(&repo_path);
        let repo_url = repo_path.to_string_lossy().to_string();

        plugins::add_plugin(&settings, &repo_url, "loose-plugin", None, false, false)
            .expect("install succeeds");

        assert!(
            workspace
                .join(".claude/commands/loose-plugin/loose.md")
                .exists(),
            "command declared in marketplace manifest should be copied even without plugin.json"
        );
    });
}
