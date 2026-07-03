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
use std::sync::{Arc, Mutex};

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
    pub doc_comment: Option<CompactString>,
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

    pub fn with_doc_comment(mut self, doc: impl Into<CompactString>) -> Self {
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
    /// File modification time (seconds since UNIX_EPOCH)
    pub mtime: u64,
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
    pub symbols: Vec<Symbol>,
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

/// A batch of embedding candidates for the EMBED stage.
///
/// Sent from COLLECT to EMBED in parallel with IndexBatch to INDEX.
/// Contains symbols that have doc_comments suitable for embedding.
#[derive(Debug)]
pub struct EmbeddingBatch {
    /// Embedding candidates: (symbol_id, doc_comment, language)
    pub candidates: Vec<(SymbolId, CompactString, Box<str>)>,
}

impl EmbeddingBatch {
    pub fn new() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }

    pub fn with_capacity(size: usize) -> Self {
        Self {
            candidates: Vec::with_capacity(size),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }
}

impl Default for EmbeddingBatch {
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
/// - `by_name`: name → `Vec<SymbolId>` for candidate resolution
/// - `by_file_id`: FileId → `Vec<SymbolId>` for local symbol lookup
///
/// Identity-ordered entry in the `by_name` candidate lists. Derived
/// `Ord` compares file_path, then start_line, then id — the id arm only
/// breaks ties within one run; the identity prefix is what holds across
/// runs (ids are session-scoped).
#[derive(Debug, PartialEq, Eq)]
struct NameCandidate {
    file_path: Box<str>,
    start_line: u32,
    id: crate::types::SymbolId,
}

impl Ord for NameCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.file_path
            .cmp(&other.file_path)
            .then_with(|| self.start_line.cmp(&other.start_line))
            .then_with(|| self.id.value().cmp(&other.id.value()))
    }
}

impl PartialOrd for NameCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Memory: ~500 bytes/symbol, 600K symbols ≈ 300MB
///
/// `by_name` candidates stay sorted by symbol identity
/// (file_path, start_line, id): insertion order is parse-completion
/// order and ids are assigned in collect-arrival order — both vary run
/// to run — so first-match consumers need a tree-stable order to pick
/// deterministically. `by_file_id` lists stay sorted by (start_line, id)
/// for the same reason: scope registration iterates them into last-wins
/// name maps, so their order decides same-name winners.
#[derive(Debug)]
pub struct SymbolLookupCache {
    by_id: dashmap::DashMap<crate::types::SymbolId, crate::Symbol>,
    by_name: dashmap::DashMap<Box<str>, Vec<NameCandidate>>,
    by_file_id: dashmap::DashMap<crate::types::FileId, Vec<(u32, crate::types::SymbolId)>>,
    /// Re-exported paths: "pkg.helper" -> the symbol defined at "pkg.a.helper"
    /// when pkg's namespace imports it. Populated by the Phase 2 pre-pass.
    module_aliases: dashmap::DashMap<Box<str>, crate::types::SymbolId>,
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
            module_aliases: dashmap::DashMap::new(),
        }
    }

    /// Create with pre-allocated capacity.
    pub fn with_capacity(symbols: usize) -> Self {
        Self {
            by_id: dashmap::DashMap::with_capacity(symbols),
            by_name: dashmap::DashMap::with_capacity(symbols / 10), // Fewer unique names
            by_file_id: dashmap::DashMap::with_capacity(symbols / 50), // ~50 symbols/file avg
            module_aliases: dashmap::DashMap::new(),
        }
    }

    /// Insert a symbol into the cache.
    pub fn insert(&self, symbol: crate::Symbol) {
        let id = symbol.id;
        let file_id = symbol.file_id;
        let start_line = symbol.range.start_line;
        let name: Box<str> = symbol.name.as_ref().into();
        let candidate = NameCandidate {
            file_path: symbol.file_path.clone(),
            start_line,
            id,
        };

        // Insert into by_id
        self.by_id.insert(id, symbol);

        // Insert into by_name at the identity-sorted position
        {
            let mut entry = self.by_name.entry(name).or_default();
            let pos = entry
                .binary_search(&candidate)
                .unwrap_or_else(|insert_at| insert_at);
            entry.insert(pos, candidate);
        }

        // Insert into by_file_id at the (start_line, id)-sorted position;
        // file_path is constant within a file, so this is identity order
        {
            let mut entry = self.by_file_id.entry(file_id).or_default();
            let pos = entry
                .binary_search_by_key(&(start_line, id.0), |&(line, sid)| (line, sid.0))
                .unwrap_or_else(|insert_at| insert_at);
            entry.insert(pos, (start_line, id));
        }
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

    /// Get candidate symbol IDs by name (O(1)), in identity order.
    pub fn lookup_candidates(&self, name: &str) -> Vec<crate::types::SymbolId> {
        self.by_name
            .get(name)
            .map(|r| r.value().iter().map(|c| c.id).collect())
            .unwrap_or_default()
    }

    /// Whether any candidate exists for `name` (O(1), no clone).
    pub fn has_candidates(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Get symbol IDs defined in a file, in identity (source) order.
    ///
    /// Used by CONTEXT stage to find local symbols for a file.
    pub fn symbols_in_file(&self, file_id: crate::types::FileId) -> Vec<crate::types::SymbolId> {
        self.by_file_id
            .get(&file_id)
            .map(|r| r.value().iter().map(|&(_, id)| id).collect())
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

    /// All file ids present in the cache.
    pub fn file_ids(&self) -> Vec<crate::types::FileId> {
        self.by_file_id.iter().map(|e| *e.key()).collect()
    }

    /// Register a re-exported path for a symbol.
    pub fn register_module_alias(&self, path: &str, id: crate::types::SymbolId) {
        self.module_aliases.insert(path.into(), id);
    }

    /// Resolve a re-exported path ("pkg.helper") to the defining symbol.
    pub fn resolve_module_alias(&self, path: &str) -> Option<crate::types::SymbolId> {
        self.module_aliases.get(path).map(|r| *r.value())
    }

    /// Populate module aliases from per-module import lists.
    ///
    /// Python semantics: an import binds the name in the importing module's
    /// namespace, so `pkg/__init__.py: from pkg.a import helper` exposes
    /// `pkg.helper` (module_path stripping already maps `__init__` to the
    /// package itself; plain modules re-export the same way). Glob imports
    /// (`from pkg.a import *`) expose the source module's public
    /// module-level names. Entries are (module_path, imports) pairs with
    /// paths already normalized to absolute form at parse time. Iterates so
    /// re-export chains converge.
    pub fn populate_module_aliases(
        &self,
        entries: &[(String, Vec<Import>)],
        language: &LanguageId,
    ) {
        // Chains are shallow in practice; the cap only guards degenerate cycles.
        const MAX_ROUNDS: usize = 8;

        for _ in 0..MAX_ROUNDS {
            let mut progressed = false;

            for (module, imports) in entries {
                for import in imports {
                    if import.is_glob || import.path.starts_with('.') {
                        continue;
                    }
                    let (module_part, name) = match import.path.rfind('.') {
                        Some(pos) => (&import.path[..pos], &import.path[pos + 1..]),
                        None => continue,
                    };
                    let local = import.alias.as_deref().unwrap_or(name);
                    let alias_key = format!("{module}.{local}");
                    if alias_key == import.path {
                        continue;
                    }
                    if self.module_aliases.contains_key(alias_key.as_str()) {
                        continue;
                    }

                    // The imported path may itself be a re-export (chain hop).
                    let target = self.resolve_module_alias(&import.path).or_else(|| {
                        self.lookup_candidates(name).into_iter().find(|&id| {
                            self.by_id.get(&id).is_some_and(|sym| {
                                sym.language_id.as_ref() == Some(language)
                                    && (sym.module_path.as_deref() == Some(module_part)
                                        || (sym.kind == crate::types::SymbolKind::Module
                                            && sym.module_path.as_deref() == Some(&import.path)))
                            })
                        })
                    });

                    if let Some(id) = target {
                        self.module_aliases.insert(alias_key.into(), id);
                        progressed = true;
                    }
                }
            }

            progressed |= self.expand_glob_reexports(entries, language);

            if !progressed {
                break;
            }
        }
    }

    /// Expand `from module import *` re-exports.
    ///
    /// Exposes each public module-level name of the glob source under the
    /// importing module, plus names the source itself re-exports (chained
    /// globs converge through the caller's fixpoint loop). Inserts only
    /// missing keys, so explicit imports of the same name win.
    fn expand_glob_reexports(
        &self,
        entries: &[(String, Vec<Import>)],
        language: &LanguageId,
    ) -> bool {
        use std::collections::HashMap;

        // Glob source module -> importing modules
        let mut globs: HashMap<&str, Vec<&str>> = HashMap::new();
        for (module, imports) in entries {
            for import in imports {
                if import.is_glob && !import.path.starts_with('.') {
                    globs
                        .entry(import.path.as_str())
                        .or_default()
                        .push(module.as_str());
                }
            }
        }
        if globs.is_empty() {
            return false;
        }

        let mut progressed = false;

        // Names defined in the glob source module. `import *` without
        // __all__ skips underscore-prefixed names; __all__ contents are not
        // modeled (an alias is only consulted for paths source code
        // actually imports).
        for entry in self.by_id.iter() {
            let sym = entry.value();
            if sym.language_id.as_ref() != Some(language)
                || sym.kind == crate::types::SymbolKind::Module
                || sym.name.starts_with('_')
                || !matches!(
                    sym.scope_context,
                    None | Some(crate::symbol::ScopeContext::Module)
                        | Some(crate::symbol::ScopeContext::Global)
                )
            {
                continue;
            }
            let Some(targets) = sym.module_path.as_deref().and_then(|mp| globs.get(mp)) else {
                continue;
            };
            for module in targets {
                let key = format!("{module}.{}", sym.name);
                if !self.module_aliases.contains_key(key.as_str()) {
                    self.module_aliases.insert(key.into(), sym.id);
                    progressed = true;
                }
            }
        }

        // Names the glob source exposes via its own aliases (re-export chains).
        let existing: Vec<(Box<str>, crate::types::SymbolId)> = self
            .module_aliases
            .iter()
            .map(|e| (e.key().clone(), *e.value()))
            .collect();
        for (key, id) in existing {
            let Some(pos) = key.rfind('.') else { continue };
            let (src, name) = (&key[..pos], &key[pos + 1..]);
            if name.starts_with('_') {
                continue;
            }
            let Some(targets) = globs.get(src) else {
                continue;
            };
            for module in targets {
                let new_key = format!("{module}.{name}");
                if !self.module_aliases.contains_key(new_key.as_str()) {
                    self.module_aliases.insert(new_key.into(), id);
                    progressed = true;
                }
            }
        }

        progressed
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

        // No cross-language fallback - return NotFound
        // Cross-language resolution causes incorrect relationships
        ResolveResult::NotFound
    }

    fn get(&self, id: SymbolId) -> Option<Symbol> {
        self.by_id.get(&id).map(|r| r.value().clone())
    }

    fn resolve_module_alias(&self, path: &str) -> Option<SymbolId> {
        SymbolLookupCache::resolve_module_alias(self, path)
    }

    fn symbols_in_file(&self, file_id: FileId) -> Vec<SymbolId> {
        SymbolLookupCache::symbols_in_file(self, file_id)
    }

    fn lookup_candidates(&self, name: &str) -> Vec<SymbolId> {
        SymbolLookupCache::lookup_candidates(self, name)
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
        // Re-exported path registered by the Phase 2 pre-pass - exact match
        // ahead of the approximate module comparison below.
        if let Some(id) = self.resolve_module_alias(path) {
            return Some(id);
        }

        // Extract the symbol name from path (last segment)
        let name = path
            .rsplit("::")
            .next()
            .or_else(|| path.rsplit('.').next())
            .or_else(|| path.rsplit('/').next())?;

        // Qualifier: the import path minus its last segment
        let qualifier = path
            .rsplit_once("::")
            .or_else(|| path.rsplit_once('.'))
            .or_else(|| path.rsplit_once('/'))
            .map(|(q, _)| q);

        // Look up candidates and filter by module path + language
        let candidates = self.lookup_candidates(name);
        for id in candidates {
            if let Some(sym) = self.by_id.get(&id) {
                if sym.language_id.as_ref() == Some(&language_id) {
                    // Module path matches at segment boundaries: against the
                    // qualifier for symbols inside a module, against the full
                    // path for module symbols imported by name.
                    if let Some(ref module_path) = sym.module_path {
                        if segment_suffix_match(module_path, path)
                            || qualifier.is_some_and(|q| segment_suffix_match(module_path, q))
                        {
                            return Some(id);
                        }
                    }
                }
            }
        }
        None
    }
}

/// True when the two paths are equal, or the shorter is a suffix of the
/// longer starting at a segment boundary (`::`, `.`, or `/`). Replaces
/// bidirectional substring contains, which admitted mid-segment matches
/// (`util` vs `xutil`) and mid-path infixes.
fn segment_suffix_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (longer, shorter) = if a.len() > b.len() { (a, b) } else { (b, a) };
    longer
        .strip_suffix(shorter)
        .is_some_and(|prefix| prefix.ends_with("::") || prefix.ends_with(['.', '/']))
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
// Phase 1 orchestration options
// ═══════════════════════════════════════════════════════════════════════════

/// File input for `Pipeline::run_phase1`.
pub enum FileSource {
    /// DISCOVER stage walks the root.
    Walk(PathBuf),
    /// Feeder thread sends the list; an empty list short-circuits to empty stats.
    List(Vec<PathBuf>),
}

/// Progress rendering for `Pipeline::run_phase1`.
#[derive(Default)]
pub enum ProgressSink {
    #[default]
    Silent,
    /// Single bar attached to the INDEX stage.
    Bar(Arc<crate::io::status_line::ProgressBar>),
    /// bar1 = EMBED, bar2 = INDEX. Pair with `Phase1Options::embed`;
    /// bar1 has no producer otherwise.
    Dual(Arc<crate::io::status_line::DualProgressBar>),
}

/// Embedding generation for `Pipeline::run_phase1`.
///
/// The EMBED stage runs iff present; pool and store are required together.
pub struct EmbedOptions {
    pub pool: Arc<crate::semantic::EmbeddingBackend>,
    pub semantic: Arc<Mutex<crate::semantic::SimpleSemanticSearch>>,
}

/// Per-run options for `Pipeline::run_phase1`.
#[derive(Default)]
pub struct Phase1Options {
    pub progress: ProgressSink,
    pub embed: Option<EmbedOptions>,
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

    /// [PIPELINE API] Uses storage::StorageError with proper `#[from]` conversion.
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

    fn python_symbol(id: u32, file: u32, name: &str, module_path: &str) -> Symbol {
        let mut sym = Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(file).unwrap(),
            Range::new(1, 0, 1, 10),
        );
        sym.module_path = Some(module_path.into());
        sym.language_id = Some(LanguageId::new("python"));
        sym
    }

    fn import(path: &str, alias: Option<&str>) -> Import {
        Import {
            file_id: FileId::new(1).unwrap(),
            path: path.to_string(),
            alias: alias.map(String::from),
            is_glob: false,
            is_type_only: false,
        }
    }

    fn python_id() -> LanguageId {
        LanguageId::new("python")
    }

    #[test]
    fn test_module_alias_reexport() {
        let cache = SymbolLookupCache::new();
        cache.insert(python_symbol(1, 1, "helper", "pkg.a"));

        // pkg/__init__.py: from pkg.a import helper  => exposes pkg.helper
        let entries = vec![("pkg".to_string(), vec![import("pkg.a.helper", None)])];
        cache.populate_module_aliases(&entries, &python_id());

        assert_eq!(
            cache.resolve_module_alias("pkg.helper"),
            Some(SymbolId::new(1).unwrap())
        );
    }

    #[test]
    fn test_module_alias_reexport_aliased_and_chain() {
        let cache = SymbolLookupCache::new();
        cache.insert(python_symbol(1, 1, "helper", "pkg.inner.a"));

        // pkg/inner/__init__.py: from pkg.inner.a import helper
        // pkg/__init__.py:       from pkg.inner import helper as h
        // Chain converges regardless of entry order.
        let entries = vec![
            (
                "pkg".to_string(),
                vec![import("pkg.inner.helper", Some("h"))],
            ),
            (
                "pkg.inner".to_string(),
                vec![import("pkg.inner.a.helper", None)],
            ),
        ];
        cache.populate_module_aliases(&entries, &python_id());

        let id = SymbolId::new(1).unwrap();
        assert_eq!(cache.resolve_module_alias("pkg.inner.helper"), Some(id));
        assert_eq!(cache.resolve_module_alias("pkg.h"), Some(id));
        assert_eq!(cache.resolve_module_alias("pkg.helper"), None);
    }

    #[test]
    fn test_module_alias_glob_reexport() {
        let cache = SymbolLookupCache::new();
        let mut public = python_symbol(1, 1, "BaseModel", "pkg.main");
        public.scope_context = Some(crate::symbol::ScopeContext::Module);
        cache.insert(public);
        let mut private = python_symbol(2, 1, "_internal", "pkg.main");
        private.scope_context = Some(crate::symbol::ScopeContext::Module);
        cache.insert(private);

        // pkg/__init__.py: from pkg.main import *
        let mut glob = import("pkg.main", None);
        glob.is_glob = true;
        let entries = vec![("pkg".to_string(), vec![glob])];
        cache.populate_module_aliases(&entries, &python_id());

        assert_eq!(
            cache.resolve_module_alias("pkg.BaseModel"),
            Some(SymbolId::new(1).unwrap())
        );
        // Underscore names are not glob-exported
        assert_eq!(cache.resolve_module_alias("pkg._internal"), None);
    }

    #[test]
    fn test_module_alias_glob_chain() {
        let cache = SymbolLookupCache::new();
        let mut sym = python_symbol(1, 1, "thing", "pkg.sub.impl");
        sym.scope_context = Some(crate::symbol::ScopeContext::Module);
        cache.insert(sym);

        // pkg/sub/__init__.py: from pkg.sub.impl import *
        // pkg/__init__.py:     from pkg.sub import *
        let mut inner_glob = import("pkg.sub.impl", None);
        inner_glob.is_glob = true;
        let mut outer_glob = import("pkg.sub", None);
        outer_glob.is_glob = true;
        let entries = vec![
            ("pkg".to_string(), vec![outer_glob]),
            ("pkg.sub".to_string(), vec![inner_glob]),
        ];
        cache.populate_module_aliases(&entries, &python_id());

        let id = SymbolId::new(1).unwrap();
        assert_eq!(cache.resolve_module_alias("pkg.sub.thing"), Some(id));
        assert_eq!(cache.resolve_module_alias("pkg.thing"), Some(id));
    }

    #[test]
    fn test_module_alias_skips_unresolved() {
        let cache = SymbolLookupCache::new();
        cache.insert(python_symbol(1, 1, "helper", "pkg.a"));

        let entries = vec![(
            "pkg".to_string(),
            vec![import("os.path", None), import("pkg.b.missing", None)],
        )];
        cache.populate_module_aliases(&entries, &python_id());

        assert_eq!(cache.resolve_module_alias("pkg.path"), None);
        assert_eq!(cache.resolve_module_alias("pkg.missing"), None);
    }

    fn rust_symbol(id: u32, name: &str, module_path: &str) -> Symbol {
        let mut sym = Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(1, 0, 1, 10),
        );
        sym.module_path = Some(module_path.into());
        sym.language_id = Some(LanguageId::new("rust"));
        sym
    }

    #[test]
    fn find_by_import_path_matches_at_segment_boundaries_only() {
        let rust = LanguageId::new("rust");

        // Substring-colliding module paths must not match (old
        // bidirectional contains admitted both).
        let cache = SymbolLookupCache::new();
        cache.insert(rust_symbol(1, "helper", "util"));
        assert_eq!(cache.find_by_import_path("xutil::helper", rust), None);

        let cache = SymbolLookupCache::new();
        cache.insert(rust_symbol(1, "helper", "app::core"));
        assert_eq!(cache.find_by_import_path("myapp::core::helper", rust), None);

        // Exact qualifier match survives.
        let cache = SymbolLookupCache::new();
        cache.insert(rust_symbol(1, "helper", "app::util"));
        assert_eq!(
            cache.find_by_import_path("app::util::helper", rust),
            Some(SymbolId::new(1).unwrap())
        );

        // Relative import: qualifier is a segment-suffix of the module path.
        let cache = SymbolLookupCache::new();
        cache.insert(rust_symbol(1, "func", "crate::module::helpers"));
        assert_eq!(
            cache.find_by_import_path("helpers::func", rust),
            Some(SymbolId::new(1).unwrap())
        );

        // Module symbol imported by its full path.
        let cache = SymbolLookupCache::new();
        cache.insert(rust_symbol(1, "util", "app::util"));
        assert_eq!(
            cache.find_by_import_path("app::util", rust),
            Some(SymbolId::new(1).unwrap())
        );
    }

    #[test]
    fn symbols_in_file_returns_identity_order() {
        let mk = |id: u32, line: u32| {
            let mut sym = Symbol::new(
                SymbolId::new(id).unwrap(),
                "sym",
                SymbolKind::Function,
                FileId::new(1).unwrap(),
                Range::new(line, 0, line + 1, 0),
            );
            sym.language_id = Some(LanguageId::new("rust"));
            sym
        };
        let expected: Vec<SymbolId> = [(1, 10), (3, 30), (2, 50)]
            .iter()
            .map(|&(id, _)| SymbolId::new(id).unwrap())
            .collect();

        // Insertion order varies run to run (collect-arrival); the
        // returned order must be source order either way.
        let forward = SymbolLookupCache::new();
        for &(id, line) in &[(1u32, 10u32), (3, 30), (2, 50)] {
            forward.insert(mk(id, line));
        }
        let reverse = SymbolLookupCache::new();
        for &(id, line) in &[(2u32, 50u32), (3, 30), (1, 10)] {
            reverse.insert(mk(id, line));
        }

        assert_eq!(forward.symbols_in_file(FileId::new(1).unwrap()), expected);
        assert_eq!(reverse.symbols_in_file(FileId::new(1).unwrap()), expected);
    }

    #[test]
    fn segment_suffix_match_boundary_cases() {
        assert!(segment_suffix_match("app.util", "app.util"));
        assert!(segment_suffix_match("app.util", "util"));
        assert!(segment_suffix_match("util", "app.util"));
        assert!(segment_suffix_match("app::util", "util"));
        assert!(segment_suffix_match("a/b/c", "c"));

        assert!(!segment_suffix_match("app.xutil", "util"));
        assert!(!segment_suffix_match("xutil", "util"));
        assert!(!segment_suffix_match("myapp.core", "app.core"));
        assert!(!segment_suffix_match("app.core.util", "app.core"));
    }
}
