//! Stdio serve accepts both protocol generations on rmcp 3.x:
//! legacy `initialize` handshake and 2026-07-28 stateless requests,
//! with `server/discover` answered as the back-compat probe.

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
pub fn stdio_target() -> i32 {
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

fn tamper_emission_version(workspace: &Path) {
    let path = workspace.join(".codanna/index/index.meta");
    let raw = std::fs::read_to_string(&path).expect("read index.meta");
    let mut meta: Value = serde_json::from_str(&raw).expect("parse index.meta");
    meta.as_object_mut()
        .expect("index.meta is an object")
        .remove("emission_version");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&meta).expect("serialize"),
    )
    .expect("write tampered index.meta");
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
    workspace
}

struct ServeSession {
    child: Child,
    stdin: std::process::ChildStdin,
    rx: Receiver<String>,
}

fn spawn_serve(workspace: &Path) -> ServeSession {
    let bin = codanna_binary();
    let test_home = workspace.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");
    let mut child = Command::new(&bin)
        .args(["serve"])
        .current_dir(workspace)
        .env("HOME", &test_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn serve");

    let stdin = child.stdin.take().expect("child stdin");
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

    ServeSession { child, stdin, rx }
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

fn stateless_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// A 2026-07-28 client probes with bare `server/discover`, then sends a
/// stateless `tools/list` carrying the required `_meta` keys. The server
/// answers both; no handshake ever happens.
#[test]
fn serve_stdio_answers_discover_and_serves_stateless_request() {
    let workspace = seed_workspace();
    let mut session = spawn_serve(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover"
        })
    )
    .expect("write discover");
    session.stdin.flush().expect("flush discover");

    let discover = recv_json(&session.rx);
    assert_eq!(discover["id"], 1, "discover response id\n{discover}");
    let versions = discover["result"]["supportedVersions"]
        .as_array()
        .unwrap_or_else(|| panic!("discover result carries supportedVersions\n{discover}"));
    assert!(
        versions.iter().any(|v| v == "2026-07-28"),
        "server must advertise 2026-07-28\n{discover}"
    );
    assert!(
        discover["result"]["instructions"].is_string(),
        "discover result carries the server instructions\n{discover}"
    );

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write stateless tools/list");
    session.stdin.flush().expect("flush tools/list");

    let tools = recv_json(&session.rx);
    assert_eq!(tools["id"], 2, "tools/list response id\n{tools}");
    let list = tools["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("stateless tools/list returns tools\n{tools}"));
    assert_eq!(
        list.len(),
        9,
        "all 9 tools served without a handshake\n{tools}"
    );

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(
        status.success(),
        "serve exits clean after stateless session, got {status:?}"
    );
}

/// Pinning lock: the legacy `initialize` handshake passes through the
/// probe interceptor untouched and the session serves all tools.
#[test]
fn serve_stdio_legacy_handshake_unaffected() {
    let workspace = seed_workspace();
    let mut session = spawn_serve(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "dual-gen-test", "version": "0"}
            }
        })
    )
    .expect("write initialize");
    writeln!(
        session.stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .expect("write initialized notification");
    session.stdin.flush().expect("flush handshake");

    let init = recv_json(&session.rx);
    assert_eq!(init["id"], 1, "initialize response id\n{init}");
    assert!(
        init["result"]["serverInfo"]["name"].is_string(),
        "legacy initialize carries serverInfo\n{init}"
    );

    writeln!(
        session.stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    )
    .expect("write legacy tools/list");
    session.stdin.flush().expect("flush tools/list");

    let tools = recv_json(&session.rx);
    let list = tools["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("legacy tools/list returns tools\n{tools}"));
    assert_eq!(list.len(), 9, "all 9 tools on the legacy session\n{tools}");

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(
        status.success(),
        "serve exits clean after legacy session, got {status:?}"
    );
}

/// A gate-refused index still serves both generations degraded: the bare
/// probe is answered by the stale server (heal command in instructions)
/// and the process keeps the gate exit code at session end.
#[test]
fn serve_stdio_stale_answers_probe_and_keeps_gate_exit() {
    let workspace = seed_workspace();
    tamper_emission_version(workspace.path());
    let mut session = spawn_serve(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover"})
    )
    .expect("write probe");
    session.stdin.flush().expect("flush probe");

    let discover = recv_json(&session.rx);
    assert_eq!(discover["id"], 1, "probe response id\n{discover}");
    let instructions = discover["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("stale discover carries instructions\n{discover}"));
    assert!(
        instructions.contains("INDEX STALE"),
        "stale probe answer names the stale state\n{instructions}"
    );
    assert!(
        instructions.contains("codanna index"),
        "stale probe answer carries the heal command\n{instructions}"
    );

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert_eq!(
        status.code(),
        Some(7),
        "stale serve keeps the gate exit code after the probe session"
    );
}

/// An unsupported `_meta` protocol version is refused per-request with
/// `UnsupportedProtocolVersionError` (-32022); the server neither hangs
/// nor dies and keeps serving supported requests on the same stdio.
#[test]
fn serve_stdio_unsupported_version_fails_closed_per_request() {
    let workspace = seed_workspace();
    let mut session = spawn_serve(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2099-01-01",
                "io.modelcontextprotocol/clientCapabilities": {}
            }}
        })
    )
    .expect("write unsupported-version request");
    session.stdin.flush().expect("flush");

    let err = recv_json(&session.rx);
    assert_eq!(err["id"], 1, "error response id\n{err}");
    assert_eq!(
        err["error"]["code"], -32022,
        "UnsupportedProtocolVersionError code\n{err}"
    );
    assert!(
        err["error"]["data"]["supported"]
            .as_array()
            .is_some_and(|s| s.iter().any(|v| v == "2026-07-28")),
        "error data names the supported versions\n{err}"
    );

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write valid request after refusal");
    session.stdin.flush().expect("flush valid request");

    let tools = recv_json(&session.rx);
    assert_eq!(
        tools["result"]["tools"].as_array().map(Vec::len),
        Some(9),
        "server keeps serving after the refusal\n{tools}"
    );

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit after refusal session");
}
