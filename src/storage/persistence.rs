//! Simple persistence layer for the POC
//! 
//! This module provides basic save/load functionality using bincode serialization.
//! For production, this would be replaced with rkyv for zero-copy deserialization.

use std::path::PathBuf;
use std::fs;
use std::sync::Arc;
use crate::{IndexData, SimpleIndexer, Settings, IndexError, IndexResult};
use crate::storage::{IndexMetadata, DataSource};

/// Manages persistence of the index
pub struct IndexPersistence {
    base_path: PathBuf,
}

impl IndexPersistence {
    /// Create a new persistence manager
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }
    
    /// Create a snapshot of the index (always saves bincode regardless of settings)
    pub fn save_snapshot(&self, indexer: &SimpleIndexer) -> IndexResult<()> {
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
    
    /// Get the path to the index file
    fn index_path(&self) -> PathBuf {
        self.base_path.join("index.bin")
    }
    
    /// Save the indexer to disk (respects use_bincode_snapshots setting)
    #[must_use = "Save errors should be handled to ensure data is persisted"]
    pub fn save(&self, indexer: &SimpleIndexer) -> IndexResult<()> {
        // Always update metadata
        let settings = indexer.settings();
        let mut metadata = IndexMetadata::load(&self.base_path).unwrap_or_else(|_| {
            IndexMetadata::new(settings.indexing.use_bincode_snapshots)
        });
        
        // Update bincode_enabled to match current settings
        metadata.bincode_enabled = settings.indexing.use_bincode_snapshots;
        
        metadata.update_counts(
            indexer.symbol_count() as u32,
            indexer.data().file_map.len() as u32,
        );
        
        // Check if bincode snapshots are enabled
        if !settings.indexing.use_bincode_snapshots {
            // Update metadata to reflect Tantivy-only
            metadata.data_source = DataSource::Tantivy {
                path: self.base_path.join("tantivy"),
                doc_count: indexer.document_count().unwrap_or(0),
                timestamp: crate::indexing::get_utc_timestamp(),
            };
            metadata.save(&self.base_path)?;
            return Ok(());
        }
        
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
        fs::write(&temp_path, &data)
            .map_err(|e| IndexError::FileWrite {
                path: temp_path.clone(),
                source: e,
            })?;
        fs::rename(&temp_path, self.index_path())
            .map_err(|e| IndexError::FileWrite {
                path: self.index_path(),
                source: e,
            })?;
        
        // Update metadata for bincode save
        metadata.data_source = DataSource::Bincode {
            path: self.index_path(),
            size_bytes: data.len() as u64,
            timestamp: crate::indexing::get_utc_timestamp(),
        };
        metadata.mark_snapshot();
        metadata.save(&self.base_path)?;
        
        Ok(())
    }
    
    /// Load the indexer from disk
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load(&self) -> IndexResult<SimpleIndexer> {
        // Load metadata to understand data sources
        let metadata = IndexMetadata::load(&self.base_path).ok();
        
        // Try to load bincode first
        match fs::read(self.index_path()) {
            Ok(data) => {
                let index_data: IndexData = bincode::deserialize(&data)
                    .map_err(|e| IndexError::LoadError {
                        path: self.index_path(),
                        source: Box::new(e),
                    })?;
                
                // Display source info
                if let Some(meta) = metadata {
                    meta.display_source();
                }
                
                // Create indexer from loaded data
                Ok(SimpleIndexer::from_data(index_data))
            }
            Err(_) => {
                // If bincode doesn't exist, try loading from Tantivy
                let tantivy_path = self.base_path.join("tantivy");
                if tantivy_path.join("meta.json").exists() {
                    eprintln!("Bincode snapshot not found, loading from Tantivy index...");
                    
                    // Create an empty IndexData, from_data will load from Tantivy
                    Ok(SimpleIndexer::from_data(IndexData::new()))
                } else {
                    Err(IndexError::FileRead {
                        path: self.index_path(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "Neither bincode nor Tantivy index found"
                        ),
                    })
                }
            }
        }
    }
    
    /// Load the indexer from disk with custom settings
    #[must_use = "Load errors should be handled appropriately"]
    pub fn load_with_settings(&self, settings: Arc<Settings>) -> IndexResult<SimpleIndexer> {
        // Load metadata to understand data sources
        let metadata = IndexMetadata::load(&self.base_path).ok();
        
        // Try to load bincode first
        match fs::read(self.index_path()) {
            Ok(data) => {
                let index_data: IndexData = bincode::deserialize(&data)
                    .map_err(|e| IndexError::LoadError {
                        path: self.index_path(),
                        source: Box::new(e),
                    })?;
                
                // Display source info
                if let Some(meta) = metadata {
                    meta.display_source();
                }
                
                // Create indexer from loaded data with settings
                Ok(SimpleIndexer::from_data_with_settings(index_data, settings))
            }
            Err(_) => {
                // If bincode doesn't exist, try loading from Tantivy
                let tantivy_path = self.base_path.join("tantivy");
                if tantivy_path.join("meta.json").exists() {
                    eprintln!("Bincode snapshot not found, loading from Tantivy index...");
                    
                    // Create an empty IndexData, from_data_with_settings will load from Tantivy
                    Ok(SimpleIndexer::from_data_with_settings(IndexData::new(), settings))
                } else {
                    Err(IndexError::FileRead {
                        path: self.index_path(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "Neither bincode nor Tantivy index found"
                        ),
                    })
                }
            }
        }
    }
    
    /// Check if an index exists (either bincode or Tantivy)
    pub fn exists(&self) -> bool {
        // Check if bincode exists
        if self.index_path().exists() {
            return true;
        }
        
        // Check if Tantivy index exists
        let tantivy_path = self.base_path.join("tantivy");
        tantivy_path.join("meta.json").exists()
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