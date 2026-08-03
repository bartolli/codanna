//! Streamable HTTP serves both protocol generations on rmcp 3.x:
//! 2026-07-28 requests run sessionless (no `Mcp-Session-Id` minted);
//! legacy clients keep protocol sessions through the deprecation
//! window. SSE responses carry no event ids (resumability removed).

use serde_json::{Value, json};
use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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

fn write_fixture(workspace: &Path) {
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(
        src.join("alpha.rs"),
        r#"
pub fn http_target() -> i32 {
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
    let src_path = crate::common::toml_path_literal(&src_abs);

    let settings = format!(
        r#"
index_path = ".codanna/index"

[indexing]
indexed_paths = [{src_path}]

[semantic_search]
enabled = false
"#
    );

    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
}

fn seed_workspace() -> TempDir {
    let workspace = TempDir::new().expect("temp dir");
    write_fixture(workspace.path());
    write_settings(workspace.path());
    let test_home = workspace.path().join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");
    let status = Command::new(codanna_binary())
        .args(["index", "src", "--no-progress"])
        .current_dir(workspace.path())
        .env("HOME", &test_home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run seed index");
    assert!(status.success(), "seed index should succeed");
    workspace
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

struct HttpServe {
    child: Child,
    port: u16,
}

impl Drop for HttpServe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_http_serve(workspace: &Path) -> HttpServe {
    let port = free_port();
    let test_home = workspace.join(".home");
    let child = Command::new(codanna_binary())
        .args(["serve", "--http", "--bind", &format!("127.0.0.1:{port}")])
        .current_dir(workspace)
        .env("HOME", &test_home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn serve --http");

    let serve = HttpServe { child, port };
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, _, _) = http_request(serve.port, "GET", "/health", &[], None);
        if status == 200 {
            return serve;
        }
        assert!(
            Instant::now() < deadline,
            "serve --http did not become healthy within 20s"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Minimal HTTP/1.1 exchange over std TCP. SSE responses never EOF, so
/// the reader stops on read-timeout and returns what arrived; callers
/// assert on the accumulated frames.
fn http_request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String, String) {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new(), String::new());
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");

    let body_bytes = body.unwrap_or("");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n", body_bytes.len()));
    req.push_str(body_bytes);
    if stream.write_all(req.as_bytes()).is_err() {
        return (0, String::new(), String::new());
    }

    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw);
                // One JSON-RPC response per POST: stop once a data frame
                // (SSE) or a JSON body with a result/error has arrived.
                if text.contains("\"result\"") || text.contains("\"error\"") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let text = String::from_utf8_lossy(&raw).to_string();
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return (0, String::new(), text);
    };
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

/// ASCII-case-insensitive header lookup preserving the value's case.
fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then_some(v.trim())
    })
}

fn mcp_headers<'a>(method: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut h = vec![
        ("Authorization", "Bearer mcp-access-token-dummy"),
        ("Content-Type", "application/json"),
        ("Accept", "application/json, text/event-stream"),
        ("Mcp-Method", method),
    ];
    h.extend_from_slice(extra);
    h
}

/// Extract the first JSON-RPC payload from a response body that may be
/// SSE-framed (`data: {...}`) or plain JSON.
fn response_payload(body: &str) -> Value {
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str(rest) {
                return v;
            }
        }
    }
    for line in body.lines() {
        if line.trim_start().starts_with('{') {
            if let Ok(v) = serde_json::from_str(line.trim_start()) {
                return v;
            }
        }
    }
    panic!("no JSON-RPC payload in response body:\n{body}");
}

fn stateless_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// A 2026-07-28 client POSTs `tools/list` with per-request `_meta` and
/// the `Mcp-Method` header. The request succeeds without any session:
/// no `Mcp-Session-Id` is minted.
#[test]
fn serve_http_stateless_request_without_session() {
    let workspace = seed_workspace();
    let serve = spawn_http_serve(workspace.path());

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": { "_meta": stateless_meta() }
    })
    .to_string();

    let (status, head, resp_body) = http_request(
        serve.port,
        "POST",
        "/mcp",
        &mcp_headers("tools/list", &[("MCP-Protocol-Version", "2026-07-28")]),
        Some(&body),
    );

    assert_eq!(status, 200, "stateless POST succeeds\nhead:\n{head}");
    assert!(
        header_value(&head, "mcp-session-id").is_none(),
        "no session is minted for a 2026-07-28 request\nhead:\n{head}"
    );

    let payload = response_payload(&resp_body);
    let tools = payload["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("stateless tools/list returns tools\n{payload}"));
    assert_eq!(tools.len(), 9, "all 9 tools served sessionless\n{payload}");
}

/// A legacy client completes the initialize handshake, receives an
/// `Mcp-Session-Id`, and keeps working through the deprecation window.
#[test]
fn serve_http_legacy_client_keeps_session() {
    let workspace = seed_workspace();
    let serve = spawn_http_serve(workspace.path());

    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "http-legacy-test", "version": "0"}
        }
    })
    .to_string();

    let (status, head, body) = http_request(
        serve.port,
        "POST",
        "/mcp",
        &mcp_headers("initialize", &[]),
        Some(&init),
    );
    assert_eq!(status, 200, "legacy initialize succeeds\nhead:\n{head}");
    let payload = response_payload(&body);
    assert!(
        payload["result"]["serverInfo"]["name"].is_string(),
        "initialize result carries serverInfo\n{payload}"
    );
    let sid = header_value(&head, "mcp-session-id")
        .unwrap_or_else(|| panic!("legacy initialize mints a session\nhead:\n{head}"))
        .to_string();

    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
    let (status, _, _) = http_request(
        serve.port,
        "POST",
        "/mcp",
        &mcp_headers("notifications/initialized", &[("Mcp-Session-Id", &sid)]),
        Some(&initialized),
    );
    assert!(
        status == 200 || status == 202,
        "initialized notification accepted, got {status}"
    );

    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}).to_string();
    let (status, head, body) = http_request(
        serve.port,
        "POST",
        "/mcp",
        &mcp_headers("tools/list", &[("Mcp-Session-Id", &sid)]),
        Some(&list),
    );
    assert_eq!(status, 200, "legacy tools/list succeeds\nhead:\n{head}");
    let payload = response_payload(&body);
    assert_eq!(
        payload["result"]["tools"].as_array().map(Vec::len),
        Some(9),
        "all 9 tools on the legacy session\n{payload}"
    );
}

/// Resumability is gone from the transport: response frames carry no
/// SSE event ids (nothing for `Last-Event-ID` to reference), and a
/// request re-issued under a new JSON-RPC id succeeds as a fresh
/// request.
#[test]
fn serve_http_no_event_ids_and_reissue_succeeds() {
    let workspace = seed_workspace();
    let serve = spawn_http_serve(workspace.path());

    let request = |id: u64| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": { "_meta": stateless_meta() }
        })
        .to_string()
    };

    let (status, _, body) = http_request(
        serve.port,
        "POST",
        "/mcp",
        &mcp_headers("tools/list", &[("MCP-Protocol-Version", "2026-07-28")]),
        Some(&request(1)),
    );
    assert_eq!(status, 200);
    assert!(
        !body.lines().any(|l| l.trim_start().starts_with("id:")),
        "response frames must carry no SSE event ids\n{body}"
    );

    let (status, _, body) = http_request(
        serve.port,
        "POST",
        "/mcp",
        &mcp_headers("tools/list", &[("MCP-Protocol-Version", "2026-07-28")]),
        Some(&request(2)),
    );
    assert_eq!(status, 200, "re-issued request succeeds as fresh");
    let payload = response_payload(&body);
    assert_eq!(
        payload["id"], 2,
        "response answers the re-issued id\n{payload}"
    );
    assert_eq!(
        payload["result"]["tools"].as_array().map(Vec::len),
        Some(9),
        "re-issue serves the full result\n{payload}"
    );
}
