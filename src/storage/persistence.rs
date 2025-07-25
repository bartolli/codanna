//! Simple persistence layer for the POC
//! 
//! This module provides basic save/load functionality using bincode serialization.
//! For production, this would be replaced with rkyv for zero-copy deserialization.

use std::path::PathBuf;
use std::fs;
use std::sync::Arc;
use crate::{IndexData, SimpleIndexer, Settings, IndexError, IndexResult};

/// Manages persistence of the index
pub struct IndexPersistence {
    base_path: PathBuf,
}

impl IndexPersistence {
    /// Create a new persistence manager
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }
    
    /// Get the path to the index file
    fn index_path(&self) -> PathBuf {
        self.base_path.join("index.bin")
    }
    
    /// Save the indexer to disk
    #[must_use = "Save errors should be handled to ensure data is persisted"]
    pub fn save(&self, indexer: &SimpleIndexer) -> IndexResult<()> {
        // Create directory if it doesn't exist
        fs::create_dir_all(&self.base_path)
            .map_err(|e| IndexError::FileWrite {
                path: self.base_path.clone(),
                source: e,
            })?;
        
        // Serialize only the data
        let data = bincode::serialize(indexer.data())
            .map_err(|e| IndexError::PersistenceError {
                path: self.index_path(),
                source: Box::new(e),
            })?;
        
        // Write to file atomically (write to temp, then rename)
        let temp_path = self.index_path().with_extension("tmp");
        fs::write(&temp_path, data)
            .map_err(|e| IndexError::FileWrite {
                path: temp_path.clone(),
                source: e,
            })?;
        fs::rename(&temp_path, self.index_path())
            .map_err(|e| IndexError::FileWrite {
                path: self.index_path(),
                source: e,
            })?;
        
        Ok(())
    }
    
    /// Load the indexer from disk
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load(&self) -> IndexResult<SimpleIndexer> {
        let data = fs::read(self.index_path())
            .map_err(|e| IndexError::FileRead {
                path: self.index_path(),
                source: e,
            })?;
        let index_data: IndexData = bincode::deserialize(&data)
            .map_err(|e| IndexError::LoadError {
                path: self.index_path(),
                source: Box::new(e),
            })?;
        
        // Create indexer from loaded data
        Ok(SimpleIndexer::from_data(index_data))
    }
    
    /// Load the indexer from disk with custom settings
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load_with_settings(&self, settings: Arc<Settings>) -> IndexResult<SimpleIndexer> {
        let data = fs::read(self.index_path())
            .map_err(|e| IndexError::FileRead {
                path: self.index_path(),
                source: e,
            })?;
        let index_data: IndexData = bincode::deserialize(&data)
            .map_err(|e| IndexError::LoadError {
                path: self.index_path(),
                source: Box::new(e),
            })?;
        
        // Create indexer from loaded data with settings
        Ok(SimpleIndexer::from_data_with_settings(index_data, settings))
    }
    
    /// Check if an index exists
    pub fn exists(&self) -> bool {
        self.index_path().exists()
    }
    
    /// Delete the persisted index
    pub fn clear(&self) -> Result<(), std::io::Error> {
        if self.exists() {
            fs::remove_file(self.index_path())?;
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
        
        // Create an indexer and add some data
        let indexer = SimpleIndexer::new();
        
        // Save it
        persistence.save(&indexer).unwrap();
        assert!(persistence.exists());
        
        // Load it back
        let loaded = persistence.load().unwrap();
        
        // Should have same symbol count (0 for empty indexer)
        assert_eq!(indexer.symbol_count(), loaded.symbol_count());
    }
    
    #[test]
    fn test_clear() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = IndexPersistence::new(temp_dir.path().to_path_buf());
        
        let indexer = SimpleIndexer::new();
        persistence.save(&indexer).unwrap();
        assert!(persistence.exists());
        
        persistence.clear().unwrap();
        assert!(!persistence.exists());
    }
}