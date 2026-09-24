//! A relative import whose target file left the index must not bind by
//! name to a same-named export in another registered root, on either
//! indexing lane.
//!
//! Positive control: with `repo_a/target` present, both callers resolve
//! into it (golden captured from the release binary of the tree before
//! typed negative evidence landed). Defect: after the target is deleted,
//! the incremental lane and the forced rebuild each bound both callers to
//! `repo_b/target` on that tree.

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

fn run_ok(workspace: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = run_cli(workspace, args);
    assert_eq!(code, 0, "{args:?}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    stdout
}

/// Two registered roots under a workspace root, both holding `target`;
/// `repo_a/caller` imports `sharedTarget` from its own root.
fn write_workspace(ext: &str) -> TempDir {
    let temp = TempDir::new().expect("temp workspace");
    let root = temp.path().canonicalize().expect("canonical workspace");
    for repo in ["repo_a", "repo_b"] {
        std::fs::create_dir_all(root.join(repo)).expect("create repo");
        std::fs::write(
            root.join(repo).join(format!("target.{ext}")),
            "export function sharedTarget() { return 42; }\n",
        )
        .expect("write target");
    }
    std::fs::write(
        root.join("repo_a").join(format!("caller.{ext}")),
        "import { sharedTarget } from './target';\nexport function entry() { return sharedTarget(); }\nexport const arrowEntry = () => sharedTarget();\n",
    )
    .expect("write caller");

    let codanna_dir = root.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    let settings = format!(
        "workspace_root = {}\nindex_path = \".codanna/index\"\n\n[indexing]\nindexed_paths = [{}, {}]\n\n[semantic_search]\nenabled = false\n",
        crate::common::toml_path_literal(&root),
        crate::common::toml_path_literal(&root.join("repo_a")),
        crate::common::toml_path_literal(&root.join("repo_b")),
    );
    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
    temp
}

fn cell(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn ident(value: &Value) -> String {
    let path = value.as_str().unwrap_or("-").replace('\\', "/");
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

/// The relationship multiset of `dump --edges`, normalized like the
/// committed goldens: ids stripped, endpoints as `<root>/<file>`, every
/// metadata field kept, sorted, no dedup.
fn normalized_edges(workspace: &Path) -> String {
    let stdout = run_ok(workspace, &["dump", "--edges"]);
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {e}: {l}")))
        .collect();
    let summary = lines.last().expect("summary row");
    assert_eq!(summary["type"], "summary", "{stdout}");
    assert_eq!(summary["data"]["orphan_edges_dropped"], 0, "{stdout}");
    assert_eq!(summary["data"]["duplicate_symbol_ids"], 0, "{stdout}");

    let mut rows: Vec<String> = lines
        .iter()
        .filter(|env| env["type"] == "result" && env["meta"]["entity_type"] == "relationship")
        .map(|env| {
            let d = &env["data"];
            let m = &d["metadata"];
            let meta_cell = |key: &str| {
                if m.is_null() {
                    "-".to_string()
                } else {
                    cell(&m[key])
                }
            };
            [
                cell(&d["relation"]),
                cell(&d["from"]["name"]),
                cell(&d["from"]["kind"]),
                ident(&d["from"]["file_path"]),
                cell(&d["from"]["line"]),
                cell(&d["to"]["name"]),
                cell(&d["to"]["kind"]),
                ident(&d["to"]["file_path"]),
                cell(&d["to"]["line"]),
                meta_cell("line"),
                meta_cell("column"),
                meta_cell("receiver"),
                meta_cell("static_call"),
                meta_cell("context"),
            ]
            .join("\t")
        })
        .collect();
    rows.sort();
    rows.into_iter().map(|r| r + "\n").collect()
}

fn caller_rows(rows: &str) -> Vec<&str> {
    rows.lines()
        .filter(|row| row.starts_with("Calls\tentry\t") || row.starts_with("Calls\tarrowEntry\t"))
        .collect()
}

fn deleted_target_binds_nothing(ext: &str, golden: &str) {
    let temp = write_workspace(ext);
    let workspace = temp.path().canonicalize().expect("canonical workspace");

    run_ok(&workspace, &["index", "--no-progress"]);
    assert_eq!(
        normalized_edges(&workspace),
        golden,
        "{ext}: positive control, both callers resolve into repo_a/target"
    );

    std::fs::remove_file(workspace.join("repo_a").join(format!("target.{ext}")))
        .expect("delete target");

    // Both lanes run before any assertion so a failure reports each lane.
    let mut wrong: Vec<String> = Vec::new();
    for (lane, args) in [
        ("incremental", &["index", "--no-progress"][..]),
        ("forced rebuild", &["index", "--force", "--no-progress"][..]),
    ] {
        run_ok(&workspace, args);
        let rows = normalized_edges(&workspace);
        if !caller_rows(&rows).is_empty() {
            wrong.push(format!("{ext} {lane}:\n{rows}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "a dangling import must not bind to repo_b/target:\n{}",
        wrong.join("")
    );
}

/// A tsconfig path alias resolves only through the `index` command: the
/// provider cache is rebuilt from `config_files` there, and the rules are
/// read from `.codanna` relative to the process cwd. The alias is not a
/// relative specifier, so the lookup is `Unknown` and main's row survives.
#[test]
fn ts_tsconfig_alias_import_rows_equal_golden() {
    let temp = TempDir::new().expect("temp workspace");
    let root = temp.path().canonicalize().expect("canonical workspace");
    let lib = root.join("lib");
    std::fs::create_dir_all(lib.join("src")).expect("create src");
    std::fs::write(
        lib.join("src/dep.ts"),
        "export function f() { return 1; }\n",
    )
    .expect("write dep");
    std::fs::write(
        lib.join("src/caller.ts"),
        "import { f } from '@/dep';\nexport function entry() { return f(); }\n",
    )
    .expect("write caller");
    std::fs::write(
        lib.join("tsconfig.json"),
        "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"./src/*\"] } } }\n",
    )
    .expect("write tsconfig");

    let codanna_dir = root.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    let settings = format!(
        "index_path = \".codanna/index\"\n\n[indexing]\nindexed_paths = [{}]\n\n[semantic_search]\nenabled = false\n\n[languages.typescript]\nconfig_files = [{}]\n",
        crate::common::toml_path_literal(&lib),
        crate::common::toml_path_literal(&lib.join("tsconfig.json")),
    );
    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");

    run_ok(&root, &["index", "--force", "--no-progress"]);
    assert_eq!(
        normalized_edges(&root),
        include_str!("../fixtures/dangling_import/f8-alias-ts.rows")
    );
}

/// A workspace whose `lib/tsconfig.json` is `tsconfig`, registered as a
/// `config_files` entry, holding `files` under `lib`. Indexed on the
/// force lane; returns the normalized relationship multiset.
fn config_governed_edges(tsconfig: &str, files: &[(&str, &str)]) -> String {
    let temp = TempDir::new().expect("temp workspace");
    let root = temp.path().canonicalize().expect("canonical workspace");
    let lib = root.join("lib");
    for (rel, content) in files {
        let path = lib.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, content).expect("write file");
    }
    std::fs::write(lib.join("tsconfig.json"), tsconfig).expect("write tsconfig");

    let codanna_dir = root.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    let settings = format!(
        "index_path = \".codanna/index\"\n\n[indexing]\nindexed_paths = [{}]\n\n[semantic_search]\nenabled = false\n\n[languages.typescript]\nconfig_files = [{}]\n",
        crate::common::toml_path_literal(&lib),
        crate::common::toml_path_literal(&lib.join("tsconfig.json")),
    );
    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");

    run_ok(&root, &["index", "--force", "--no-progress"]);
    normalized_edges(&root)
}

const CALL_F_FROM_DEP: &str =
    "import { f } from './dep';\nexport function entry() { return f(); }\n";

// A config that redirects relative specifiers withholds negative
// evidence: `./dep` names `dep_native.ts` under `moduleSuffixes`, and
// `generated/dep.ts` under `rootDirs`; main's rows survive.
#[test]
fn ts_module_suffixes_config_rows_equal_golden() {
    assert_eq!(
        config_governed_edges(
            "{ \"compilerOptions\": { \"moduleSuffixes\": [\"_native\", \"\"] } }\n",
            &[
                ("src/dep_native.ts", "export function f() { return 1; }\n"),
                ("src/caller.ts", CALL_F_FROM_DEP),
            ],
        ),
        include_str!("../fixtures/dangling_import/cfg-suffixes-ts.rows")
    );
}

#[test]
fn ts_root_dirs_config_rows_equal_golden() {
    assert_eq!(
        config_governed_edges(
            "{ \"compilerOptions\": { \"rootDirs\": [\"src\", \"generated\"] } }\n",
            &[
                ("generated/dep.ts", "export function f() { return 1; }\n"),
                ("src/caller.ts", CALL_F_FROM_DEP),
            ],
        ),
        include_str!("../fixtures/dangling_import/cfg-rootdirs-ts.rows")
    );
}

/// Three registered roots: `repo_a` (caller and target), a TypeScript
/// namesake in `repo_b`, a Python namesake in `repo_c`.
fn write_three_root_workspace() -> TempDir {
    let temp = TempDir::new().expect("temp workspace");
    let root = temp.path().canonicalize().expect("canonical workspace");
    for repo in ["repo_a", "repo_b", "repo_c"] {
        std::fs::create_dir_all(root.join(repo)).expect("create repo");
    }
    for repo in ["repo_a", "repo_b"] {
        std::fs::write(
            root.join(repo).join("target.ts"),
            "export function sharedTarget() { return 42; }\n",
        )
        .expect("write target");
    }
    std::fs::write(
        root.join("repo_c/target.py"),
        "def sharedTarget():\n    return 42\n",
    )
    .expect("write python target");
    std::fs::write(
        root.join("repo_a/caller.ts"),
        "import { sharedTarget } from './target';\nexport function entry() { return sharedTarget(); }\nexport const arrowEntry = () => sharedTarget();\n",
    )
    .expect("write caller");

    let codanna_dir = root.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");
    let settings = format!(
        "workspace_root = {}\nindex_path = \".codanna/index\"\n\n[indexing]\nindexed_paths = [{}, {}, {}]\n\n[semantic_search]\nenabled = false\n",
        crate::common::toml_path_literal(&root),
        crate::common::toml_path_literal(&root.join("repo_a")),
        crate::common::toml_path_literal(&root.join("repo_b")),
        crate::common::toml_path_literal(&root.join("repo_c")),
    );
    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
    temp
}

// A name-only disambiguation witness: with the target deleted and two
// namesakes of which exactly one is same-language, the pre-fix tree
// bound both callers to the Python namesake on both lanes.
#[test]
fn ts_deleted_target_with_cross_language_namesake_binds_nothing_on_either_lane() {
    let temp = write_three_root_workspace();
    let workspace = temp.path().canonicalize().expect("canonical workspace");

    run_ok(&workspace, &["index", "--no-progress"]);
    assert_eq!(
        normalized_edges(&workspace),
        include_str!("../fixtures/dangling_import/f3b-ts.rows"),
        "positive control, both callers resolve into repo_a/target"
    );

    std::fs::remove_file(workspace.join("repo_a/target.ts")).expect("delete target");

    let mut wrong: Vec<String> = Vec::new();
    for (lane, args) in [
        ("incremental", &["index", "--no-progress"][..]),
        ("forced rebuild", &["index", "--force", "--no-progress"][..]),
    ] {
        run_ok(&workspace, args);
        let rows = normalized_edges(&workspace);
        if !caller_rows(&rows).is_empty() {
            wrong.push(format!("{lane}:\n{rows}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "a dangling import must not bind a namesake in any language:\n{}",
        wrong.join("")
    );
}

#[test]
fn ts_deleted_relative_import_target_binds_nothing_on_either_lane() {
    deleted_target_binds_nothing("ts", include_str!("../fixtures/dangling_import/f1-ts.rows"));
}

#[test]
fn js_deleted_relative_import_target_binds_nothing_on_either_lane() {
    deleted_target_binds_nothing("js", include_str!("../fixtures/dangling_import/f1-js.rows"));
}
