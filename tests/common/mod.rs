use std::path::PathBuf;
use tempfile::TempDir;
use std::fs;
use std::sync::Arc;
use codanna::{SimpleIndexer, Settings};

pub struct TestProject {
    pub dir: TempDir,
}

impl TestProject {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("Failed to create temp dir"),
        }
    }

    pub fn add_file(&self, path: &str, content: &str) -> PathBuf {
        let file_path = self.dir.path().join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        fs::write(&file_path, content).expect("Failed to write file");
        file_path
    }

    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

/// Creates a SimpleIndexer with an isolated index directory for testing.
/// This prevents Tantivy lock conflicts when tests run in parallel.
pub fn create_test_indexer() -> (SimpleIndexer, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let index_path = temp_dir.path().join("index");
    
    let mut settings = Settings::default();
    settings.index_path = index_path;
    settings.workspace_root = Some(temp_dir.path().to_path_buf());
    
    let indexer = SimpleIndexer::with_settings(Arc::new(settings));
    (indexer, temp_dir)
}

pub mod sample_code {
    pub const SIMPLE_FUNCTION: &str = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;

    pub const STRUCT_WITH_IMPL: &str = r#"
pub struct Calculator {
    value: i32,
}

impl Calculator {
    pub fn new(value: i32) -> Self {
        Self { value }
    }

    pub fn add(&mut self, other: i32) {
        self.value += other;
    }

    pub fn get_value(&self) -> i32 {
        self.value
    }
}
"#;

    pub const FUNCTION_WITH_CALLS: &str = r#"
fn helper(x: i32) -> i32 {
    x * 2
}

fn process(items: Vec<i32>) -> Vec<i32> {
    items.into_iter()
        .map(|x| helper(x))
        .collect()
}

fn main() {
    let data = vec![1, 2, 3];
    let result = process(data);
    println!("{:?}", result);
}
"#;

    pub const TRAIT_WITH_IMPL: &str = r#"
pub trait Operation {
    fn execute(&self, value: i32) -> i32;
}

pub struct Addition {
    amount: i32,
}

impl Operation for Addition {
    fn execute(&self, value: i32) -> i32 {
        value + self.amount
    }
}

pub struct Multiplication {
    factor: i32,
}

impl Operation for Multiplication {
    fn execute(&self, value: i32) -> i32 {
        value * self.factor
    }
}
"#;
}

#[macro_export]
macro_rules! assert_symbol {
    ($symbol:expr, name: $name:expr, kind: $kind:expr) => {
        assert_eq!($symbol.name.as_str(), $name, "Symbol name mismatch");
        assert_eq!($symbol.kind, $kind, "Symbol kind mismatch");
    };
}

#[macro_export]
macro_rules! assert_range {
    ($range:expr, start: ($start_line:expr, $start_col:expr), end: ($end_line:expr, $end_col:expr)) => {
        assert_eq!($range.start_line, $start_line, "Start line mismatch");
        assert_eq!($range.start_column, $start_col, "Start column mismatch");
        assert_eq!($range.end_line, $end_line, "End line mismatch");
        assert_eq!($range.end_column, $end_col, "End column mismatch");
    };
}