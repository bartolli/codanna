//! IndexFacade - Bridge component wrapping DocumentIndex + Pipeline + SemanticSearch
//!
//! Provides a unified API that matches SimpleIndexer's interface while using Pipeline
//! for indexing and DocumentIndex for queries. This enables gradual migration from
//! SimpleIndexer to the parallel Pipeline architecture.
//!
//! ## Architecture
//!
//! ```text
//! IndexFacade
//!   ├── DocumentIndex (Arc) - All query operations
//!   ├── Pipeline - All mutation/indexing operations
//!   ├── SimpleSemanticSearch (Option<Arc<Mutex>>) - Semantic search
//!   ├── SymbolCache (Option<Arc>) - O(1) symbol lookups
//!   └── indexed_paths (HashSet) - Directory tracking
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let facade = IndexFacade::new(settings)?;
//! facade.index_directory(&path)?;  // Uses Pipeline
//! let symbols = facade.find_symbols_by_name("main")?;  // Uses DocumentIndex
//! ```

use crate::config::Settings;
use crate::indexing::pipeline::Pipeline;
use crate::semantic::remote::run_async;
use crate::semantic::{
    EmbeddingBackend, EmbeddingPool, RemoteEmbedder, SemanticSearchError, SimpleSemanticSearch,
};
use crate::storage::{DocumentIndex, SearchResult};
use crate::symbol::context::{ContextIncludes, SymbolContext, SymbolRelationships};
use crate::{FileId, IndexError, RelationKind, Relationship, Symbol, SymbolId, SymbolKind};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Result type for facade operations
pub type FacadeResult<T> = Result<T, IndexError>;

/// Statistics for indexing operations
#[derive(Debug, Clone, Default)]
pub struct IndexingStats {
    pub files_indexed: usize,
    pub symbols_found: usize,
    pub relationships_resolved: usize,
    /// Files removed by deleted-file cleanup.
    pub files_removed: usize,
    /// Symbols removed by deleted-file cleanup (modified-file cleanup
    /// excluded — those symbols re-add in the same run).
    pub symbols_removed: usize,
}

/// Statistics for sync operations
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub added_dirs: usize,
    pub removed_dirs: usize,
    pub files_indexed: usize,
    pub symbols_found: usize,
    pub files_modified: usize,
    pub files_added: usize,
}

impl SyncStats {
    pub fn has_changes(&self) -> bool {
        self.added_dirs > 0
            || self.removed_dirs > 0
            || self.files_modified > 0
            || self.files_added > 0
    }
}

/// IndexFacade - Unified interface for code intelligence operations
///
/// This facade wraps DocumentIndex (for queries) and Pipeline (for indexing),
/// providing an API compatible with SimpleIndexer for gradual migration.
pub struct IndexFacade {
    /// Document storage (Tantivy-based) - used for all queries
    document_index: Arc<DocumentIndex>,

    /// Parallel indexing pipeline - used for mutations
    pipeline: Pipeline,

    /// Optional semantic search for doc comment embeddings
    semantic_search: Option<Arc<Mutex<SimpleSemanticSearch>>>,

    /// Optional embedding pool for parallel embedding generation
    embedding_pool: Option<Arc<EmbeddingBackend>>,

    /// Configuration
    settings: Arc<Settings>,

    /// Tracked indexed directories (canonicalized paths)
    indexed_paths: HashSet<PathBuf>,

    /// Base path for index storage
    index_base: PathBuf,

    /// Set to true when load_semantic_search fails with DimensionMismatch so
    /// hot-reload and other callers do not retry on every reload cycle.
    semantic_incompatible: bool,

    /// Persisted semantic metadata for status/reporting when semantic search
    /// is not loaded into memory (for example, lite facade loads).
    semantic_metadata_snapshot: Option<crate::semantic::SemanticMetadata>,
}

impl IndexFacade {
    /// Create a new IndexFacade with the given settings.
    ///
    /// Creates or opens the DocumentIndex and initializes the Pipeline.
    pub fn new(settings: Arc<Settings>) -> FacadeResult<Self> {
        // Construct the full index path
        let index_base = if let Some(ref workspace_root) = settings.workspace_root {
            workspace_root.join(&settings.index_path)
        } else {
            settings.index_path.clone()
        };

        // Tantivy data goes under index_path/tantivy
        let tantivy_path = index_base.join("tantivy");

        let document_index = Arc::new(DocumentIndex::new(&tantivy_path, &settings)?);

        let pipeline = Pipeline::with_settings(settings.clone());

        Ok(Self {
            document_index,
            pipeline,
            semantic_search: None,
            embedding_pool: None,
            settings,
            indexed_paths: HashSet::new(),
            index_base,
            semantic_incompatible: false,
            semantic_metadata_snapshot: None,
        })
    }

    /// Create facade from existing components (for server integration).
    pub fn from_components(
        document_index: Arc<DocumentIndex>,
        pipeline: Pipeline,
        semantic_search: Option<Arc<Mutex<SimpleSemanticSearch>>>,
        settings: Arc<Settings>,
    ) -> Self {
        let index_base = if let Some(ref workspace_root) = settings.workspace_root {
            workspace_root.join(&settings.index_path)
        } else {
            settings.index_path.clone()
        };

        Self {
            document_index,
            pipeline,
            semantic_search,
            embedding_pool: None,
            settings,
            indexed_paths: HashSet::new(),
            index_base,
            semantic_incompatible: false,
            semantic_metadata_snapshot: None,
        }
    }

    /// Get a reference to the underlying DocumentIndex.
    pub fn document_index(&self) -> &Arc<DocumentIndex> {
        &self.document_index
    }

    /// Get a reference to the Pipeline.
    pub fn pipeline(&self) -> &Pipeline {
        &self.pipeline
    }

    /// Get a reference to the settings.
    pub fn settings(&self) -> &Arc<Settings> {
        &self.settings
    }

    /// Get the index base path.
    pub fn index_base(&self) -> &Path {
        &self.index_base
    }

    // =========================================================================
    // Semantic Search Management
    // =========================================================================

    /// Enable semantic search with the configured model.
    pub fn enable_semantic_search(&mut self) -> FacadeResult<()> {
        let semantic_path = self.index_base.join("semantic");
        std::fs::create_dir_all(&semantic_path)?;

        let backend = build_embedding_backend(&self.settings.semantic_search)?;
        let backend = Arc::new(backend);

        // In remote mode, skip local fastembed init; use new_empty so the
        // SemanticSearch instance carries the correct dimension from the backend.
        let is_remote = self.settings.semantic_search.remote_url.is_some()
            || std::env::var("CODANNA_EMBED_URL").is_ok();
        let semantic = if is_remote {
            SimpleSemanticSearch::new_empty(
                backend.dimensions(),
                &resolve_remote_model_name(&self.settings.semantic_search),
            )
        } else {
            let model = &self.settings.semantic_search.model;
            SimpleSemanticSearch::from_model_name(model)?
        };

        self.semantic_search = Some(Arc::new(Mutex::new(semantic)));
        self.semantic_metadata_snapshot = self.get_semantic_metadata();
        self.embedding_pool = Some(backend);

        Ok(())
    }

    /// Check if semantic search is enabled.
    pub fn has_semantic_search(&self) -> bool {
        self.semantic_search.is_some()
    }

    /// Returns true if a previous load_semantic_search call failed with
    /// DimensionMismatch, meaning retrying would always fail until re-indexed.
    pub fn is_semantic_incompatible(&self) -> bool {
        self.semantic_incompatible
    }

    /// Save semantic search data to disk.
    pub fn save_semantic_search(&self, path: &Path) -> FacadeResult<()> {
        if let Some(ref semantic) = self.semantic_search {
            let sem = semantic.lock().map_err(|_| IndexError::lock_error())?;
            sem.save(path)?;
        }
        Ok(())
    }

    /// Load semantic search data from disk.
    ///
    /// This only loads pre-computed embeddings for querying.
    /// Embedding pool for generating new embeddings is initialized lazily.
    pub fn load_semantic_search(&mut self, path: &Path) -> FacadeResult<bool> {
        if path.join("metadata.json").exists() {
            let is_remote = self.settings.semantic_search.remote_url.is_some()
                || std::env::var("CODANNA_EMBED_URL").is_ok();
            let load_result = if is_remote {
                SimpleSemanticSearch::load_remote(path)
            } else {
                SimpleSemanticSearch::load(path)
            };
            match load_result {
                Ok(semantic) => {
                    // Restore the embedding backend so query-time remote embedding
                    // works immediately without waiting for a lazy reindex call.
                    if self.embedding_pool.is_none() {
                        match build_embedding_backend(&self.settings.semantic_search) {
                            Ok(b) => self.embedding_pool = Some(Arc::new(b)),
                            Err(e) => tracing::warn!("Failed to restore embedding backend: {e}"),
                        }
                    }

                    // Verify dimension and backend kind compatibility.
                    if let Some(ref pool) = self.embedding_pool {
                        let backend_dim = pool.dimensions();
                        let index_dim = semantic.dimensions();

                        if backend_dim != index_dim {
                            self.semantic_incompatible = true;
                            return Err(IndexError::SemanticSearch(
                                SemanticSearchError::DimensionMismatch {
                                    expected: backend_dim,
                                    actual: index_dim,
                                    suggestion: format!(
                                        "Index was built with {index_dim}-dimensional embeddings \
                                         but current backend produces {backend_dim}d. \
                                         Re-index with: codanna index <path> --force"
                                    ),
                                },
                            ));
                        }

                        // Warn when backend kind changed but dimensions happen to match.
                        // Embedding spaces differ between models so similarity scores may
                        // be meaningless. Only a --force re-index can fully fix this.
                        let index_is_remote = semantic.is_remote_index();
                        let backend_is_remote =
                            matches!(pool.as_ref(), EmbeddingBackend::Remote(_));
                        if index_is_remote != backend_is_remote {
                            tracing::warn!(
                                target: "semantic",
                                "Backend kind changed (index={}, current={}). \
                                 Embedding spaces may differ — similarity scores could be inaccurate. \
                                 Re-index with --force to fix.",
                                if index_is_remote { "remote" } else { "local" },
                                if backend_is_remote { "remote" } else { "local" },
                            );
                        }
                    }

                    self.semantic_search = Some(Arc::new(Mutex::new(semantic)));
                    self.semantic_metadata_snapshot = self.get_semantic_metadata();
                    return Ok(true);
                }
                Err(SemanticSearchError::DimensionMismatch {
                    expected,
                    actual,
                    ref suggestion,
                }) => {
                    // Dimension mismatch: index is structurally incompatible with the
                    // current backend. Mark this facade so callers do not retry on every
                    // cycle. The error propagates upward; callers that need the process
                    // to survive (startup, hot-reload) swallow it and continue text-only.
                    // Callers that want to fail fast can treat this Err as fatal.
                    self.semantic_incompatible = true;
                    tracing::error!(
                        target: "semantic",
                        "Semantic index dimension mismatch (expected={expected}, actual={actual}): {suggestion}"
                    );
                    return Err(IndexError::SemanticSearch(
                        SemanticSearchError::DimensionMismatch {
                            expected,
                            actual,
                            suggestion: suggestion.to_string(),
                        },
                    ));
                }
                Err(e) => {
                    // Other errors (missing file, corrupt data) — warn and continue
                    // without semantic search rather than blocking startup.
                    tracing::warn!("Failed to load semantic search, continuing without it: {e}");
                }
            }
        }
        Ok(false)
    }

    /// Load persisted semantic metadata without initializing the semantic backend.
    pub fn load_semantic_metadata_snapshot(&mut self, path: &Path) -> FacadeResult<bool> {
        if !path.join("metadata.json").exists() {
            self.semantic_metadata_snapshot = None;
            return Ok(false);
        }

        let metadata = crate::semantic::SemanticMetadata::load(path)?;
        self.semantic_metadata_snapshot = Some(metadata);
        Ok(true)
    }

    /// Ensure embedding backend is initialized for generating new embeddings.
    ///
    /// Called lazily by methods that need to compute embeddings (reindexing, watcher).
    pub fn ensure_embedding_pool(&mut self) -> FacadeResult<()> {
        if self.embedding_pool.is_some() {
            return Ok(());
        }

        let backend = build_embedding_backend(&self.settings.semantic_search)?;
        self.embedding_pool = Some(Arc::new(backend));
        tracing::debug!("Initialized embedding backend for incremental updates");
        Ok(())
    }

    /// Get semantic search embedding count.
    pub fn semantic_search_embedding_count(&self) -> usize {
        self.semantic_search
            .as_ref()
            .map(|s| s.lock().map(|sem| sem.embedding_count()).unwrap_or(0))
            .or_else(|| {
                self.semantic_metadata_snapshot
                    .as_ref()
                    .map(|m| m.embedding_count)
            })
            .unwrap_or(0)
    }

    /// Get semantic search metadata.
    pub fn get_semantic_metadata(&self) -> Option<crate::semantic::SemanticMetadata> {
        self.semantic_search
            .as_ref()
            .and_then(|s| s.lock().ok().and_then(|sem| sem.metadata().cloned()))
            .or_else(|| self.semantic_metadata_snapshot.clone())
    }

    // =========================================================================
    // Symbol Query Methods (delegate to DocumentIndex)
    // =========================================================================

    /// Find a symbol by name.
    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        self.document_index
            .find_symbols_by_name(name, None)
            .ok()
            .and_then(|symbols| symbols.first().map(|s| s.id))
    }

    /// Find all symbols by name with optional language filter.
    pub fn find_symbols_by_name(&self, name: &str, language_filter: Option<&str>) -> Vec<Symbol> {
        self.document_index
            .find_symbols_by_name(name, language_filter)
            .unwrap_or_default()
    }

    /// Get a symbol by ID.
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        self.document_index.find_symbol_by_id(id).ok().flatten()
    }

    /// Symbol counts by kind and by language in one pass. Both index-info
    /// renderings consume this single assembly; the two maps partition the
    /// same symbol set (languageless legacy rows appear only in kinds).
    pub fn symbol_stats(
        &self,
    ) -> (
        std::collections::BTreeMap<String, usize>,
        std::collections::BTreeMap<String, usize>,
    ) {
        let mut kinds = std::collections::BTreeMap::new();
        let mut languages = std::collections::BTreeMap::new();
        for symbol in self.get_all_symbols() {
            *kinds.entry(format!("{:?}", symbol.kind)).or_insert(0usize) += 1;
            if let Some(lang) = symbol.language_id.as_ref() {
                *languages.entry(lang.as_str().to_string()).or_insert(0usize) += 1;
            }
        }
        (kinds, languages)
    }

    /// Get all symbols, sized by the exact symbol count.
    ///
    /// Returns empty vec on error for SimpleIndexer API compatibility.
    pub fn get_all_symbols(&self) -> Vec<Symbol> {
        let total = match self.document_index.count_symbols() {
            Ok(0) => return Vec::new(),
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(target: "facade", "get_all_symbols count error: {e}");
                return Vec::new();
            }
        };
        self.document_index
            .get_all_symbols(total)
            .unwrap_or_else(|e| {
                tracing::warn!(target: "facade", "get_all_symbols error: {e}");
                Vec::new()
            })
    }

    /// Get symbols by file ID.
    ///
    /// Returns empty vec on error for SimpleIndexer API compatibility.
    pub fn get_symbols_by_file(&self, file_id: FileId) -> Vec<Symbol> {
        self.document_index
            .find_symbols_by_file(file_id)
            .unwrap_or_default()
    }

    // =========================================================================
    // Relationship Query Methods (delegate to DocumentIndex)
    // =========================================================================

    /// Get functions called by a symbol.
    pub fn get_called_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_from(symbol_id, RelationKind::Calls)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (_, to_id, _) in relationships {
            if let Some(symbol) = self.get_symbol(to_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get functions called by a symbol with metadata.
    pub fn get_called_functions_with_metadata(
        &self,
        symbol_id: SymbolId,
    ) -> Vec<(Symbol, Option<crate::relationship::RelationshipMetadata>)> {
        let relationships = self
            .document_index
            .get_relationships_from(symbol_id, RelationKind::Calls)
            .unwrap_or_default();

        let mut results = Vec::new();
        for (_, to_id, rel) in relationships {
            if let Some(symbol) = self.get_symbol(to_id) {
                results.push((symbol, rel.metadata));
            }
        }
        results
    }

    /// Get functions that call a symbol.
    pub fn get_calling_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_to(symbol_id, RelationKind::Calls)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (from_id, _, _) in relationships {
            if let Some(symbol) = self.get_symbol(from_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get functions that call a symbol with metadata.
    pub fn get_calling_functions_with_metadata(
        &self,
        symbol_id: SymbolId,
    ) -> Vec<(Symbol, Option<crate::relationship::RelationshipMetadata>)> {
        let relationships = self
            .document_index
            .get_relationships_to(symbol_id, RelationKind::Calls)
            .unwrap_or_default();

        let mut results = Vec::new();
        for (from_id, _, rel) in relationships {
            if let Some(symbol) = self.get_symbol(from_id) {
                results.push((symbol, rel.metadata));
            }
        }
        results
    }

    /// Get implementations of a trait/interface.
    pub fn get_implementations(&self, trait_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_to(trait_id, RelationKind::Implements)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (from_id, _, _) in relationships {
            if let Some(symbol) = self.get_symbol(from_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get traits implemented by a type.
    pub fn get_implemented_traits(&self, type_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_from(type_id, RelationKind::Implements)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (_, to_id, _) in relationships {
            if let Some(symbol) = self.get_symbol(to_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get classes/types extended by a class.
    pub fn get_extends(&self, class_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_from(class_id, RelationKind::Extends)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (_, to_id, _) in relationships {
            if let Some(symbol) = self.get_symbol(to_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get classes that extend a base class.
    pub fn get_extended_by(&self, base_class_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_to(base_class_id, RelationKind::Extends)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (from_id, _, _) in relationships {
            if let Some(symbol) = self.get_symbol(from_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get types/symbols used by a symbol.
    pub fn get_uses(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_from(symbol_id, RelationKind::Uses)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (_, to_id, _) in relationships {
            if let Some(symbol) = self.get_symbol(to_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get symbols that use a type.
    pub fn get_used_by(&self, type_id: SymbolId) -> Vec<Symbol> {
        let relationships = self
            .document_index
            .get_relationships_to(type_id, RelationKind::Uses)
            .unwrap_or_default();

        let mut symbols = Vec::new();
        for (from_id, _, _) in relationships {
            if let Some(symbol) = self.get_symbol(from_id) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    /// Get relationships for a symbol (by symbol ID).
    pub fn get_relationships_for_symbol(
        &self,
        symbol_id: SymbolId,
    ) -> FacadeResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let mut all_rels = Vec::new();

        // Get outgoing relationships
        for kind in &[
            RelationKind::Calls,
            RelationKind::Uses,
            RelationKind::Implements,
            RelationKind::Extends,
            RelationKind::Defines,
        ] {
            if let Ok(rels) = self.document_index.get_relationships_from(symbol_id, *kind) {
                all_rels.extend(rels);
            }
        }

        // Get incoming relationships
        for kind in &[
            RelationKind::Calls,
            RelationKind::Uses,
            RelationKind::Implements,
            RelationKind::Extends,
        ] {
            if let Ok(rels) = self.document_index.get_relationships_to(symbol_id, *kind) {
                all_rels.extend(rels);
            }
        }

        Ok(all_rels)
    }

    // =========================================================================
    // Complex Query Methods (facade-level orchestration)
    // =========================================================================

    /// Get symbol context with configurable relationship inclusion.
    pub fn get_symbol_context(
        &self,
        symbol_id: SymbolId,
        include: ContextIncludes,
    ) -> Option<SymbolContext> {
        let symbol = self.get_symbol(symbol_id)?;
        let file_path = self
            .document_index
            .get_file_path(symbol.file_id)
            .ok()
            .flatten()
            .map(|p| self.document_index.to_portable_file_path(&p).unwrap_or(p))
            .unwrap_or_else(|| symbol.file_path.to_string());

        let mut relationships = SymbolRelationships::default();

        if include.contains(ContextIncludes::IMPLEMENTATIONS) {
            let impls = self.get_implementations(symbol_id);
            if !impls.is_empty() {
                relationships.implemented_by = Some(impls);
            }
            // Also get what this type implements
            let implemented = self.get_implemented_traits(symbol_id);
            if !implemented.is_empty() {
                relationships.implements = Some(implemented);
            }
        }

        if include.contains(ContextIncludes::DEFINITIONS) {
            if let Ok(rels) = self
                .document_index
                .get_relationships_from(symbol_id, RelationKind::Defines)
            {
                let defines: Vec<Symbol> = rels
                    .iter()
                    .filter_map(|(_, to_id, _)| self.get_symbol(*to_id))
                    .collect();
                if !defines.is_empty() {
                    relationships.defines = Some(defines);
                }
            }
        }

        if include.contains(ContextIncludes::CALLS) {
            let calls = self.get_called_functions_with_metadata(symbol_id);
            if !calls.is_empty() {
                relationships.calls = Some(calls);
            }
        }

        if include.contains(ContextIncludes::CALLERS) {
            let callers = self.get_calling_functions_with_metadata(symbol_id);
            if !callers.is_empty() {
                relationships.called_by = Some(callers);
            }
        }

        if include.contains(ContextIncludes::EXTENDS) {
            let extends = self.get_extends(symbol_id);
            if !extends.is_empty() {
                relationships.extends = Some(extends);
            }
            let extended_by = self.get_extended_by(symbol_id);
            if !extended_by.is_empty() {
                relationships.extended_by = Some(extended_by);
            }
        }

        if include.contains(ContextIncludes::USES) {
            let uses = self.get_uses(symbol_id);
            if !uses.is_empty() {
                relationships.uses = Some(uses);
            }
            let used_by = self.get_used_by(symbol_id);
            if !used_by.is_empty() {
                relationships.used_by = Some(used_by);
            }
        }

        Some(SymbolContext {
            symbol,
            file_path,
            relationships,
        })
    }

    /// Get dependencies (what a symbol depends on).
    pub fn get_dependencies(&self, symbol_id: SymbolId) -> HashMap<RelationKind, Vec<Symbol>> {
        let mut deps: HashMap<RelationKind, Vec<Symbol>> = HashMap::new();

        for kind in &[
            RelationKind::Calls,
            RelationKind::Uses,
            RelationKind::Implements,
            RelationKind::Defines,
        ] {
            let rels = self
                .document_index
                .get_relationships_from(symbol_id, *kind)
                .unwrap_or_default();
            let symbols: Vec<Symbol> = rels
                .iter()
                .filter_map(|(_, to_id, _)| self.get_symbol(*to_id))
                .collect();
            if !symbols.is_empty() {
                deps.insert(*kind, symbols);
            }
        }

        deps
    }

    /// Get dependents (what depends on a symbol).
    pub fn get_dependents(&self, symbol_id: SymbolId) -> HashMap<RelationKind, Vec<Symbol>> {
        let mut deps: HashMap<RelationKind, Vec<Symbol>> = HashMap::new();

        for kind in &[
            RelationKind::Calls,
            RelationKind::Uses,
            RelationKind::Implements,
        ] {
            let rels = self
                .document_index
                .get_relationships_to(symbol_id, *kind)
                .unwrap_or_default();
            let symbols: Vec<Symbol> = rels
                .iter()
                .filter_map(|(from_id, _, _)| self.get_symbol(*from_id))
                .collect();
            if !symbols.is_empty() {
                deps.insert(*kind, symbols);
            }
        }

        deps
    }

    /// Get impact radius (BFS traversal of dependents).
    pub fn get_impact_radius(
        &self,
        symbol_id: SymbolId,
        max_depth: Option<usize>,
    ) -> Vec<SymbolId> {
        let max_depth = max_depth.unwrap_or(2);
        let mut visited = HashSet::new();
        let mut queue = std::collections::VecDeque::new();

        queue.push_back((symbol_id, 0usize));
        visited.insert(symbol_id);

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            // Get dependents via Calls, Uses, Implements, Extends
            for kind in &[
                RelationKind::Calls,
                RelationKind::Uses,
                RelationKind::Implements,
                RelationKind::Extends,
            ] {
                if let Ok(rels) = self.document_index.get_relationships_to(current_id, *kind) {
                    for (from_id, _, _) in rels {
                        if visited.insert(from_id) {
                            queue.push_back((from_id, depth + 1));
                        }
                    }
                }
            }
        }

        // Remove the initial symbol from results
        visited.remove(&symbol_id);
        visited.into_iter().collect()
    }

    // =========================================================================
    // Search Methods
    // =========================================================================

    /// Full-text search for symbols.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind_filter: Option<SymbolKind>,
        module_filter: Option<&str>,
        language_filter: Option<&str>,
    ) -> FacadeResult<Vec<SearchResult>> {
        self.document_index
            .search(query, limit, kind_filter, module_filter, language_filter)
            .map_err(Into::into)
    }

    /// Semantic search using doc comment embeddings.
    pub fn semantic_search_docs(
        &self,
        query: &str,
        limit: usize,
    ) -> FacadeResult<Vec<(Symbol, f32)>> {
        self.semantic_search_docs_with_language(query, limit, None)
    }

    /// Semantic search with language filter.
    pub fn semantic_search_docs_with_language(
        &self,
        query: &str,
        limit: usize,
        language_filter: Option<&str>,
    ) -> FacadeResult<Vec<(Symbol, f32)>> {
        let semantic = self
            .semantic_search
            .as_ref()
            .ok_or(IndexError::SemanticSearchNotEnabled)?;

        let sem = semantic.lock().map_err(|_| IndexError::lock_error())?;

        // When the semantic search has no local model (built with remote embeddings),
        // generate the query vector via the embedding backend regardless of whether
        // the backend is currently remote or local — the pool just needs to produce
        // a vector of the right dimension.
        let results = if sem.has_local_model() {
            sem.search_with_language(query, limit, language_filter)?
        } else {
            let pool = self.embedding_pool.as_ref().ok_or_else(|| {
                IndexError::General(
                    "Remote-mode index requires an embedding backend for queries. \
                     Set CODANNA_EMBED_URL or re-index with a local model."
                        .to_string(),
                )
            })?;
            let query_vec = pool.embed_one(query)?;
            sem.search_with_embedding_and_language(&query_vec, limit, language_filter)?
        };

        let mut symbols = Vec::new();
        for (symbol_id, score) in results {
            if let Some(symbol) = self.get_symbol(symbol_id) {
                symbols.push((symbol, score));
            }
        }

        Ok(symbols)
    }

    /// Semantic search with score threshold.
    pub fn semantic_search_docs_with_threshold(
        &self,
        query: &str,
        limit: usize,
        threshold: f32,
    ) -> FacadeResult<Vec<(Symbol, f32)>> {
        self.semantic_search_docs_with_threshold_and_language(query, limit, threshold, None)
    }

    /// Semantic search with threshold and language filter.
    pub fn semantic_search_docs_with_threshold_and_language(
        &self,
        query: &str,
        limit: usize,
        threshold: f32,
        language_filter: Option<&str>,
    ) -> FacadeResult<Vec<(Symbol, f32)>> {
        let results = self.semantic_search_docs_with_language(query, limit, language_filter)?;

        Ok(results
            .into_iter()
            .filter(|(_, score)| *score >= threshold)
            .collect())
    }

    // =========================================================================
    // File Operations
    // =========================================================================

    /// Get file ID for a path.
    pub fn get_file_id_for_path(&self, path: &str) -> Option<FileId> {
        self.document_index
            .get_file_info(path)
            .ok()
            .flatten()
            .map(|(id, _, _)| id)
    }

    /// Get file path for a FileId, in the emitted contract shape.
    ///
    /// Returns None on error for SimpleIndexer API compatibility.
    pub fn get_file_path(&self, file_id: FileId) -> Option<String> {
        self.document_index
            .get_file_path(file_id)
            .ok()
            .flatten()
            .map(|p| self.document_index.to_portable_file_path(&p).unwrap_or(p))
    }

    /// Get all indexed file paths.
    pub fn get_all_indexed_paths(&self) -> Vec<PathBuf> {
        self.document_index
            .get_all_indexed_paths()
            .unwrap_or_default()
    }

    // =========================================================================
    // Statistics Methods
    // =========================================================================

    /// Get the number of indexed symbols.
    pub fn symbol_count(&self) -> usize {
        self.document_index.count_symbols().unwrap_or(0)
    }

    /// Get the number of indexed files.
    pub fn file_count(&self) -> u32 {
        self.document_index.count_files().unwrap_or(0) as u32
    }

    /// Get the number of relationships.
    pub fn relationship_count(&self) -> usize {
        self.document_index.count_relationships().unwrap_or(0)
    }

    /// Get total Tantivy document count.
    pub fn document_count(&self) -> FacadeResult<u64> {
        self.document_index.document_count().map_err(Into::into)
    }

    // =========================================================================
    // Directory Tracking
    // =========================================================================

    /// Add a directory to tracked indexed paths.
    pub fn add_indexed_path(&mut self, dir_path: &Path) {
        if let Ok(canonical) = dir_path.canonicalize() {
            // Skip if already covered by an existing parent directory
            let already_covered = self
                .indexed_paths
                .iter()
                .any(|p| canonical.starts_with(p) && canonical != *p);
            if already_covered {
                return;
            }

            // Remove any child paths that would be covered by this directory
            self.indexed_paths
                .retain(|p| !p.starts_with(&canonical) || *p == canonical);
            self.indexed_paths.insert(canonical);
        } else {
            self.indexed_paths.insert(dir_path.to_path_buf());
        }
    }

    /// Get tracked indexed paths.
    pub fn get_indexed_paths(&self) -> &HashSet<PathBuf> {
        &self.indexed_paths
    }

    /// Update indexed paths from a vector.
    pub fn set_indexed_paths(&mut self, paths: Vec<PathBuf>) {
        self.indexed_paths = paths.into_iter().collect();
    }

    // =========================================================================
    // Mutation Methods (delegate to Pipeline)
    // =========================================================================

    /// Index a single file using the parallel pipeline.
    ///
    /// Returns `IndexingResult::Indexed` with the file ID on success.
    /// File records key off path text: an uncanonical root or file path
    /// (`./src`, `x/../x`) addresses a key space disjoint from the
    /// registered indexed_paths walks, re-indexing every file as new and
    /// doubling the index. Normalize every externally-supplied path once,
    /// here; nonexistent paths pass through raw so callers keep their
    /// error reporting.
    fn canonical_or_raw(path: &std::path::Path) -> std::path::PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    pub fn index_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> crate::IndexResult<crate::IndexingResult> {
        let path = &Self::canonical_or_raw(path.as_ref());
        if self.has_semantic_search() {
            if let Err(e) = self.ensure_embedding_pool() {
                tracing::warn!("Failed to initialize embedding pool: {e}");
            }
        }
        let stats = self.pipeline.index_file_single(
            path,
            Arc::clone(&self.document_index),
            self.semantic_search.clone(),
            self.embedding_pool.clone(),
        )?;

        Ok(crate::IndexingResult::Indexed(stats.file_id))
    }

    /// Index a single file with optional force re-indexing.
    ///
    /// When `force` is true, removes the file first to ensure a fresh re-index.
    pub fn index_file_with_force(
        &mut self,
        path: impl AsRef<std::path::Path>,
        force: bool,
    ) -> crate::IndexResult<crate::IndexingResult> {
        let path = path.as_ref();

        if force {
            // Remove first to force re-index. Not-indexed files return Ok,
            // so any error here is a real cleanup failure and must not be
            // masked: swallowing it desyncs the semantic store from Tantivy.
            self.remove_file(path)?;
        }

        self.index_file(path)
    }

    /// Remove a file from the index.
    ///
    /// Uses the Pipeline's cleanup stage to remove symbols and embeddings.
    pub fn remove_file(&mut self, path: impl AsRef<std::path::Path>) -> crate::IndexResult<()> {
        let path = &Self::canonical_or_raw(path.as_ref());
        let semantic_path = self.settings.index_path.join("semantic");

        use crate::indexing::pipeline::stages::CleanupStage;
        let cleanup_stage = if let Some(ref sem) = self.semantic_search {
            CleanupStage::new(Arc::clone(&self.document_index), &semantic_path)
                .with_semantic(Arc::clone(sem))
        } else {
            CleanupStage::new(Arc::clone(&self.document_index), &semantic_path)
        };

        cleanup_stage.cleanup_files(std::slice::from_ref(path))?;
        Ok(())
    }

    /// Index a directory using the parallel pipeline.
    ///
    /// This is the primary indexing entry point using Pipeline.
    pub fn index_directory(&mut self, path: &Path, force: bool) -> FacadeResult<IndexingStats> {
        let path = &Self::canonical_or_raw(path);
        if self.has_semantic_search() {
            if let Err(e) = self.ensure_embedding_pool() {
                tracing::warn!("Failed to initialize embedding pool: {e}");
            }
        }
        let stats = self.pipeline.index_incremental(
            path,
            Arc::clone(&self.document_index),
            self.semantic_search.clone(),
            self.embedding_pool.clone(),
            force,
        )?;

        // Update tracked paths
        self.add_indexed_path(path);

        Ok(IndexingStats {
            files_indexed: stats.new_files + stats.modified_files,
            symbols_found: stats.index_stats.symbols_found,
            relationships_resolved: stats.phase2_stats.defines_resolved
                + stats.phase2_stats.calls_resolved
                + stats.phase2_stats.other_resolved,
            files_removed: stats.deleted_files,
            symbols_removed: stats.deleted_symbols,
        })
    }

    /// Index a directory with advanced options.
    ///
    /// Provides options for progress reporting, dry-run mode, force re-indexing,
    /// and limiting the number of files.
    pub fn index_directory_with_options(
        &mut self,
        dir: impl AsRef<Path>,
        progress: bool,
        dry_run: bool,
        force: bool,
        max_files: Option<usize>,
    ) -> crate::IndexResult<crate::indexing::progress::IndexStats> {
        use crate::indexing::FileWalker;
        use crate::indexing::progress::IndexStats;

        let dir = &Self::canonical_or_raw(dir.as_ref());
        let walker = FileWalker::new(Arc::clone(&self.settings));
        let files: Vec<_> = walker.walk(dir).collect();

        // Apply max_files limit if specified
        let files = if let Some(max) = max_files {
            files.into_iter().take(max).collect()
        } else {
            files
        };

        let total_files = files.len();

        // Handle dry-run mode
        if dry_run {
            println!("Would index {total_files} files:");
            for (i, file_path) in files.iter().enumerate() {
                if i < 5 {
                    println!("  {}", file_path.display());
                } else if i == 5 && total_files > 5 {
                    println!("  ... and {} more files", total_files - 5);
                    break;
                }
            }

            let mut stats = IndexStats::new();
            stats.files_indexed = total_files;
            return Ok(stats);
        }

        // Auto-force mode for empty indexes (clean index behaves like --force)
        let force = force || self.document_count().unwrap_or(0) == 0;

        if self.has_semantic_search() {
            if let Err(e) = self.ensure_embedding_pool() {
                tracing::warn!("Failed to initialize embedding pool: {e}");
            }
        }

        // Use Pipeline for indexing with progress flag
        // The pipeline manages progress bars internally for clean sequential display
        let pipeline_stats = self.pipeline.index_incremental_with_progress_flag(
            dir,
            Arc::clone(&self.document_index),
            self.semantic_search.clone(),
            self.embedding_pool.clone(),
            force,
            progress && total_files > 0,
            total_files,
        )?;

        // Update tracked paths
        self.add_indexed_path(dir);

        // Convert to IndexStats format using pipeline's actual timing
        let mut stats = IndexStats::default();
        stats.files_indexed = pipeline_stats.new_files + pipeline_stats.modified_files;
        stats.symbols_found = pipeline_stats.index_stats.symbols_found;
        stats.files_removed = pipeline_stats.deleted_files;
        stats.symbols_removed = pipeline_stats.deleted_symbols;
        stats.elapsed = pipeline_stats.elapsed;

        Ok(stats)
    }

    /// Sync with configuration (compare stored vs config paths).
    ///
    /// Returns (added_dirs, removed_dirs, files_indexed, symbols_found).
    pub fn sync_with_config(
        &mut self,
        stored_paths: Option<Vec<PathBuf>>,
        config_paths: &[PathBuf],
        progress: bool,
    ) -> FacadeResult<SyncStats> {
        let stored = stored_paths.unwrap_or_default();
        let stored_set: HashSet<PathBuf> = stored.iter().cloned().collect();
        let config_set: HashSet<PathBuf> = config_paths.iter().cloned().collect();

        // Determine what to add and remove
        let to_add: Vec<&PathBuf> = config_set.difference(&stored_set).collect();
        let to_remove: Vec<&PathBuf> = stored_set.difference(&config_set).collect();

        let mut stats = SyncStats::default();

        if self.has_semantic_search() && !to_add.is_empty() {
            if let Err(e) = self.ensure_embedding_pool() {
                tracing::warn!("Failed to initialize embedding pool: {e}");
            }
        }

        // Index new directories with progress if enabled
        // Use force=true since these are new directories being indexed for the first time
        for path in &to_add {
            // Visual separator and directory label (stderr syncs with progress bars)
            eprintln!();
            eprintln!("Indexing directory: {}", path.display());

            // Count files first for accurate progress bar
            let file_count = if progress {
                use crate::indexing::FileWalker;
                let walker = FileWalker::new(Arc::clone(&self.settings));
                walker.walk(path).count()
            } else {
                0
            };

            let result = self.pipeline.index_incremental_with_progress_flag(
                path,
                Arc::clone(&self.document_index),
                self.semantic_search.clone(),
                self.embedding_pool.clone(),
                true, // force: new directories should be fully indexed
                progress,
                file_count,
            )?;
            stats.files_indexed += result.new_files + result.modified_files;
            stats.symbols_found += result.index_stats.symbols_found;
        }
        stats.added_dirs = to_add.len();

        // Remove files from removed directories
        for path in &to_remove {
            self.remove_directory_files(path)?;
        }
        stats.removed_dirs = to_remove.len();

        // Update tracked paths
        self.indexed_paths = config_set;

        Ok(stats)
    }

    /// Remove all files from a directory.
    fn remove_directory_files(&self, _dir: &Path) -> FacadeResult<()> {
        // TODO: Implement using CleanupStage
        // For now, this is a placeholder
        Ok(())
    }
}

// ── Embedding backend factory ──────────────────────────────────────────────

/// Resolve the effective remote model name, applying env-var-first precedence.
///
/// Both `build_embedding_backend` and `new_empty` call sites use this so that
/// the model name embedded in saved metadata always matches what the backend uses.
pub fn resolve_remote_model_name(cfg: &crate::config::SemanticSearchConfig) -> String {
    std::env::var("CODANNA_EMBED_MODEL")
        .ok()
        .or_else(|| cfg.remote_model.clone())
        .unwrap_or_else(|| "text-embedding-ada-002".to_string())
}

/// Format a human-readable semantic search status line for CLI output.
pub fn format_semantic_status(cfg: &crate::config::SemanticSearchConfig) -> String {
    let is_remote = std::env::var("CODANNA_EMBED_URL").is_ok() || cfg.remote_url.is_some();
    let threshold = cfg.threshold;

    if is_remote {
        let model = resolve_remote_model_name(cfg);
        format!("Semantic search enabled (backend: remote, model: {model}, threshold: {threshold})")
    } else {
        let model = &cfg.model;
        format!("Semantic search enabled (model: {model}, threshold: {threshold})")
    }
}

pub fn build_embedding_backend(
    cfg: &crate::config::SemanticSearchConfig,
) -> FacadeResult<EmbeddingBackend> {
    // Env vars override config file
    let remote_url = std::env::var("CODANNA_EMBED_URL")
        .ok()
        .or_else(|| cfg.remote_url.clone());

    if let Some(url) = remote_url {
        let model = resolve_remote_model_name(cfg);

        let dim: Option<usize> = match std::env::var("CODANNA_EMBED_DIM") {
            Ok(s) => {
                let parsed = s.parse::<usize>().map_err(|_| {
                    IndexError::General(format!(
                        "CODANNA_EMBED_DIM must be a positive integer, got: {s:?}"
                    ))
                })?;
                if parsed == 0 {
                    return Err(IndexError::General(
                        "CODANNA_EMBED_DIM must be greater than zero".to_string(),
                    ));
                }
                Some(parsed)
            }
            Err(_) => cfg.remote_dim,
        };

        // API key from env var only -- secrets must not live in config files.
        let api_key = std::env::var("CODANNA_EMBED_API_KEY").ok();

        tracing::info!(
            target: "semantic",
            "Using remote embedding backend: url={url} model={model} auth={}",
            if api_key.is_some() { "bearer" } else { "none" }
        );

        let url_owned = url.clone();
        let model_owned = model.clone();
        let embedder =
            run_async(
                async move { RemoteEmbedder::new(&url_owned, &model_owned, dim, api_key).await },
            )
            .map_err(|e| IndexError::General(format!("Remote embedder init failed: {e}")))?;

        return Ok(EmbeddingBackend::Remote(Arc::new(embedder)));
    }

    // Local fastembed pool
    let pool_size = cfg.embedding_threads;
    let embedding_model = crate::vector::parse_embedding_model(&cfg.model)
        .map_err(|e| IndexError::General(format!("Failed to parse embedding model: {e}")))?;
    let pool = EmbeddingPool::new(pool_size, embedding_model)
        .map_err(|e| IndexError::General(format!("Local embedding pool init failed: {e}")))?;

    Ok(EmbeddingBackend::Local(pool))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: facade construction on a corrupt tantivy dir must return
    // Err, not panic. The CLI/server fallback paths call this exactly when
    // the index dir failed to load.
    #[test]
    fn new_returns_err_on_corrupt_tantivy_dir() {
        let dir = tempfile::tempdir().unwrap();
        let tantivy_dir = dir.path().join("tantivy");
        std::fs::create_dir_all(&tantivy_dir).unwrap();
        std::fs::write(tantivy_dir.join("meta.json"), b"not valid json").unwrap();

        let settings = Settings {
            index_path: dir.path().to_path_buf(),
            workspace_root: None,
            ..Default::default()
        };

        let result = IndexFacade::new(std::sync::Arc::new(settings));
        assert!(result.is_err());
    }

    // Regression: file records key off the walk root's textual form. An
    // uncanonical root used to address a key space disjoint from the
    // canonical indexed_paths walk, re-indexing every file as new and
    // doubling the index (witnessed live: 2x13370 symbols after
    // `rm -rf .codanna/index` + `codanna index .`).
    #[test]
    fn uncanonical_walk_root_does_not_double_index() {
        let dir = tempfile::tempdir().unwrap();
        let corpus = dir.path().join("proj").join("src");
        std::fs::create_dir_all(&corpus).unwrap();
        std::fs::write(
            corpus.join("a.rs"),
            "pub fn alpha() { beta(); }\npub fn beta() {}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let root = dir.path().join("proj");
        let canonical = root.canonicalize().unwrap();
        facade.index_directory(&canonical, false).unwrap();
        let count = facade.document_count().unwrap();
        assert!(count > 0, "seed pass must index the corpus");

        let alias = root.join("..").join("proj");
        facade.index_directory(&alias, false).unwrap();
        assert_eq!(
            facade.document_count().unwrap(),
            count,
            "an uncanonical alias of an indexed root must not duplicate records"
        );
    }

    // Regression: every symbol-card surface requests
    // ContextIncludes::SYMBOL_CARD. The CLI JSON paths used to request a
    // subset, rendering extends/extended_by/uses null while the MCP text
    // handler showed the same store's edges.
    #[test]
    fn symbol_card_context_carries_extends_both_directions() {
        use crate::symbol::context::ContextIncludes;

        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };

        let source = dir.path().join("classes.py");
        std::fs::write(
            &source,
            "class Base:\n    def m(self):\n        pass\n\n\nclass Derived(Base):\n    def m(self):\n        pass\n",
        )
        .unwrap();

        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let derived = facade
            .find_symbols_by_name("Derived", None)
            .pop()
            .expect("Derived indexed");
        let ctx = facade
            .get_symbol_context(derived.id, ContextIncludes::SYMBOL_CARD)
            .expect("context for Derived");
        let extends = ctx
            .relationships
            .extends
            .expect("extends fetched under SYMBOL_CARD");
        assert!(
            extends.iter().any(|s| s.name.as_ref() == "Base"),
            "Derived extends Base"
        );

        let base = facade
            .find_symbols_by_name("Base", None)
            .pop()
            .expect("Base indexed");
        let ctx = facade
            .get_symbol_context(base.id, ContextIncludes::SYMBOL_CARD)
            .expect("context for Base");
        let extended_by = ctx
            .relationships
            .extended_by
            .expect("extended_by fetched under SYMBOL_CARD");
        assert!(
            extended_by.iter().any(|s| s.name.as_ref() == "Derived"),
            "Base extended by Derived"
        );
    }

    // Regression: get_all_symbols sampled the first 10k symbol docs and
    // consumers (get_index_info kind counts) presented the sample as
    // totals.
    #[test]
    fn get_all_symbols_uncapped_beyond_10k() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        facade.document_index.start_batch().unwrap();
        for i in 1..=10500u32 {
            let kind = if i <= 100 {
                crate::SymbolKind::Struct
            } else {
                crate::SymbolKind::Function
            };
            let sym = crate::Symbol::new(
                crate::SymbolId::new(i).unwrap(),
                format!("sym_{i}").as_str(),
                kind,
                crate::FileId::new(1).unwrap(),
                crate::Range::new(i, 0, i, 10),
            );
            facade
                .document_index
                .add_document(&sym, "src/generated.rs")
                .unwrap();
        }
        facade.document_index.commit_batch().unwrap();

        let symbols = facade.get_all_symbols();
        assert_eq!(
            symbols.len(),
            10500,
            "expected all symbols, got a capped sample"
        );
        let structs = symbols
            .iter()
            .filter(|s| s.kind == crate::SymbolKind::Struct)
            .count();
        assert_eq!(structs, 100);
    }

    // Regression: a deletion-only incremental run must surface removal
    // counts across the facade stats boundary instead of reading as a
    // no-op ("Index up to date"). Modified-file cleanup must NOT count:
    // its symbols re-add in the same run.
    #[test]
    fn deletion_only_run_reports_removal_counts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fixture");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("alpha.py"), "def alpha():\n    pass\n").unwrap();
        std::fs::write(
            root.join("beta.py"),
            "def beta_one():\n    pass\n\n\ndef beta_two():\n    pass\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let seed = facade.index_directory(&root, false).unwrap();
        assert_eq!(seed.files_indexed, 2);
        assert_eq!(seed.files_removed, 0);

        std::fs::remove_file(root.join("beta.py")).unwrap();
        let stats = facade.index_directory(&root, false).unwrap();
        assert_eq!(stats.files_indexed, 0, "no files re-indexed");
        assert_eq!(stats.files_removed, 1, "deletion must surface");
        assert_eq!(
            stats.symbols_removed, 3,
            "beta.py carried <module> + two functions"
        );
    }

    // Regression: force re-index of a not-yet-indexed file must still
    // succeed after remove_file errors stopped being swallowed.
    #[test]
    fn index_file_with_force_succeeds_on_unindexed_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };

        let source = dir.path().join("sample.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();

        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        let result = facade.index_file_with_force(&source, true);
        assert!(result.is_ok(), "force on unindexed file: {result:?}");
    }

    fn settings_with_broken_typescript(dir: &std::path::Path) -> Settings {
        let mut settings = Settings {
            index_path: dir.join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .languages
            .get_mut("typescript")
            .expect("typescript registered by default")
            .parser_options
            .insert("function_wrappers".into(), serde_json::json!(42));
        settings
    }

    // Regression: a language whose parser cannot construct (malformed
    // parser_options) must fail the run, not report success with every
    // file of that language silently skipped.
    #[test]
    fn index_directory_fails_when_parser_construction_fails() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.ts"), "export function main() {}\n").unwrap();

        let settings = settings_with_broken_typescript(dir.path());
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let result = facade.index_directory(&root, false);
        let err = result.expect_err("construction failure must fail the run");
        let msg = err.to_string();
        assert!(
            msg.contains("typescript") && msg.contains("function_wrappers"),
            "error must name the language and cause: {msg}"
        );
    }

    // A healthy language in the same run must not mask the broken one:
    // partial success still fails.
    #[test]
    fn index_directory_mixed_languages_still_fails_on_broken_language() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("lib.rs"), "pub fn alpha() {}\n").unwrap();
        std::fs::write(root.join("app.ts"), "export function main() {}\n").unwrap();

        let settings = settings_with_broken_typescript(dir.path());
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let result = facade.index_directory(&root, false);
        assert!(
            result.is_err(),
            "run with a healthy language must still fail: {result:?}"
        );
    }

    // Regression: a failed re-index must not evict the file's old rows.
    // Cleanup used to commit before parse; a construction failure then
    // left the deletion standing (durable data loss until config fix).
    #[test]
    fn index_file_retains_old_rows_when_reindex_parse_fails() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("app.ts");
        std::fs::write(&source, "export function main() {}\n").unwrap();

        let seeded = {
            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_file(&source).unwrap();
            facade.symbol_count()
        };
        assert!(seeded > 0, "seed must index symbols");

        std::fs::write(
            &source,
            "export function main() {}\nexport function extra() {}\n",
        )
        .unwrap();

        let settings = settings_with_broken_typescript(dir.path());
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade
            .index_file(&source)
            .expect_err("construction failure must surface");
        assert_eq!(
            facade.symbol_count(),
            seeded,
            "failed re-index must leave the old rows in place"
        );
    }

    // Same invariant on the directory incremental path: the modified
    // file's rows survive a run whose parser cannot construct.
    #[test]
    fn index_directory_retains_old_rows_when_reindex_construction_fails() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("lib.rs"), "pub fn alpha() {}\n").unwrap();
        std::fs::write(root.join("app.ts"), "export function main() {}\n").unwrap();

        let seeded = {
            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_directory(&root, false).unwrap();
            facade.symbol_count()
        };
        assert!(seeded > 0, "seed must index symbols");

        std::fs::write(
            root.join("app.ts"),
            "export function main() {}\nexport function extra() {}\n",
        )
        .unwrap();
        // Discover's fast path skips same-second rewrites on stored mtime;
        // push mtime forward so the file registers as modified.
        std::fs::File::options()
            .write(true)
            .open(root.join("app.ts"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();

        let settings = settings_with_broken_typescript(dir.path());
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade
            .index_directory(&root, false)
            .expect_err("construction failure must fail the run");
        assert_eq!(
            facade.symbol_count(),
            seeded,
            "failed incremental run must leave the modified file's rows in place"
        );
    }

    // Lexical-this walk, end to end through the real js parser: the
    // story's minimized reproducer. The arrow shadows the method's name;
    // the persisted edge must target the ClassMember method, never the
    // arrow itself.
    #[test]
    fn js_arrow_this_shadow_resolves_to_method_not_self_loop() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.js");
        std::fs::write(
            &source,
            "class Widget {\n  render() {\n    const render = () => this.render();\n    return render;\n  }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let arrow = facade
            .find_symbols_by_name("render", None)
            .into_iter()
            .find(|s| s.kind == SymbolKind::Function)
            .expect("arrow symbol indexed");
        let callees = facade.get_called_functions(arrow.id);
        assert_eq!(
            callees.len(),
            1,
            "arrow must call exactly the lexical method: {callees:?}"
        );
        assert_eq!(callees[0].kind, SymbolKind::Method, "callee is the method");
        assert_ne!(callees[0].id, arrow.id, "never a self-loop");
    }

    // TypeScript twin of the lexical-this lock: modifiers and a return
    // type must not break the barrier-to-member range equality the walk
    // depends on.
    #[test]
    fn ts_arrow_this_shadow_resolves_to_method_not_self_loop() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.ts");
        std::fs::write(
            &source,
            "class Widget {\n  private render(): number {\n    const render = () => this.render();\n    return render();\n  }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let arrow = facade
            .find_symbols_by_name("render", None)
            .into_iter()
            .find(|s| s.kind == SymbolKind::Function)
            .expect("arrow symbol indexed");
        let callees = facade.get_called_functions(arrow.id);
        assert_eq!(
            callees.len(),
            1,
            "arrow must call exactly the lexical method: {callees:?}"
        );
        assert_eq!(callees[0].kind, SymbolKind::Method, "callee is the method");
        assert_ne!(callees[0].id, arrow.id, "never a self-loop");
    }

    // Python twin: a nested def without its own `self` parameter
    // captures the enclosing method's `self` lexically, so the innermost
    // this-barrier is the method. The nested def shadows the method's
    // name, so a scope-lookup resolution would self-loop.
    #[test]
    fn py_nested_def_self_call_resolves_to_method_not_self_loop() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.py");
        std::fs::write(
            &source,
            "class Widget:\n    def render(self):\n        def render():\n            return self.render()\n        return render\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let mut renders = facade.find_symbols_by_name("render", None);
        renders.sort_by_key(|s| s.range.start_line);
        assert_eq!(
            renders.len(),
            2,
            "method and nested def both indexed: {renders:?}"
        );
        let method_id = renders[0].id;
        let nested_id = renders[1].id;

        let callees = facade.get_called_functions(nested_id);
        assert_eq!(
            callees.len(),
            1,
            "nested def must call exactly the lexical method: {callees:?}"
        );
        assert_eq!(callees[0].id, method_id, "callee is the enclosing method");
        assert_ne!(callees[0].id, nested_id, "never a self-loop");
    }

    // A nested def binding its own `self` is its own barrier: the name is
    // rebound, the enclosing method does not own that `self`, and the call
    // fails closed rather than resolving to the shadowed member.
    #[test]
    fn py_nested_def_rebinding_self_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.py");
        std::fs::write(
            &source,
            "class Widget:\n    def keep(self, x):\n        def keep(self):\n            return self.keep(x)\n        return keep\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let mut keeps = facade.find_symbols_by_name("keep", None);
        keeps.sort_by_key(|s| s.range.start_line);
        assert_eq!(
            keeps.len(),
            2,
            "method and nested def both indexed: {keeps:?}"
        );

        let callees = facade.get_called_functions(keeps[1].id);
        assert!(
            callees.is_empty(),
            "a rebound `self` must fail closed: {callees:?}"
        );
    }

    // A decorated method nests its `function_definition` inside a
    // `decorated_definition`, so the barrier span and the member symbol's
    // own range must still agree for the walk to land.
    #[test]
    fn py_decorated_method_barrier_matches_member_range() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.py");
        std::fs::write(
            &source,
            "class Widget:\n    @property\n    def value(self):\n        def value():\n            return self.compute()\n        return value\n\n    def compute(self):\n        return 1\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let compute_id = facade
            .find_symbols_by_name("compute", None)
            .first()
            .expect("compute indexed")
            .id;
        let mut values = facade.find_symbols_by_name("value", None);
        values.sort_by_key(|s| s.range.start_line);
        assert_eq!(values.len(), 2, "method and nested def indexed: {values:?}");

        let callees = facade.get_called_functions(values[1].id);
        assert_eq!(
            callees.len(),
            1,
            "decorated method must still own the nested def's `self`: {callees:?}"
        );
        assert_eq!(callees[0].id, compute_id, "callee is the sibling member");
    }

    // A comment inside the parameter list is its first named child, ahead
    // of `self`. The method must still register as a barrier, or every
    // nested def under a lint-suppressed signature fails closed.
    #[test]
    fn py_comment_before_self_parameter_still_barriers() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.py");
        std::fs::write(
            &source,
            "class Widget:\n    def build(  # noqa\n        self, x\n    ):\n        def inner(schema):\n            return self.other(schema)\n        return inner\n\n    def other(self, s):\n        return 1\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let inner_id = facade
            .find_symbols_by_name("inner", None)
            .first()
            .expect("nested def indexed")
            .id;
        let other_id = facade
            .find_symbols_by_name("other", None)
            .first()
            .expect("sibling member indexed")
            .id;

        let callees = facade.get_called_functions(inner_id);
        assert_eq!(
            callees.len(),
            1,
            "comment-led parameter list must not break the barrier: {callees:?}"
        );
        assert_eq!(callees[0].id, other_id, "callee is the sibling member");
    }

    // A lambda whose own parameter is named `self` rebinds the name, so it
    // owns its `self` exactly as a def would. Without a barrier of its own
    // the walk would run past it to the enclosing method and resolve a name
    // that never referred to the instance.
    #[test]
    fn py_lambda_rebinding_self_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.py");
        std::fs::write(
            &source,
            "class Widget:\n    def m(self):\n        def outer(x):\n            f = lambda self: self.other()\n            return f\n        return outer\n\n    def other(self):\n        return 1\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let other_id = facade
            .find_symbols_by_name("other", None)
            .first()
            .expect("sibling member indexed")
            .id;
        for sym in facade.find_symbols_by_name("outer", None) {
            let callees = facade.get_called_functions(sym.id);
            assert!(
                !callees.iter().any(|c| c.id == other_id),
                "a lambda-rebound `self` must not reach the enclosing method: {callees:?}"
            );
        }
    }

    // `cls` is the second alias in the vocabulary: a classmethod owns its
    // `cls` and is a barrier, so a nested def capturing it reaches the
    // classmethod's class member.
    #[test]
    fn py_classmethod_cls_capture_resolves_to_member() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("widget.py");
        std::fs::write(
            &source,
            "class Widget:\n    @classmethod\n    def make(cls):\n        def build():\n            return cls.helper()\n        return build\n\n    @classmethod\n    def helper(cls):\n        return 1\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let helper_id = facade
            .find_symbols_by_name("helper", None)
            .first()
            .expect("classmethod member indexed")
            .id;
        let build_id = facade
            .find_symbols_by_name("build", None)
            .first()
            .expect("nested def indexed")
            .id;

        let callees = facade.get_called_functions(build_id);
        assert_eq!(
            callees.len(),
            1,
            "nested def must reach the classmethod's member via `cls`: {callees:?}"
        );
        assert_eq!(
            callees[0].id, helper_id,
            "callee is the sibling classmethod"
        );
    }

    // A php enum is a container like a class: its methods carry class
    // evidence, so a `$this` call between them resolves. The class in the
    // same fixture is the control — it already resolves today.
    #[test]
    fn php_enum_method_self_call_resolves_to_sibling_member() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("status.php");
        std::fs::write(
            &source,
            "<?php\nenum Status: string {\n    case pending = 'pending';\n\n    public function description(): string { return 'd'; }\n\n    public function toArray() {\n        return ['description' => $this->description()];\n    }\n}\n\nclass C {\n    public function alpha() { return $this->beta(); }\n    public function beta() { return 1; }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        // Control: the class arm resolves today.
        let alpha_id = facade
            .find_symbols_by_name("alpha", None)
            .first()
            .expect("class method indexed")
            .id;
        let beta_id = facade
            .find_symbols_by_name("beta", None)
            .first()
            .expect("class method indexed")
            .id;
        let control = facade.get_called_functions(alpha_id);
        assert_eq!(
            control.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![beta_id],
            "control: class `$this` call must resolve"
        );

        let to_array_id = facade
            .find_symbols_by_name("toArray", None)
            .first()
            .expect("enum method indexed")
            .id;
        let description_id = facade
            .find_symbols_by_name("description", None)
            .first()
            .expect("enum method indexed")
            .id;
        let callees = facade.get_called_functions(to_array_id);
        assert_eq!(
            callees.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![description_id],
            "enum `$this` call must resolve to the sibling member"
        );
    }

    // The enum symbol itself must exist and carry the Enum kind, matching
    // the vocabulary java/kotlin/swift/rust already emit. Before the
    // container arm the symbol was absent entirely.
    #[test]
    fn php_enum_indexes_as_enum_kind_with_members_defined() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("status.php");
        std::fs::write(
            &source,
            "<?php\nenum Status: string {\n    case pending = 'pending';\n\n    public function description(): string { return 'd'; }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let status = facade
            .find_symbols_by_name("Status", None)
            .into_iter()
            .next()
            .expect("enum symbol indexed");
        assert_eq!(
            status.kind,
            SymbolKind::Enum,
            "php enum takes the Enum kind"
        );

        let deps = facade.get_dependencies(status.id);
        let defined = deps
            .get(&RelationKind::Defines)
            .cloned()
            .unwrap_or_default();
        assert!(
            defined.iter().any(|s| s.name.as_ref() == "description"),
            "enum members are Defines targets: {defined:?}"
        );
    }

    // php enums implement interfaces (the laravel witness is
    // `enum ArrayableStatus: string implements Arrayable`), so the
    // interface clause must be read on the enum arm too.
    #[test]
    fn php_enum_implements_clause_emits_edge() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("status.php");
        std::fs::write(
            &source,
            "<?php\ninterface Arrayable {\n    public function toArray();\n}\n\nenum Status: string implements Arrayable {\n    case pending = 'pending';\n\n    public function toArray() { return []; }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let status_id = facade
            .find_symbols_by_name("Status", None)
            .first()
            .expect("enum symbol indexed")
            .id;
        let implemented = facade.get_implemented_traits(status_id);
        assert!(
            implemented.iter().any(|s| s.name.as_ref() == "Arrayable"),
            "enum implements clause must emit an edge: {implemented:?}"
        );
    }

    // Enum cases are members: Constant kind, scoped to the enum, reachable
    // as Defines targets. Matches rust enum_variant / kotlin and swift
    // enum_entry.
    #[test]
    fn php_enum_case_indexes_as_constant_member() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("status.php");
        std::fs::write(
            &source,
            "<?php\nenum Status: string {\n    case pending = 'pending';\n\n    public function d(): string { return 'd'; }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let pending = facade
            .find_symbols_by_name("pending", None)
            .into_iter()
            .next()
            .expect("enum case indexed");
        assert_eq!(
            pending.kind,
            SymbolKind::Constant,
            "enum case takes the Constant kind"
        );

        let status_id = facade
            .find_symbols_by_name("Status", None)
            .first()
            .expect("enum symbol indexed")
            .id;
        let deps = facade.get_dependencies(status_id);
        let defined = deps
            .get(&RelationKind::Defines)
            .cloned()
            .unwrap_or_default();
        assert!(
            defined.iter().any(|s| s.name.as_ref() == "pending"),
            "enum case is a Defines target of its enum: {defined:?}"
        );
    }

    // `case` is ambiguous in php: an enum case is a member, a switch case
    // is control flow. Only the former is a symbol. A pure (unbacked) case
    // is a member too.
    #[test]
    fn php_pure_enum_case_is_a_symbol_and_switch_case_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("mixed.php");
        std::fs::write(
            &source,
            "<?php\nenum Flag {\n    case bare;\n}\n\nfunction pick($x) {\n    switch ($x) {\n        case NOTASYMBOL:\n            return 1;\n    }\n    return 0;\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_file(&source).unwrap();

        let bare = facade
            .find_symbols_by_name("bare", None)
            .into_iter()
            .next()
            .expect("unbacked enum case indexed");
        assert_eq!(bare.kind, SymbolKind::Constant, "pure case is a Constant");

        assert!(
            facade.find_symbols_by_name("NOTASYMBOL", None).is_empty(),
            "a switch case is control flow, not a member"
        );
    }

    // Single-file path (watcher reindex): the error names the language,
    // not an anonymous parse failure with an empty path.
    #[test]
    fn index_file_names_language_on_construction_failure() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("app.ts");
        std::fs::write(&source, "export function main() {}\n").unwrap();

        let settings = settings_with_broken_typescript(dir.path());
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let err = facade
            .index_file(&source)
            .expect_err("construction failure must surface");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot initialize typescript parser"),
            "error must carry the typed construction message: {msg}"
        );
    }

    // Lane-parity lock for the inheritance-witness arm: a bare call to a
    // member the caller's class inherits resolves to the imported
    // parent's member — never the same-name decoy that sorts first —
    // identically in the force lane and the incremental lane. History:
    // before the module_path round-trip fix the incremental lane
    // first-picked the decoy while the force lane failed closed; before
    // the witness arm both lanes failed closed.
    #[test]
    fn incremental_lane_matches_fresh_verdict_on_receiverless_member_call() {
        for force in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            for pkg in ["z", "b", "c"] {
                std::fs::create_dir_all(src.join(pkg)).unwrap();
            }
            std::fs::write(
                src.join("z/Base.java"),
                "package z;\npublic class Base { protected void helper() { } }\n",
            )
            .unwrap();
            std::fs::write(
                src.join("b/Child.java"),
                "package b;\nimport z.Base;\npublic class Child extends Base { public void run() { helper(); } }\n",
            )
            .unwrap();
            std::fs::write(
                src.join("c/Other.java"),
                "package c;\npublic class Other { protected void helper() { } }\n",
            )
            .unwrap();

            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_directory(&src, force).unwrap();

            let runs = facade.find_symbols_by_name("run", None);
            assert_eq!(runs.len(), 1, "one run symbol expected (force={force})");
            let callees = facade.get_called_functions(runs[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert_eq!(
                callees.len(),
                1,
                "inherited bare call must resolve on the witness (force={force}), got: {picked:?}"
            );
            let path = facade.get_file_path(callees[0].file_id).unwrap_or_default();
            assert!(
                callees[0].name.as_ref() == "helper" && path.ends_with("z/Base.java"),
                "must resolve to the inherited parent's member, not the decoy \
                 (force={force}), got: {picked:?}"
            );
        }
    }

    // Inheritance-witness arm, kotlin same-package shape (the ktor
    // witness class): the parent is not imported, so the hop resolves
    // through the exactly-one same-module Class survivor — the same
    // evidence the Extends edge itself resolves through.
    #[test]
    fn kotlin_bare_call_to_inherited_member_resolves_on_witness() {
        for force in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("Base.kt"),
                "package p\n\nopen class Base {\n    protected fun helper() {\n    }\n}\n",
            )
            .unwrap();
            std::fs::write(
                src.join("Child.kt"),
                "package p\n\nclass Child : Base() {\n    fun run() {\n        helper()\n    }\n}\n",
            )
            .unwrap();

            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_directory(&src, force).unwrap();

            let runs = facade.find_symbols_by_name("run", None);
            assert_eq!(runs.len(), 1, "one run symbol expected (force={force})");
            let callees = facade.get_called_functions(runs[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert_eq!(
                callees.len(),
                1,
                "inherited bare call must resolve on the witness (force={force}), got: {picked:?}"
            );
            let path = facade.get_file_path(callees[0].file_id).unwrap_or_default();
            assert!(
                callees[0].name.as_ref() == "helper" && path.ends_with("Base.kt"),
                "must resolve to the superclass member (force={force}), got: {picked:?}"
            );
        }
    }

    // Slice 1b tracer bullet: an inherited `self.helper()` whose member
    // lives in the parent's file resolves on the inheritance walk from
    // the self-form miss path — to the parent's member, never the
    // same-name decoy. Production lanes only: fresh (auto-force shape),
    // then a seeded incremental re-index of the consumer. Python module
    // identity is path-derived, so incremental-on-empty (a lane the
    // facade's auto-force forbids anyway) degenerates and locks nothing.
    #[test]
    fn python_inherited_self_call_resolves_on_walk() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let pkg = src.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("__init__.py"), "").unwrap();
        std::fs::write(
            pkg.join("base.py"),
            "class Base:\n    def helper(self):\n        pass\n",
        )
        .unwrap();
        let consumer = "from pkg.base import Base\n\n\nclass Child(Base):\n    def run(self):\n        self.helper()\n";
        std::fs::write(pkg.join("child.py"), consumer).unwrap();
        std::fs::write(
            pkg.join("other.py"),
            "class Other:\n    def helper(self):\n        pass\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let assert_resolves = |facade: &IndexFacade, leg: &str| {
            let runs = facade.find_symbols_by_name("run", None);
            assert_eq!(runs.len(), 1, "one run symbol expected ({leg})");
            let callees = facade.get_called_functions(runs[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert!(
                callees.iter().any(|s| s.name.as_ref() == "helper"
                    && facade
                        .get_file_path(s.file_id)
                        .unwrap_or_default()
                        .ends_with("base.py")),
                "inherited self-form call must resolve to the parent's member \
                 ({leg}), got: {picked:?}"
            );
        };

        facade.index_directory(&src, true).unwrap();
        assert_resolves(&facade, "fresh");

        // Touch only the consumer; the parent and decoy stay unchanged.
        std::fs::write(pkg.join("child.py"), format!("{consumer}\n# touched\n")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(pkg.join("child.py"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        facade.index_directory(&src, false).unwrap();
        assert_resolves(&facade, "seeded incremental");
    }

    // Slice 1b, kotlin twin: an explicit `this.helper()` whose member is
    // inherited resolves on the walk — to the superclass member, never
    // the same-name decoy in an unrelated class. Production lanes:
    // fresh, then seeded incremental re-index of the consumer.
    #[test]
    fn kotlin_inherited_this_call_resolves_on_walk() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("Base.kt"),
            "package p\n\nopen class Base {\n    protected fun helper() {\n    }\n}\n",
        )
        .unwrap();
        let consumer = "package p\n\nclass Child : Base() {\n    fun run() {\n        this.helper()\n    }\n}\n";
        std::fs::write(src.join("Child.kt"), consumer).unwrap();
        std::fs::write(
            src.join("Other.kt"),
            "package p\n\nclass Other {\n    internal fun helper() {\n    }\n}\n",
        )
        .unwrap();

        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let assert_resolves = |facade: &IndexFacade, leg: &str| {
            let runs = facade.find_symbols_by_name("run", None);
            assert_eq!(runs.len(), 1, "one run symbol expected ({leg})");
            let callees = facade.get_called_functions(runs[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert!(
                callees.iter().any(|s| s.name.as_ref() == "helper"
                    && facade
                        .get_file_path(s.file_id)
                        .unwrap_or_default()
                        .ends_with("Base.kt")),
                "inherited this-call must resolve to the superclass member \
                 ({leg}), got: {picked:?}"
            );
        };

        facade.index_directory(&src, true).unwrap();
        assert_resolves(&facade, "fresh");

        std::fs::write(src.join("Child.kt"), format!("{consumer}\n// touched\n")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(src.join("Child.kt"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        facade.index_directory(&src, false).unwrap();
        assert_resolves(&facade, "seeded incremental");
    }

    // Found-arm member gate: a bare call whose sole same-language
    // candidate is another class's member — no receiver, no import, no
    // inheritance witness — fails closed. Module identity plus
    // candidate count is not evidence for a member pick; the same rule
    // already gates the Ambiguous path in `disambiguate`.
    #[test]
    fn java_bare_cross_file_member_pick_fails_closed_without_witness() {
        for force in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("Widget.java"),
                "package p;\npublic class Widget { public void setup() { } }\n",
            )
            .unwrap();
            std::fs::write(
                src.join("Factory.java"),
                "package p;\npublic class Factory { public void make() { setup(); } }\n",
            )
            .unwrap();

            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_directory(&src, force).unwrap();

            let makes = facade.find_symbols_by_name("make", None);
            assert_eq!(makes.len(), 1, "one make symbol expected (force={force})");
            let callees = facade.get_called_functions(makes[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert!(
                callees.is_empty(),
                "unwitnessed cross-file member pick must fail closed \
                 (force={force}), got: {picked:?}"
            );
        }
    }

    // Receiver-carrying exemption, both directions: a binding-inferred
    // receiver whose type places the member on the chain is class
    // evidence and survives the Found-arm gate; a chain-mismatched
    // receiver dies (pre-gate, at the instance-type check). TypeScript
    // fixture: its binding channel emits the name-to-type shape from
    // `const w = new Widget()`, and its class members are tier-3
    // visible cross-module, so the row reaches the Found arm (python
    // methods are not Public at tier 3 and detour to the typed-receiver
    // global path; kotlin records expression-text types; java's
    // `collect_variable_types` is a stub). No import statement: import
    // identity must not mask the receiver evidence under test.
    #[test]
    fn receiver_typed_member_call_survives_gate_and_mismatch_dies() {
        for force in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("widget.ts"),
                "export class Widget {\n    setup(): void {\n    }\n}\n",
            )
            .unwrap();
            std::fs::write(
                src.join("gadget.ts"),
                "export class Gadget {\n    frob(): void {\n    }\n}\n",
            )
            .unwrap();
            std::fs::write(
                src.join("factory.ts"),
                "export function good(): void {\n    const w = new Widget();\n    w.setup();\n}\n\nexport function bad(): void {\n    const g = new Gadget();\n    g.setup();\n}\n",
            )
            .unwrap();

            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_directory(&src, force).unwrap();

            let named = |name: &str| {
                let syms = facade.find_symbols_by_name(name, None);
                assert_eq!(syms.len(), 1, "one {name} symbol expected (force={force})");
                syms.into_iter().next().unwrap()
            };

            let good_callees = facade.get_called_functions(named("good").id);
            assert!(
                good_callees.iter().any(|s| s.name.as_ref() == "setup"
                    && facade
                        .get_file_path(s.file_id)
                        .unwrap_or_default()
                        .ends_with("widget.ts")),
                "chain-verified receiver call must survive the gate \
                 (force={force}), got: {good_callees:?}"
            );

            let bad_callees = facade.get_called_functions(named("bad").id);
            assert!(
                !bad_callees.iter().any(|s| s.name.as_ref() == "setup"),
                "chain-mismatched receiver call must fail closed \
                 (force={force}), got: {bad_callees:?}"
            );
        }
    }

    // Cross-file same-type member: a self-form call whose member is
    // defined in another file of the SAME type (rust split impl
    // blocks) resolves on the named-ClassMember match — caller and
    // member both declare membership in Widget — behind exactly-one
    // same-language discipline and the same-tree constraint. The
    // Other.setup decoy is filtered by the named match. Production
    // lanes only, and the indexed path is PRE-REGISTERED in settings:
    // rust module identity is path-derived and the List lane has no
    // walk root, so its strip base comes from the registered indexed
    // paths — exactly the shape production incremental runs have
    // (invariant: bare test contexts without registered paths
    // degenerate to module None and the arm correctly fails closed).
    #[test]
    fn rust_split_impl_self_call_resolves_on_named_member() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("widget.rs"),
            "pub struct Widget {}\n\nimpl Widget {\n    pub fn setup(&self) {\n    }\n}\n",
        )
        .unwrap();
        let consumer = "use crate::widget::Widget;\n\nimpl Widget {\n    pub fn make(&self) {\n        self.setup();\n    }\n}\n";
        std::fs::write(src.join("consumer.rs"), consumer).unwrap();
        std::fs::write(
            src.join("other.rs"),
            "pub struct Other {}\n\nimpl Other {\n    pub fn setup(&self) {\n    }\n}\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .add_indexed_path(src.clone())
            .expect("register indexed path");
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        let assert_resolves = |facade: &IndexFacade, leg: &str| {
            let makes = facade.find_symbols_by_name("make", None);
            assert_eq!(makes.len(), 1, "one make symbol expected ({leg})");
            let callees = facade.get_called_functions(makes[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert!(
                callees.iter().any(|s| s.name.as_ref() == "setup"
                    && facade
                        .get_file_path(s.file_id)
                        .unwrap_or_default()
                        .ends_with("widget.rs")),
                "split-impl self call must resolve to the same type's \
                 member on the named-ClassMember witness ({leg}), \
                 got: {picked:?}"
            );
        };

        facade.index_directory(&src, true).unwrap();
        assert_resolves(&facade, "fresh");

        std::fs::write(src.join("consumer.rs"), format!("{consumer}\n// touched\n")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(src.join("consumer.rs"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        facade.index_directory(&src, false).unwrap();
        assert_resolves(&facade, "seeded incremental");
    }

    // Same-file name claimant vetoes the cross-file borrow: when the
    // caller's own file holds ANY member named like the call (under
    // whatever class), the tree-wide named match must not borrow a
    // same-named-class copy from another file. Witnessed leak:
    // three.js minified twin bundles — independent minification
    // scrambles class names, so the caller's class name matches the
    // TWIN bundle's copy while its own bundle's copy sits same-file
    // under a different name. The row still resolves through the
    // local tier to the same-file claimant.
    #[test]
    fn same_file_claimant_vetoes_cross_file_named_borrow() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        // Shared parent dir: both modules root at `lib`, so the (b)
        // same-tree constraint admits the twin — the veto is the only
        // discipline left between the caller and the wrong copy.
        let lib = src.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(
            lib.join("appA.js"),
            "class Painter {\n    parse() {\n        this.createNodeFromType();\n    }\n}\n\nclass Registry {\n    createNodeFromType() {\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            lib.join("appB.js"),
            "class Painter {\n    createNodeFromType() {\n    }\n}\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .add_indexed_path(src.clone())
            .expect("register indexed path");
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_directory(&src, true).unwrap();

        let parses = facade.find_symbols_by_name("parse", None);
        assert_eq!(parses.len(), 1, "one parse symbol expected");
        let callees = facade.get_called_functions(parses[0].id);
        let picked: Vec<String> = callees
            .iter()
            .map(|s| {
                format!(
                    "{}@{}",
                    s.name,
                    facade.get_file_path(s.file_id).unwrap_or_default()
                )
            })
            .collect();
        assert!(
            !callees
                .iter()
                .any(|s| s.name.as_ref() == "createNodeFromType"
                    && facade
                        .get_file_path(s.file_id)
                        .unwrap_or_default()
                        .ends_with("appB.js")),
            "cross-file borrow past a same-file claimant is a wrong-copy \
             pick, got: {picked:?}"
        );
    }

    // Language gate on the split-type premise: php declares one class
    // per file, so a same-named class in another file is a DIFFERENT
    // class — the named match must not borrow its member (witnessed:
    // laravel Schema\Grammars\SqlServerGrammar callers borrowing
    // Query\Grammars\SqlServerGrammar's wrapTable — namespace twins,
    // no inheritance relation). The arm runs only where the language
    // has split-type syntax (rust impl blocks, cpp out-of-line,
    // csharp partial).
    #[test]
    fn php_namespace_twin_class_member_stays_closed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let a = src.join("schema");
        let b = src.join("query");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            a.join("Widget.php"),
            "<?php\nnamespace App\\Schema;\n\nclass Widget {\n    public function make() {\n        $this->setup(1);\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            b.join("Widget.php"),
            "<?php\nnamespace App\\Query;\n\nclass Widget {\n    public function setup($x) {\n    }\n}\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .add_indexed_path(src.clone())
            .expect("register indexed path");
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_directory(&src, true).unwrap();

        let makes = facade.find_symbols_by_name("make", None);
        assert_eq!(makes.len(), 1, "one make symbol expected");
        let callees = facade.get_called_functions(makes[0].id);
        assert!(
            !callees.iter().any(|s| s.name.as_ref() == "setup"),
            "a namespace twin's member is another class's member, got: {callees:?}"
        );
    }

    // Duplicate type copies: two same-named types in one tree, both
    // declaring the member — the named match cannot pick a copy, so
    // exactly-one discipline fails closed.
    #[test]
    fn rust_split_impl_duplicate_type_copies_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("widget.rs"),
            "pub struct Widget {}\n\nimpl Widget {\n    pub fn setup(&self) {\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            src.join("twin.rs"),
            "pub struct Widget {}\n\nimpl Widget {\n    pub fn setup(&self) {\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            src.join("consumer.rs"),
            "use crate::widget::Widget;\n\nimpl Widget {\n    pub fn make(&self) {\n        self.setup();\n    }\n}\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .add_indexed_path(src.clone())
            .expect("register indexed path");
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_directory(&src, true).unwrap();

        let makes = facade.find_symbols_by_name("make", None);
        assert_eq!(makes.len(), 1, "one make symbol expected");
        let callees = facade.get_called_functions(makes[0].id);
        assert!(
            !callees.iter().any(|s| s.name.as_ref() == "setup"),
            "two named claimants cannot license a copy pick, got: {callees:?}"
        );
    }

    // Cross-tree block, the (b) discipline: a single global claimant
    // in ANOTHER tree (different module root) is not a candidate — the
    // caller's own same-named class lacking the member must not borrow
    // it across trees.
    #[test]
    fn python_cross_tree_single_claimant_stays_closed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let pkg_a = src.join("pkg_a");
        let pkg_b = src.join("pkg_b");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_b).unwrap();
        std::fs::write(pkg_a.join("__init__.py"), "").unwrap();
        std::fs::write(pkg_b.join("__init__.py"), "").unwrap();
        std::fs::write(
            pkg_a.join("widget.py"),
            "class Widget:\n    def setup(self):\n        pass\n",
        )
        .unwrap();
        std::fs::write(
            pkg_b.join("consumer.py"),
            "class Widget:\n    def make(self):\n        self.setup()\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings
            .add_indexed_path(src.clone())
            .expect("register indexed path");
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
        facade.index_directory(&src, true).unwrap();

        let makes = facade.find_symbols_by_name("make", None);
        assert_eq!(makes.len(), 1, "one make symbol expected");
        let callees = facade.get_called_functions(makes[0].id);
        assert!(
            !callees.iter().any(|s| s.name.as_ref() == "setup"),
            "a cross-tree claimant must not be borrowed, got: {callees:?}"
        );
    }

    // Own-scope exemption: a bare call to the caller's own non-public
    // member keeps its same-file evidence — the gate fires only on
    // cross-file picks. The cross-file public decoy guards that the
    // pick stays on the caller's own member.
    #[test]
    fn own_member_bare_call_survives_gate() {
        for force in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(
                src.join("Widget.java"),
                "package p;\npublic class Widget {\n    private void setup() { }\n    public void make() { setup(); }\n}\n",
            )
            .unwrap();
            std::fs::write(
                src.join("Decoy.java"),
                "package p;\npublic class Decoy { public void setup() { } }\n",
            )
            .unwrap();

            let settings = Settings {
                index_path: dir.path().join("index"),
                workspace_root: None,
                ..Default::default()
            };
            let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();
            facade.index_directory(&src, force).unwrap();

            let makes = facade.find_symbols_by_name("make", None);
            assert_eq!(makes.len(), 1, "one make symbol expected (force={force})");
            let callees = facade.get_called_functions(makes[0].id);
            let picked: Vec<String> = callees
                .iter()
                .map(|s| {
                    format!(
                        "{}@{}",
                        s.name,
                        facade.get_file_path(s.file_id).unwrap_or_default()
                    )
                })
                .collect();
            assert!(
                callees.iter().any(|s| s.name.as_ref() == "setup"
                    && facade
                        .get_file_path(s.file_id)
                        .unwrap_or_default()
                        .ends_with("Widget.java")),
                "own-member bare call must survive on same-file evidence \
                 (force={force}), got: {picked:?}"
            );
        }
    }

    // Regression: re-indexing a file used to delete every edge pointing INTO
    // it. CleanupStage removed relationships in both directions for the
    // file's symbols, and the re-index that followed re-derived only that
    // file's OWN outgoing edges -- so edges owned by unchanged files died
    // silently and healed only on --force. Witnessed on gin @ 9914178:
    // 2159 -> 1969 edges after touching two files, identical under the CLI
    // lane, `serve --watch`, and the `codanna mcp <TOOL> --watch` preflight.
    #[test]
    fn reindexing_a_file_preserves_edges_pointing_into_it() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pkg");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("__init__.py"), "").unwrap();
        std::fs::write(
            src.join("base.py"),
            "class Base:\n    def helper(self):\n        return 1\n",
        )
        .unwrap();
        std::fs::write(
            src.join("child.py"),
            "from pkg.base import Base\n\n\
             class Child(Base):\n    def run(self):\n        return self.helper()\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        // The incremental lane has no walk root; its strip base comes from
        // registered indexed paths (see .claude/rules/verification-gate.md).
        settings.add_indexed_path(src.clone()).unwrap();
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        facade.index_directory(&src, false).unwrap();
        let fresh = facade.relationship_count();
        assert!(
            fresh > 0,
            "seed pass must produce cross-file edges to make this meaningful"
        );
        let inbound_before = inbound_edge_names(&facade, "helper");
        assert!(
            !inbound_before.is_empty(),
            "fixture must produce at least one caller of helper before the touch"
        );

        // Touch the edge TARGET file. Its own content is semantically
        // unchanged; the edges at risk originate in child.py, which is
        // untouched.
        std::fs::write(
            src.join("base.py"),
            "class Base:\n    def helper(self):\n        return 1\n\n# touched\n",
        )
        .unwrap();
        facade.index_file(src.join("base.py")).unwrap();

        assert_eq!(
            inbound_edge_names(&facade, "helper"),
            inbound_before,
            "edges owned by the unchanged child.py must survive a re-index of base.py"
        );
        assert_eq!(
            facade.relationship_count(),
            fresh,
            "re-indexing a file must not shed edges pointing into it"
        );
    }

    // Rebind must not resurrect. A symbol the edit genuinely removed has no
    // replacement, so its inbound edges stay dead -- this is the invariant
    // that makes deleting them during cleanup safe. A best-effort rebind that
    // left unmatched captures in place would trade a recall gap for an edge
    // pointing at a symbol that no longer exists.
    #[test]
    fn reindexing_drops_inbound_edges_whose_target_the_edit_removed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pkg");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("__init__.py"), "").unwrap();
        std::fs::write(
            src.join("base.py"),
            "class Base:\n    def helper(self):\n        return 1\n",
        )
        .unwrap();
        std::fs::write(
            src.join("child.py"),
            "from pkg.base import Base\n\n\
             class Child(Base):\n    def run(self):\n        return self.helper()\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings.add_indexed_path(src.clone()).unwrap();
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        facade.index_directory(&src, false).unwrap();
        assert_eq!(
            inbound_edge_names(&facade, "helper").len(),
            1,
            "fixture must start with one caller of helper"
        );

        // The edit deletes helper outright.
        std::fs::write(
            src.join("base.py"),
            "class Base:\n    def other(self):\n        return 1\n",
        )
        .unwrap();
        facade.index_file(src.join("base.py")).unwrap();

        assert!(
            facade.find_symbols_by_name("helper", None).is_empty(),
            "helper is gone from source, so it must be gone from the index"
        );
        let run = facade.find_symbols_by_name("run", None);
        assert_eq!(run.len(), 1, "run must still exist");
        // Asserting merely "no callee named helper" is too weak: a
        // best-effort rebind would re-point the edge at some OTHER symbol in
        // the file and slip past that check. run called exactly one thing,
        // and that thing is gone, so its callee set must be empty.
        let callees: Vec<String> = facade
            .get_called_functions(run[0].id)
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        assert!(
            callees.is_empty(),
            "an edge whose target the edit removed must be dropped, not rebound \
             to a surviving symbol; got: {callees:?}"
        );
    }

    // The batch incremental lane is a separate entry point from the
    // single-file lane the watcher uses, and it is the one behind bare
    // `codanna index` and the `codanna mcp <TOOL> --watch` preflight. Both
    // must preserve inbound edges; fixing only one leaves the defect live for
    // the CLI-only agent workflow.
    #[test]
    fn batch_incremental_reindex_preserves_edges_pointing_into_the_changed_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pkg");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("__init__.py"), "").unwrap();
        std::fs::write(
            src.join("base.py"),
            "class Base:\n    def helper(self):\n        return 1\n",
        )
        .unwrap();
        std::fs::write(
            src.join("child.py"),
            "from pkg.base import Base\n\n\
             class Child(Base):\n    def run(self):\n        return self.helper()\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings.add_indexed_path(src.clone()).unwrap();
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        facade.index_directory(&src, false).unwrap();
        let fresh = facade.relationship_count();
        let inbound_before = inbound_edge_names(&facade, "helper");
        assert!(
            !inbound_before.is_empty(),
            "fixture must produce at least one caller of helper before the touch"
        );

        std::fs::write(
            src.join("base.py"),
            "class Base:\n    def helper(self):\n        return 1\n\n# touched\n",
        )
        .unwrap();
        // Re-index through the DIRECTORY lane, not index_file.
        facade.index_directory(&src, false).unwrap();

        assert_eq!(
            inbound_edge_names(&facade, "helper"),
            inbound_before,
            "batch incremental must preserve edges owned by the unchanged child.py"
        );
        assert_eq!(
            facade.relationship_count(),
            fresh,
            "batch incremental must not shed edges pointing into the changed file"
        );
    }

    // Two files edited in one run, with edges between them. The naive capture
    // (exclude only the captured file's own symbols) records a from-id living
    // in the OTHER changed file, which is itself getting fresh ids -- the
    // rebind then persists an edge from a dead symbol. Witnessed on gin as 4
    // `<orphan:Some(N)>` rows gained; single-file tests cannot reach it.
    #[test]
    fn reindexing_two_mutually_referencing_files_at_once_creates_no_orphan_edges() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pkg");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("__init__.py"), "").unwrap();
        let base = "class Base:\n    def helper(self):\n        return 1\n";
        let child = "from pkg.base import Base\n\n\
                     class Child(Base):\n    def run(self):\n        return self.helper()\n";
        std::fs::write(src.join("base.py"), base).unwrap();
        std::fs::write(src.join("child.py"), child).unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings.add_indexed_path(src.clone()).unwrap();
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        facade.index_directory(&src, false).unwrap();
        let fresh = facade.relationship_count();

        // Touch BOTH ends of the cross-file edge in the same run.
        std::fs::write(src.join("base.py"), format!("{base}\n# touched\n")).unwrap();
        std::fs::write(src.join("child.py"), format!("{child}\n# touched\n")).unwrap();
        facade.index_directory(&src, false).unwrap();

        assert_eq!(
            facade.relationship_count(),
            fresh,
            "editing both ends at once must not gain duplicate or orphan edges"
        );
        // Every surviving edge must have a live symbol on both ends.
        let helper = facade.find_symbols_by_name("helper", None);
        assert_eq!(helper.len(), 1);
        for (from, to, _) in facade.get_relationships_for_symbol(helper[0].id).unwrap() {
            assert!(
                facade.get_symbol(from).is_some(),
                "edge from a dead symbol id {from:?} survived the rebind"
            );
            assert!(
                facade.get_symbol(to).is_some(),
                "edge to a dead symbol id {to:?} survived the rebind"
            );
        }
    }

    // The line is a proxy for identity and any edit above a symbol breaks it.
    // Two impl blocks each defining `new` are told apart by their containing
    // type, which no range shift can move. Verified with a PREPEND, not an
    // append: appending leaves every start line intact and so cannot exercise
    // the tie at all.
    #[test]
    fn rebind_disambiguates_same_name_members_by_scope_across_a_line_shift() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let types = "pub struct Alpha;\npub struct Beta;\n\
                     impl Alpha {\n    pub fn make() -> u32 { 1 }\n}\n\
                     impl Beta {\n    pub fn make() -> u32 { 2 }\n}\n";
        std::fs::write(src.join("types.rs"), types).unwrap();
        std::fs::write(
            src.join("user.rs"),
            "use crate::types::Alpha;\npub fn go() -> u32 { Alpha::make() }\n",
        )
        .unwrap();

        let mut settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        settings.add_indexed_path(src.clone()).unwrap();
        let mut facade = IndexFacade::new(std::sync::Arc::new(settings)).unwrap();

        facade.index_directory(&src, false).unwrap();
        let fresh = facade.relationship_count();
        let go = facade.find_symbols_by_name("go", None);
        assert_eq!(go.len(), 1);
        let before: Vec<String> = facade
            .get_called_functions(go[0].id)
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        assert!(
            before.iter().any(|n| n == "make"),
            "fixture must resolve Alpha::make before the shift; got {before:?}"
        );

        // PREPEND: every symbol in types.rs shifts down one line.
        std::fs::write(src.join("types.rs"), format!("// shifted\n{types}")).unwrap();
        facade.index_file(src.join("types.rs")).unwrap();

        let go = facade.find_symbols_by_name("go", None);
        let after: Vec<String> = facade
            .get_called_functions(go[0].id)
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        assert_eq!(
            after, before,
            "an edge into one of two same-named members must survive an edit \
             that shifts every line in the target file"
        );
        assert_eq!(facade.relationship_count(), fresh);
    }

    /// Names of symbols holding an inbound edge to the (single) symbol
    /// called `name`. Sorted so comparisons are order-independent.
    fn inbound_edge_names(facade: &IndexFacade, name: &str) -> Vec<String> {
        let targets = facade.find_symbols_by_name(name, None);
        assert_eq!(targets.len(), 1, "fixture expects exactly one `{name}`");
        let mut callers: Vec<String> = facade
            .get_relationships_for_symbol(targets[0].id)
            .unwrap()
            .into_iter()
            .filter(|(_, to, _)| *to == targets[0].id)
            .map(|(from, _, rel)| {
                let from_name = facade
                    .get_symbol(from)
                    .map(|s| s.name.to_string())
                    .unwrap_or_else(|| format!("<{from:?}>"));
                format!("{from_name}:{:?}", rel.kind)
            })
            .collect();
        callers.sort();
        callers
    }
}
