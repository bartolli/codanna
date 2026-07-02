//! Tantivy-based full-text search for documentation and code
//!
//! This module provides rich full-text search capabilities using Tantivy,
//! enabling semantic search across documentation, code, and symbols.

use super::StorageResult;
use crate::{SymbolId, SymbolKind};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::RwLock;
use tantivy::{
    Index, IndexReader, IndexSettings, IndexWriter, ReloadPolicy, TantivyDocument as Document,
    directory::MmapDirectory,
    tokenizer::{NgramTokenizer, TextAnalyzer},
};

mod codec;
mod query;
mod schema;
mod writer;

pub use codec::VectorMetadata;
pub use schema::IndexSchema;

/// Search result with rich metadata
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub symbol_id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: u32,
    pub column: u16,
    pub doc_comment: Option<String>,
    pub signature: Option<String>,
    pub module_path: String,
    pub score: f32,
    pub highlights: Vec<TextHighlight>,
    pub context: Option<String>,
}

/// Highlighted text region
#[derive(Debug, Clone, Serialize)]
pub struct TextHighlight {
    pub field: String,
    pub start: usize,
    pub end: usize,
}

/// Document index for full-text search
pub struct DocumentIndex {
    index: Index,
    reader: IndexReader,
    schema: IndexSchema,
    index_path: PathBuf,
    pub(crate) writer: RwLock<Option<IndexWriter<Document>>>,
    /// Tantivy heap size in bytes
    heap_size: usize,
    /// Maximum retry attempts for transient errors
    max_retry_attempts: u32,
    /// Pending symbol counter during batch operations
    pending_symbol_counter: Mutex<Option<u32>>,
    /// Pending file counter during batch operations
    pending_file_counter: Mutex<Option<u32>>,
}

impl std::fmt::Debug for DocumentIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentIndex")
            .field("index_path", &self.index_path)
            .field("schema", &self.schema)
            .finish()
    }
}

impl DocumentIndex {
    /// Create a new document index
    pub fn new(
        index_path: impl AsRef<Path>,
        settings: &crate::config::Settings,
    ) -> StorageResult<Self> {
        let index_path = index_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&index_path)?;

        // Extract and validate heap size
        let heap_size = settings.indexing.tantivy_heap_mb * 1_000_000;
        let heap_size = heap_size.clamp(10_000_000, 1_000_000_000); // 10MB-1GB

        let max_retry_attempts = settings.indexing.max_retry_attempts;

        let (schema, index_schema) = IndexSchema::build();

        // Create or open the index
        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(&index_path)?
        } else {
            let dir = MmapDirectory::open(&index_path)?;
            Index::create(dir, schema, IndexSettings::default())?
        };

        // Register custom tokenizer for partial matching (ngram with min_gram=3, max_gram=10)
        // This allows "Archive" to match "ArchiveAppService"
        let ngram_tokenizer =
            TextAnalyzer::builder(NgramTokenizer::new(3, 10, false).unwrap()).build();
        index.tokenizers().register("ngram", ngram_tokenizer);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        // If opening existing index, reload to get latest segments
        if index_path.join("meta.json").exists() {
            reader.reload()?;
        }

        Ok(Self {
            index,
            reader,
            schema: index_schema,
            index_path,
            writer: RwLock::new(None),
            heap_size,
            max_retry_attempts,
            pending_symbol_counter: Mutex::new(None),
            pending_file_counter: Mutex::new(None),
        })
    }

    /// Get the path where the index is stored
    ///
    /// TODO: Potential use cases for this method:
    /// - Recreating the index if corrupted
    /// - Moving or copying the index to another location
    /// - Displaying index location in diagnostics or status commands
    /// - Cleaning up the entire index directory
    /// - Backing up the index data
    pub fn path(&self) -> &Path {
        &self.index_path
    }

    // Internal methods for storage operations (accessible within crate)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn test_document_index_creation() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        assert_eq!(index.document_count().unwrap(), 0);
    }

    #[test]
    fn test_document_index_debug_impl() {
        let temp_dir = TempDir::new().unwrap();

        // Test Debug without vector support
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();
        let debug_str = format!("{index:?}");
        assert!(debug_str.contains("DocumentIndex"));
        assert!(debug_str.contains("index_path"));
    }

    // ==================== Language Filtering Tests ====================
    // TDD tests for Sprint 4: Task 4.1 - Language filtering support
}
