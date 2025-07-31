//! Test that incremental indexing preserves correct symbol counts

use codanna::indexing::SimpleIndexer;
use codanna::storage::IndexPersistence;
use codanna::Settings;
use std::sync::Arc;
use tempfile::TempDir;
use std::fs;

#[test]
fn test_incremental_update_preserves_symbol_count() {
    let temp_dir = TempDir::new().unwrap();
    let index_path = temp_dir.path().join("test_index");
    
    // Create test files
    let src_dir = temp_dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    
    // File 1
    let file1_path = src_dir.join("file1.rs");
    fs::write(&file1_path, r#"
/// First struct
pub struct FirstStruct {
    value: i32,
}

impl FirstStruct {
    /// Create new instance
    pub fn new(value: i32) -> Self {
        Self { value }
    }
    
    /// Get value
    pub fn get_value(&self) -> i32 {
        self.value
    }
}
"#).unwrap();
    
    // File 2
    let file2_path = src_dir.join("file2.rs");
    fs::write(&file2_path, r#"
/// Second struct
pub struct SecondStruct {
    name: String,
}

impl SecondStruct {
    /// Create new instance
    pub fn new(name: String) -> Self {
        Self { name }
    }
    
    /// Get name
    pub fn get_name(&self) -> &str {
        &self.name
    }
}
"#).unwrap();
    
    // Create indexer and index both files
    let settings = Arc::new(Settings {
        index_path: index_path.clone(),
        ..Settings::default()
    });
    
    let mut indexer = SimpleIndexer::with_settings(settings.clone());
    
    // Index both files
    indexer.index_file(&file1_path).unwrap();
    indexer.index_file(&file2_path).unwrap();
    
    let initial_symbol_count = indexer.symbol_count();
    println!("Initial symbol count: {}", initial_symbol_count);
    assert!(initial_symbol_count >= 6, "Should have at least 6 symbols (2 structs + 4 methods)");
    
    // Save the index
    let persistence = IndexPersistence::new(index_path.clone());
    persistence.save(&indexer).unwrap();
    
    // Modify file1
    fs::write(&file1_path, r#"
/// First struct modified
pub struct FirstStruct {
    value: i32,
    extra: String, // Added field
}

impl FirstStruct {
    /// Create new instance
    pub fn new(value: i32) -> Self {
        Self { 
            value,
            extra: String::new(),
        }
    }
    
    /// Get value
    pub fn get_value(&self) -> i32 {
        self.value
    }
    
    /// Get extra - NEW METHOD
    pub fn get_extra(&self) -> &str {
        &self.extra
    }
}
"#).unwrap();
    
    // Re-index the modified file
    indexer.index_file(&file1_path).unwrap();
    
    let after_update_symbol_count = indexer.symbol_count();
    println!("Symbol count after update: {}", after_update_symbol_count);
    
    // We should have one more symbol (the new method)
    assert_eq!(
        after_update_symbol_count, 
        initial_symbol_count + 1,
        "Should have exactly one more symbol after adding a method"
    );
    
    // Save and reload to verify persistence
    persistence.save(&indexer).unwrap();
    
    let reloaded_indexer = persistence.load_with_settings(settings.clone()).unwrap();
    let reloaded_symbol_count = reloaded_indexer.symbol_count();
    println!("Symbol count after reload: {}", reloaded_symbol_count);
    
    assert_eq!(
        reloaded_symbol_count,
        after_update_symbol_count,
        "Symbol count should be preserved after save/load"
    );
    
    // Verify the new method exists
    let symbols = reloaded_indexer.find_symbols_by_name("get_extra");
    assert_eq!(symbols.len(), 1, "Should find the new method");
    assert_eq!(symbols[0].name.as_ref(), "get_extra");
}