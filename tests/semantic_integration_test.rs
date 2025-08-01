//! Test semantic search integration with SimpleIndexer

use codanna::SimpleIndexer;

#[test]
fn test_semantic_search_integration() {
    // Create a temporary directory for the index
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let settings = std::sync::Arc::new(codanna::Settings {
        index_path: temp_dir.path().join("index"),
        ..Default::default()
    });
    
    let mut indexer = SimpleIndexer::with_settings(settings);
    
    // Enable semantic search
    indexer.enable_semantic_search().expect("Failed to enable semantic search");
    assert!(indexer.has_semantic_search());
    
    // Create test file with documented functions
    let test_content = "
use serde_json::Value;

/// Parse JSON data from a string and return a structured object.
/// 
/// This function handles various JSON formats and provides comprehensive error handling.
/// It supports both pretty-printed and compact JSON formats.
pub fn parse_json(input: &str) -> Result<Value, String> {
    serde_json::from_str(input).map_err(|e| e.to_string())
}

/// Serialize a data structure to JSON format.
/// 
/// This function supports pretty printing and custom serialization options.
/// It can handle any type that implements the Serialize trait.
pub fn serialize_to_json<T: serde::Serialize>(data: &T) -> Result<String, String> {
    serde_json::to_string_pretty(data).map_err(|e| e.to_string())
}

/// Calculate the factorial of a number recursively.
/// 
/// Returns None for negative numbers to avoid invalid results.
/// Uses tail recursion optimization for better performance.
pub fn factorial(n: i32) -> Option<u64> {
    if n < 0 {
        None
    } else if n == 0 || n == 1 {
        Some(1)
    } else {
        factorial(n - 1).map(|prev| prev * n as u64)
    }
}

/// Authenticate user with username and password.
/// 
/// This function validates user credentials and returns a session token
/// on successful authentication. The token includes an expiration time
/// for security purposes.
pub fn authenticate_user(username: &str, password: &str) -> Result<String, String> {
    if username == \"admin\" && password == \"secret\" {
        Ok(\"token-123\".to_string())
    } else {
        Err(\"Invalid credentials\".to_string())
    }
}

// Some undocumented helper functions
fn validate_input(input: &str) -> bool {
    !input.is_empty()
}
";

    // Write test file
    let test_file = temp_dir.path().join("test.rs");
    std::fs::write(&test_file, test_content).expect("Failed to write test file");
    
    // Index the file
    indexer.index_file(&test_file).expect("Failed to index file");
    
    // Print all indexed symbols with doc comments
    println!();
    println!("=== Indexed Symbols ===");
    let all_symbols = indexer.get_all_symbols();
    println!("Total symbols: {}", all_symbols.len());
    
    let mut doc_symbols = Vec::new();
    for sym in &all_symbols {
        if let Some(ref doc) = sym.doc_comment {
            let preview = doc.lines().next().unwrap_or("").chars().take(60).collect::<String>();
            println!("  {} - {}...", sym.name.as_ref(), preview);
            doc_symbols.push((sym.name.as_ref(), doc));
        }
    }
    println!("Symbols with docs: {}", doc_symbols.len());
    
    // Test semantic search for JSON-related functions
    println!();
    println!("=== Semantic Search Test 1: parse JSON data ===");
    let results = indexer.semantic_search_docs("parse JSON data", 5)
        .expect("Semantic search failed");
    
    println!("Results:");
    for (i, (symbol, score)) in results.iter().enumerate() {
        println!("  {}. {} (score: {:.3})", i + 1, symbol.name.as_ref(), score);
    }
    
    assert!(!results.is_empty(), "Should find results for JSON query");
    
    // Find parse_json in the results
    let parse_json_result = results.iter()
        .find(|(sym, _)| sym.name.as_ref() == "parse_json")
        .expect("Should find parse_json function");
    
    let (_, score) = parse_json_result;
    println!("parse_json score: {:.3}", score);
    assert!(score > &0.6, "parse_json should have high similarity score (>0.6)");
    
    // Test with threshold
    println!();
    println!("=== Semantic Search Test 2: user authentication login ===");
    let threshold_results = indexer.semantic_search_docs_with_threshold(
        "user authentication login", 
        10, 
        0.6
    ).expect("Threshold search failed");
    
    println!("Results with threshold 0.6:");
    for (i, (symbol, score)) in threshold_results.iter().enumerate() {
        println!("  {}. {} (score: {:.3})", i + 1, symbol.name.as_ref(), score);
    }
    
    // Should find the authenticate_user function
    let auth_found = threshold_results.iter()
        .any(|(sym, _)| sym.name.as_ref() == "authenticate_user");
    assert!(auth_found, "Should find authenticate_user function");
    
    // Test serialize function
    println!();
    println!("=== Semantic Search Test 3: convert object to JSON string ===");
    let serialize_results = indexer.semantic_search_docs("convert object to JSON string", 5)
        .expect("Search failed");
    
    println!("Results:");
    for (i, (symbol, score)) in serialize_results.iter().enumerate() {
        println!("  {}. {} (score: {:.3})", i + 1, symbol.name.as_ref(), score);
    }
    
    // Verify unrelated queries return lower scores
    println!();
    println!("=== Semantic Search Test 4: matrix multiplication (unrelated) ===");
    let unrelated_results = indexer.semantic_search_docs("matrix multiplication", 5)
        .expect("Search failed");
    
    if !unrelated_results.is_empty() {
        println!("Results:");
        for (i, (symbol, score)) in unrelated_results.iter().enumerate() {
            println!("  {}. {} (score: {:.3})", i + 1, symbol.name.as_ref(), score);
        }
        let (_, score) = &unrelated_results[0];
        assert!(score < &0.3, "Unrelated queries should have low scores (<0.3)");
    } else {
        println!("No results found (expected for unrelated query)");
    }
    
    // Test factorial search
    println!();
    println!("=== Semantic Search Test 5: recursive calculation factorial ===");
    let factorial_results = indexer.semantic_search_docs("recursive calculation factorial", 5)
        .expect("Search failed");
    
    println!("Results:");
    for (i, (symbol, score)) in factorial_results.iter().enumerate() {
        println!("  {}. {} (score: {:.3})", i + 1, symbol.name.as_ref(), score);
    }
    
    let factorial_found = factorial_results.iter()
        .any(|(sym, _)| sym.name.as_ref() == "factorial");
    assert!(factorial_found, "Should find factorial function");
}

#[test]
fn test_semantic_search_without_enabling() {
    let indexer = SimpleIndexer::new();
    
    // Should fail when semantic search is not enabled
    let result = indexer.semantic_search_docs("test query", 5);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not enabled"));
}

#[test]
fn test_semantic_search_empty_docs() {
    // Create a temporary directory for the index
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let settings = std::sync::Arc::new(codanna::Settings {
        index_path: temp_dir.path().join("index"),
        ..Default::default()
    });
    
    let mut indexer = SimpleIndexer::with_settings(settings);
    indexer.enable_semantic_search().expect("Failed to enable semantic search");
    
    // Create test file with functions without doc comments
    let test_content = "
pub fn undocumented_function() {
    // no doc comment
}

// Regular comment, not a doc comment
pub fn another_function() {
    // implementation
}
";
    
    let test_file = temp_dir.path().join("test_no_docs.rs");
    std::fs::write(&test_file, test_content).expect("Failed to write test file");
    
    indexer.index_file(&test_file).expect("Failed to index file");
    
    // Should return error since no doc comments were indexed
    let result = indexer.semantic_search_docs("function", 5);
    
    // The search should fail because there are no embeddings
    assert!(result.is_err(), "Search should fail when no embeddings are available");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("No embeddings available"), "Error should mention no embeddings");
}