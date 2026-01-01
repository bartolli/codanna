//! Core types for the parallel indexing pipeline
//!
//! This module defines the data structures that flow through pipeline stages.
//! Key design principle: Parse stage produces "raw" types without IDs,
//! Collect stage assigns IDs and produces final types.

use crate::parsing::{Import, LanguageId, PipelineSymbolCache, ResolveResult};
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
    /// SHA256 hash of file content for change detection (compatible with Tantivy)
    pub content_hash: String,
    pub language_id: LanguageId,
    pub module_path: Option<String>,
    pub raw_symbols: Vec<RawSymbol>,
    pub raw_imports: Vec<RawImport>,
    pub raw_relationships: Vec<RawRelationship>,
}

impl ParsedFile {
    pub fn new(path: PathBuf, content_hash: String, language_id: LanguageId) -> Self {
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
    /// SHA256 hash of file content for change detection (compatible with Tantivy)
    pub content_hash: String,
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
    /// SHA256 hash of file content for change detection (compatible with Tantivy)
    pub hash: String,
}

impl FileContent {
    pub fn new(path: PathBuf, content: String, hash: String) -> Self {
        Self {
            path,
            content,
            hash,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Symbol lookup cache for Phase 2 resolution
// ═══════════════════════════════════════════════════════════════════════════

// Re-export CallerContext from parsing::resolution for convenience
pub use crate::parsing::CallerContext;

/// In-memory symbol cache for O(1) lookups during Phase 2 resolution.
///
/// Built during Phase 1 INDEX stage by retaining symbols after Tantivy write.
/// Provides lock-free concurrent reads for parallel resolution.
///
/// Key design:
/// - `by_id`: SymbolId → Symbol for direct lookups
/// - `by_name`: name → Vec<SymbolId> for candidate resolution
/// - `by_file_id`: FileId → Vec<SymbolId> for local symbol lookup
///
/// Memory: ~500 bytes/symbol, 600K symbols ≈ 300MB
#[derive(Debug)]
pub struct SymbolLookupCache {
    by_id: dashmap::DashMap<crate::types::SymbolId, crate::Symbol>,
    by_name: dashmap::DashMap<Box<str>, Vec<crate::types::SymbolId>>,
    by_file_id: dashmap::DashMap<crate::types::FileId, Vec<crate::types::SymbolId>>,
}

impl Default for SymbolLookupCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolLookupCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            by_id: dashmap::DashMap::new(),
            by_name: dashmap::DashMap::new(),
            by_file_id: dashmap::DashMap::new(),
        }
    }

    /// Create with pre-allocated capacity.
    pub fn with_capacity(symbols: usize) -> Self {
        Self {
            by_id: dashmap::DashMap::with_capacity(symbols),
            by_name: dashmap::DashMap::with_capacity(symbols / 10), // Fewer unique names
            by_file_id: dashmap::DashMap::with_capacity(symbols / 50), // ~50 symbols/file avg
        }
    }

    /// Insert a symbol into the cache.
    pub fn insert(&self, symbol: crate::Symbol) {
        let id = symbol.id;
        let file_id = symbol.file_id;
        let name: Box<str> = symbol.name.as_ref().into();

        // Insert into by_id
        self.by_id.insert(id, symbol);

        // Insert into by_name (append to candidates)
        self.by_name.entry(name).or_default().push(id);

        // Insert into by_file_id (append to file's symbols)
        self.by_file_id.entry(file_id).or_default().push(id);
    }

    /// Get symbol by ID (O(1)).
    pub fn get(&self, id: crate::types::SymbolId) -> Option<crate::Symbol> {
        self.by_id.get(&id).map(|r| r.value().clone())
    }

    /// Get symbol reference by ID (O(1), no clone).
    pub fn get_ref(
        &self,
        id: crate::types::SymbolId,
    ) -> Option<dashmap::mapref::one::Ref<'_, crate::types::SymbolId, crate::Symbol>> {
        self.by_id.get(&id)
    }

    /// Get candidate symbol IDs by name (O(1)).
    pub fn lookup_candidates(&self, name: &str) -> Vec<crate::types::SymbolId> {
        self.by_name
            .get(name)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    /// Get symbol IDs defined in a file (O(1)).
    ///
    /// Used by CONTEXT stage to find local symbols for a file.
    pub fn symbols_in_file(&self, file_id: crate::types::FileId) -> Vec<crate::types::SymbolId> {
        self.by_file_id
            .get(&file_id)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    /// Number of files in cache.
    pub fn file_count(&self) -> usize {
        self.by_file_id.len()
    }

    /// Number of symbols in cache.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Number of unique names in cache.
    pub fn unique_names(&self) -> usize {
        self.by_name.len()
    }
}

impl PipelineSymbolCache for SymbolLookupCache {
    fn resolve(
        &self,
        name: &str,
        caller: &CallerContext,
        to_range: Option<&Range>,
        imports: &[Import],
    ) -> ResolveResult {
        // Get all candidates by name first
        let candidates = self.lookup_candidates(name);
        if candidates.is_empty() {
            return ResolveResult::NotFound;
        }

        // Tier 1: Local - Same file + defined before to_range
        let local_matches: Vec<_> = candidates
            .iter()
            .filter_map(|&id| {
                let sym = self.by_id.get(&id)?;
                if sym.file_id != caller.file_id {
                    return None;
                }
                // If we have a to_range, prefer symbols defined before it
                if let Some(ref_range) = to_range {
                    if sym.range.start_line <= ref_range.start_line {
                        return Some(id);
                    }
                    // Symbol defined after reference - still local but lower priority
                    return Some(id);
                }
                Some(id)
            })
            .collect();

        if local_matches.len() == 1 {
            return ResolveResult::Found(local_matches[0]);
        }
        if local_matches.len() > 1 {
            // Multiple local matches - let RESOLVE stage disambiguate by range
            // (shadowing: closest definition before call site wins)
            return ResolveResult::Ambiguous(local_matches);
        }

        // Tier 2: Import - Name matches import alias or last segment
        for import in imports {
            // Check if name matches import alias
            if import.alias.as_deref() == Some(name) {
                // Look for symbol matching import path
                if let Some(id) = self.find_by_import_path(&import.path, caller.language_id) {
                    return ResolveResult::Found(id);
                }
            }
            // Check if name matches last segment of import path
            let last_segment = import
                .path
                .rsplit("::")
                .next()
                .or_else(|| import.path.rsplit('.').next())
                .or_else(|| import.path.rsplit('/').next());
            if last_segment == Some(name) {
                if let Some(id) = self.find_by_import_path(&import.path, caller.language_id) {
                    return ResolveResult::Found(id);
                }
            }
        }

        // Tier 3: Same language with three-level visibility check
        // Visibility: same file → same module → different module (must be Public)
        let same_language: Vec<_> = candidates
            .iter()
            .filter_map(|&id| {
                let sym = self.by_id.get(&id)?;
                if sym.language_id.as_ref() != Some(&caller.language_id) {
                    return None;
                }
                // Three-level visibility check:
                // 1. Same file = always visible
                if sym.file_id == caller.file_id {
                    return Some(id);
                }
                // 2. Same module = always visible
                if caller.is_same_module(sym.module_path.as_deref()) {
                    return Some(id);
                }
                // 3. Different module = must be Public
                if sym.visibility == crate::Visibility::Public {
                    return Some(id);
                }
                None
            })
            .collect();

        if same_language.len() == 1 {
            return ResolveResult::Found(same_language[0]);
        }
        if same_language.len() > 1 {
            return ResolveResult::Ambiguous(same_language);
        }

        // Tier 4: Cross-file fallback with same visibility rules
        let visible_candidates: Vec<_> = candidates
            .iter()
            .filter_map(|&id| {
                let sym = self.by_id.get(&id)?;
                // Same file = always visible
                if sym.file_id == caller.file_id {
                    return Some(id);
                }
                // Same module = always visible
                if caller.is_same_module(sym.module_path.as_deref()) {
                    return Some(id);
                }
                // Different module = must be Public
                if sym.visibility == crate::Visibility::Public {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();

        if visible_candidates.len() == 1 {
            return ResolveResult::Found(visible_candidates[0]);
        }
        if !visible_candidates.is_empty() {
            return ResolveResult::Ambiguous(visible_candidates);
        }

        ResolveResult::NotFound
    }

    fn get(&self, id: SymbolId) -> Option<Symbol> {
        self.by_id.get(&id).map(|r| r.value().clone())
    }

    fn symbols_in_file(&self, file_id: FileId) -> Vec<SymbolId> {
        self.by_file_id
            .get(&file_id)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    fn lookup_candidates(&self, name: &str) -> Vec<SymbolId> {
        self.by_name
            .get(name)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }
}

impl SymbolLookupCache {
    /// Build cache from all symbols in a DocumentIndex.
    ///
    /// [PIPELINE API] Used for single-file indexing when we need a complete cache
    /// for Phase 2 resolution but don't have symbols in memory.
    ///
    /// Note: This queries Tantivy for all symbols, which is expensive for large indexes.
    /// For bulk indexing, prefer building the cache during INDEX stage.
    pub fn from_index(index: &crate::storage::DocumentIndex) -> PipelineResult<Self> {
        // Get total count to pre-allocate
        let count = index.document_count().unwrap_or(0) as usize;
        let cache = Self::with_capacity(count);

        // Load all symbols from Tantivy
        // Use a large limit to get all symbols
        let symbols = index
            .get_all_symbols(1_000_000)
            .map_err(|e| PipelineError::Index(crate::IndexError::Storage(e)))?;

        for symbol in symbols {
            cache.insert(symbol);
        }

        Ok(cache)
    }

    /// Find symbol by import path and language.
    fn find_by_import_path(&self, path: &str, language_id: LanguageId) -> Option<SymbolId> {
        // Extract the symbol name from path (last segment)
        let name = path
            .rsplit("::")
            .next()
            .or_else(|| path.rsplit('.').next())
            .or_else(|| path.rsplit('/').next())?;

        // Look up candidates and filter by module path + language
        let candidates = self.lookup_candidates(name);
        for id in candidates {
            if let Some(sym) = self.by_id.get(&id) {
                if sym.language_id.as_ref() == Some(&language_id) {
                    // Check if module_path matches (approximately)
                    if let Some(ref module_path) = sym.module_path {
                        if path.contains(module_path.as_ref()) || module_path.contains(path) {
                            return Some(id);
                        }
                    }
                }
            }
        }
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 2 types - CONTEXT, RESOLVE, WRITE stages
// ═══════════════════════════════════════════════════════════════════════════

/// Context for resolving relationships in a single file.
///
/// Built by CONTEXT stage via `behavior.build_resolution_context_with_pipeline_cache()`.
/// Contains ResolutionScope for language-specific resolution including path alias enhancement.
pub struct ResolutionContext {
    /// File being resolved
    pub file_id: FileId,
    /// Language of the file
    pub language_id: LanguageId,
    /// Imports declared in this file
    pub imports: Vec<Import>,
    /// Symbols defined in this file
    pub local_symbols: Vec<SymbolId>,
    /// Language-specific resolution scope from behavior
    pub scope: Box<dyn crate::parsing::ResolutionScope>,
    /// Unresolved relationships originating from this file
    pub unresolved_rels: Vec<UnresolvedRelationship>,
}

impl std::fmt::Debug for ResolutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolutionContext")
            .field("file_id", &self.file_id)
            .field("language_id", &self.language_id)
            .field("imports", &self.imports.len())
            .field("local_symbols", &self.local_symbols.len())
            .field("unresolved_rels", &self.unresolved_rels.len())
            .finish()
    }
}

impl ResolutionContext {
    /// Number of relationships to resolve
    pub fn relationship_count(&self) -> usize {
        self.unresolved_rels.len()
    }

    /// Resolve a name using the language-specific scope
    pub fn resolve(&self, name: &str) -> Option<SymbolId> {
        self.scope.resolve(name)
    }
}

/// A fully resolved relationship ready for storage.
#[derive(Debug, Clone)]
pub struct ResolvedRelationship {
    pub from_id: SymbolId,
    pub to_id: SymbolId,
    pub kind: RelationKind,
    pub metadata: Option<RelationshipMetadata>,
}

impl ResolvedRelationship {
    pub fn new(from_id: SymbolId, to_id: SymbolId, kind: RelationKind) -> Self {
        Self {
            from_id,
            to_id,
            kind,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: RelationshipMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Batch of resolved relationships ready for WRITE stage.
#[derive(Debug, Default)]
pub struct ResolvedBatch {
    pub relationships: Vec<ResolvedRelationship>,
}

impl ResolvedBatch {
    pub fn new() -> Self {
        Self {
            relationships: Vec::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            relationships: Vec::with_capacity(cap),
        }
    }

    pub fn push(&mut self, rel: ResolvedRelationship) {
        self.relationships.push(rel);
    }

    pub fn len(&self) -> usize {
        self.relationships.len()
    }

    pub fn is_empty(&self) -> bool {
        self.relationships.is_empty()
    }

    pub fn merge(&mut self, other: ResolvedBatch) {
        self.relationships.extend(other.relationships);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DISCOVER stage output - categorized file lists
// ═══════════════════════════════════════════════════════════════════════════

/// Result of incremental file discovery.
///
/// Categorizes files into new, modified, and deleted for efficient incremental indexing.
#[derive(Debug, Default)]
pub struct DiscoverResult {
    /// Files that exist on disk but not in the index.
    pub new_files: Vec<PathBuf>,
    /// Files that exist in both but have different hashes.
    pub modified_files: Vec<PathBuf>,
    /// Files that exist in the index but not on disk.
    pub deleted_files: Vec<PathBuf>,
}

impl DiscoverResult {
    /// Total number of files that need processing.
    pub fn files_to_process(&self) -> usize {
        self.new_files.len() + self.modified_files.len()
    }

    /// Total number of files that need cleanup.
    pub fn files_to_cleanup(&self) -> usize {
        self.deleted_files.len() + self.modified_files.len()
    }

    /// Check if there's any work to do.
    pub fn is_empty(&self) -> bool {
        self.new_files.is_empty() && self.modified_files.is_empty() && self.deleted_files.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Error types
// ═══════════════════════════════════════════════════════════════════════════

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

/// Statistics from single-file indexing.
///
/// [PIPELINE API] Returned by `Pipeline::index_file_single()`.
#[derive(Debug, Clone)]
pub struct SingleFileStats {
    /// The file ID assigned to this file.
    pub file_id: crate::FileId,
    /// Whether the file was actually indexed (false if cached).
    pub indexed: bool,
    /// Whether the file was cached (unchanged since last index).
    pub cached: bool,
    /// Number of symbols found in the file.
    pub symbols_found: usize,
    /// Number of relationships resolved.
    pub relationships_resolved: usize,
    /// Time taken.
    pub elapsed: std::time::Duration,
}

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
        let mut parsed = ParsedFile::new(
            PathBuf::from("test.rs"),
            "abc123def456".to_string(),
            LanguageId::new("rust"),
        );

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
