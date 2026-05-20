//! SHA-256 computation utilities for configuration file hashing

use super::{ResolutionError, ResolutionResult, Sha256Hash};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Compute SHA-256 hash of string content
///
/// Returns a hex-encoded SHA-256 hash (64 characters)
pub fn compute_sha256(content: &str) -> Sha256Hash {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    Sha256Hash(hex::encode(result))
}

/// Compute SHA-256 hash of a file's contents
///
/// Reads the entire file and computes its SHA-256 hash.
/// Returns ResolutionError for I/O failures.
pub fn compute_file_sha(path: &Path) -> ResolutionResult<Sha256Hash> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ResolutionError::cache_io(path.to_path_buf(), e))?;
    Ok(compute_sha256(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_deterministic() {
        let hash1 = compute_sha256("test content");
        let hash2 = compute_sha256("test content");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn sha256_differs_for_different_content() {
        let hash1 = compute_sha256("content A");
        let hash2 = compute_sha256("content B");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn sha256_hex_is_64_chars() {
        let hash = compute_sha256("any content");
        assert_eq!(hash.0.len(), 64);
        assert!(hash.0.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
