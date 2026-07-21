//! Exit-code contract matrix for `codanna mcp` (tool x outcome x mode).
//!
//! The envelope's declared `exit_code` and the delivered process exit must
//! agree on every JSON path; text mode follows the same 0/1/2 vocabulary
//! (success / not_found / error) for outcome-bearing tools.

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
pub fn unique_target() -> i32 {
    41
}

pub fn unique_caller() -> i32 {
    unique_target() + 1
}

pub fn dup_name() -> i32 {
    1
}
"#,
    )
    .expect("write alpha fixture");
    std::fs::write(
        src.join("beta.rs"),
        r#"
pub fn dup_name() -> i32 {
    2
}
"#,
    )
    .expect("write beta fixture");
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

fn envelope_exit_code(stdout: &str) -> i64 {
    let payload: Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("stdout is not a JSON envelope: {e}\nstdout:\n{stdout}"));
    payload["exit_code"]
        .as_i64()
        .unwrap_or_else(|| panic!("envelope has no exit_code\nstdout:\n{stdout}"))
}

#[test]
fn mcp_exit_codes_match_envelope_across_tools_outcomes_and_modes() {
    let workspace = TempDir::new().expect("temp dir");
    write_fixture(workspace.path());
    write_settings(workspace.path());

    let (index_code, index_stdout, index_stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(
        index_code, 0,
        "index should succeed\nstdout:\n{index_stdout}\nstderr:\n{index_stderr}"
    );

    // JSON mode: process exit must equal the envelope's declared exit_code.
    // (tool invocation, expected exit)
    let json_cases: &[(&[&str], i32)] = &[
        (&["mcp", "find_symbol", "name:unique_target", "--json"], 0),
        (&["mcp", "find_symbol", "name:zz_missing", "--json"], 1),
        (
            &["mcp", "get_calls", "function_name:zz_missing", "--json"],
            1,
        ),
        (
            &["mcp", "find_callers", "function_name:zz_missing", "--json"],
            1,
        ),
        (
            &["mcp", "analyze_impact", "symbol_name:zz_missing", "--json"],
            1,
        ),
        (&["mcp", "search_symbols", "query:zzqqxx", "--json"], 1),
        (&["mcp", "get_calls", "function_name:dup_name", "--json"], 2),
        (&["mcp", "get_calls", "name:unique_target", "--json"], 2),
        (
            &[
                "mcp",
                "analyze_impact",
                "symbol_name:unique_target",
                "bogus:1",
                "--json",
            ],
            2,
        ),
    ];

    for (args, expected) in json_cases {
        let (code, stdout, stderr) = run_cli(workspace.path(), args);
        assert_eq!(
            code, *expected,
            "process exit for {args:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        let declared = envelope_exit_code(&stdout);
        assert_eq!(
            declared, *expected as i64,
            "envelope exit_code must equal process exit for {args:?}\nstdout:\n{stdout}"
        );
    }

    // depth: is a documented alias of max_depth: on analyze_impact —
    // it must apply, never silently default.
    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &[
            "mcp",
            "analyze_impact",
            "symbol_name:unique_target",
            "depth:1",
            "--json",
        ],
    );
    assert_eq!(code, 0, "alias run should succeed\nstderr:\n{stderr}");
    let payload: Value = serde_json::from_str(&stdout).expect("parse alias envelope");
    assert_eq!(
        payload["meta"]["depth"].as_i64(),
        Some(1),
        "depth alias must apply, not default\nstdout:\n{stdout}"
    );

    // Text mode: same 0/1/2 vocabulary, no envelope to compare against.
    let text_cases: &[(&[&str], i32)] = &[
        (&["mcp", "find_symbol", "name:unique_target"], 0),
        (&["mcp", "find_symbol", "name:zz_missing"], 1),
        (&["mcp", "get_calls", "function_name:zz_missing"], 1),
        (&["mcp", "get_calls", "function_name:dup_name"], 2),
        (&["mcp", "get_calls", "name:unique_target"], 2),
        (
            &[
                "mcp",
                "analyze_impact",
                "symbol_name:unique_target",
                "bogus:1",
            ],
            2,
        ),
    ];

    for (args, expected) in text_cases {
        let (code, stdout, stderr) = run_cli(workspace.path(), args);
        assert_eq!(
            code, *expected,
            "text-mode process exit for {args:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}
