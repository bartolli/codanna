//! `codanna dump`: the resolved graph as a JSON Lines envelope stream
//! (`begin`, one `result` per item, terminal `summary`); the stale-index
//! gate refuses it like every other read verb.

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
    let test_home = workspace.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");
    let output = Command::new(codanna_binary())
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

fn seed_workspace() -> TempDir {
    let workspace = TempDir::new().expect("temp dir");
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(
        src.join("lib.rs"),
        "pub fn callee() {}\n\npub fn caller() {\n    callee();\n}\n",
    )
    .expect("write fixture");
    let src_abs = src.canonicalize().expect("canonical src");
    let src_path = crate::common::toml_path_literal(&src_abs);
    let codanna_dir = workspace.path().join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    std::fs::write(
        codanna_dir.join("settings.toml"),
        format!(
            "index_path = \".codanna/index\"\n\n[indexing]\nindexed_paths = [{src_path}]\n\n[semantic_search]\nenabled = false\n"
        ),
    )
    .expect("write settings");
    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", "src", "--no-progress"]);
    assert_eq!(code, 0, "seed index\nstdout:\n{stdout}\nstderr:\n{stderr}");
    workspace
}

fn parse_lines(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {e}: {l}")))
        .collect()
}

#[test]
fn dump_streams_begin_result_summary_and_exits_zero() {
    let workspace = seed_workspace();
    let (code, stdout, stderr) = run_cli(workspace.path(), &["dump"]);
    assert_eq!(code, 0, "dump exit\nstdout:\n{stdout}\nstderr:\n{stderr}");

    let lines = parse_lines(&stdout);
    assert!(
        lines.len() >= 4,
        "begin + >=1 symbol + >=1 edge + summary:\n{stdout}"
    );
    let first = &lines[0];
    assert_eq!(first["type"], "begin");
    assert_eq!(first["meta"]["entity_type"], "graph");
    assert_eq!(first["meta"]["schema_version"], "1.0.0");
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "summary");
    assert_eq!(last["exit_code"], 0);

    let symbols = lines
        .iter()
        .filter(|l| l["type"] == "result" && l["meta"]["entity_type"] == "symbol")
        .count();
    let relationships = lines
        .iter()
        .filter(|l| l["type"] == "result" && l["meta"]["entity_type"] == "relationship")
        .count();
    assert!(symbols >= 2, "callee and caller are symbol items");
    assert!(
        relationships >= 1,
        "caller -> callee is a relationship item"
    );
    assert_eq!(last["data"]["symbols"], symbols);
    assert_eq!(last["data"]["relationships"], relationships);
    assert_eq!(last["meta"]["count"], symbols + relationships);

    let (code, info, _) = run_cli(workspace.path(), &["mcp", "get_index_info", "--json"]);
    assert_eq!(code, 0);
    let info: Value = serde_json::from_str(&info).expect("index info json");
    assert_eq!(info["data"]["symbol_count"], symbols);
    assert_eq!(info["data"]["relationship_count"], relationships);
}

#[test]
fn dump_refuses_stale_index_before_writing_any_line() {
    let workspace = seed_workspace();
    let meta_path = workspace.path().join(".codanna/index/index.meta");
    let raw = std::fs::read_to_string(&meta_path).expect("read index.meta");
    let mut meta: Value = serde_json::from_str(&raw).expect("parse index.meta");
    meta.as_object_mut()
        .expect("object")
        .insert("emission_version".to_string(), Value::from(999));
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).expect("tamper");

    let (code, stdout, stderr) = run_cli(workspace.path(), &["dump"]);
    assert_eq!(
        code, 7,
        "stale dump must refuse\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.is_empty(), "no stream lines on refusal:\n{stdout}");
    assert!(
        stderr.contains("Run 'codanna index' to rebuild"),
        "heal command\n{stderr}"
    );
}
