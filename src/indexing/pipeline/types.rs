//! Core types for the parallel indexing pipeline
//!
//! This module defines the data structures that flow through pipeline stages.
//! Key design principle: Parse stage produces "raw" types without IDs,
//! Collect stage assigns IDs and produces final types.

use crate::parsing::{Import, LanguageId};
use crate::relationship::RelationshipMetadata;
use crate::symbol::ScopeContext;
use crate::types::{CompactString, FileId, Range, SymbolId};
use crate::{RelationKind, Symbol, SymbolKind, Visibility};
use std::path::PathBuf;
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════
// PARSE stage output - no IDs assigned yet
// ═══════════════════════════════════════════════════════════════════════════

/// Symbol extracted from parsing, before ID assignment.
///
/// The COLLECT stage converts this to a full `Symbol` with ID.
#[derive(Debug, Clone)]
pub struct RawSymbol {
    pub name: CompactString,
    pub kind: SymbolKind,
    pub range: Range,
    pub signature: Option<Box<str>>,
    pub doc_comment: Option<Box<str>>,
    pub visibility: Visibility,
    pub scope_context: Option<ScopeContext>,
}

impl RawSymbol {
    pub fn new(name: impl Into<CompactString>, kind: SymbolKind, range: Range) -> Self {
        Self {
            name: name.into(),
            kind,
            range,
            signature: None,
            doc_comment: None,
            visibility: Visibility::Public,
            scope_context: None,
        }
    }

    pub fn with_signature(mut self, sig: impl Into<Box<str>>) -> Self {
        self.signature = Some(sig.into());
        self
    }

    pub fn with_doc_comment(mut self, doc: impl Into<Box<str>>) -> Self {
        self.doc_comment = Some(doc.into());
        self
    }

    pub fn with_visibility(mut self, vis: Visibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn with_scope_context(mut self, ctx: ScopeContext) -> Self {
        self.scope_context = Some(ctx);
        self
    }
}

/// Import extracted from parsing, before FileId assignment.
///
/// The COLLECT stage converts this to a full `Import` with FileId.
#[derive(Debug, Clone)]
pub struct RawImport {
    pub path: String,
    pub alias: Option<String>,
    pub is_glob: bool,
    pub is_type_only: bool,
}

impl RawImport {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            alias: None,
            is_glob: false,
            is_type_only: false,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    pub fn as_glob(mut self) -> Self {
        self.is_glob = true;
        self
    }

    pub fn as_type_only(mut self) -> Self {
        self.is_type_only = true;
        self
    }

    /// Convert to full Import with FileId
    pub fn into_import(self, file_id: FileId) -> Import {
        Import {
            file_id,
            path: self.path,
            alias: self.alias,
            is_glob: self.is_glob,
            is_type_only: self.is_type_only,
        }
    }
}

/// Relationship extracted from parsing, before resolution.
///
/// Contains ranges for disambiguation when multiple symbols share the same name:
/// - `from_range`: Position of the calling symbol (maps to from_id in COLLECT)
/// - `to_range`: Position of the reference/call site (helps Phase 2 resolution)
#[derive(Debug, Clone)]
pub struct RawRelationship {
    pub from_name: Arc<str>,
    pub from_range: Range,
    pub to_name: Arc<str>,
    pub to_range: Range,
    pub kind: RelationKind,
    pub metadata: Option<RelationshipMetadata>,
}

impl RawRelationship {
    pub fn new(
        from_name: impl Into<Arc<str>>,
        from_range: Range,
        to_name: impl Into<Arc<str>>,
        to_range: Range,
        kind: RelationKind,
    ) -> Self {
        Self {
            from_name: from_name.into(),
            from_range,
            to_name: to_name.into(),
            to_range,
            kind,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: RelationshipMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Complete output from parsing a single file.
///
/// Contains all extracted data without any IDs assigned.
/// The COLLECT stage processes this to assign FileId and SymbolIds.
#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub content_hash: u64,
    pub language_id: LanguageId,
    pub module_path: Option<String>,
    pub raw_symbols: Vec<RawSymbol>,
    pub raw_imports: Vec<RawImport>,
    pub raw_relationships: Vec<RawRelationship>,
}

impl ParsedFile {
    pub fn new(path: PathBuf, content_hash: u64, language_id: LanguageId) -> Self {
        Self {
            path,
            content_hash,
            language_id,
            module_path: None,
            raw_symbols: Vec::new(),
            raw_imports: Vec::new(),
            raw_relationships: Vec::new(),
        }
    }

    pub fn with_module_path(mut self, module_path: impl Into<String>) -> Self {
        self.module_path = Some(module_path.into());
        self
    }

    pub fn symbol_count(&self) -> usize {
        self.raw_symbols.len()
    }

    pub fn import_count(&self) -> usize {
        self.raw_imports.len()
    }

    pub fn relationship_count(&self) -> usize {
        self.raw_relationships.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// COLLECT stage output - IDs assigned, ready for indexing
// ═══════════════════════════════════════════════════════════════════════════

/// File registration data for Tantivy.
///
/// Captures all metadata needed to track indexed files:
/// - Identity: path, file_id
/// - Change detection: content_hash
/// - Parser selection: language_id
/// - Incremental indexing: timestamp
#[derive(Debug, Clone)]
pub struct FileRegistration {
    pub path: PathBuf,
    pub file_id: FileId,
    pub content_hash: u64,
    pub language_id: LanguageId,
    /// Unix timestamp when the file was indexed
    pub timestamp: u64,
}

/// Unresolved relationship with from_id populated.
///
/// This is the same as the existing `UnresolvedRelationship` in simple.rs,
/// kept compatible for reuse during resolution.
#[derive(Debug, Clone)]
pub struct UnresolvedRelationship {
    pub from_id: Option<SymbolId>,
    pub from_name: Arc<str>,
    pub to_name: Arc<str>,
    pub file_id: FileId,
    pub kind: RelationKind,
    pub metadata: Option<RelationshipMetadata>,
    pub to_range: Option<Range>,
}

/// A batch of data ready to be written to Tantivy.
///
/// The INDEX stage receives these batches and writes them efficiently.
#[derive(Debug)]
pub struct IndexBatch {
    /// Symbols with their file paths (for Tantivy document creation)
    pub symbols: Vec<(Symbol, PathBuf)>,
    /// Imports ready to store
    pub imports: Vec<Import>,
    /// Relationships to resolve after all symbols are indexed
    pub unresolved_relationships: Vec<UnresolvedRelationship>,
    /// Files to register in the index
    pub file_registrations: Vec<FileRegistration>,
}

impl IndexBatch {
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            imports: Vec::new(),
            unresolved_relationships: Vec::new(),
            file_registrations: Vec::new(),
        }
    }

    pub fn with_capacity(symbols: usize, imports: usize, rels: usize) -> Self {
        Self {
            symbols: Vec::with_capacity(symbols),
            imports: Vec::with_capacity(imports),
            unresolved_relationships: Vec::with_capacity(rels),
            file_registrations: Vec::new(),
        }
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty() && self.imports.is_empty() && self.file_registrations.is_empty()
    }

    /// Merge another batch into this one
    pub fn merge(&mut self, other: IndexBatch) {
        self.symbols.extend(other.symbols);
        self.imports.extend(other.imports);
        self.unresolved_relationships
            .extend(other.unresolved_relationships);
        self.file_registrations.extend(other.file_registrations);
    }
}

impl Default for IndexBatch {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// READ stage types
// ═══════════════════════════════════════════════════════════════════════════

/// File content read from disk, ready for parsing.
#[derive(Debug)]
pub struct FileContent {
    pub path: PathBuf,
    pub content: String,
    pub hash: u64,
}

impl FileContent {
    pub fn new(path: PathBuf, content: String, hash: u64) -> Self {
        Self {
            path,
            content,
            hash,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Error types
// ═══════════════════════════════════════════════════════════════════════════

/// Errors that can occur during pipeline execution.
///
/// [PIPELINE API] This error type uses proper #[from] conversions:
/// - StorageError (from storage/error.rs) converts automatically
/// - IndexError converts automatically
///
/// TODO(refactor): Once pipeline is complete, consolidate with src/error.rs
/// and remove the duplicate StorageError definition there.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("Failed to read file {path}: {source}")]
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse file {path}: {reason}")]
    Parse { path: PathBuf, reason: String },

    #[error("Unsupported file type: {path}")]
    UnsupportedFileType { path: PathBuf },

    #[error("Channel send error: {0}")]
    ChannelSend(String),

    #[error("Channel receive error: {0}")]
    ChannelRecv(String),

    #[error("Index error: {0}")]
    Index(#[from] crate::IndexError),

    /// [PIPELINE API] Uses storage::StorageError with proper #[from] conversion.
    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
}

/// Result type for pipeline operations.
pub type PipelineResult<T> = Result<T, PipelineError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_symbol_builder() {
        let range = Range::new(1, 0, 1, 10);
        let sym = RawSymbol::new("test_fn", SymbolKind::Function, range)
            .with_signature("fn test_fn() -> i32")
            .with_visibility(Visibility::Public);

        assert_eq!(&*sym.name, "test_fn");
        assert_eq!(sym.kind, SymbolKind::Function);
        assert!(sym.signature.is_some());
    }

    #[test]
    fn test_raw_import_conversion() {
        let raw = RawImport::new("std::collections::HashMap").with_alias("Map");

        let file_id = FileId::new(1).unwrap();
        let import = raw.into_import(file_id);

        assert_eq!(import.path, "std::collections::HashMap");
        assert_eq!(import.alias, Some("Map".to_string()));
        assert_eq!(import.file_id, file_id);
    }

    #[test]
    fn test_parsed_file_counts() {
        let mut parsed = ParsedFile::new(PathBuf::from("test.rs"), 12345, LanguageId::new("rust"));

        parsed.raw_symbols.push(RawSymbol::new(
            "foo",
            SymbolKind::Function,
            Range::new(1, 0, 1, 10),
        ));
        parsed.raw_symbols.push(RawSymbol::new(
            "bar",
            SymbolKind::Function,
            Range::new(2, 0, 2, 10),
        ));

        assert_eq!(parsed.symbol_count(), 2);
        assert_eq!(parsed.import_count(), 0);
    }

    #[test]
    fn test_index_batch_merge() {
        let mut batch1 = IndexBatch::new();
        let mut batch2 = IndexBatch::new();

        // Add some imports to batch2
        batch2.imports.push(Import {
            file_id: FileId::new(1).unwrap(),
            path: "test".to_string(),
            alias: None,
            is_glob: false,
            is_type_only: false,
        });

        batch1.merge(batch2);
        assert_eq!(batch1.imports.len(), 1);
    }
}
