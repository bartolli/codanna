//! Simple persistence layer for the POC
//! 
//! This module provides basic save/load functionality using bincode serialization.
//! For production, this would be replaced with rkyv for zero-copy deserialization.

use std::path::PathBuf;
use std::fs;
use std::sync::Arc;
use crate::{IndexData, SimpleIndexer, Settings};

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
    pub fn save(&self, indexer: &SimpleIndexer) -> Result<(), Box<dyn std::error::Error>> {
        // Create directory if it doesn't exist
        fs::create_dir_all(&self.base_path)?;
        
        // Serialize only the data
        let data = bincode::serialize(indexer.data())?;
        
        // Write to file atomically (write to temp, then rename)
        let temp_path = self.index_path().with_extension("tmp");
        fs::write(&temp_path, data)?;
        fs::rename(temp_path, self.index_path())?;
        
        Ok(())
    }
    
    /// Load the indexer from disk
    pub fn load(&self) -> Result<SimpleIndexer, Box<dyn std::error::Error>> {
        let data = fs::read(self.index_path())?;
        let index_data: IndexData = bincode::deserialize(&data)?;
        
        // Create indexer from loaded data
        Ok(SimpleIndexer::from_data(index_data))
    }
    
    /// Load the indexer from disk with custom settings
    pub fn load_with_settings(&self, settings: Arc<Settings>) -> Result<SimpleIndexer, Box<dyn std::error::Error>> {
        let data = fs::read(self.index_path())?;
        let index_data: IndexData = bincode::deserialize(&data)?;
        
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