use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn with_temp_workspace<F>(test: F)
where
    F: FnOnce(&Path),
{
    let temp_dir = TempDir::new().expect("create temp dir");
    test(temp_dir.path());
}

fn prepare_workspace(workspace: &Path) {
    let config_dir = workspace.join(".codanna");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    let settings_path = config_dir.join("settings.toml");
    if !settings_path.exists() {
        std::fs::write(&settings_path, b"index_path = \"index\"\n").expect("write settings file");
    }
}

fn create_marketplace_repo(workspace: &Path, plugin_name: &str) -> String {
    let repo_path = workspace.join("cli-marketplace");
    let plugin_root = repo_path.join("plugin");
    let marketplace_dir = repo_path.join(".claude-plugin");
    let plugin_manifest_dir = plugin_root.join(".claude-plugin");

    std::fs::create_dir_all(&plugin_manifest_dir).expect("create plugin manifest dir");
    std::fs::create_dir_all(&marketplace_dir).expect("create marketplace dir");

    let marketplace_json = format!(
        r#"{{
    "name": "cli-marketplace",
    "owner": {{"name": "Test"}},
    "plugins": [
        {{
            "name": "{plugin_name}",
            "source": "./plugin",
            "description": "CLI plugin"
        }}
    ]
}}"#
    );
    std::fs::write(marketplace_dir.join("marketplace.json"), marketplace_json)
        .expect("write marketplace manifest");

    let plugin_manifest = format!(
        r#"{{
    "name": "{plugin_name}",
    "version": "0.1.0",
    "description": "CLI plugin",
    "author": {{ "name": "Test" }}
}}"#
    );
    std::fs::write(plugin_manifest_dir.join("plugin.json"), plugin_manifest)
        .expect("write plugin manifest");

    std::fs::create_dir_all(plugin_root.join("commands")).expect("create commands dir");
    std::fs::write(
        plugin_root.join("commands/cli-command.md"),
        "# CLI Command\n\nExample command.",
    )
    .expect("write command file");

    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&repo_path)
            .output()
            .unwrap_or_else(|e| panic!("failed to run git {}: {e}", args.join(" ")));
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["add", "-A"]);
    run(&["commit", "-m", "initial commit"]);

    repo_path.to_str().unwrap().to_string()
}

fn codanna_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_codanna") {
        return PathBuf::from(path);
    }

    if let Ok(path) = env::var("CODANNA_BIN") {
        let bin = PathBuf::from(path);
        if bin.exists() {
            return bin;
        }
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().expect("current dir"));

    let debug_bin = manifest_dir.join("target/debug/codanna");
    if debug_bin.exists() {
        return debug_bin;
    }

    let status = Command::new("cargo")
        .args(["build", "--bin", "codanna"])
        .current_dir(&manifest_dir)
        .status()
        .expect("build codanna binary");
    assert!(status.success(), "cargo build failed");
    debug_bin
}

fn run_cli(workspace: &Path, args: &[&str]) -> (i32, String, String) {
    let bin = codanna_binary();
    let test_home = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".codanna-test");
    std::fs::create_dir_all(&test_home).expect("create test home directory");

    let output = Command::new(&bin)
        .args(args)
        .current_dir(workspace)
        .env("HOME", &test_home)
        .output()
        .expect("run codanna CLI");

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    (code, stdout, stderr)
}

#[test]
fn dry_run_add_reports_expected_output() {
    with_temp_workspace(|workspace| {
        prepare_workspace(workspace);
        let repo_url = create_marketplace_repo(workspace, "demo-plugin");
        let (code, stdout, stderr) = run_cli(
            workspace,
            &["plugin", "add", &repo_url, "demo-plugin", "--dry-run"],
        );

        assert_eq!(code, 0, "dry-run add should succeed, stderr: {stderr}");
        assert!(
            stdout.contains("DRY RUN: Would install plugin 'demo-plugin'"),
            "stdout should mention dry-run install, got:\n{stdout}"
        );
    });
}

#[test]
fn dry_run_update_succeeds_without_install() {
    with_temp_workspace(|workspace| {
        prepare_workspace(workspace);
        let (code, stdout, stderr) =
            run_cli(workspace, &["plugin", "update", "demo-plugin", "--dry-run"]);

        assert_eq!(code, 3);
        assert!(stdout.is_empty());
        assert!(
            stderr.contains("Plugin 'demo-plugin' is not installed"),
            "stderr should mention missing plugin, got:\n{stderr}"
        );
    });
}

#[test]
fn dry_run_remove_succeeds_without_install() {
    with_temp_workspace(|workspace| {
        prepare_workspace(workspace);
        let (code, stdout, stderr) =
            run_cli(workspace, &["plugin", "remove", "demo-plugin", "--dry-run"]);

        assert_eq!(code, 0, "dry-run remove should succeed, stderr: {stderr}");
        assert!(
            stdout.contains("DRY RUN: Would remove plugin 'demo-plugin'"),
            "stdout should mention dry-run remove, got:\n{stdout}"
        );
    });
}

#[test]
fn update_reports_already_up_to_date() {
    with_temp_workspace(|workspace| {
        prepare_workspace(workspace);
        let repo_url = create_marketplace_repo(workspace, "demo-plugin");

        let (code, stdout, stderr) =
            run_cli(workspace, &["plugin", "add", &repo_url, "demo-plugin"]);
        assert_eq!(code, 0, "install should succeed, stderr: {stderr}");
        assert!(
            stdout.contains("Plugin 'demo-plugin' installed"),
            "stdout should mention successful install, got:\n{stdout}"
        );

        let (code, stdout, stderr) = run_cli(workspace, &["plugin", "update", "demo-plugin"]);
        assert_eq!(code, 0, "update should succeed, stderr: {stderr}");
        assert!(
            stdout.contains("Plugin 'demo-plugin' already up to date"),
            "stdout should report up-to-date status, got:\n{stdout}"
        );
    });
}
