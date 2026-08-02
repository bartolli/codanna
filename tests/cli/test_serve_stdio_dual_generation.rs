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

fn spawn_serve_watch(workspace: &Path) -> ServeSession {
    spawn_serve_with_args(workspace, &["serve", "--watch"])
}

fn spawn_serve(workspace: &Path) -> ServeSession {
    spawn_serve_with_args(workspace, &["serve"])
}

fn spawn_serve_with_args(workspace: &Path, args: &[&str]) -> ServeSession {
    let bin = codanna_binary();
    let test_home = workspace.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");
    let mut child = Command::new(&bin)
        .args(args)
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

/// List results carry the cache contract: `ttlMs` 3600000 and
/// `cacheScope` "private". The tool list is static per binary;
/// `toolsListChanged` covers upgrades.
#[test]
fn serve_stdio_tools_list_carries_cache_contract() {
    let workspace = seed_workspace();
    let mut session = spawn_serve(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write tools/list");
    session.stdin.flush().expect("flush");

    let tools = recv_json(&session.rx);
    assert_eq!(
        tools["result"]["ttlMs"], 3_600_000,
        "list results carry the locked ttlMs\n{tools}"
    );
    assert_eq!(
        tools["result"]["cacheScope"], "private",
        "list results carry the locked cacheScope\n{tools}"
    );

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit, got {status:?}");
}

/// A stateless client opts in via `subscriptions/listen`; a watched
/// file created afterwards produces a change notification tagged with
/// the subscription id.
#[test]
fn serve_stdio_listen_delivers_tagged_change_notifications() {
    let workspace = seed_workspace();
    let mut session = spawn_serve_watch(workspace.path());

    // Establish the stateless session first: `subscriptions/listen` as
    // the session's very first request is not dispatched by rmcp's
    // first-request path (recorded story limitation).
    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write session opener");
    session.stdin.flush().expect("flush opener");
    let opener = recv_json(&session.rx);
    assert_eq!(opener["id"], 1, "opener response id\n{opener}");

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "subscriptions/listen",
            "params": {
                "_meta": stateless_meta(),
                "notifications": {
                    "resourcesListChanged": true,
                    "resourceSubscriptions": ["file://src/alpha.rs"]
                }
            }
        })
    )
    .expect("write listen");
    session.stdin.flush().expect("flush listen");

    let ack = recv_json(&session.rx);
    assert_eq!(
        ack["method"], "notifications/subscriptions/acknowledged",
        "listen is acknowledged, not refused\n{ack}"
    );
    assert_eq!(
        ack["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"], 2,
        "acknowledgment carries the subscription id\n{ack}"
    );

    // The watch lane emits FileReindexed for modifies AND creates
    // (the create fix routes creates through the reindex leg), mapped
    // to notifications/resources/updated on the subscribed URI.
    let fixture = workspace.path().join("src/alpha.rs");
    let mut content = std::fs::read_to_string(&fixture).expect("read fixture");
    content.push_str("\npub fn gamma() -> i32 {\n    3\n}\n");
    std::fs::write(&fixture, content).expect("modify watched file");

    let deadline = Instant::now() + Duration::from_secs(20);
    let tagged = loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("change notification before timeout");
        let line = session
            .rx
            .recv_timeout(remaining)
            .expect("server line before timeout");
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if msg["method"] == "notifications/resources/updated" {
            break msg;
        }
    };
    assert_eq!(
        tagged["params"]["uri"], "file://src/alpha.rs",
        "notification names the subscribed resource\n{tagged}"
    );
    assert_eq!(
        tagged["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"], 2,
        "notification carries the subscription id\n{tagged}"
    );

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit, got {status:?}");
}

/// A list-only subscriber is told when the resource list GROWS: a
/// watched file created mid-session produces `list_changed`, while a
/// modify of an existing file stays silent on that category.
#[test]
fn serve_stdio_listen_list_only_subscriber_hears_creates() {
    let workspace = seed_workspace();
    let mut session = spawn_serve_watch(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write session opener");
    session.stdin.flush().expect("flush opener");
    let opener = recv_json(&session.rx);
    assert_eq!(opener["id"], 1, "opener response id\n{opener}");

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "subscriptions/listen",
            "params": {
                "_meta": stateless_meta(),
                "notifications": { "resourcesListChanged": true }
            }
        })
    )
    .expect("write listen");
    session.stdin.flush().expect("flush listen");
    let ack = recv_json(&session.rx);
    assert_eq!(
        ack["method"], "notifications/subscriptions/acknowledged",
        "listen is acknowledged\n{ack}"
    );

    create_and_await_list_changed(
        &session.rx,
        &workspace.path().join("src/beta.rs"),
        "pub fn beta() -> i32 {\n    2\n}\n",
    );

    // A modify must not notify the list category (no over-notification).
    let fixture = workspace.path().join("src/alpha.rs");
    let mut content = std::fs::read_to_string(&fixture).expect("read fixture");
    content.push_str("\npub fn epsilon() -> i32 {\n    5\n}\n");
    std::fs::write(&fixture, content).expect("modify watched file");

    let quiet = Instant::now() + Duration::from_secs(5);
    while let Some(remaining) = quiet.checked_duration_since(Instant::now()) {
        match session.rx.recv_timeout(remaining) {
            Ok(line) => {
                let msg: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if msg["method"] == "notifications/resources/list_changed" {
                    panic!("a modify must not notify the list category\n{msg}");
                }
            }
            Err(_) => break,
        }
    }

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit, got {status:?}");
}

/// Create a watched file and wait for its `list_changed`. The watch on
/// the parent dir may still be registering at session start under
/// parallel-suite load; a write before it lands emits no event.
/// Re-touching is safe in every interleaving: a processed create makes
/// the retry a known-file modify, which the list category filters.
fn create_and_await_list_changed(rx: &Receiver<String>, path: &Path, content: &str) {
    for _ in 0..3 {
        std::fs::write(path, content).expect("write watched file");
        let deadline = Instant::now() + Duration::from_secs(8);
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(line) => {
                    let msg: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if msg["method"] == "notifications/resources/list_changed" {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
    }
    panic!("no list_changed for created file {}", path.display());
}

fn recv_list_changed(rx: &Receiver<String>, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| panic!("list_changed before timeout: {what}"));
        let line = rx
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("server line before timeout: {what}"));
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if msg["method"] == "notifications/resources/list_changed" {
            return;
        }
    }
}

fn assert_no_more_list_changed(rx: &Receiver<String>, what: &str) {
    let quiet = Instant::now() + Duration::from_secs(5);
    while let Some(remaining) = quiet.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                let msg: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if msg["method"] == "notifications/resources/list_changed" {
                    panic!("duplicate list_changed: {what}\n{msg}");
                }
            }
            Err(_) => break,
        }
    }
}

/// One delete, one `list_changed` — including for a file created within
/// the same watch session (the historical double-broadcast shape). The
/// settled-burst wave collapses duplicate and double-routed remove
/// observations into one batch sync and one broadcast.
#[test]
fn serve_stdio_listen_delete_notifies_list_exactly_once() {
    let workspace = seed_workspace();
    let mut session = spawn_serve_watch(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write session opener");
    session.stdin.flush().expect("flush opener");
    let opener = recv_json(&session.rx);
    assert_eq!(opener["id"], 1, "opener response id\n{opener}");

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "subscriptions/listen",
            "params": {
                "_meta": stateless_meta(),
                "notifications": { "resourcesListChanged": true }
            }
        })
    )
    .expect("write listen");
    session.stdin.flush().expect("flush listen");
    let ack = recv_json(&session.rx);
    assert_eq!(
        ack["method"], "notifications/subscriptions/acknowledged",
        "listen is acknowledged\n{ack}"
    );

    // Create within the session; consume the create's own list_changed.
    let beta = workspace.path().join("src/beta.rs");
    create_and_await_list_changed(&session.rx, &beta, "pub fn beta() -> i32 {\n    2\n}\n");

    // The historical double-broadcast shape: delete the session-created
    // file. Exactly one list_changed.
    std::fs::remove_file(&beta).expect("delete session-created file");
    recv_list_changed(&session.rx, "delete of session-created beta.rs");
    assert_no_more_list_changed(&session.rx, "delete of session-created beta.rs");

    // Control: delete a pre-session file. Exactly one.
    std::fs::remove_file(workspace.path().join("src/alpha.rs")).expect("delete pre-session file");
    recv_list_changed(&session.rx, "delete of pre-session alpha.rs");
    assert_no_more_list_changed(&session.rx, "delete of pre-session alpha.rs");

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit, got {status:?}");
}

/// A stateless session that never opts in via `subscriptions/listen`
/// receives zero unsolicited notifications when watched files change.
#[test]
fn serve_stdio_no_optin_no_unsolicited_notifications() {
    let workspace = seed_workspace();
    let mut session = spawn_serve_watch(workspace.path());

    writeln!(
        session.stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
    )
    .expect("write opener");
    session.stdin.flush().expect("flush opener");
    let opener = recv_json(&session.rx);
    assert_eq!(opener["id"], 1, "opener response id\n{opener}");

    let fixture = workspace.path().join("src/alpha.rs");
    let mut content = std::fs::read_to_string(&fixture).expect("read fixture");
    content.push_str("\npub fn delta() -> i32 {\n    4\n}\n");
    std::fs::write(&fixture, content).expect("modify watched file");

    // Quiet window well past debounce (500ms) + reindex.
    let deadline = Instant::now() + Duration::from_secs(5);
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match session.rx.recv_timeout(remaining) {
            Ok(line) => {
                let msg: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(method) = msg["method"].as_str() {
                    panic!("unsolicited notification without opt-in: {method}\n{msg}");
                }
            }
            Err(_) => break,
        }
    }

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit, got {status:?}");
}

/// The legacy notification lane emits only standard MCP methods:
/// no `notifications/codanna/*` custom notifications reach the wire.
/// Stateless sessions are covered by the no-opt-in lock, which
/// rejects any unsolicited method including `notifications/message`.
#[test]
fn serve_stdio_legacy_lane_sends_no_custom_notifications() {
    let workspace = seed_workspace();
    let mut session = spawn_serve_watch(workspace.path());

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
                "clientInfo": {"name": "legacy-notify-test", "version": "0"}
            }
        })
    )
    .expect("write initialize");
    writeln!(
        session.stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .expect("write initialized");
    session.stdin.flush().expect("flush handshake");
    let init = recv_json(&session.rx);
    assert_eq!(init["id"], 1, "initialize response id\n{init}");

    let fixture = workspace.path().join("src/alpha.rs");
    let mut content = std::fs::read_to_string(&fixture).expect("read fixture");
    content.push_str("\npub fn epsilon() -> i32 {\n    5\n}\n");
    std::fs::write(&fixture, content).expect("modify watched file");

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut standard_seen = Vec::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match session.rx.recv_timeout(remaining) {
            Ok(line) => {
                let msg: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(method) = msg["method"].as_str() {
                    assert!(
                        !method.starts_with("notifications/codanna/"),
                        "custom notification on the wire: {method}\n{msg}"
                    );
                    standard_seen.push(method.to_string());
                }
            }
            Err(_) => break,
        }
    }
    assert!(
        standard_seen
            .iter()
            .any(|m| m == "notifications/resources/updated"),
        "legacy lane still delivers the standard resource notification, saw: {standard_seen:?}"
    );

    drop(session.stdin);
    let status = wait_with_timeout(&mut session.child, Duration::from_secs(10));
    assert!(status.success(), "clean exit, got {status:?}");
}
