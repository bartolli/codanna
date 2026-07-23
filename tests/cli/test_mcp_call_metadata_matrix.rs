//! Call-site metadata matrix for `codanna mcp` (language x tool x call form).
//!
//! Every Calls edge carries 1-indexed `call_line`/`call_column` matching the
//! call token's source line exactly, on both `get_calls` and `find_callers`,
//! for every call form the parser emits an edge for. Rust emission is the
//! reference per the line-index convention (0-indexed stored ranges,
//! 1-indexed scalar fields at the JSON boundary).

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

const RS_FIXTURE: &str = r#"pub struct RsWidget;

impl RsWidget {
    pub fn rs_new() -> Self {
        RsWidget
    }
    pub fn rs_helper(&self) -> i32 {
        1
    }
    pub fn rs_run(&self) -> i32 {
        self.rs_helper()
    }
}

pub fn rs_target() -> i32 {
    2
}

pub fn rs_free_caller() -> i32 {
    rs_target()
}

pub fn rs_ctor_caller() -> i32 {
    let w = RsWidget::rs_new();
    w.rs_run()
}
"#;

const PY_FIXTURE: &str = r#"class PyWidget:
    def py_helper(self):
        return 1

    def py_run(self):
        return self.py_helper()


def py_target():
    return 2


def py_free_caller():
    return py_target()


def py_ctor_caller():
    w = PyWidget()
    return w.py_run()
"#;

const JS_FIXTURE: &str = r#"class JsWidget {
  jsHelper() {
    return 1;
  }
  jsRun() {
    return this.jsHelper();
  }
}

function jsTarget() {
  return 2;
}

function jsFreeCaller() {
  return jsTarget();
}

function jsCtorCaller() {
  const w = new JsWidget();
  return w.jsRun();
}
"#;

const TS_FIXTURE: &str = r#"class TsWidget {
  tsHelper(): number {
    return 1;
  }
  tsRun(): number {
    return this.tsHelper();
  }
}

function tsTarget(): number {
  return 2;
}

function tsFreeCaller(): number {
  return tsTarget();
}

function tsCtorCaller(): number {
  const w = new TsWidget();
  return w.tsRun();
}
"#;

const GO_FIXTURE: &str = r#"package fixture

type GoWidget struct{}

func (w GoWidget) GoHelper() int {
	return 1
}

func (w GoWidget) GoRun() int {
	return w.GoHelper()
}

func GoTarget() int {
	return 2
}

func GoFreeCaller() int {
	return GoTarget()
}

func GoCtorCaller() int {
	w := GoWidget{}
	return w.GoRun()
}
"#;

fn write_fixtures(workspace: &Path) {
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    for (name, content) in [
        ("fixture.rs", RS_FIXTURE),
        ("fixture.py", PY_FIXTURE),
        ("fixture.js", JS_FIXTURE),
        ("fixture.ts", TS_FIXTURE),
        ("fixture.go", GO_FIXTURE),
    ] {
        std::fs::write(src.join(name), content).expect("write fixture");
    }
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

/// 1-indexed line of the call token in the fixture: exact stripped-line
/// match wins (rust expression-style lines), substring match second.
fn source_line(fixture: &str, token: &str) -> u32 {
    for (i, line) in fixture.lines().enumerate() {
        if line.trim() == token {
            return (i + 1) as u32;
        }
    }
    for (i, line) in fixture.lines().enumerate() {
        if line.contains(token) {
            return (i + 1) as u32;
        }
    }
    panic!("call token '{token}' not found in fixture");
}

#[test]
fn call_metadata_exact_across_languages_tools_and_forms() {
    let workspace = TempDir::new().expect("temp dir");
    write_fixtures(workspace.path());
    write_settings(workspace.path());

    let (index_code, index_stdout, index_stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(
        index_code, 0,
        "index should succeed\nstdout:\n{index_stdout}\nstderr:\n{index_stderr}"
    );

    // (fixture, tool, query symbol, edge symbol, call token)
    let matrix: &[(&str, &str, &str, &str, &str)] = &[
        // rust: reference — function, self-method, static-assoc, recv-method
        (
            RS_FIXTURE,
            "get_calls",
            "rs_free_caller",
            "rs_target",
            "rs_target()",
        ),
        (
            RS_FIXTURE,
            "get_calls",
            "rs_run",
            "rs_helper",
            "self.rs_helper()",
        ),
        (
            RS_FIXTURE,
            "get_calls",
            "rs_ctor_caller",
            "rs_new",
            "RsWidget::rs_new()",
        ),
        (
            RS_FIXTURE,
            "get_calls",
            "rs_ctor_caller",
            "rs_run",
            "w.rs_run()",
        ),
        (
            RS_FIXTURE,
            "find_callers",
            "rs_target",
            "rs_free_caller",
            "rs_target()",
        ),
        // python: function + ctor ride the plain channel, methods the method channel
        (
            PY_FIXTURE,
            "get_calls",
            "py_free_caller",
            "py_target",
            "return py_target()",
        ),
        (
            PY_FIXTURE,
            "get_calls",
            "py_run",
            "py_helper",
            "self.py_helper()",
        ),
        (
            PY_FIXTURE,
            "get_calls",
            "py_ctor_caller",
            "PyWidget",
            "PyWidget()",
        ),
        (
            PY_FIXTURE,
            "get_calls",
            "py_ctor_caller",
            "py_run",
            "w.py_run()",
        ),
        (
            PY_FIXTURE,
            "find_callers",
            "py_target",
            "py_free_caller",
            "return py_target()",
        ),
        // javascript: plain channel + method channel (double-increment regression)
        (
            JS_FIXTURE,
            "get_calls",
            "jsFreeCaller",
            "jsTarget",
            "return jsTarget()",
        ),
        (
            JS_FIXTURE,
            "get_calls",
            "jsRun",
            "jsHelper",
            "this.jsHelper()",
        ),
        (
            JS_FIXTURE,
            "get_calls",
            "jsCtorCaller",
            "jsRun",
            "w.jsRun()",
        ),
        (
            JS_FIXTURE,
            "find_callers",
            "jsHelper",
            "jsRun",
            "this.jsHelper()",
        ),
        // typescript: mirrors javascript
        (
            TS_FIXTURE,
            "get_calls",
            "tsFreeCaller",
            "tsTarget",
            "return tsTarget()",
        ),
        (
            TS_FIXTURE,
            "get_calls",
            "tsRun",
            "tsHelper",
            "this.tsHelper()",
        ),
        (
            TS_FIXTURE,
            "get_calls",
            "tsCtorCaller",
            "tsRun",
            "w.tsRun()",
        ),
        (
            TS_FIXTURE,
            "find_callers",
            "tsHelper",
            "tsRun",
            "this.tsHelper()",
        ),
        // go: plain channel + method channel (double-increment regression)
        (
            GO_FIXTURE,
            "get_calls",
            "GoFreeCaller",
            "GoTarget",
            "return GoTarget()",
        ),
        (GO_FIXTURE, "get_calls", "GoRun", "GoHelper", "w.GoHelper()"),
        (
            GO_FIXTURE,
            "find_callers",
            "GoTarget",
            "GoFreeCaller",
            "return GoTarget()",
        ),
        (
            GO_FIXTURE,
            "find_callers",
            "GoHelper",
            "GoRun",
            "w.GoHelper()",
        ),
    ];

    for (fixture, tool, query, edge_to, token) in matrix {
        let expected = source_line(fixture, token);
        let (code, stdout, stderr) = run_cli(
            workspace.path(),
            &["mcp", tool, &format!("function_name:{query}"), "--json"],
        );
        assert_eq!(
            code, 0,
            "{tool} {query} should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        let payload: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("{tool} {query}: bad envelope: {e}\n{stdout}"));
        let row = payload["data"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["name"].as_str() == Some(edge_to))
            .unwrap_or_else(|| panic!("{tool} {query}: edge to {edge_to} missing\n{stdout}"));
        let call_line = row["call_line"]
            .as_u64()
            .unwrap_or_else(|| panic!("{tool} {query} -> {edge_to}: call_line absent\nrow: {row}"));
        assert_eq!(
            call_line as u32, expected,
            "{tool} {query} -> {edge_to}: call_line must equal the 1-indexed source line of `{token}`\nrow: {row}"
        );
        assert!(
            row["call_column"].as_u64().is_some(),
            "{tool} {query} -> {edge_to}: call_column absent\nrow: {row}"
        );
    }
}
