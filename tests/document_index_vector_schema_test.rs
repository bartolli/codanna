//! Tests for vector field integration in DocumentIndex schema
//!
//! This test validates Task 1.1: Add Vector Fields to Tantivy Schema
//! from the Vector Integration Plan.

use codanna::storage::{DocumentIndex, tantivy::IndexSchema};
use codanna::types::{SymbolId, SymbolKind, FileId};
use tempfile::TempDir;

#[test]
fn test_schema_with_vector_fields() {
    // Test that vector fields are properly included in the schema
    let (schema, index_schema) = IndexSchema::build();
    
    // Verify vector fields exist in schema
    assert!(schema.get_field("cluster_id").is_ok(), "cluster_id field should exist");
    assert!(schema.get_field("vector_id").is_ok(), "vector_id field should exist");
    assert!(schema.get_field("has_vector").is_ok(), "has_vector field should exist");
    
    // Verify field types are correct
    let cluster_id_entry = schema.get_field_entry(index_schema.cluster_id);
    assert!(cluster_id_entry.is_fast(), "cluster_id should be a FAST field");
    assert!(cluster_id_entry.is_stored(), "cluster_id should be STORED");
    
    let vector_id_entry = schema.get_field_entry(index_schema.vector_id);
    assert!(vector_id_entry.is_fast(), "vector_id should be a FAST field");
    assert!(vector_id_entry.is_stored(), "vector_id should be STORED");
    
    let has_vector_entry = schema.get_field_entry(index_schema.has_vector);
    assert!(has_vector_entry.is_fast(), "has_vector should be a FAST field");
    assert!(has_vector_entry.is_stored(), "has_vector should be STORED");
}

#[test]
fn test_document_with_vector_fields() {
    let temp_dir = TempDir::new().unwrap();
    let index = DocumentIndex::new(temp_dir.path()).unwrap();
    
    // Start batch
    index.start_batch().unwrap();
    
    // Add a document with vector fields using the public API
    let symbol_id = SymbolId::new(1).unwrap();
    let file_id = FileId::new(1).unwrap();
    
    // First add a regular document
    index.add_document(
        symbol_id,
        "test_function",
        SymbolKind::Function,
        file_id,
        "test.rs",
        1,
        1,
        Some("Test function"),
        Some("fn test_function()"),
        "mod test",
        Some("fn test_function() { }"),
    ).unwrap();
    
    // For now, we'll just verify the schema supports vector fields
    // The actual vector field population will be added in later tasks
    // when we integrate with VectorSearchEngine
    
    // Commit batch
    index.commit_batch().unwrap();
    
    // Search for the document
    let results = index.search("test_function", 10, Some(SymbolKind::Function), None).unwrap();
    assert_eq!(results.len(), 1);
    
    // Just verify the document was stored successfully
    // The schema validation is done in test_schema_with_vector_fields
    assert_eq!(results[0].name, "test_function");
}

#[test]
fn test_vector_fields_dont_break_existing_functionality() {
    // Ensure adding vector fields doesn't break existing document storage
    let temp_dir = TempDir::new().unwrap();
    let index = DocumentIndex::new(temp_dir.path()).unwrap();
    
    // Start batch
    index.start_batch().unwrap();
    
    // Add multiple documents
    for i in 1..=5 {
        let symbol_id = SymbolId::new(i).unwrap();
        let file_id = FileId::new(1).unwrap();
        
        index.add_document(
            symbol_id,
            &format!("function_{}", i),
            SymbolKind::Function,
            file_id,
            "test.rs",
            (i as u32) * 10,
            1,
            Some(&format!("Function {} documentation", i)),
            Some(&format!("fn function_{}()", i)),
            "test",
            Some(&format!("fn function_{}() {{ }}", i)),
        ).unwrap();
    }
    
    // Commit batch
    index.commit_batch().unwrap();
    
    // Verify all documents are searchable
    let results = index.search("function", 10, None, None).unwrap();
    assert_eq!(results.len(), 5);
    
    // Verify document count
    assert_eq!(index.document_count().unwrap(), 5); // 5 symbols
}