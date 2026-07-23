//! file_path contract shape: emitted paths are relative to the file's
//! indexed root in both indexing modes. Out-of-tree indexes store
//! absolute paths internally; the serving boundary decodes them, so
//! in-tree and out-of-tree emit identical shapes for the same file.

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

fn write_corpus(dir: &Path) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(
        src.join("alpha.rs"),
        r#"
pub fn portable_target() -> i32 {
    1
}
"#,
    )
    .expect("write corpus fixture");
}

fn write_settings(workspace: &Path, indexed_path: &Path) {
    let codanna_dir = workspace.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    let indexed = indexed_path.to_str().expect("path is valid UTF-8");
    let settings = format!(
        r#"
index_path = ".codanna/index"

[indexing]
indexed_paths = ["{indexed}"]

[semantic_search]
enabled = false
"#
    );
    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
}

fn emitted_file_path(workspace: &Path) -> String {
    let (code, stdout, stderr) = run_cli(
        workspace,
        &["mcp", "find_symbol", "name:portable_target", "--json"],
    );
    assert_eq!(
        code, 0,
        "find_symbol should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let payload: Value = serde_json::from_str(&stdout).expect("JSON envelope");
    payload["data"][0]["file_path"]
        .as_str()
        .expect("file_path present")
        .to_string()
}

#[test]
fn out_of_tree_index_emits_root_relative_file_path() {
    let corpus = TempDir::new().expect("corpus dir");
    let corpus_root = corpus.path().canonicalize().expect("canonical corpus");
    write_corpus(&corpus_root);

    let workspace = TempDir::new().expect("workspace dir");
    write_settings(workspace.path(), &corpus_root);

    let corpus_arg = corpus_root.to_str().expect("UTF-8 path");
    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", corpus_arg, "--no-progress"]);
    assert_eq!(
        code, 0,
        "out-of-tree index should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert_eq!(emitted_file_path(workspace.path()), "src/alpha.rs");

    // search_symbols rows carry the same shape.
    let (code, stdout, _) = run_cli(
        workspace.path(),
        &["mcp", "search_symbols", "query:portable_target", "limit:3"],
    );
    assert_eq!(code, 0, "search should succeed");
    assert!(
        stdout.contains("src/alpha.rs") && !stdout.contains(corpus_arg),
        "search rows must show root-relative paths\nstdout:\n{stdout}"
    );
}

#[test]
fn in_tree_and_out_of_tree_emit_identical_shape() {
    // In-tree: workspace contains the source; init sets workspace_root.
    let ws_in = TempDir::new().expect("in-tree ws");
    write_corpus(ws_in.path());
    let (code, _, stderr) = run_cli(ws_in.path(), &["init"]);
    assert_eq!(code, 0, "init should succeed\nstderr:\n{stderr}");
    let (code, stdout, stderr) = run_cli(ws_in.path(), &["index", "src", "--no-progress"]);
    assert_eq!(
        code, 0,
        "in-tree index should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let in_tree = emitted_file_path(ws_in.path());

    // Out-of-tree: separate corpus indexed by absolute path.
    let corpus = TempDir::new().expect("corpus dir");
    let corpus_root = corpus.path().canonicalize().expect("canonical corpus");
    write_corpus(&corpus_root);
    let ws_out = TempDir::new().expect("out-of-tree ws");
    write_settings(ws_out.path(), &corpus_root);
    let (code, _, stderr) = run_cli(
        ws_out.path(),
        &["index", corpus_root.to_str().unwrap(), "--no-progress"],
    );
    assert_eq!(
        code, 0,
        "out-of-tree index should succeed\nstderr:\n{stderr}"
    );
    let out_of_tree = emitted_file_path(ws_out.path());

    assert_eq!(in_tree, out_of_tree, "shapes must be mode-independent");
    assert_eq!(in_tree, "src/alpha.rs");
}
