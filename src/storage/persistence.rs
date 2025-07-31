//! Simplified persistence layer for Tantivy-only storage
//! 
//! This module manages metadata and ensures Tantivy index exists.
//! All actual data is stored in Tantivy.

use std::path::PathBuf;
use std::sync::Arc;
use crate::{SimpleIndexer, Settings, IndexError, IndexResult};
use crate::storage::{IndexMetadata, DataSource};

/// Manages persistence of the index
#[derive(Debug)]
pub struct IndexPersistence {
    base_path: PathBuf,
}

impl IndexPersistence {
    /// Create a new persistence manager
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }
    
    /// Save metadata for the index
    #[must_use = "Save errors should be handled to ensure data is persisted"]
    pub fn save(&self, indexer: &SimpleIndexer) -> IndexResult<()> {
        // Update metadata
        let mut metadata = IndexMetadata::load(&self.base_path).unwrap_or_else(|_| {
            IndexMetadata::new()
        });
        
        metadata.update_counts(
            indexer.symbol_count() as u32,
            indexer.file_count() as u32,
        );
        
        // Update metadata to reflect Tantivy
        metadata.data_source = DataSource::Tantivy {
            path: self.base_path.join("tantivy"),
            doc_count: indexer.document_count().unwrap_or(0),
            timestamp: crate::indexing::get_utc_timestamp(),
        };
        
        metadata.save(&self.base_path)?;
        Ok(())
    }
    
    /// Load the indexer from disk
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load(&self) -> IndexResult<SimpleIndexer> {
        self.load_with_settings(Arc::new(Settings::default()))
    }
    
    /// Load the indexer from disk with custom settings
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load_with_settings(&self, settings: Arc<Settings>) -> IndexResult<SimpleIndexer> {
        // Load metadata to understand data sources
        let metadata = IndexMetadata::load(&self.base_path).ok();
        
        // Check if Tantivy index exists
        let tantivy_path = self.base_path.join("tantivy");
        if tantivy_path.join("meta.json").exists() {
            // Create indexer that will load from Tantivy
            let indexer = SimpleIndexer::with_settings(settings);
            
            // Display source info with fresh counts
            if let Some(meta) = metadata {
                // Get fresh counts from the actual index
                let fresh_symbol_count = indexer.symbol_count();
                let fresh_file_count = indexer.file_count();
                
                // Display the metadata but with fresh counts
                match &meta.data_source {
                    DataSource::Tantivy { path, doc_count, .. } => {
                        eprintln!("Loaded from Tantivy index: {} ({} documents)", path.display(), doc_count);
                    }
                    DataSource::Fresh => {
                        eprintln!("Created fresh index");
                    }
                }
                eprintln!("Index contains {} symbols from {} files", fresh_symbol_count, fresh_file_count);
            }
            
            Ok(indexer)
        } else {
            Err(IndexError::FileRead {
                path: tantivy_path,
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Tantivy index not found"
                ),
            })
        }
    }
    
    /// Check if an index exists
    pub fn exists(&self) -> bool {
        // Check if Tantivy index exists
        let tantivy_path = self.base_path.join("tantivy");
        tantivy_path.join("meta.json").exists()
    }
    
    /// Delete the persisted index
    pub fn clear(&self) -> Result<(), std::io::Error> {
        let tantivy_path = self.base_path.join("tantivy");
        if tantivy_path.exists() {
            std::fs::remove_dir_all(tantivy_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = IndexPersistence::new(temp_dir.path().to_path_buf());
        
        // Create an indexer
        let indexer = SimpleIndexer::new();
        
        // Save it
        persistence.save(&indexer).unwrap();
        
        // Check metadata exists
        let metadata_path = temp_dir.path().join("index.meta");
        assert!(metadata_path.exists());
    }
    
    #[test]
    fn test_exists() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = IndexPersistence::new(temp_dir.path().to_path_buf());
        
        // Initially doesn't exist
        assert!(!persistence.exists());
        
        // Create tantivy directory with meta.json
        let tantivy_path = temp_dir.path().join("tantivy");
        std::fs::create_dir_all(&tantivy_path).unwrap();
        std::fs::write(tantivy_path.join("meta.json"), "{}").unwrap();
        
        // Now it exists
        assert!(persistence.exists());
    }
}