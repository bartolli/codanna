//! Emission-semantics gate: an index stamped with a different (or no)
//! emission version is never read or incrementally extended.
//! `codanna index` heals by full rebuild; read paths and dry-run refuse
//! with the heal command; `--force` clears unconditionally.

use serde_json::{Value, json};
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
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

fn recv_json(rx: &Receiver<String>) -> Value {
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("server response before timeout");
    serde_json::from_str(&line).expect("valid JSON-RPC line")
}

fn wait_with_timeout(child: &mut Child, deadline: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return status;
        }
        if start.elapsed() > deadline {
            let _ = child.kill();
            panic!("serve did not exit within {deadline:?} after stdin EOF");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Stale index + stdio serve: the refusal is protocol-level, not a
/// pre-handshake exit. Client-spawned servers lose stderr, so the heal
/// command rides the `instructions` field; zero tools keeps the refusal
/// fail-closed. The process still exits with the gate code at session end.
#[test]
fn serve_stale_stdio_completes_degraded_handshake() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path(), None);

    let bin = codanna_binary();
    let test_home = workspace.path().join(".home");
    let mut child = Command::new(&bin)
        .args(["serve"])
        .current_dir(workspace.path())
        .env("HOME", &test_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");

    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "gate-test", "version": "0"}
            }
        })
    )
    .expect("write initialize");
    stdin.flush().expect("flush initialize");

    let init = recv_json(&rx);
    assert_eq!(init["id"], 1, "initialize response id\n{init}");
    let instructions = init["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("initialize result carries instructions\n{init}"));
    assert!(
        instructions.contains("INDEX STALE"),
        "instructions must name the stale state\n{instructions}"
    );
    assert!(
        instructions.contains("codanna index"),
        "instructions must carry the heal command\n{instructions}"
    );
    assert!(
        instructions.contains("restart this MCP server"),
        "instructions must name the restart duty\n{instructions}"
    );

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .expect("write initialized");
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    )
    .expect("write tools/list");
    stdin.flush().expect("flush tools/list");

    let tools = recv_json(&rx);
    assert_eq!(tools["id"], 2, "tools/list response id\n{tools}");
    let list = tools["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list result carries a tools array\n{tools}"));
    assert!(
        list.is_empty(),
        "degraded serve must advertise zero tools\n{tools}"
    );

    drop(stdin);
    let status = wait_with_timeout(&mut child, Duration::from_secs(10));
    assert_eq!(
        status.code(),
        Some(REFUSAL_EXIT),
        "degraded serve exits with the gate code after session end"
    );

    let mut stderr_text = String::new();
    use std::io::Read;
    child
        .stderr
        .take()
        .expect("child stderr")
        .read_to_string(&mut stderr_text)
        .expect("read stderr");
    assert!(
        stderr_text.contains("emission semantics changed"),
        "stderr keeps the heal text for terminal users\n{stderr_text}"
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
