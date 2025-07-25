//! Metadata tracking for index state and data sources

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use crate::IndexResult;

/// Metadata about the index state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Version of the index format
    pub version: u32,
    
    /// Current data source
    pub data_source: DataSource,
    
    /// Number of symbols in the index
    pub symbol_count: u32,
    
    /// Number of files in the index
    pub file_count: u32,
    
    /// Last modification timestamp
    pub last_modified: u64,
    
}

/// Describes where the index data came from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    /// Loaded from Tantivy index
    Tantivy { 
        path: PathBuf, 
        doc_count: u64,
        timestamp: u64,
    },
    
    /// Fresh index (not loaded)
    Fresh,
}

impl IndexMetadata {
    /// Create new metadata for a fresh index
    pub fn new() -> Self {
        Self {
            version: 1,
            data_source: DataSource::Fresh,
            symbol_count: 0,
            file_count: 0,
            last_modified: crate::indexing::get_utc_timestamp(),
        }
    }
    
    /// Update counts from the indexer
    pub fn update_counts(&mut self, symbol_count: u32, file_count: u32) {
        self.symbol_count = symbol_count;
        self.file_count = file_count;
        self.last_modified = crate::indexing::get_utc_timestamp();
    }
    
    
    /// Save metadata to file
    pub fn save(&self, base_path: &Path) -> IndexResult<()> {
        let metadata_path = base_path.join("index.meta");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| crate::IndexError::General(format!("Failed to serialize metadata: {}", e)))?;
        
        fs::write(&metadata_path, json)
            .map_err(|e| crate::IndexError::FileWrite {
                path: metadata_path,
                source: e,
            })?;
        
        Ok(())
    }
    
    /// Load metadata from file
    pub fn load(base_path: &Path) -> IndexResult<Self> {
        let metadata_path = base_path.join("index.meta");
        
        if !metadata_path.exists() {
            return Ok(Self::new());
        }
        
        let json = fs::read_to_string(&metadata_path)
            .map_err(|e| crate::IndexError::FileRead {
                path: metadata_path.clone(),
                source: e,
            })?;
        
        serde_json::from_str(&json)
            .map_err(|e| crate::IndexError::General(format!("Failed to parse metadata: {}", e)))
    }
    
    /// Display source information to the user
    pub fn display_source(&self) {
        match &self.data_source {
            DataSource::Tantivy { path, doc_count, .. } => {
                eprintln!("Loaded from Tantivy index: {} ({} documents)", path.display(), doc_count);
            }
            DataSource::Fresh => {
                eprintln!("Created fresh index");
            }
        }
        eprintln!("Index contains {} symbols from {} files", self.symbol_count, self.file_count);
    }
}