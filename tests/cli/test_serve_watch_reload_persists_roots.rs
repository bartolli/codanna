//! A root added to `indexed_paths` while `serve --watch` runs is indexed
//! by the settings reload and recorded in the index metadata, so the next
//! command's startup sync does not read it as missing.

use serde_json::Value;
use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

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

fn write_root(root: &Path, functions: &[&str]) {
    std::fs::create_dir_all(root).expect("create root");
    for name in functions {
        std::fs::write(
            root.join(format!("{name}.rs")),
            format!("pub fn {name}() -> i32 {{\n    1\n}}\n"),
        )
        .expect("write fixture");
    }
}

fn write_settings(workspace: &Path, roots: &[&str]) {
    let codanna_dir = workspace.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");

    // Canonical absolute paths: the metadata stores canonical roots, and
    // the settings reload diffs the file's paths verbatim.
    let literals = roots
        .iter()
        .map(|root| {
            let abs = workspace
                .join(root)
                .canonicalize()
                .expect("root dir should exist and be resolvable");
            crate::common::toml_path_literal(&abs)
        })
        .collect::<Vec<_>>()
        .join(", ");

    let settings = format!(
        r#"
index_path = ".codanna/index"

[indexing]
indexed_paths = [{literals}]

[semantic_search]
enabled = false
"#
    );

    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
}

struct ServeWatch {
    child: Child,
    stderr: Receiver<String>,
}

impl Drop for ServeWatch {
    fn drop(&mut self) {
        // The child may already have exited; kill and reap are best effort.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_line(rx: &Receiver<String>, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        match rx.recv_timeout(remaining) {
            Ok(line) if line.contains(needle) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

fn spawn_serve_watch(workspace: &Path) -> ServeWatch {
    let test_home = workspace.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");
    let mut child = Command::new(codanna_binary())
        .args(["serve", "--watch"])
        .current_dir(workspace)
        .env("HOME", &test_home)
        .env("RUST_LOG", "warn,codanna::watcher::unified=info")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve --watch");

    let stderr = child.stderr.take().expect("child stderr");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            eprintln!("{line}");
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let serve = ServeWatch { child, stderr: rx };
    assert!(
        wait_for_line(&serve.stderr, "[watcher] started", Duration::from_secs(10)),
        "watcher did not become ready"
    );
    serve
}

fn metadata_records_root(meta_path: &Path, root: &Path, timeout: Duration) -> (bool, String) {
    let deadline = Instant::now() + timeout;
    loop {
        let raw = std::fs::read_to_string(meta_path).unwrap_or_default();
        let recorded = serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|meta| {
                meta["indexed_paths"].as_array().map(|paths| {
                    paths
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|path| Path::new(path) == root)
                })
            })
            .unwrap_or(false);
        if recorded || Instant::now() >= deadline {
            return (recorded, raw);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn settings_reload_records_the_added_root_in_index_metadata() {
    let workspace = TempDir::new().expect("temp dir");
    write_root(&workspace.path().join("alpha"), &["alpha_one"]);
    write_root(&workspace.path().join("beta"), &["beta_one", "beta_two"]);
    write_settings(workspace.path(), &["alpha"]);
    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", "--no-progress"]);
    assert_eq!(code, 0, "seed index\nstdout:\n{stdout}\nstderr:\n{stderr}");

    let serve = spawn_serve_watch(workspace.path());
    write_settings(workspace.path(), &["alpha", "beta"]);
    assert!(
        wait_for_line(
            &serve.stderr,
            "index reloaded, refreshing",
            Duration::from_secs(30)
        ),
        "settings reload not observed on serve stderr"
    );

    let beta = workspace
        .path()
        .join("beta")
        .canonicalize()
        .expect("beta exists");
    let meta_path = workspace.path().join(".codanna/index/index.meta");
    let (recorded, raw) = metadata_records_root(&meta_path, &beta, Duration::from_secs(10));
    assert!(
        recorded,
        "added root {} missing from index.meta indexed_paths\n{raw}",
        beta.display()
    );
    drop(serve);
}
