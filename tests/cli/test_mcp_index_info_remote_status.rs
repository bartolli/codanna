use serde_json::{Value, json};
use std::env;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use tempfile::TempDir;

fn spawn_embedding_server(max_requests: usize, dimension: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let addr = listener.local_addr().expect("local addr");

    thread::spawn(move || {
        for _ in 0..max_requests {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };

            let mut buffer = Vec::new();
            let mut header_end = None;
            let mut content_length = 0usize;

            loop {
                let mut chunk = [0u8; 4096];
                let Ok(read) = stream.read(&mut chunk) else {
                    break;
                };
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);

                if header_end.is_none() {
                    if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(end + 4);
                        let headers = String::from_utf8_lossy(&buffer[..end + 4]);
                        for line in headers.lines() {
                            let lower = line.to_ascii_lowercase();
                            if let Some(value) = lower.strip_prefix("content-length:") {
                                content_length = value.trim().parse().expect("content length");
                            }
                        }
                    }
                }

                if let Some(end) = header_end {
                    if buffer.len() >= end + content_length {
                        break;
                    }
                }
            }

            let body = if let Some(end) = header_end {
                &buffer[end..end + content_length]
            } else {
                &[][..]
            };

            let payload: Value = serde_json::from_slice(body).expect("parse embed request");
            let inputs = payload["input"]
                .as_array()
                .expect("request input should be an array");

            let data: Vec<Value> = inputs
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    json!({
                        "index": index,
                        "embedding": vec![0.25_f32; dimension],
                    })
                })
                .collect();

            let response_body = json!({ "data": data }).to_string();
            let content_length = response_body.len();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n{response_body}"
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    format!("http://{addr}")
}

fn codanna_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_codanna") {
        return PathBuf::from(path);
    }

    if let Ok(path) = env::var("CODANNA_BIN") {
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

fn write_fixture_source(src_dir: &Path) {
    std::fs::create_dir_all(src_dir).expect("create src dir");
    std::fs::write(
        src_dir.join("lib.rs"),
        r#"
/// Remote semantic status fixture.
pub mod fixture {
    /// Returns the answer with a little math.
    pub fn documented_function(value: i32) -> i32 {
        value + 42
    }
}
"#,
    )
    .expect("write fixture source");
}

fn write_extra_source(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create extra dir");
    std::fs::write(
        dir.join("extra.rs"),
        r#"
/// Second-root fixture.
pub fn extra_root_function(value: i32) -> i32 {
    value * 2
}
"#,
    )
    .expect("write extra source");
}

fn write_settings(workspace: &Path, base_url: &str) {
    write_settings_with_roots(workspace, base_url, &["src"]);
}

fn write_settings_with_roots(workspace: &Path, base_url: &str, roots: &[&str]) {
    let codanna_dir = workspace.join(".codanna");
    std::fs::create_dir_all(&codanna_dir).expect("create .codanna");

    // Use canonicalized absolute paths so metadata persisted by `index`
    // matches what settings.toml declares -- avoids sync mismatch on status reads.
    // On macOS, TempDir returns /var/... but canonicalize resolves to /private/var/...
    let src_path = roots
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
indexed_paths = [{src_path}]

[semantic_search]
enabled = true
model = "AllMiniLML6V2"
remote_url = "{base_url}"
remote_model = "snowflake-arctic-embed:latest"
remote_dim = 4
"#
    );

    std::fs::write(codanna_dir.join("settings.toml"), settings).expect("write settings");
}

#[test]
fn mcp_get_index_info_reports_remote_semantic_status_and_model() {
    let workspace = TempDir::new().expect("temp dir");
    write_fixture_source(&workspace.path().join("src"));
    let base_url = spawn_embedding_server(64, 4);
    write_settings(workspace.path(), &base_url);

    let (index_code, index_stdout, index_stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(
        index_code, 0,
        "remote index should succeed\nstdout:\n{index_stdout}\nstderr:\n{index_stderr}"
    );
    assert!(
        index_stderr.contains("backend: remote, model: snowflake-arctic-embed:latest"),
        "stderr should report the effective remote model\nstderr:\n{index_stderr}"
    );
    assert!(
        !index_stderr.contains("Semantic search enabled (model: AllMiniLML6V2"),
        "stderr should not claim the local default model in remote mode\nstderr:\n{index_stderr}"
    );

    let index_meta_path = workspace.path().join(".codanna/index/index.meta");
    assert!(
        index_meta_path.exists(),
        "index should persist index metadata at {}\nstdout:\n{index_stdout}\nstderr:\n{index_stderr}",
        index_meta_path.display()
    );

    let index_meta: Value = serde_json::from_str(
        &std::fs::read_to_string(&index_meta_path).expect("read index metadata"),
    )
    .expect("parse index metadata");
    let indexed_paths = index_meta["indexed_paths"]
        .as_array()
        .expect("indexed_paths should be an array");
    assert_eq!(indexed_paths.len(), 1);
    let first_path = indexed_paths[0].as_str().expect("path should be a string");
    assert!(
        std::path::Path::new(first_path).ends_with("src"),
        "indexed path should be 'src' or end in a 'src' component, got: {first_path}"
    );

    let (info_code, info_stdout, info_stderr) =
        run_cli(workspace.path(), &["mcp", "get_index_info", "--json"]);
    assert_eq!(
        info_code, 0,
        "mcp get_index_info should succeed\nstdout:\n{info_stdout}\nstderr:\n{info_stderr}"
    );
    assert!(
        !info_stderr.contains("Indexing directory:"),
        "status read should not trigger sync indexing noise\nstderr:\n{info_stderr}"
    );
    assert!(
        !info_stderr.contains("Progress:"),
        "status read should not emit progress bars\nstderr:\n{info_stderr}"
    );
    assert!(
        !info_stderr.contains("LINKS:"),
        "status read should not emit relationship progress\nstderr:\n{info_stderr}"
    );

    let payload: Value = serde_json::from_str(&info_stdout)
        .unwrap_or_else(|e| panic!("failed to parse JSON output: {e}\nstdout:\n{info_stdout}"));
    let semantic = &payload["data"]["semantic_search"];

    assert_eq!(semantic["enabled"], Value::Bool(true));
    assert_eq!(
        semantic["model_name"],
        Value::String("snowflake-arctic-embed:latest".into())
    );
    assert_eq!(semantic["dimensions"], Value::from(4));

    let embeddings = semantic["embeddings"]
        .as_u64()
        .expect("semantic embeddings count should be present");
    assert!(embeddings > 0, "expected persisted embeddings count > 0");
}

fn embedding_count(workspace: &Path) -> u64 {
    let (code, stdout, stderr) = run_cli(workspace, &["mcp", "get_index_info", "--json"]);
    assert_eq!(
        code, 0,
        "get_index_info\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let payload: Value = serde_json::from_str(&stdout).expect("parse get_index_info JSON");
    payload["data"]["semantic_search"]["embeddings"]
        .as_u64()
        .expect("semantic embeddings count should be present")
}

/// A root added to the config is normally indexed by the next command's
/// startup sync. A command that loads no embedder would store the root's
/// hashes without embeddings, and no later sync embeds it; the sync
/// leaves such roots to `codanna index` and says so.
#[test]
fn non_semantic_command_leaves_an_added_root_for_codanna_index() {
    let workspace = TempDir::new().expect("temp dir");
    write_fixture_source(&workspace.path().join("src"));
    write_extra_source(&workspace.path().join("extra"));
    let base_url = spawn_embedding_server(64, 4);
    write_settings(workspace.path(), &base_url);
    let (code, stdout, stderr) = run_cli(
        workspace.path(),
        &["index", "src", "--force", "--no-progress"],
    );
    assert_eq!(code, 0, "index\nstdout:\n{stdout}\nstderr:\n{stderr}");
    let before = embedding_count(workspace.path());
    assert!(before > 0, "fixture root should be embedded");

    write_settings_with_roots(workspace.path(), &base_url, &["src", "extra"]);
    let (code, stdout, stderr) = run_cli(workspace.path(), &["dump", "--symbols"]);
    assert_eq!(code, 0, "dump\nstdout:\n{stdout}\nstderr:\n{stderr}");
    let hint = "1 configured root(s) not yet indexed; run 'codanna index' to index them";
    assert_eq!(
        stderr.lines().filter(|line| line.contains(hint)).count(),
        1,
        "exactly one hint line\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("extra_root_function"),
        "a non-semantic command must not index the added root\nstdout:\n{stdout}"
    );
    assert_eq!(
        embedding_count(workspace.path()),
        before,
        "embedding count unchanged after the non-semantic command"
    );

    let (code, stdout, stderr) = run_cli(workspace.path(), &["index", "--no-progress"]);
    assert_eq!(code, 0, "index\nstdout:\n{stdout}\nstderr:\n{stderr}");
    let (code, stdout, stderr) = run_cli(workspace.path(), &["dump", "--symbols"]);
    assert_eq!(code, 0, "dump\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("extra_root_function"),
        "codanna index indexes the added root\nstdout:\n{stdout}"
    );
    assert!(
        embedding_count(workspace.path()) > before,
        "codanna index embeds the added root"
    );
}
