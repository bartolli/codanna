//! `codanna mcp-test` drives the rmcp 3 client against a serve child:
//! the client probes `server/discover`, negotiates the stateless
//! 2026-07-28 generation, and lists/calls tools on that session.

use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
pub fn client_target() -> i32 {
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

/// Run `codanna mcp-test` in the workspace with a hard deadline; the
/// command owns its serve child, so a hang here would otherwise hang
/// the test runner.
fn run_mcp_test(workspace: &Path, deadline: Duration) -> (i32, String, String) {
    run_mcp_test_with_args(workspace, deadline, &[])
}

fn run_mcp_test_with_args(
    workspace: &Path,
    deadline: Duration,
    extra_args: &[&str],
) -> (i32, String, String) {
    let bin = codanna_binary();
    let test_home = workspace.join(".home");
    std::fs::create_dir_all(&test_home).expect("create test home");

    let mut child = Command::new(&bin)
        .arg("mcp-test")
        .args(extra_args)
        .current_dir(workspace)
        .env("HOME", &test_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-test");

    let mut stdout_pipe = child.stdout.take().expect("child stdout");
    let mut stderr_pipe = child.stderr.take().expect("child stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll mcp-test") {
            break status;
        }
        if start.elapsed() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break child
                .try_wait()
                .expect("reap killed mcp-test")
                .expect("killed child has an exit status");
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout = stdout_reader.join().expect("stdout reader panicked");
    let stderr = stderr_reader.join().expect("stderr reader panicked");
    assert!(
        start.elapsed() <= deadline,
        "mcp-test exceeded {deadline:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    (status.code().unwrap_or(-1), stdout, stderr)
}

/// The client probes `server/discover` and negotiates 2026-07-28; tool
/// listing, `get_index_info`, and the custom requests all answer on the
/// stateless session.
#[test]
fn mcp_test_negotiates_stateless_and_calls_tools() {
    let workspace = seed_workspace();
    let (code, stdout, stderr) = run_mcp_test(workspace.path(), Duration::from_secs(120));

    assert_eq!(
        code, 0,
        "mcp-test exits clean\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("2026-07-28"),
        "client negotiates the stateless generation\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("find_symbol"),
        "tool listing includes find_symbol\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Index contains 1 symbols"),
        "get_index_info renders on the stateless session\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("\"symbols\": 1"),
        "index-stats custom request answers\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("\"reindexed\""),
        "force-reindex custom request answers\nstdout:\n{stdout}"
    );
}

/// A server child that dies on the probe (shipped codanna <= 0.12.0
/// exits on pre-handshake `server/discover`) gets a diagnostic naming
/// the probe death, not just a raw connection error.
#[cfg(unix)]
#[test]
fn mcp_test_names_probe_death_when_server_dies() {
    let workspace = TempDir::new().expect("temp dir");
    let (code, _stdout, stderr) = run_mcp_test_with_args(
        workspace.path(),
        Duration::from_secs(60),
        &["--server-binary", "/usr/bin/false"],
    );

    assert_eq!(
        code, 1,
        "mcp-test fails against a dead server\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("pre-handshake server/discover probe"),
        "diagnostic names the probe death\nstderr:\n{stderr}"
    );
}
