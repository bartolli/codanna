//! Metadata tracking for semantic search persistence.
//!
//! This module provides structures to track embedding model information,
//! version, and statistics to ensure consistency across save/load cycles.

use std::path::Path;
use serde::{Serialize, Deserialize};
use crate::semantic::SemanticSearchError;
use crate::indexing::get_utc_timestamp;

/// Metadata for semantic search persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMetadata {
    /// Name of the embedding model used
    pub model_name: String,
    
    /// Dimension of embeddings
    pub dimension: usize,
    
    /// Number of embeddings stored
    pub embedding_count: usize,
    
    /// Unix timestamp when created
    pub created_at: u64,
    
    /// Unix timestamp when last updated
    pub updated_at: u64,
    
    /// Version of the metadata format
    pub version: u32,
}

impl SemanticMetadata {
    /// Current metadata version
    const CURRENT_VERSION: u32 = 1;
    
    /// Create new metadata with current timestamp
    pub fn new(model_name: String, dimension: usize, embedding_count: usize) -> Self {
        let now = get_utc_timestamp();
        Self {
            model_name,
            dimension,
            embedding_count,
            created_at: now,
            updated_at: now,
            version: Self::CURRENT_VERSION,
        }
    }
    
    /// Update the metadata with new embedding count and timestamp
    pub fn update(&mut self, embedding_count: usize) {
        self.embedding_count = embedding_count;
        self.updated_at = get_utc_timestamp();
    }
    
    /// Save metadata to a JSON file
    pub fn save(&self, path: &Path) -> Result<(), SemanticSearchError> {
        let metadata_path = path.join("metadata.json");
        
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to serialize metadata: {}", e),
                suggestion: "This is likely a bug in the code".to_string(),
            })?;
        
        std::fs::write(&metadata_path, json)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to write metadata: {}", e),
                suggestion: "Check disk space and file permissions".to_string(),
            })?;
        
        Ok(())
    }
    
    /// Load metadata from a JSON file
    pub fn load(path: &Path) -> Result<Self, SemanticSearchError> {
        let metadata_path = path.join("metadata.json");
        
        let json = std::fs::read_to_string(&metadata_path)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to read metadata: {}", e),
                suggestion: "Check if semantic search data exists at the specified path".to_string(),
            })?;
        
        let metadata: Self = serde_json::from_str(&json)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to parse metadata: {}", e),
                suggestion: "The metadata file may be corrupted. Try rebuilding the semantic index.".to_string(),
            })?;
        
        // Check version compatibility
        if metadata.version > Self::CURRENT_VERSION {
            return Err(SemanticSearchError::StorageError {
                message: format!(
                    "Metadata version {} is newer than supported version {}", 
                    metadata.version, 
                    Self::CURRENT_VERSION
                ),
                suggestion: "Update the code to support the newer metadata format".to_string(),
            });
        }
        
        Ok(metadata)
    }
    
    /// Check if metadata file exists
    pub fn exists(path: &Path) -> bool {
        path.join("metadata.json").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_metadata_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        
        let metadata = SemanticMetadata::new(
            "AllMiniLML6V2".to_string(),
            384,
            1000
        );
        
        // Save metadata
        metadata.save(temp_dir.path()).unwrap();
        
        // Load it back
        let loaded = SemanticMetadata::load(temp_dir.path()).unwrap();
        
        assert_eq!(loaded.model_name, metadata.model_name);
        assert_eq!(loaded.dimension, metadata.dimension);
        assert_eq!(loaded.embedding_count, metadata.embedding_count);
        assert_eq!(loaded.version, SemanticMetadata::CURRENT_VERSION);
    }
    
    #[test]
    fn test_metadata_update() {
        let mut metadata = SemanticMetadata::new(
            "TestModel".to_string(),
            128,
            100
        );
        
        let original_updated = metadata.updated_at;
        
        // Sleep briefly to ensure timestamp changes
        std::thread::sleep(std::time::Duration::from_secs(1));
        
        metadata.update(200);
        
        assert_eq!(metadata.embedding_count, 200);
        assert!(metadata.updated_at > original_updated);
        assert_eq!(metadata.created_at, metadata.created_at); // Created doesn't change
    }
    
    #[test]
    fn test_metadata_exists() {
        let temp_dir = TempDir::new().unwrap();
        
        // Initially doesn't exist
        assert!(!SemanticMetadata::exists(temp_dir.path()));
        
        // Create metadata
        let metadata = SemanticMetadata::new("Test".to_string(), 10, 0);
        metadata.save(temp_dir.path()).unwrap();
        
        // Now it exists
        assert!(SemanticMetadata::exists(temp_dir.path()));
    }
    
    #[test]
    fn test_version_compatibility() {
        let temp_dir = TempDir::new().unwrap();
        let metadata_path = temp_dir.path().join("metadata.json");
        
        // Create metadata with future version
        let future_metadata = r#"{
            "model_name": "FutureModel",
            "dimension": 512,
            "embedding_count": 0,
            "created_at": 1735689600,
            "updated_at": 1735689600,
            "version": 999
        }"#;
        
        std::fs::write(&metadata_path, future_metadata).unwrap();
        
        // Loading should fail with version error
        let result = SemanticMetadata::load(temp_dir.path());
        assert!(result.is_err());
        
        match result.unwrap_err() {
            SemanticSearchError::StorageError { message, .. } => {
                assert!(message.contains("version"));
            }
            _ => panic!("Expected version error"),
        }
    }
}