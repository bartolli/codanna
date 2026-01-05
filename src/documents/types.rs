//! Core types for document chunking and embedding.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::path::PathBuf;

/// Unique identifier for a document chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(NonZeroU32);

impl ChunkId {
    /// Create a new ChunkId from a non-zero value.
    pub fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    /// Create a ChunkId from a u32, returning None if zero.
    pub fn from_u32(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    /// Get the inner value as u32.
    pub fn value(&self) -> u32 {
        self.0.get()
    }

    /// Get the inner value as u32 (alias for value()).
    pub fn get(&self) -> u32 {
        self.0.get()
    }

    /// Convert to bytes for storage (little-endian).
    pub fn to_bytes(&self) -> [u8; 4] {
        self.0.get().to_le_bytes()
    }

    /// Create from bytes (little-endian).
    pub fn from_bytes(bytes: [u8; 4]) -> Option<Self> {
        let value = u32::from_le_bytes(bytes);
        Self::from_u32(value)
    }
}

/// Unique identifier for a document collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CollectionId(NonZeroU32);

impl CollectionId {
    /// Create a new CollectionId from a non-zero value.
    pub fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    /// Create a CollectionId from a u32, returning None if zero.
    pub fn from_u32(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    /// Get the inner value as u32.
    pub fn value(&self) -> u32 {
        self.0.get()
    }

    /// Get the inner value as u32 (alias for value()).
    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

/// A chunk of a document with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    /// Unique identifier for this chunk.
    pub id: ChunkId,

    /// Collection this chunk belongs to.
    pub collection_id: CollectionId,

    /// Path to the source document (relative to collection root).
    pub source_path: PathBuf,

    /// Byte range in the source document (start, end).
    pub byte_range: (usize, usize),

    /// Heading hierarchy context (e.g., ["Chapter 1", "Section 1.2"]).
    pub heading_context: Vec<String>,

    /// The actual text content of this chunk.
    pub content: String,
}

impl DocumentChunk {
    /// Create a new document chunk.
    pub fn new(
        id: ChunkId,
        collection_id: CollectionId,
        source_path: PathBuf,
        byte_range: (usize, usize),
        heading_context: Vec<String>,
        content: String,
    ) -> Self {
        Self {
            id,
            collection_id,
            source_path,
            byte_range,
            heading_context,
            content,
        }
    }

    /// Get a preview of the content (first N characters).
    pub fn preview(&self, max_chars: usize) -> &str {
        if self.content.len() <= max_chars {
            &self.content
        } else {
            // Find a safe UTF-8 boundary
            let mut end = max_chars;
            while end > 0 && !self.content.is_char_boundary(end) {
                end -= 1;
            }
            &self.content[..end]
        }
    }

    /// Get the length of the content in characters.
    pub fn char_count(&self) -> usize {
        self.content.chars().count()
    }
}

/// State of a source file for change detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    /// Path to the source file.
    pub path: PathBuf,

    /// Collection this file belongs to.
    #[serde(default)]
    pub collection: String,

    /// SHA256 hash of the file content.
    pub content_hash: String,

    /// Chunk IDs generated from this file (for deletion on change).
    pub chunk_ids: Vec<ChunkId>,

    /// UTC timestamp when last indexed.
    pub last_indexed: u64,

    /// File modification time (seconds since UNIX_EPOCH) for fast change detection.
    #[serde(default)]
    pub mtime: u64,
}

impl FileState {
    /// Create a new file state.
    pub fn new(
        path: PathBuf,
        collection: String,
        content_hash: String,
        chunk_ids: Vec<ChunkId>,
        mtime: u64,
    ) -> Self {
        Self {
            path,
            collection,
            content_hash,
            chunk_ids,
            last_indexed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            mtime,
        }
    }

    /// Check if the file has changed based on content hash.
    pub fn has_changed(&self, new_hash: &str) -> bool {
        self.content_hash != new_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_id_roundtrip() {
        let id = ChunkId::from_u32(42).unwrap();
        let bytes = id.to_bytes();
        let recovered = ChunkId::from_bytes(bytes).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn test_chunk_id_zero_returns_none() {
        assert!(ChunkId::from_u32(0).is_none());
    }

    #[test]
    fn test_document_chunk_preview() {
        let chunk = DocumentChunk::new(
            ChunkId::from_u32(1).unwrap(),
            CollectionId::from_u32(1).unwrap(),
            PathBuf::from("test.md"),
            (0, 100),
            vec!["Chapter 1".to_string()],
            "Hello, world! This is a test.".to_string(),
        );

        assert_eq!(chunk.preview(5), "Hello");
        assert_eq!(chunk.preview(100), "Hello, world! This is a test.");
    }

    #[test]
    fn test_file_state_change_detection() {
        let state = FileState::new(
            PathBuf::from("test.md"),
            "docs".to_string(),
            "abc123".to_string(),
            vec![ChunkId::from_u32(1).unwrap()],
            1700000000,
        );

        assert!(!state.has_changed("abc123"));
        assert!(state.has_changed("def456"));
    }
}
