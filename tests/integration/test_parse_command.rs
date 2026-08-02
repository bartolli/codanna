//! Tests for the parse command JSONL output
//!
//! Following TDD approach - tests written before implementation

use serde_json::Value;
use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Get the codanna binary path, using CODANNA_BIN env var or building debug binary
fn get_codanna_binary() -> PathBuf {
    // 1. Check environment variable (set by full-test.sh or CI)
    if let Ok(bin_path) = env::var("CODANNA_BIN") {
        let path = PathBuf::from(bin_path);
        if path.exists() {
            return path;
        }
        eprintln!(
            "Warning: CODANNA_BIN set to '{}' but file doesn't exist",
            path.display()
        );
    }

    // 2. The cargo-provided path for this crate's bin target: exact,
    //    honors CARGO_TARGET_DIR, and carries .exe on Windows.
    if let Some(path) = option_env!("CARGO_BIN_EXE_codanna") {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
    }

    // 3. For standalone tests, ensure debug binary exists
    // Use absolute path based on CARGO_MANIFEST_DIR to handle CI working directory issues
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().expect("Failed to get current directory"));

    let debug_bin = manifest_dir.join("target/debug/codanna");

    if !debug_bin.exists() {
        eprintln!("Building debug binary for tests...");
        eprintln!("Expected at: {}", debug_bin.display());

        // Skip building in CI - binary should already exist
        if env::var("CI").is_ok() || env::var("GITHUB_ACTIONS").is_ok() {
            panic!(
                "Debug binary not found at {} in CI environment. Check build step.",
                debug_bin.display()
            );
        }

        let status = Command::new("cargo")
            .args(["build", "--bin", "codanna"])
            .current_dir(&manifest_dir)
            .status()
            .expect("Failed to build codanna binary");

        if !status.success() {
            panic!("Failed to build codanna binary");
        }
    }

    debug_bin
}

/// Helper to run codanna parse and capture output
fn run_parse_command(code: &str, lang_ext: &str, max_depth: Option<usize>) -> String {
    // Create a truly unique temporary file using uuid-like naming
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    // Generate a unique ID combining multiple factors for guaranteed uniqueness
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let thread_id = format!("{:?}", std::thread::current().id())
        .replace("ThreadId(", "")
        .replace(")", "");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "codanna_test_{}_{}_{}_{}.{}",
        std::process::id(),
        thread_id,
        timestamp,
        id,
        lang_ext
    ));

    // Write the test file
    if let Err(e) = std::fs::write(&temp_file, code) {
        panic!("Failed to write test file {}: {}", temp_file.display(), e);
    }

    // Get the binary path using our centralized helper
    let binary_path = get_codanna_binary();

    let mut cmd = Command::new(&binary_path);
    cmd.arg("parse").arg(&temp_file);

    if let Some(depth) = max_depth {
        cmd.arg("--max-depth").arg(depth.to_string());
    }

    let output = cmd.output().expect("Failed to run parse command");

    // Clean up temp file
    std::fs::remove_file(&temp_file).ok();

    // Check if the command failed
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "Parse command failed with exit code {:?}\nstderr: {}\nstdout: {}",
            output.status.code(),
            stderr,
            String::from_utf8_lossy(&output.stdout)
        );
    }

    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Helper to parse JSONL output into Vec of JSON values
fn parse_jsonl(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("Invalid JSON"))
        .collect()
}

#[test]
fn test_position_information() {
    let code = "fn test() {}";

    let output = run_parse_command(code, "rs", None);
    let nodes = parse_jsonl(&output);

    // Every node should have start and end positions
    for node in &nodes {
        assert!(
            node.get("start").is_some(),
            "Node should have start position"
        );
        assert!(node.get("end").is_some(), "Node should have end position");

        // Positions should be [line, column] arrays
        let start = node["start"].as_array().expect("Start should be array");
        let end = node["end"].as_array().expect("End should be array");

        assert_eq!(start.len(), 2, "Start position should have [line, column]");
        assert_eq!(end.len(), 2, "End position should have [line, column]");

        // All values should be numbers
        assert!(start[0].is_u64(), "Line should be number");
        assert!(start[1].is_u64(), "Column should be number");
    }
}

#[test]
fn test_kind_id_present() {
    let code = "let x = 42;";

    let output = run_parse_command(code, "rs", None);
    let nodes = parse_jsonl(&output);

    // Every node should have a kind_id
    for node in &nodes {
        assert!(node.get("kind_id").is_some(), "Node should have kind_id");
        assert!(node["kind_id"].is_u64(), "kind_id should be a number");
    }
}

#[test]
fn test_typescript_parsing() {
    let code = r#"
interface User {
    name: string;
    age: number;
}

function greet(user: User): void {
    console.log(user.name);
}
"#;

    let output = run_parse_command(code, "ts", None);
    let nodes = parse_jsonl(&output);

    // Should have interface and function
    let interface_node = nodes
        .iter()
        .find(|n| n["node"] == "interface_declaration")
        .expect("Should have interface_declaration");

    let function_node = nodes
        .iter()
        .find(|n| n["node"] == "function_declaration")
        .expect("Should have function_declaration");

    // Both should be at depth 1 (children of program)
    assert_eq!(interface_node["depth"], 1);
    assert_eq!(function_node["depth"], 1);
}

#[test]
fn test_python_parsing() {
    let code = r#"
class Calculator:
    def add(self, a, b):
        return a + b
        
    def subtract(self, a, b):
        return a - b
"#;

    let output = run_parse_command(code, "py", None);
    let nodes = parse_jsonl(&output);

    // Should have class and methods
    let class_node = nodes
        .iter()
        .find(|n| n["node"] == "class_definition")
        .expect("Should have class_definition");

    let method_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n["node"] == "function_definition")
        .collect();

    assert_eq!(method_nodes.len(), 2, "Should have 2 methods");

    // Methods should be children of class (possibly through block)
    for method in method_nodes {
        assert!(
            method["depth"].as_u64() > class_node["depth"].as_u64(),
            "Methods should be deeper than class"
        );
    }
}

#[test]
fn test_output_is_streaming_jsonl() {
    let code = "fn test() { let x = 1; }";

    let output = run_parse_command(code, "rs", None);

    // Each line should be valid JSON
    for line in output.lines() {
        if !line.trim().is_empty() {
            serde_json::from_str::<Value>(line).expect("Each line should be valid JSON");
        }
    }

    // Should have multiple lines (not one big JSON)
    let line_count = output.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(line_count > 1, "Should have multiple JSON lines");
}
