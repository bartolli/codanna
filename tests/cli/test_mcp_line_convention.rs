//! Line-convention and location-composition contract for `codanna mcp`.
//!
//! Scalar `line` fields are 1-indexed editor coordinates on every channel
//! (search rows, tuple relationshipMetadata, inline call_line); text
//! locations name real places (callee def + explicit call site). Search
//! rows report the symbol's true kind via the one kind vocabulary.

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

// Fixture layout is load-bearing: assertions below name these exact
// 1-indexed lines.
const RUST_FIXTURE: &str = "\
pub struct MarkerStructKind;

pub fn line_target() -> i32 {
    7
}

pub fn line_caller() -> i32 {
    line_target() + 1
}
";
const TARGET_DEF_LINE: i64 = 3;
const CALLER_CALL_LINE: i64 = 8;

const JAVA_FIXTURE: &str = "\
public class MarkerClassKind {
    public int markerMethodKind() {
        return 7;
    }
}
";

fn setup(workspace: &Path) {
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(src.join("fixture.rs"), RUST_FIXTURE).expect("write rust fixture");
    std::fs::write(src.join("Marker.java"), JAVA_FIXTURE).expect("write java fixture");

    let codanna_dir = workspace.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    let src_abs = src.canonicalize().expect("resolvable src");
    let settings = format!(
        r#"
index_path = ".codanna/index"

[indexing]
indexed_paths = ["{}"]

[semantic_search]
enabled = false
"#,
        src_abs.to_str().expect("utf-8 path")
    );
    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");

    let (code, stdout, stderr) = run_cli(workspace, &["index", "src", "--force", "--no-progress"]);
    assert_eq!(
        code, 0,
        "index should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn json_data(stdout: &str) -> Value {
    let payload: Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON: {e}\nstdout:\n{stdout}"));
    payload["data"].clone()
}

#[test]
fn search_rows_carry_one_indexed_lines_and_true_kinds() {
    let workspace = TempDir::new().expect("temp dir");
    setup(workspace.path());

    let (code, stdout, _) = run_cli(
        workspace.path(),
        &["mcp", "search_symbols", "query:line_target", "--json"],
    );
    assert_eq!(code, 0);
    let row = &json_data(&stdout)[0]["symbol"];
    assert_eq!(
        row["line"].as_i64(),
        Some(TARGET_DEF_LINE),
        "search line must be the 1-indexed def line\nrow: {row}"
    );

    for (query, expected_kind, expected_lang) in [
        ("MarkerStructKind", "Struct", "rust"),
        ("MarkerClassKind", "Class", "java"),
        ("markerMethodKind", "Method", "java"),
    ] {
        let (code, stdout, _) = run_cli(
            workspace.path(),
            &["mcp", "search_symbols", &format!("query:{query}"), "--json"],
        );
        assert_eq!(code, 0, "search for {query}");
        let row = &json_data(&stdout)[0]["symbol"];
        assert_eq!(
            row["kind"].as_str(),
            Some(expected_kind),
            "search must report the true kind for {query}\nrow: {row}"
        );
        assert_eq!(
            row["language_id"].as_str(),
            Some(expected_lang),
            "search rows must carry language_id\nrow: {row}"
        );
    }
}

#[test]
fn index_info_languages_map_partitions_symbol_count() {
    let workspace = TempDir::new().expect("temp dir");
    setup(workspace.path());

    let (code, stdout, _) = run_cli(workspace.path(), &["mcp", "get_index_info", "--json"]);
    assert_eq!(code, 0);
    let data = json_data(&stdout);
    let languages = data["languages"]
        .as_object()
        .expect("languages map present");
    assert!(languages.contains_key("rust") && languages.contains_key("java"));
    let sum: i64 = languages.values().filter_map(|v| v.as_i64()).sum();
    assert_eq!(
        Some(sum),
        data["symbol_count"].as_i64(),
        "languages map must partition symbol_count"
    );

    let (code, stdout, _) = run_cli(workspace.path(), &["mcp", "get_index_info"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Languages:") && stdout.contains("- java:"),
        "text rendering must show the languages section\nstdout:\n{stdout}"
    );
}

#[test]
fn tuple_metadata_line_matches_inline_call_line() {
    let workspace = TempDir::new().expect("temp dir");
    setup(workspace.path());

    // Inline channel: find_callers call_line
    let (code, stdout, _) = run_cli(
        workspace.path(),
        &["mcp", "find_callers", "function_name:line_target", "--json"],
    );
    assert_eq!(code, 0);
    let inline = json_data(&stdout)[0]["call_line"]
        .as_i64()
        .expect("caller row carries call_line");
    assert_eq!(inline, CALLER_CALL_LINE, "inline call_line is 1-indexed");

    // Tuple channel: find_symbol relationships.called_by metadata line
    let (code, stdout, _) = run_cli(
        workspace.path(),
        &["mcp", "find_symbol", "name:line_target", "--json"],
    );
    assert_eq!(code, 0);
    let called_by = &json_data(&stdout)[0]["relationships"]["called_by"][0];
    let tuple_line = called_by[1]["line"]
        .as_i64()
        .expect("tuple metadata carries line");
    assert_eq!(
        tuple_line, inline,
        "tuple relationshipMetadata.line must equal inline call_line on the same edge"
    );
}

#[test]
fn get_calls_text_names_def_and_call_site() {
    let workspace = TempDir::new().expect("temp dir");
    setup(workspace.path());

    let (code, stdout, _) = run_cli(
        workspace.path(),
        &["mcp", "get_calls", "function_name:line_caller"],
    );
    assert_eq!(code, 0, "stdout:\n{stdout}");
    let line = stdout
        .lines()
        .find(|l| l.contains("line_target"))
        .unwrap_or_else(|| panic!("no callee row\nstdout:\n{stdout}"));
    assert!(
        line.contains(&format!("fixture.rs:{TARGET_DEF_LINE}")),
        "callee location must be its def line\nrow: {line}"
    );
    assert!(
        line.contains("(called at ") && line.contains(&format!(":{CALLER_CALL_LINE})")),
        "call site must be named explicitly in the caller's file\nrow: {line}"
    );
}
