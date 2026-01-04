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
use crate::semantic::{EmbeddingPool, SimpleSemanticSearch};
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
}

/// Statistics for sync operations
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub added_dirs: usize,
    pub removed_dirs: usize,
    pub files_indexed: usize,
    pub symbols_found: usize,
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
    embedding_pool: Option<Arc<EmbeddingPool>>,

    /// Optional fast symbol cache for O(1) lookups
    symbol_cache: Option<Arc<crate::storage::symbol_cache::ConcurrentSymbolCache>>,

    /// Configuration
    settings: Arc<Settings>,

    /// Tracked indexed directories (canonicalized paths)
    indexed_paths: HashSet<PathBuf>,

    /// Base path for index storage
    index_base: PathBuf,
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
            symbol_cache: None,
            settings,
            indexed_paths: HashSet::new(),
            index_base,
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
            symbol_cache: None,
            settings,
            indexed_paths: HashSet::new(),
            index_base,
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

        let model = &self.settings.semantic_search.model;

        let semantic = SimpleSemanticSearch::from_model_name(model)?;
        self.semantic_search = Some(Arc::new(Mutex::new(semantic)));

        // Create embedding pool for parallel generation
        let pool_size = self.settings.semantic_search.embedding_threads;
        let embedding_model = crate::vector::parse_embedding_model(model)
            .map_err(|e| IndexError::General(format!("Failed to parse embedding model: {e}")))?;
        let pool = EmbeddingPool::new(pool_size, embedding_model)?;
        self.embedding_pool = Some(Arc::new(pool));

        Ok(())
    }

    /// Check if semantic search is enabled.
    pub fn has_semantic_search(&self) -> bool {
        self.semantic_search.is_some()
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
    pub fn load_semantic_search(&mut self, path: &Path) -> FacadeResult<bool> {
        if path.join("metadata.json").exists() {
            match SimpleSemanticSearch::load(path) {
                Ok(semantic) => {
                    self.semantic_search = Some(Arc::new(Mutex::new(semantic)));
                    return Ok(true);
                }
                Err(e) => {
                    tracing::warn!("Failed to load semantic search: {e}");
                }
            }
        }
        Ok(false)
    }

    /// Get semantic search embedding count.
    pub fn semantic_search_embedding_count(&self) -> usize {
        self.semantic_search
            .as_ref()
            .map(|s| s.lock().map(|sem| sem.embedding_count()).unwrap_or(0))
            .unwrap_or(0)
    }

    /// Get semantic search metadata.
    pub fn get_semantic_metadata(&self) -> Option<crate::semantic::SemanticMetadata> {
        self.semantic_search
            .as_ref()
            .and_then(|s| s.lock().ok().and_then(|sem| sem.metadata().cloned()))
    }

    // =========================================================================
    // Symbol Query Methods (delegate to DocumentIndex)
    // =========================================================================

    /// Find a symbol by name (uses cache if available for O(1) lookup).
    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        // Try cache first for O(1) lookup
        if let Some(ref cache) = self.symbol_cache {
            if let Some(id) = cache.lookup_by_name(name) {
                return Some(id);
            }
        }

        // Fall back to DocumentIndex query
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

    /// Get all symbols (with limit).
    ///
    /// Returns empty vec on error for SimpleIndexer API compatibility.
    pub fn get_all_symbols(&self) -> Vec<Symbol> {
        self.document_index
            .get_all_symbols(10000)
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
        let results = sem.search_with_language(query, limit, language_filter)?;

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
            .map(|(id, _)| id)
    }

    /// Get file path for a FileId.
    ///
    /// Returns None on error for SimpleIndexer API compatibility.
    pub fn get_file_path(&self, file_id: FileId) -> Option<String> {
        self.document_index.get_file_path(file_id).ok().flatten()
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
    // Symbol Cache Management
    // =========================================================================

    /// Build the symbol cache for fast lookups.
    pub fn build_symbol_cache(&mut self) -> FacadeResult<()> {
        use crate::storage::symbol_cache::{ConcurrentSymbolCache, SymbolHashCache};

        // Clear existing cache to release mmap
        self.symbol_cache = None;

        let cache_path = self.index_base.join("symbol_cache.bin");

        // Get all symbols from index
        let symbols = self.document_index.get_all_symbols(1_000_000)?;

        if symbols.is_empty() {
            return Ok(());
        }

        // Build cache using SymbolHashCache static method
        SymbolHashCache::build_from_symbols(&cache_path, symbols.iter())?;

        // Load for immediate use
        if let Ok(hash_cache) = SymbolHashCache::open(&cache_path) {
            self.symbol_cache = Some(Arc::new(ConcurrentSymbolCache::new(hash_cache)));
        }

        Ok(())
    }

    /// Load existing symbol cache.
    pub fn load_symbol_cache(&mut self) -> FacadeResult<()> {
        use crate::storage::symbol_cache::{ConcurrentSymbolCache, SymbolHashCache};

        let cache_path = self.index_base.join("symbol_cache.bin");

        if cache_path.exists() {
            if let Ok(hash_cache) = SymbolHashCache::open(&cache_path) {
                self.symbol_cache = Some(Arc::new(ConcurrentSymbolCache::new(hash_cache)));
            }
        }

        Ok(())
    }

    /// Clear the symbol cache.
    pub fn clear_symbol_cache(&mut self, delete_file: bool) -> FacadeResult<()> {
        self.symbol_cache = None;

        if delete_file {
            let cache_path = self.index_base.join("symbol_cache.bin");
            if cache_path.exists() {
                std::fs::remove_file(&cache_path)?;
            }
        }

        Ok(())
    }

    // =========================================================================
    // Mutation Methods (delegate to Pipeline)
    // =========================================================================

    /// Index a single file using the parallel pipeline.
    ///
    /// Returns `IndexingResult::Indexed` with the file ID on success.
    pub fn index_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> crate::IndexResult<crate::IndexingResult> {
        let path = path.as_ref();
        let stats = self.pipeline.index_file_single(
            path,
            Arc::clone(&self.document_index),
            self.semantic_search.clone(),
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
            // Remove first to force re-index
            let _ = self.remove_file(path);
        }

        self.index_file(path)
    }

    /// Remove a file from the index.
    ///
    /// Uses the Pipeline's cleanup stage to remove symbols and embeddings.
    pub fn remove_file(&mut self, path: impl AsRef<std::path::Path>) -> crate::IndexResult<()> {
        let path = path.as_ref();
        let semantic_path = self.settings.index_path.join("semantic");

        use crate::indexing::pipeline::stages::CleanupStage;
        let cleanup_stage = if let Some(ref sem) = self.semantic_search {
            CleanupStage::new(Arc::clone(&self.document_index), &semantic_path)
                .with_semantic(Arc::clone(sem))
        } else {
            CleanupStage::new(Arc::clone(&self.document_index), &semantic_path)
        };

        cleanup_stage.cleanup_files(&[path.to_path_buf()])?;
        Ok(())
    }

    /// Index a directory using the parallel pipeline.
    ///
    /// This is the primary indexing entry point using Pipeline.
    pub fn index_directory(&mut self, path: &Path, force: bool) -> FacadeResult<IndexingStats> {
        let stats = self.pipeline.index_incremental(
            path,
            Arc::clone(&self.document_index),
            self.semantic_search.clone(),
            self.embedding_pool.clone(),
            force,
        )?;

        // Update tracked paths
        self.add_indexed_path(path);

        // Rebuild symbol cache after indexing
        self.build_symbol_cache()?;

        Ok(IndexingStats {
            files_indexed: stats.new_files + stats.modified_files,
            symbols_found: stats.index_stats.symbols_found,
            relationships_resolved: stats.phase2_stats.defines_resolved
                + stats.phase2_stats.calls_resolved
                + stats.phase2_stats.other_resolved,
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

        let dir = dir.as_ref();
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

        // Rebuild symbol cache after indexing
        self.build_symbol_cache()?;

        // Convert to IndexStats format using pipeline's actual timing
        let mut stats = IndexStats::default();
        stats.files_indexed = pipeline_stats.new_files + pipeline_stats.modified_files;
        stats.symbols_found = pipeline_stats.index_stats.symbols_found;
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

            // Rebuild cache after each directory
            self.build_symbol_cache()?;
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
