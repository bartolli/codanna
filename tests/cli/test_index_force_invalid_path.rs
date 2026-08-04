//! The force lane clears the persisted index during facade creation;
//! CLI path validation used to run only afterwards, so `codanna index
//! <missing path> --force` destroyed the store and rebuilt nothing.
//! The pre-clear existence gate refuses before any destruction.

use std::collections::BTreeSet;
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
pub fn force_gate_target() -> i32 {
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
    let src_path = crate::common::toml_path_literal(&src_abs);

    let settings = format!(
        r#"
index_path = ".codanna/index"

[indexing]
indexed_paths = [{src_path}]

[semantic_search]
enabled = false
"#
    );

    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
}

fn index_dir_entries(workspace: &Path) -> BTreeSet<String> {
    std::fs::read_dir(workspace.join(".codanna/index"))
        .expect("read index dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect()
}

#[test]
fn force_with_missing_path_refuses_before_clearing_index() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path();
    write_fixture(workspace);
    write_settings(workspace);

    let (exit, stdout, stderr) = run_cli(workspace, &["index", "src"]);
    assert_eq!(
        exit, 0,
        "seed must succeed\nstdout:{stdout}\nstderr:{stderr}"
    );

    let meta = workspace.join(".codanna/index/index.meta");
    assert!(meta.exists(), "seed must persist index.meta");
    let meta_before = std::fs::read(&meta).expect("read index.meta");
    let entries_before = index_dir_entries(workspace);

    let (exit, stdout, stderr) = run_cli(workspace, &["index", "missing-dir", "--force"]);
    assert_ne!(
        exit, 0,
        "force with a missing path must fail\nstdout:{stdout}\nstderr:{stderr}"
    );
    assert!(
        stderr.contains("Path does not exist"),
        "refusal must name the missing path:\nstderr:{stderr}"
    );

    assert!(
        meta.exists(),
        "persisted index must survive a refused force run"
    );
    let meta_after = std::fs::read(&meta).expect("read index.meta");
    assert_eq!(
        meta_before, meta_after,
        "index.meta must be byte-identical after the refused run"
    );
    assert_eq!(
        entries_before,
        index_dir_entries(workspace),
        "index directory contents must be untouched after the refused run"
    );
}

#[test]
fn force_with_existing_path_still_rebuilds() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path();
    write_fixture(workspace);
    write_settings(workspace);

    let (exit, stdout, stderr) = run_cli(workspace, &["index", "src"]);
    assert_eq!(
        exit, 0,
        "seed must succeed\nstdout:{stdout}\nstderr:{stderr}"
    );

    let (exit, stdout, stderr) = run_cli(workspace, &["index", "src", "--force"]);
    assert_eq!(
        exit, 0,
        "force with an existing path must rebuild\nstdout:{stdout}\nstderr:{stderr}"
    );
    assert!(
        stdout.contains("Index saved to"),
        "force rebuild must index and save:\nstdout:{stdout}\nstderr:{stderr}"
    );
}
