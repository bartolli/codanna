//! Exit-code contract for a settings file that fails to parse.
//!
//! `Settings::load` treats a missing file as defaults, so a load error means
//! an existing `.codanna/settings.toml` did not parse. That is
//! `ExitCode::ConfigError` (6) for every command that loads settings, and
//! `init --force` stays reachable as the recovery.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn codanna_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_codanna") {
        let bin = PathBuf::from(path);
        if bin.exists() {
            return bin;
        }
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().expect("current dir"));

    let debug_bin = if cfg!(windows) {
        manifest_dir.join("target/debug/codanna.exe")
    } else {
        manifest_dir.join("target/debug/codanna")
    };
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
    let test_home = workspace.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");

    let output = Command::new(&bin)
        .args(args)
        .current_dir(workspace)
        .env("HOME", &test_home)
        .output()
        .expect("run codanna CLI");

    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn settings_parse_failure_exits_config_error_and_init_force_recovers() {
    let workspace = TempDir::new().expect("temp dir");
    let ws = workspace.path();

    let (code, stdout, stderr) = run_cli(ws, &["init"]);
    assert_eq!(
        code, 0,
        "init should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let settings_path = ws.join(".codanna").join("settings.toml");
    let mut settings = std::fs::read_to_string(&settings_path).expect("init writes settings.toml");
    assert!(
        settings.lines().any(|line| line == "[documents]"),
        "fixture precondition: init emits a [documents] table header\n{settings}"
    );
    settings.push_str("\n[documents]\n");
    std::fs::write(&settings_path, settings).expect("write duplicate-table settings");

    // Both commands exit at config load; `documents list` never reaches
    // its store, so no Tantivy directory is opened from this test.
    for args in [&["documents", "list"][..], &["config"][..]] {
        let (code, stdout, stderr) = run_cli(ws, args);
        assert_eq!(
            code, 6,
            "{args:?} on an unparsable settings file\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stderr.contains("Configuration error"),
            "stderr names the configuration error for {args:?}\nstderr:\n{stderr}"
        );
        assert!(
            stderr.contains("codanna init --force"),
            "stderr names the recovery for {args:?}\nstderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("Using default configuration"),
            "a parse failure is not a fallback to defaults for {args:?}\nstderr:\n{stderr}"
        );
    }

    let (code, stdout, stderr) = run_cli(ws, &["init", "--force"]);
    assert_eq!(
        code, 0,
        "init --force must stay reachable on an unparsable settings file\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let (code, stdout, stderr) = run_cli(ws, &["config"]);
    assert_eq!(
        code, 0,
        "config after recovery\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
