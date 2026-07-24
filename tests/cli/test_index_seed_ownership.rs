//! The index command owns indexing work. Pre-dispatch sync must not
//! race ahead of it against a freshly created index: deleting
//! `.codanna/index` and re-running `codanna index` used to run the
//! full pass in the sync phase (with the sync arm's "Indexing
//! directory:" label) and leave the command phase reporting
//! "Index up to date".

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

fn write_fixture(workspace: &Path) {
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(
        src.join("alpha.rs"),
        r#"
pub fn seed_target() -> i32 {
    1
}
"#,
    )
    .expect("write fixture");
}

fn write_settings(workspace: &Path) {
    let codanna_dir = workspace.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");

    let src_abs = workspace
        .join("src")
        .canonicalize()
        .expect("src dir should exist and be resolvable");
    let src_path = src_abs.to_str().expect("src path should be valid UTF-8");

    let settings = format!(
        r#"
index_path = ".codanna/index"

[indexing]
indexed_paths = ["{src_path}"]

[semantic_search]
enabled = false
"#
    );

    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
}

fn assert_single_command_phase_pass(exit: i32, stdout: &str, stderr: &str) {
    assert_eq!(
        exit, 0,
        "reindex must succeed\nstdout:{stdout}\nstderr:{stderr}"
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.contains("Index up to date"),
        "command phase must do the work, not no-op behind sync:\n{combined}"
    );
    assert!(
        !combined.contains("Indexing directory:"),
        "sync arm must not run the pass:\n{combined}"
    );
    assert!(
        stdout.contains("Index saved to"),
        "command phase must index and save:\nstdout:{stdout}\nstderr:{stderr}"
    );
}

#[test]
fn index_with_path_after_index_deletion_runs_single_pass() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path();
    write_fixture(workspace);
    write_settings(workspace);

    let (exit, stdout, stderr) = run_cli(workspace, &["index", "src"]);
    assert_eq!(
        exit, 0,
        "seed must succeed\nstdout:{stdout}\nstderr:{stderr}"
    );

    std::fs::remove_dir_all(workspace.join(".codanna/index")).expect("delete index dir");

    let (exit, stdout, stderr) = run_cli(workspace, &["index", "src"]);
    assert_single_command_phase_pass(exit, &stdout, &stderr);
}

#[test]
fn bare_index_after_index_deletion_runs_single_pass() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path();
    write_fixture(workspace);
    write_settings(workspace);

    let (exit, stdout, stderr) = run_cli(workspace, &["index"]);
    assert_eq!(
        exit, 0,
        "seed must succeed\nstdout:{stdout}\nstderr:{stderr}"
    );

    std::fs::remove_dir_all(workspace.join(".codanna/index")).expect("delete index dir");

    let (exit, stdout, stderr) = run_cli(workspace, &["index"]);
    assert_single_command_phase_pass(exit, &stdout, &stderr);
}
