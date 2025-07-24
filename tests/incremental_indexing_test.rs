//! Tests for incremental indexing functionality

use codebase_intelligence::{SimpleIndexer, calculate_hash};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_hash_based_indexing() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.rs");
    
    // Create initial file content
    let initial_content = r#"
fn main() {
    println!("Hello, world!");
}
"#;
    fs::write(&test_file, initial_content).unwrap();
    
    // Index the file
    let mut indexer = SimpleIndexer::new();
    let file_id = indexer.index_file(&test_file).unwrap();
    let initial_symbol_count = indexer.symbol_count();
    
    // Verify we found the main function
    assert_eq!(initial_symbol_count, 1);
    let main_symbol = indexer.find_symbol("main");
    assert!(main_symbol.is_some());
    
    // Re-index the same file without changes
    let file_id2 = indexer.index_file(&test_file).unwrap();
    assert_eq!(file_id, file_id2);
    assert_eq!(indexer.symbol_count(), initial_symbol_count);
    
    // Modify the file
    let updated_content = r#"
fn main() {
    println!("Hello, world!");
}

fn helper() {
    println!("Helper function");
}
"#;
    fs::write(&test_file, updated_content).unwrap();
    
    // Re-index the modified file
    let file_id3 = indexer.index_file(&test_file).unwrap();
    assert_eq!(file_id, file_id3); // Same file ID
    assert_eq!(indexer.symbol_count(), 2); // Now we have 2 functions
    
    // Verify both functions exist
    assert!(indexer.find_symbol("main").is_some());
    assert!(indexer.find_symbol("helper").is_some());
}

#[test]
fn test_file_removal_and_reindexing() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.rs");
    
    // Create file with multiple symbols
    let content = r#"
struct Foo {
    value: i32,
}

impl Foo {
    fn new() -> Self {
        Self { value: 0 }
    }
    
    fn get_value(&self) -> i32 {
        self.value
    }
}
"#;
    fs::write(&test_file, content).unwrap();
    
    // Index the file
    let mut indexer = SimpleIndexer::new();
    let _file_id = indexer.index_file(&test_file).unwrap();
    
    // Should have: Foo (struct), new (method), get_value (method)
    assert_eq!(indexer.symbol_count(), 3);
    
    // Replace with simpler content
    let new_content = r#"
fn simple_function() {
    // Just one function now
}
"#;
    fs::write(&test_file, new_content).unwrap();
    
    // Re-index
    indexer.index_file(&test_file).unwrap();
    
    // Should only have one symbol now
    assert_eq!(indexer.symbol_count(), 1);
    assert!(indexer.find_symbol("simple_function").is_some());
    assert!(indexer.find_symbol("Foo").is_none());
    assert!(indexer.find_symbol("new").is_none());
}

#[test]
fn test_hash_consistency() {
    let content = "fn test() { }";
    
    // Hash should be deterministic
    let hash1 = calculate_hash(content);
    let hash2 = calculate_hash(content);
    assert_eq!(hash1, hash2);
    
    // Different content should have different hash
    let different_content = "fn test() { } ";
    let hash3 = calculate_hash(different_content);
    assert_ne!(hash1, hash3);
}

#[test]
fn test_incremental_indexing_performance() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create multiple files
    let mut files = Vec::new();
    for i in 0..5 {
        let file_path = temp_dir.path().join(format!("file{}.rs", i));
        let content = format!("fn function_{}() {{ }}", i);
        fs::write(&file_path, &content).unwrap();
        files.push(file_path);
    }
    
    // Index all files
    let mut indexer = SimpleIndexer::new();
    for file in &files {
        indexer.index_file(file).unwrap();
    }
    assert_eq!(indexer.symbol_count(), 5);
    
    // Re-index without changes - should be fast (no parsing)
    for file in &files {
        indexer.index_file(file).unwrap();
    }
    assert_eq!(indexer.symbol_count(), 5);
    
    // Change one file
    let content = "fn function_2() { }\nfn extra_function() { }";
    fs::write(&files[2], content).unwrap();
    
    // Re-index all - only one should actually be parsed
    for file in &files {
        indexer.index_file(file).unwrap();
    }
    assert_eq!(indexer.symbol_count(), 6);
}