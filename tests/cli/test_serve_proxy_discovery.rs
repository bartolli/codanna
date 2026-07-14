//! Real-process end-to-end coverage for `codanna serve --proxy` discovery.
//!
//! Unlike the hermetic two-thread race test in `serve_discovery.rs`'s own
//! `#[cfg(test)]` module, these tests drive the actual `codanna` binary as a
//! subprocess: `discover_or_spawn` resolves its child via
//! `std::env::current_exe()`, which inside a test *binary* resolves to the
//! test harness rather than `codanna` -- so the real spawn path can only be
//! exercised by making `codanna` itself the process that calls it. Running
//! two `codanna serve --proxy` children against one temp workspace exercises
//! two concurrent, cross-process `discover_or_spawn` calls, racing on the
//! `.codanna/http.lock` `O_EXCL` file -- a strictly stronger check than an
//! in-process, two-thread race.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tempfile::TempDir;

use crate::support::{codanna_binary, run_cli};

/// Upper bound for every blocking wait in this file so a stuck proxy or
/// backing server FAILS the test instead of hanging CI.
const DEADLINE: Duration = Duration::from_secs(30);

/// Build a workspace with a uniquely-named fixture symbol, semantic search
/// disabled (see module docs on the KNOWN RISK below), and an already-built
/// index, ready for `codanna serve --proxy` to discover/spawn against.
///
/// KNOWN RISK verified before writing these tests: `Commands::Serve { .. }`
/// in non-proxy mode forces `needs_semantic_search = true` in `main.rs`, so
/// the spawned `serve --http` child takes the `load_facade` (not
/// `load_facade_lite`) path. However, `enable_semantic_search` and the
/// eager-load-if-present path in `IndexPersistence::load_facade_impl` are
/// additionally gated on `config.semantic_search.enabled` and on persisted
/// semantic data existing on disk, respectively. With `enabled = false` at
/// index time, no semantic data is ever persisted, so the spawned child does
/// not load a model on either indexing or serve.
fn prepare_workspace() -> TempDir {
    let workspace = TempDir::new().expect("create temp workspace");

    let src_dir = workspace.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");
    std::fs::write(
        src_dir.join("lib.rs"),
        r#"
/// Unique marker symbol used only by the serve --proxy discovery e2e tests.
pub fn codanna_proxy_e2e_marker() -> i32 {
    42
}
"#,
    )
    .expect("write fixture source");

    let codanna_dir = workspace.path().join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna dir");
    std::fs::write(
        codanna_dir.join("settings.toml"),
        r#"
index_path = ".codanna/index"

[semantic_search]
enabled = false
"#,
    )
    .expect("write settings.toml");

    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(
        code, 0,
        "workspace fixture index should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    workspace
}

/// Build a workspace configured with a NON-DEFAULT, absolute `index_path`
/// pointing OUTSIDE `<workspace>/.codanna/`, in a second, unrelated tempdir.
///
/// Every other fixture in this file uses the default `index_path`, where
/// `index_path.parent()` and `<workspace_root>/.codanna` happen to be the
/// same directory -- exactly why the bug this file's tests guard against
/// shipped unnoticed. `init::resolve_index_path` returns an absolute
/// `index_path` as-is (see `src/init.rs`), so this is a genuinely supported
/// configuration a real user could write, not a synthetic corner case.
///
/// Returns `(workspace, index_container)`: the workspace tempdir, and the
/// second tempdir holding the custom index, so callers can assert nothing
/// under the latter ever receives a discovery record.
fn prepare_custom_index_path_workspace() -> (TempDir, TempDir) {
    let workspace = TempDir::new().expect("create temp workspace");
    let index_container = TempDir::new().expect("create temp index container");

    let src_dir = workspace.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");
    std::fs::write(
        src_dir.join("lib.rs"),
        r#"
/// Unique marker symbol used only by the custom index_path proxy discovery
/// e2e test.
pub fn codanna_proxy_e2e_custom_index_marker() -> i32 {
    99
}
"#,
    )
    .expect("write fixture source");

    let codanna_dir = workspace.path().join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna dir");

    let custom_index_path = index_container.path().join("custom-index");
    std::fs::write(
        codanna_dir.join("settings.toml"),
        format!(
            r#"
index_path = {custom_index_path:?}

[semantic_search]
enabled = false
"#,
        ),
    )
    .expect("write settings.toml");

    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(
        code, 0,
        "workspace fixture index (custom index_path) should succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        custom_index_path.exists(),
        "index should have been written to the custom index_path, not the default location"
    );

    (workspace, index_container)
}

/// Start `codanna serve --proxy` rooted at `ws`. stdin is left piped and
/// open: `.output()` would close stdin immediately, and the stdio proxy
/// transport exits as soon as it observes stdin EOF.
fn start_proxy(ws: &Path) -> Child {
    let test_home = ws.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");

    Command::new(codanna_binary())
        .args(["serve", "--proxy"])
        .current_dir(ws)
        // `XDG_CONFIG_HOME` is set alongside `HOME` (to the SAME per-test
        // dir) because `dirs::config_dir()` -- used by both
        // `serve_tls::pinned_client` (here, to pin the backing HTTPS
        // server's cert) and `get_or_create_certificate` (there, to persist
        // it) -- prefers an ambient `XDG_CONFIG_HOME` over `HOME` on Linux.
        // Leaving it unset would let a real ambient `XDG_CONFIG_HOME` on the
        // test runner point this process at a different (real user)
        // certs directory than the one the test's own `--https` child wrote
        // to, silently breaking the HTTPS discovery tests below.
        .env("HOME", &test_home)
        .env("XDG_CONFIG_HOME", &test_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codanna serve --proxy")
}

/// Parse `Proxy: delegating to backing HTTP server at 127.0.0.1:{port} (pid
/// {pid})` (see `src/mcp/proxy.rs`'s `serve_proxy`) out of one stderr line.
fn parse_delegating_line(line: &str) -> Option<(u32, u16)> {
    let after_addr = line.split("127.0.0.1:").nth(1)?;
    let port: u16 = after_addr
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;

    let after_pid = line.split("(pid ").nth(1)?;
    let pid: u32 = after_pid
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;

    Some((pid, port))
}

/// Block (deadline-bounded) until `child`'s stderr prints the delegation
/// line, returning the backing server's `(pid, port)`.
fn await_upstream(child: &mut Child) -> (u32, u16) {
    let stderr = child
        .stderr
        .take()
        .expect("proxy child stderr should be piped");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some(found) = parse_delegating_line(&line) {
                let _ = tx.send(found);
                return;
            }
        }
    });

    rx.recv_timeout(DEADLINE)
        .expect("proxy should report the backing HTTP server within the deadline")
}

/// Like [`await_upstream`], but also returns the raw delegation line so
/// callers can assert on its dial-scheme prefix (e.g. `https://`).
#[cfg(feature = "https-server")]
fn await_upstream_with_line(child: &mut Child) -> (u32, u16, String) {
    let stderr = child
        .stderr
        .take()
        .expect("proxy child stderr should be piped");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some((pid, port)) = parse_delegating_line(&line) {
                let _ = tx.send((pid, port, line));
                return;
            }
        }
    });

    rx.recv_timeout(DEADLINE)
        .expect("proxy should report the backing server within the deadline")
}

/// Start `codanna serve --https --bind 127.0.0.1:0` rooted at `ws`, with
/// `HOME`/`XDG_CONFIG_HOME` set to the SAME per-test dir `start_proxy` uses,
/// so the proxy's `serve_tls::pinned_client` pins the exact cert this
/// process persists.
#[cfg(feature = "https-server")]
fn start_https_server(ws: &Path) -> Child {
    let test_home = ws.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");

    Command::new(codanna_binary())
        .args(["serve", "--https", "--bind", "127.0.0.1:0"])
        .current_dir(ws)
        .env("HOME", &test_home)
        .env("XDG_CONFIG_HOME", &test_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codanna serve --https")
}

/// Deadline-bounded wait for `<ws>/.codanna/serve.json` to exist and name an
/// `Https` backing server, returning the converged record.
#[cfg(feature = "https-server")]
fn wait_for_https_record(ws: &Path) -> codanna::serve_discovery::ServeRecord {
    let codanna_dir = ws.join(".codanna");
    wait_until(
        || {
            codanna::serve_discovery::read_record(&codanna_dir)
                .map(|record| record.scheme == codanna::serve_discovery::ServeScheme::Https)
                .unwrap_or(false)
        },
        DEADLINE,
        "serve.json to record an Https backing server",
    );
    codanna::serve_discovery::read_record(&codanna_dir)
        .expect("serve.json should exist once the Https record has converged")
}

/// Absolute path to the persisted server cert under the per-test config dir
/// (`XDG_CONFIG_HOME` = `HOME` = `<ws>/.home`, matching [`start_https_server`]
/// and [`start_proxy`]).
#[cfg(feature = "https-server")]
fn test_server_cert_path(ws: &Path) -> PathBuf {
    ws.join(".home")
        .join("codanna")
        .join("certs")
        .join("server.pem")
}

/// Count live processes whose cwd is `ws` (canonicalized) and whose command
/// line contains both `serve` and `--http`. cwd is the only discriminator
/// available: `spawn_detached` (`serve_discovery.rs`) sets `current_dir` on
/// the child and passes no workspace path argument.
fn count_http_children(ws: &Path) -> usize {
    let canonical_root = ws.canonicalize().expect("canonicalize workspace root");

    let mut sys = System::new();
    let refresh_kind = ProcessRefreshKind::nothing()
        .with_cwd(UpdateKind::Always)
        .with_cmd(UpdateKind::Always);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind);

    sys.processes()
        .values()
        // On Linux, sysinfo also enumerates each thread of a multi-threaded
        // process (e.g. every tokio worker thread of `serve --http`) as its
        // own `Process` entry sharing the parent's cwd and cmd line.
        // `thread_kind()` is `Some` only for those thread entries, so
        // filtering them out is required to count actual OS processes
        // rather than (cwd, cmd)-matching threads.
        .filter(|process| process.thread_kind().is_none())
        .filter(|process| {
            process.cwd() == Some(canonical_root.as_path())
                && process.cmd().iter().any(|arg| arg == OsStr::new("serve"))
                && process.cmd().iter().any(|arg| arg == OsStr::new("--http"))
        })
        .count()
}

/// Best-effort SIGKILL of `pid` via sysinfo, used both to simulate a crashed
/// backing server and to reap it at test teardown.
fn kill_pid(pid: u32) {
    let target = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    if let Some(process) = sys.process(target) {
        let _ = process.kill();
    }
}

/// Send SIGINT (not SIGKILL) to `pid`, used to trigger the backing server's
/// `shutdown_signal` future (`ctrl_c()` in `src/mcp/http_server.rs`) so its
/// graceful-shutdown `remove_record` cleanup path actually runs, instead of
/// being reaped abruptly.
fn interrupt_pid(pid: u32) {
    let target = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    if let Some(process) = sys.process(target) {
        let _ = process.kill_with(sysinfo::Signal::Interrupt);
    }
}

/// Recursively check whether any file named `filename` exists anywhere under
/// `root` (including nested directories). Used to confirm no shadow
/// discovery record leaks under a custom `index_path`'s directory tree.
fn any_file_named(root: &Path, filename: &str) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name() == Some(OsStr::new(filename)) {
                return true;
            }
        }
    }
    false
}

/// Deadline-bounded wait that distinguishes a child which terminated *on its
/// own* from one the harness had to SIGKILL: returns `None` when the deadline
/// elapsed with the child still running (it is killed and reaped either way,
/// so no process leaks).
///
/// This distinction is load-bearing for fail-closed assertions. A SIGKILLed
/// child also reports a non-success exit status, so `!status.success()` alone
/// cannot tell "the process refused to proceed" apart from "the process was
/// working fine and we killed it".
fn wait_for_self_exit(child: &mut Child, deadline: Duration) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll proxy child exit status") {
            return Some(status);
        }
        if start.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Deadline-bounded poll: panics rather than hanging if `predicate` never
/// becomes true within `deadline`.
fn wait_until(mut predicate: impl FnMut() -> bool, deadline: Duration, what: &str) {
    let start = std::time::Instant::now();
    loop {
        if predicate() {
            return;
        }
        assert!(start.elapsed() < deadline, "timed out waiting for: {what}");
        thread::sleep(Duration::from_millis(100));
    }
}

/// Kills the backing `serve --http` process recorded in
/// `<workspace>/.codanna/serve.json`, if any, when dropped. Mandatory on
/// every test in this file: without it, a failing test leaks a detached
/// `serve --http` process that outlives the test run.
struct Reaper(PathBuf);

impl Drop for Reaper {
    fn drop(&mut self) {
        let codanna_dir = self.0.join(".codanna");
        if let Some(record) = codanna::serve_discovery::read_record(&codanna_dir) {
            kill_pid(record.pid);
        }
    }
}

#[test]
fn two_concurrent_proxies_share_one_backing_http_server() {
    let workspace = prepare_workspace();
    // Declared before any proxies are started so it drops (and reaps the
    // backing server) after every other local in this test, regardless of
    // which assertion fails first.
    let _reaper = Reaper(workspace.path().to_path_buf());

    // Start both proxies back-to-back, before either can converge: no
    // serve.json record exists yet, so both take the lock-acquisition path.
    let mut proxy_a = start_proxy(workspace.path());
    let mut proxy_b = start_proxy(workspace.path());

    let (pid_a, port_a) = await_upstream(&mut proxy_a);
    let (pid_b, port_b) = await_upstream(&mut proxy_b);

    let _ = proxy_a.kill();
    let _ = proxy_a.wait();
    let _ = proxy_b.kill();
    let _ = proxy_b.wait();

    // A1: both proxies must report the same backing server -- kills "loser
    // waits on the wrong record" (mismatched pid/port, or a loser
    // SpawnTimeout because it never observed a healthy record).
    assert_eq!(
        pid_a, pid_b,
        "both proxies must delegate to the same backing server pid"
    );
    assert_eq!(
        port_a, port_b,
        "both proxies must delegate to the same backing server port"
    );

    // A2: exactly one backing `serve --http` process for the workspace --
    // the assertion the current suite otherwise cannot make. Kills "both
    // branches spawn".
    assert_eq!(
        count_http_children(workspace.path()),
        1,
        "exactly one backing `serve --http` process should exist for the workspace"
    );

    // A3: the discovery record names the same live pid both proxies
    // reported -- kills a fabricated/stale record.
    let codanna_dir = workspace.path().join(".codanna");
    let record = codanna::serve_discovery::read_record(&codanna_dir)
        .expect("serve.json should exist once both proxies have converged");
    assert_eq!(
        record.pid, pid_a,
        "serve.json pid should match both proxies' reported pid"
    );
    assert!(
        codanna::serve_discovery::pid_is_alive(record.pid),
        "serve.json pid should still be alive"
    );

    // A4: the single-flight spawn lock does not outlive the race -- kills
    // the guard `Drop` never firing.
    assert!(
        !codanna_dir.join("http.lock").exists(),
        "http.lock should not exist once both proxies have settled"
    );
}

#[test]
fn killed_server_is_respawned_and_record_is_updated() {
    let workspace = prepare_workspace();
    let _reaper = Reaper(workspace.path().to_path_buf());
    let codanna_dir = workspace.path().join(".codanna");

    let mut proxy1 = start_proxy(workspace.path());
    let (pid1, _port1) = await_upstream(&mut proxy1);
    let _ = proxy1.kill();
    let _ = proxy1.wait();

    // SIGKILL the backing server directly: this skips its graceful-shutdown
    // `remove_record` path, so serve.json is left naming a now-dead pid.
    // That staleness is the scenario under test.
    kill_pid(pid1);
    wait_until(
        || !codanna::serve_discovery::pid_is_alive(pid1),
        DEADLINE,
        "backing server pid1 to die after being killed",
    );

    let stale_record = codanna::serve_discovery::read_record(&codanna_dir)
        .expect("serve.json should still exist (stale) right after the kill");
    assert_eq!(
        stale_record.pid, pid1,
        "serve.json must still name the killed pid -- confirms the record is genuinely stale, \
         not already reclaimed"
    );

    let mut proxy2 = start_proxy(workspace.path());
    let (pid2, port2) = await_upstream(&mut proxy2);
    let _ = proxy2.kill();
    let _ = proxy2.wait();

    // B1: a fresh pid, not the stale one -- kills an impl that returns the
    // stale record without a liveness check.
    assert_ne!(
        pid2, pid1,
        "respawned backing server must have a different pid than the killed one"
    );

    // B2 + B3: the record on disk is fully and correctly rewritten to the
    // new pid/port -- kills an impl that spawns but never rewrites the
    // record, and kills a partial/torn write.
    let updated_record = codanna::serve_discovery::read_record(&codanna_dir)
        .expect("serve.json should exist after respawn");
    assert_eq!(
        updated_record.pid, pid2,
        "serve.json should be updated to the respawned server's pid"
    );
    assert_eq!(
        updated_record.port, port2,
        "serve.json should be updated to the respawned server's port"
    );
    assert!(
        codanna::serve_discovery::pid_is_alive(pid2),
        "respawned server pid should be alive"
    );
}

/// Connect a real `rmcp` stdio client to `codanna serve --proxy` rooted at
/// `ws`, mirroring the exact `().serve(TokioChildProcess::new(..))` pattern
/// `CodeIntelligenceClient::test_server` uses in `src/mcp/client.rs:28-39`,
/// but pointed at the `--proxy` subcommand (which that helper hardcodes away
/// from at `client.rs:36`) and returning the connected client instead of
/// printing to stdout.
async fn connect_proxy_client(
    ws: &Path,
) -> rmcp::service::RunningService<rmcp::service::RoleClient, ()> {
    use rmcp::service::ServiceExt;
    use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};

    let test_home = ws.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");

    let ws = ws.to_path_buf();
    ().serve(
        TokioChildProcess::new(
            tokio::process::Command::new(codanna_binary()).configure(|cmd| {
                cmd.args(["serve", "--proxy"])
                    .current_dir(&ws)
                    .env("HOME", &test_home);
            }),
        )
        .expect("spawn codanna serve --proxy as an rmcp child transport"),
    )
    .await
    .expect("rmcp client should complete the stdio initialize handshake with the proxy")
}

#[tokio::test]
async fn proxy_serves_real_mcp_traffic_from_shared_upstream() {
    let workspace = prepare_workspace();
    let _reaper = Reaper(workspace.path().to_path_buf());

    let client = tokio::time::timeout(DEADLINE, connect_proxy_client(workspace.path()))
        .await
        .expect("proxy client should connect within the deadline");

    // C1: the negotiated server name is the UPSTREAM's ("codanna",
    // server.rs:148), not the proxy's own local `get_info()` fallback
    // ("codanna-proxy", proxy.rs:176-179). Only reachable if the proxy
    // actually relayed the upstream's `initialize` response rather than
    // answering from its own fallback.
    let server_info = client
        .peer_info()
        .expect("proxy should have negotiated peer info during initialize");
    let server_name = server_info.server_info.name.as_str();
    assert_eq!(
        server_name, "codanna",
        "proxy should relay the upstream server's negotiated name, not answer locally"
    );
    assert_ne!(
        server_name, "codanna-proxy",
        "proxy must not fall back to its own local get_info() when an upstream is connected"
    );

    // C2 + C4: a non-empty, real tool list can only originate upstream (the
    // proxy registers no tools of its own), and this also proves the Bearer
    // handshake to the backing HTTP server succeeded -- a token mismatch
    // would 401 at http_server.rs:433 and fail this call outright.
    let tools = tokio::time::timeout(DEADLINE, client.list_tools(Default::default()))
        .await
        .expect("list_tools should complete within the deadline")
        .expect("list_tools should succeed through the proxy");
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(
        tool_names.contains(&"find_symbol"),
        "upstream tool list relayed through the proxy should contain find_symbol, got: {tool_names:?}"
    );
    assert!(
        tool_names.contains(&"search_symbols"),
        "upstream tool list relayed through the proxy should contain search_symbols, got: {tool_names:?}"
    );

    // C3: a real find_symbol call against the fixture symbol. The proxy
    // holds no `IndexFacade` (proxy.rs:8-9), so a match can only come from
    // the upstream index.
    let call_result = tokio::time::timeout(
        DEADLINE,
        client.call_tool(
            rmcp::model::CallToolRequestParams::new("find_symbol").with_arguments(
                serde_json::json!({ "name": "codanna_proxy_e2e_marker" })
                    .as_object()
                    .cloned()
                    .expect("json object literal"),
            ),
        ),
    )
    .await
    .expect("call_tool should complete within the deadline")
    .expect("find_symbol call should succeed through the proxy");

    let found_marker = call_result.content.iter().any(|block| match block {
        rmcp::model::ContentBlock::Text(text) => text.text.contains("codanna_proxy_e2e_marker"),
        _ => false,
    });
    assert!(
        found_marker,
        "find_symbol result relayed through the proxy should name the fixture symbol, got: {:?}",
        call_result.content
    );

    client
        .cancel()
        .await
        .expect("proxy client should shut down cleanly");
}

#[tokio::test]
async fn second_proxy_shares_one_upstream_one_record_one_pid() {
    let workspace = prepare_workspace();
    let _reaper = Reaper(workspace.path().to_path_buf());
    let codanna_dir = workspace.path().join(".codanna");

    // Proxy #1 spawns the backing server (cold path, WARM path is what
    // proxy #2 below exercises).
    let client1 = tokio::time::timeout(DEADLINE, connect_proxy_client(workspace.path()))
        .await
        .expect("proxy #1 client should connect within the deadline");
    let tools1 = tokio::time::timeout(DEADLINE, client1.list_tools(Default::default()))
        .await
        .expect("proxy #1 list_tools should complete within the deadline")
        .expect("proxy #1 list_tools should succeed");
    assert!(
        !tools1.tools.is_empty(),
        "proxy #1 should relay a non-empty tool list from the upstream"
    );

    let record_before = codanna::serve_discovery::read_record(&codanna_dir)
        .expect("serve.json should exist once proxy #1 has converged");

    // Proxy #2 starts while the record is already live: this takes the
    // `Decision::Discover` branch at serve_discovery.rs:406, not the
    // lock-acquisition path both proxy #1 here and TEST 1 exercise.
    let client2 = tokio::time::timeout(DEADLINE, connect_proxy_client(workspace.path()))
        .await
        .expect("proxy #2 client should connect within the deadline");

    // D1: both proxies' list_tools calls succeed.
    let tools2 = tokio::time::timeout(DEADLINE, client2.list_tools(Default::default()))
        .await
        .expect("proxy #2 list_tools should complete within the deadline")
        .expect("proxy #2 list_tools should succeed");
    assert!(
        !tools2.tools.is_empty(),
        "proxy #2 should relay a non-empty tool list from the same upstream"
    );

    let record_after = codanna::serve_discovery::read_record(&codanna_dir)
        .expect("serve.json should still exist after proxy #2 has converged");

    // D2 + D4: the discovery record is byte-identical before and after
    // proxy #2 starts -- one record, one pid, one port, not a second spawn.
    assert_eq!(
        record_before.pid, record_after.pid,
        "serve.json pid should be unchanged after the warm-path proxy connects"
    );
    assert_eq!(
        record_before.port, record_after.port,
        "serve.json port should be unchanged after the warm-path proxy connects"
    );

    // D3: exactly one backing `serve --http` process for the workspace with
    // both proxies connected.
    assert_eq!(
        count_http_children(workspace.path()),
        1,
        "exactly one backing `serve --http` process should exist with both proxies connected"
    );

    client1
        .cancel()
        .await
        .expect("proxy #1 client should shut down cleanly");
    client2
        .cancel()
        .await
        .expect("proxy #2 client should shut down cleanly");
}

/// THE LOAD-BEARING E2E REGRESSION TEST.
///
/// Every other fixture in this file uses the default `index_path`, where
/// `index_path.parent()` and `<workspace_root>/.codanna` happen to be the
/// same directory -- exactly why the bug this test targets shipped
/// unnoticed. Before the fix, `serve --http` derived the discovery-record
/// directory from `config.index_path.parent()` while `discover_or_spawn`
/// derived it from `workspace_root/.codanna`; under a custom `index_path`
/// (proven here via `prepare_custom_index_path_workspace`) the two diverge:
/// the proxy waits in `.codanna` for a record written elsewhere, burns the
/// full `spawn_timeout_ms`, and leaks an orphan `codanna serve` process.
#[test]
fn proxy_discovers_backing_server_with_custom_index_path() {
    let (workspace, index_container) = prepare_custom_index_path_workspace();
    // Declared immediately so a failing assertion below still reaps the
    // backing server via SIGKILL rather than leaking it -- this test is
    // about orphan processes, so it must not be able to leak one itself.
    let _reaper = Reaper(workspace.path().to_path_buf());

    let settings = codanna::Settings::default();
    let spawn_timeout = Duration::from_millis(settings.server.spawn_timeout_ms);

    let started_at = std::time::Instant::now();
    let mut proxy = start_proxy(workspace.path());
    let (proxy_pid, _proxy_port) = await_upstream(&mut proxy);
    let elapsed = started_at.elapsed();

    let _ = proxy.kill();
    let _ = proxy.wait();

    // (a) the fixed derivation reaches the backing server comfortably inside
    // `spawn_timeout_ms`. Against the pre-fix `index_path.parent()`
    // derivation, the proxy waits on the wrong directory for the *entire*
    // `spawn_timeout_ms` before giving up -- this assertion is what actually
    // trips on the bug, rather than merely relying on `await_upstream`'s much
    // longer 30s `DEADLINE` to eventually panic.
    assert!(
        elapsed < spawn_timeout,
        "proxy should reach the backing server well before spawn_timeout_ms ({}ms) elapses \
         (an index_path-derived discovery dir would burn the full timeout instead); took {elapsed:?}",
        settings.server.spawn_timeout_ms
    );

    // (b) the discovery record exists under the WORKSPACE's `.codanna/`,
    // not wherever the custom index_path happens to live.
    let codanna_dir = workspace.path().join(".codanna");
    let record = codanna::serve_discovery::read_record(&codanna_dir).expect(
        "serve.json should exist under <workspace>/.codanna while the backing server is live",
    );

    // (c) THE SHADOW-WRITE CHECK. No serve.json exists anywhere under the
    // custom index_path's directory tree while the server is live. This is
    // deliberately a separate assertion from (a): (a) alone would still pass
    // if the record were written to BOTH the correct and the old,
    // index_path-derived location (a shadow write), since the proxy would
    // still converge quickly by reading the correct one.
    assert!(
        !any_file_named(index_container.path(), "serve.json"),
        "no serve.json should exist anywhere under the custom index_path's directory while the \
         server is live -- finding one here means the old index_path-derived discovery dir has \
         been reintroduced, whether instead of or in addition to the correct one"
    );

    // (d) the pid the proxy delegates to is the pid recorded on disk.
    assert_eq!(
        proxy_pid, record.pid,
        "the pid the proxy reports delegating to must match the pid in \
         <workspace>/.codanna/serve.json"
    );

    // (e) graceful shutdown: SIGINT (not SIGKILL) so the backing server's
    // `shutdown_signal` future (`ctrl_c()`, src/mcp/http_server.rs) fires and
    // its `remove_record` cleanup path actually runs, proving serve.json is
    // removed and no orphan process remains -- not merely that this test's
    // `Reaper` cleans up after it.
    interrupt_pid(record.pid);

    wait_until(
        || codanna::serve_discovery::read_record(&codanna_dir).is_none(),
        DEADLINE,
        "serve.json to be removed after graceful shutdown",
    );
    wait_until(
        || !codanna::serve_discovery::pid_is_alive(record.pid),
        DEADLINE,
        "backing server process to exit after graceful shutdown",
    );
    assert_eq!(
        count_http_children(workspace.path()),
        0,
        "no orphan `serve --http` process should remain for the workspace after graceful shutdown"
    );
}

/// THE DIAL-SCHEME PROVENANCE TEST.
///
/// Starts a real `codanna serve --https` backing server directly (not via
/// `discover_or_spawn`/`spawn_detached`, which always spawns `--http` and
/// must not be touched by this change), waits for its `Https`-scheme
/// `serve.json` record, then starts a proxy against the SAME workspace and
/// asserts it discovers and dials that record over `https://` -- never
/// spawning its own `--http` child as a shortcut.
#[test]
#[cfg(feature = "https-server")]
fn proxy_discovers_and_dials_existing_https_server() {
    let workspace = prepare_workspace();
    let _reaper = Reaper(workspace.path().to_path_buf());

    let mut https_server = start_https_server(workspace.path());
    let record = wait_for_https_record(workspace.path());
    assert_eq!(
        record.scheme,
        codanna::serve_discovery::ServeScheme::Https,
        "serve.json should record the Https backing server this test started"
    );

    let mut proxy = start_proxy(workspace.path());
    let (_upstream_pid, upstream_port, delegating_line) = await_upstream_with_line(&mut proxy);

    let _ = proxy.kill();
    let _ = proxy.wait();

    // The proxy discovered (not spawned) the existing Https record and
    // reports delegating to that record's exact port.
    assert_eq!(
        upstream_port, record.port,
        "proxy should report delegating to the discovered Https record's port"
    );

    // Dial-scheme provenance: the delegation line must name `https://`, not
    // a hardcoded `http://`.
    assert!(
        delegating_line.contains("https://"),
        "delegation line should report the https:// dial scheme, got: {delegating_line:?}"
    );

    // THE anti-dead-code check: without this, the cheapest wrong
    // implementation -- one that ignores `record.scheme` entirely and always
    // spawns/dials an `--http` child, which would also "work" end-to-end --
    // passes every assertion above. Confirm no such `--http` child exists for
    // this workspace at all.
    assert_eq!(
        count_http_children(workspace.path()),
        0,
        "proxy must not spawn an --http child when an Https backing server is already discoverable"
    );

    // The Https child must be reaped by SIGKILL like any other backing
    // server this suite starts.
    let _ = https_server.kill();
    let _ = https_server.wait();
}

/// THE ONLY CHECK A VERIFICATION BYPASS FAILS.
///
/// After a real `--https` backing server is up and its cert is persisted,
/// overwrite the persisted cert with an UNRELATED self-signed PEM (same SANs,
/// different keypair) before starting the proxy. `serve_tls::pinned_client`
/// pins trust to whatever PEM is on disk at connect time, so the proxy now
/// pins the wrong certificate and its TLS handshake against the real backing
/// server must fail closed -- no delegation line should ever appear. If
/// `tls_certs_only` were ever replaced with `danger_accept_invalid_certs` (or
/// any other verification bypass), this is the test that would start passing
/// when it must not.
#[test]
#[cfg(feature = "https-server")]
fn proxy_refuses_https_when_pinned_cert_does_not_match() {
    let workspace = prepare_workspace();
    let _reaper = Reaper(workspace.path().to_path_buf());

    let mut https_server = start_https_server(workspace.path());
    let record = wait_for_https_record(workspace.path());
    assert_eq!(record.scheme, codanna::serve_discovery::ServeScheme::Https);

    let cert_path = test_server_cert_path(workspace.path());
    assert!(
        cert_path.is_file(),
        "the https server should have persisted a cert at {cert_path:?} before this test \
         overwrites it"
    );

    // Generate an UNRELATED self-signed cert (same SANs as the real one, but
    // an entirely different keypair) and overwrite the pinned file with it.
    let unrelated = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])
    .expect("generate unrelated self-signed certificate");
    std::fs::write(&cert_path, unrelated.cert.pem())
        .expect("overwrite pinned cert with an unrelated certificate");

    let mut proxy = start_proxy(workspace.path());

    // NOTE: the `Proxy: delegating to ...` line prints the discovered
    // record's scheme/port BEFORE the proxy attempts to actually dial it
    // (see `serve_proxy` in `src/mcp/proxy.rs`), so its presence alone does
    // not prove the connection succeeded -- it always prints once discovery
    // resolves a record, mismatched cert or not. The load-bearing signal for
    // a fail-closed pinned-cert mismatch is therefore the process's exit
    // status: `serve_proxy` propagates the TLS/handshake failure as an `Err`
    // and never reaches `service.waiting()`, so the process exits
    // non-zero instead of running indefinitely as a working proxy. Drain
    // stderr on a background thread purely so a full pipe buffer cannot
    // block the child from exiting.
    let stderr = proxy
        .stderr
        .take()
        .expect("proxy child stderr should be piped");
    let collected = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&collected);
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(mut buf) = sink.lock() {
                buf.push_str(&line);
                buf.push('\n');
            }
        }
    });

    // Dropping stdin unblocks the proxy's stdio transport if it is somehow
    // still waiting on input despite the failed handshake (it should not be).
    drop(proxy.stdin.take());

    // The proxy must terminate BY ITSELF. Asserting merely `!status.success()`
    // would be vacuous: the harness SIGKILLs a still-running child at the
    // deadline, and a killed process is also "not success" --
    // so a proxy that happily dialed the mismatched cert and kept serving would
    // be killed by the harness and still satisfy the assertion. A verification
    // bypass must not be able to pass this test, so require a self-exit.
    let exit_status = wait_for_self_exit(&mut proxy, DEADLINE);
    let stderr_text = collected.lock().map(|b| b.clone()).unwrap_or_default();

    let _ = https_server.kill();
    let _ = https_server.wait();

    let Some(exit_status) = exit_status else {
        panic!(
            "proxy kept running against a MISMATCHED pinned certificate -- it must fail closed \
             and exit. This is what a TLS verification bypass (danger_accept_invalid_certs, or \
             dropping tls_certs_only) looks like from the outside. stderr:\n{stderr_text}"
        );
    };
    assert!(
        !exit_status.success(),
        "proxy exited cleanly despite a mismatched pinned certificate; it must fail closed. \
         stderr:\n{stderr_text}"
    );

    // Provenance: it must have exited for the RIGHT reason -- while dialing the
    // HTTPS upstream -- not from some unrelated startup crash that would also
    // satisfy the assertions above.
    //
    // We deliberately do NOT assert on the words "certificate"/"handshake":
    // rmcp surfaces the failure as a transport/send error and does not render
    // rustls's underlying cause in the chain, so the proxy legitimately reports
    // only `failed to connect ... error sending request for url (https://...)`.
    // Asserting on wording this error layer never emits would make the test
    // fail against correct code. What the message DOES prove is that the proxy
    // dialed the pinned `https://` upstream and could not establish the
    // connection -- which, combined with the self-exit above, is precisely the
    // fail-closed behavior a verification bypass cannot produce.
    let lowered = stderr_text.to_lowercase();
    assert!(
        lowered.contains("https://127.0.0.1") && lowered.contains("failed to connect"),
        "proxy exited non-zero, but its stderr does not show it failing to connect to the \
         pinned https:// upstream, so this test is not actually observing the pinned-cert \
         rejection. stderr:\n{stderr_text}"
    );
}
