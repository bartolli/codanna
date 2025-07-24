//! File information tracking for incremental indexing
//! 
//! This module provides hash-based tracking of indexed files to enable
//! efficient incremental updates.

use crate::FileId;
use sha2::{Sha256, Digest};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Information about an indexed file
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// Unique identifier for this file
    pub id: FileId,
    /// Path to the file
    pub path: PathBuf,
    /// SHA256 hash of file content
    pub hash: String,
    /// UTC timestamp when last indexed (seconds since UNIX_EPOCH)
    pub last_indexed_utc: u64,
}

impl FileInfo {
    /// Create new file info with current timestamp
    pub fn new(id: FileId, path: PathBuf, content: &str) -> Self {
        Self {
            id,
            path,
            hash: calculate_hash(content),
            last_indexed_utc: get_utc_timestamp(),
        }
    }
    
    /// Check if file content has changed based on hash
    pub fn has_changed(&self, content: &str) -> bool {
        self.hash != calculate_hash(content)
    }
}

/// Calculate SHA256 hash of content
pub fn calculate_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Get current UTC timestamp in seconds since UNIX_EPOCH
pub fn get_utc_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before UNIX_EPOCH")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hash_calculation() {
        let content1 = "Hello, World!";
        let content2 = "Hello, World!";
        let content3 = "Hello, world!"; // Different case
        
        let hash1 = calculate_hash(content1);
        let hash2 = calculate_hash(content2);
        let hash3 = calculate_hash(content3);
        
        // Same content should produce same hash
        assert_eq!(hash1, hash2);
        // Different content should produce different hash
        assert_ne!(hash1, hash3);
        
        // Hash should be 64 characters (256 bits in hex)
        assert_eq!(hash1.len(), 64);
    }
    
    #[test]
    fn test_utc_timestamp() {
        let ts1 = get_utc_timestamp();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ts2 = get_utc_timestamp();
        
        // Timestamps should be monotonically increasing
        assert!(ts2 >= ts1);
        // Should be a reasonable Unix timestamp (after year 2020)
        assert!(ts1 > 1577836800); // Jan 1, 2020
    }
    
    #[test]
    fn test_file_info_change_detection() {
        let file_id = FileId::new(1).unwrap();
        let path = PathBuf::from("test.rs");
        let content = "fn main() {}";
        
        let info = FileInfo::new(file_id, path, content);
        
        // Same content should not be detected as changed
        assert!(!info.has_changed(content));
        
        // Different content should be detected as changed
        assert!(info.has_changed("fn main() { println!(\"Hello\"); }"));
    }
}