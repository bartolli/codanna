//! Emission-semantics gate: an index stamped with a different (or no)
//! emission version is never read or incrementally extended.
//! `codanna index` heals by full rebuild; read paths and dry-run refuse
//! with the heal command; `--force` clears unconditionally.

use serde_json::Value;
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
pub fn gate_target() -> i32 {
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

fn meta_path(workspace: &Path) -> PathBuf {
    workspace.join(".codanna/index/index.meta")
}

/// Rewrite index.meta to look like an index built by a binary with
/// different emission semantics. `version` None simulates a pre-gate
/// binary (no field at all).
fn tamper_emission_version(workspace: &Path, version: Option<u64>) {
    let path = meta_path(workspace);
    let raw = std::fs::read_to_string(&path).expect("read index.meta");
    let mut meta: Value = serde_json::from_str(&raw).expect("parse index.meta");
    let obj = meta.as_object_mut().expect("index.meta is an object");
    match version {
        Some(v) => {
            obj.insert("emission_version".to_string(), Value::from(v));
        }
        None => {
            obj.remove("emission_version");
        }
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&meta).expect("serialize"),
    )
    .expect("write tampered index.meta");
}

fn stored_emission_version(workspace: &Path) -> Option<u64> {
    let raw = std::fs::read_to_string(meta_path(workspace)).expect("read index.meta");
    let meta: Value = serde_json::from_str(&raw).expect("parse index.meta");
    meta.get("emission_version").and_then(Value::as_u64)
}

fn seed_workspace() -> TempDir {
    let workspace = TempDir::new().expect("temp dir");
    write_fixture(workspace.path());
    write_settings(workspace.path());
    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", "src", "--no-progress"]);
    assert_eq!(
        code, 0,
        "seed index should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stored_emission_version(workspace.path()).is_some(),
        "seed save must stamp emission_version"
    );
    workspace
}

const REFUSAL_EXIT: i32 = 7;

#[test]
fn index_heals_unstamped_index_with_full_rebuild() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path(), None);

    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", "src", "--no-progress"]);
    assert_eq!(
        code, 0,
        "heal run should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Rebuilding from scratch"),
        "heal run must announce the rebuild\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("index: none"),
        "heal message names the unstamped index\nstderr:\n{stderr}"
    );
    assert!(
        stored_emission_version(workspace.path()).is_some(),
        "healed index must be stamped"
    );

    // The healed index serves reads without the gate firing again.
    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["mcp", "find_symbol", "name:gate_target", "--json"],
    );
    assert_eq!(
        code, 0,
        "post-heal read should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn index_heals_mismatched_stamp_and_names_versions() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path(), Some(999));

    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", "src", "--no-progress"]);
    assert_eq!(
        code, 0,
        "heal run should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("index: v999"),
        "heal message names the stored version\nstderr:\n{stderr}"
    );
}

#[test]
fn read_path_refuses_stale_index_with_heal_command() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path(), None);

    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["mcp", "find_symbol", "name:gate_target", "--json"],
    );
    assert_eq!(
        code, REFUSAL_EXIT,
        "stale read must refuse\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Run 'codanna index' to rebuild"),
        "refusal must carry the heal command\nstderr:\n{stderr}"
    );
}

#[test]
fn dry_run_refuses_stale_index() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path(), None);

    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--dry-run", "--no-progress"],
    );
    assert_eq!(
        code, REFUSAL_EXIT,
        "dry-run over a stale index must refuse\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn force_bypasses_gate_and_restamps() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path(), None);

    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(
        code, 0,
        "--force should succeed on a stale index\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stored_emission_version(workspace.path()).is_some(),
        "forced rebuild must stamp emission_version"
    );
}
