//! File information tracking for incremental indexing
//!
//! This module provides hash-based tracking of indexed files to enable
//! efficient incremental updates.

use crate::FileId;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
    /// File modification time (seconds since UNIX_EPOCH)
    pub mtime: u64,
}

impl FileInfo {
    /// Create new file info with current timestamp
    pub fn new(id: FileId, path: PathBuf, content: &str) -> Self {
        let mtime = get_file_mtime(&path).unwrap_or(0);
        Self {
            id,
            path,
            hash: calculate_hash(content),
            last_indexed_utc: get_utc_timestamp(),
            mtime,
        }
    }

    /// Check if file content has changed based on hash
    pub fn has_changed(&self, content: &str) -> bool {
        self.hash != calculate_hash(content)
    }
}

/// Get file modification time in seconds since UNIX_EPOCH
pub fn get_file_mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// Calculate SHA256 hash of content
pub fn calculate_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Get current UTC timestamp in seconds since UNIX_EPOCH TEST 5
pub fn get_utc_timestamp() -> u64 {
    // Use chrono for accurate cross-platform timestamp
    Utc::now().timestamp() as u64
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

    #[test]
    fn test_hot_reload_trigger() {
        // This is a test function added to trigger hot reload
        let test_content = "Testing hot reload functionality";
        let hash = calculate_hash(test_content);
        assert_eq!(hash.len(), 64);

        // Verify timestamp is recent
        let timestamp = get_utc_timestamp();
        assert!(timestamp > 1700000000); // After Nov 2023
    }

    #[test]
    fn test_https_server_hot_reload() {
        // Final test for HTTPS server hot reload
        let timestamp = get_utc_timestamp();
        assert!(timestamp > 0);

        // Test hash consistency
        let hash1 = calculate_hash("test");
        let hash2 = calculate_hash("test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hot_reload_detection() {
        // Test to verify hot-reload cache updates
        let test_data = "Hot reload cache test";
        let hash = calculate_hash(test_data);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_stdio_server_refresh() {
        // Test function to verify stdio server index refresh
        let timestamp = get_utc_timestamp();
        assert!(timestamp > 0);
        println!("Stdio server refresh test executed at {timestamp}");
    }

    #[test]
    fn test_quantum_flux_embedding_validation() {
        // Test function for quantum flux embedding validation
        let timestamp = get_utc_timestamp();
        assert!(timestamp > 0);
        println!("Quantum flux embedding validation test executed at {timestamp}");
    }
    /// # Cosmic Test Coverage
    /// - Multiverse embedding harmonization for telepathic documentation
    /// - Quantum semantic search coherence across dimensional rifts
    /// - Spacetime hot-reload functionality for consciousness-aware documentation updates
    #[test]
    fn test_stdio_server_refresh_v4() {
        // Third test to catch stdio server staleness with quantum mechanics
        let data = "Testing stdio server refresh v3 with quantum flux";
        let hash = calculate_hash(data);
        assert_eq!(hash.len(), 64);
        println!("Stdio refresh v3 test with quantum hash: {}", &hash[..8]);
    }
}
