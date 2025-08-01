//! Integration test for semantic embedding cleanup during re-indexing

use codanna::{SimpleIndexer, Settings};
use std::sync::Arc;
use tempfile::TempDir;
use std::fs;

#[test]
fn test_semantic_embedding_cleanup_on_reindex() {
    let temp_dir = TempDir::new().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    
    // Create a test file with doc comments
    let test_file = src_dir.join("test.rs");
    let original_content = r#"
/// Parse JSON data from a string
/// 
/// This function takes a JSON string and parses it into a Value
pub fn parse_json(input: &str) -> Result<Value, Error> {
    // implementation
}

/// Calculate the hash of a string
/// 
/// Uses SHA256 to compute the hash
pub fn calculate_hash(data: &str) -> String {
    // implementation
}
"#;
    
    fs::write(&test_file, original_content).unwrap();
    
    // Create indexer with semantic search enabled
    let index_path = temp_dir.path().join("index");
    let settings = Arc::new(Settings {
        index_path: index_path.clone(),
        semantic_search: codanna::config::SemanticSearchConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    });
    
    let mut indexer = SimpleIndexer::with_settings(settings);
    indexer.enable_semantic_search().unwrap();
    
    // Initial indexing
    indexer.index_file(&test_file).unwrap();
    
    // Verify initial embeddings
    let initial_results = indexer.semantic_search_docs("parse JSON", 10).unwrap();
    assert!(!initial_results.is_empty(), "Should find JSON parsing function");
    
    let hash_results = indexer.semantic_search_docs("calculate hash SHA256", 10).unwrap();
    assert!(!hash_results.is_empty(), "Should find hash calculation function");
    
    // Get initial embedding count
    let initial_count = indexer.semantic_search_embedding_count().unwrap();
    assert_eq!(initial_count, 2, "Should have 2 embeddings initially");
    
    // Modify the file - remove one function, modify another
    let modified_content = r#"
/// Connect to a database server
/// 
/// Establishes a connection to PostgreSQL database
pub fn connect_to_database(url: &str) -> Result<Connection, Error> {
    // implementation
}

/// Calculate the hash of a string
/// 
/// Now uses SHA512 for better security
pub fn calculate_hash(data: &str) -> String {
    // implementation updated
}
"#;
    
    fs::write(&test_file, modified_content).unwrap();
    
    // Re-index the file
    indexer.index_file(&test_file).unwrap();
    
    // Verify embeddings were updated correctly
    let json_results = indexer.semantic_search_docs_with_threshold("parse JSON", 10, 0.6).unwrap();
    assert!(json_results.is_empty(), "Should not find removed JSON parsing function");
    
    let db_results = indexer.semantic_search_docs("database connection PostgreSQL", 10).unwrap();
    assert!(!db_results.is_empty(), "Should find new database connection function");
    
    let hash_results = indexer.semantic_search_docs("SHA512 hash", 10).unwrap();
    assert!(!hash_results.is_empty(), "Should find updated hash function");
    
    // Verify embedding count stayed stable
    let final_count = indexer.semantic_search_embedding_count().unwrap();
    assert_eq!(final_count, 2, "Should still have 2 embeddings after re-indexing");
}

#[test]
fn test_semantic_embedding_cleanup_file_deletion() {
    let temp_dir = TempDir::new().unwrap();
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    
    // Create two test files
    let file1 = src_dir.join("file1.rs");
    let file2 = src_dir.join("file2.rs");
    
    fs::write(&file1, r#"
/// First file function
pub fn func1() {}
"#).unwrap();
    
    fs::write(&file2, r#"
/// Second file function  
pub fn func2() {}
"#).unwrap();
    
    // Create indexer with semantic search
    let index_path = temp_dir.path().join("index");
    let settings = Arc::new(Settings {
        index_path: index_path.clone(),
        semantic_search: codanna::config::SemanticSearchConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    });
    
    let mut indexer = SimpleIndexer::with_settings(settings);
    indexer.enable_semantic_search().unwrap();
    
    // Index both files
    indexer.index_file(&file1).unwrap();
    indexer.index_file(&file2).unwrap();
    
    let initial_count = indexer.semantic_search_embedding_count().unwrap();
    assert_eq!(initial_count, 2, "Should have 2 embeddings");
    
    // Delete one file and re-index with empty content
    fs::write(&file1, "// No functions with doc comments").unwrap();
    indexer.index_file(&file1).unwrap();
    
    let final_count = indexer.semantic_search_embedding_count().unwrap();
    assert_eq!(final_count, 1, "Should have 1 embedding after removing docs from file1");
    
    // Verify correct content remains
    let results = indexer.semantic_search_docs("second file", 10).unwrap();
    assert!(!results.is_empty(), "Should still find second file function");
}