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
    /// 1-indexed editor line of the symbol's definition start
    pub line: u32,
    /// 0-indexed column (machine coordinate)
    pub column: u16,
    pub doc_comment: Option<String>,
    pub signature: Option<String>,
    pub module_path: String,
    /// Stored language identifier (e.g. "rust", "java"); None on rows
    /// persisted before the language field existed
    pub language_id: Option<String>,
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
    /// Bases for decoding stored file paths into the emitted contract
    /// shape (relative to the containing indexed root). Stored paths keep
    /// full provenance (workspace-relative in-tree, absolute out-of-tree)
    /// because incremental diffing and the watcher key on them; the
    /// relativization applies at symbol materialization only. Ordered
    /// workspace_root first, then registered roots longest-first.
    strip_bases: Vec<PathBuf>,
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
            strip_bases: Self::collect_strip_bases(settings),
        })
    }

    fn collect_strip_bases(settings: &crate::config::Settings) -> Vec<PathBuf> {
        let mut bases = Vec::new();
        if let Some(root) = &settings.workspace_root {
            bases.push(root.canonicalize().unwrap_or_else(|_| root.clone()));
        }
        let mut roots: Vec<PathBuf> = settings
            .indexing
            .indexed_paths
            .iter()
            .chain(settings.indexed_paths_cache.iter())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            .collect();
        roots.sort_by_key(|p| std::cmp::Reverse(p.as_os_str().len()));
        for root in roots {
            if !bases.contains(&root) {
                bases.push(root);
            }
        }
        bases
    }

    /// Decode a stored file path into the emitted contract shape: an
    /// absolute path under a registered base returns relative to it;
    /// already-relative and unmatched absolute paths return None and the
    /// caller keeps the stored form (fail-safe, never mangles).
    pub fn to_portable_file_path(&self, stored: &str) -> Option<String> {
        let path = Path::new(stored);
        if path.is_relative() {
            return None;
        }
        for base in &self.strip_bases {
            if let Ok(rel) = path.strip_prefix(base) {
                if rel.as_os_str().is_empty() {
                    continue;
                }
                return Some(rel.to_string_lossy().into_owned());
            }
        }
        None
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
