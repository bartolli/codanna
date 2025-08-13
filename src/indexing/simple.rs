//! Tantivy-only implementation of SimpleIndexer
//! This version uses Tantivy as the single source of truth for all data

use crate::indexing::{
    FileWalker, ImportResolver, IndexStats, IndexTransaction, ResolutionContext, TraitResolver,
    calculate_hash, get_utc_timestamp,
};
use crate::parsing::{LanguageId, MethodCall, ParserFactory, get_registry};
use crate::relationship::RelationshipMetadata;
use crate::semantic::SimpleSemanticSearch;
use crate::storage::{DocumentIndex, SearchResult};
use crate::types::SymbolCounter;
use crate::vector::{EmbeddingGenerator, VectorSearchEngine, create_symbol_text};
use crate::{
    FileId, IndexError, IndexResult, RelationKind, Relationship, Settings, Symbol, SymbolId,
    SymbolKind,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Debug print macro that respects the debug setting
macro_rules! debug_print {
    ($self:expr, $($arg:tt)*) => {
        if $self.settings.debug {
            eprintln!("DEBUG: {}", format!($($arg)*));
        }
    };
}

/// Compatibility struct for transaction support
#[derive(Debug)]
pub struct TantivyTransaction;

impl Default for TantivyTransaction {
    fn default() -> Self {
        Self::new()
    }
}

impl TantivyTransaction {
    pub fn new() -> Self {
        Self
    }

    pub fn complete(&mut self) {
        // No-op - Tantivy handles this internally
    }
}

/// Unresolved relationship data
#[derive(Debug, Clone)]
struct UnresolvedRelationship {
    from_name: Arc<str>,
    to_name: Arc<str>,
    file_id: FileId,
    kind: RelationKind,
    #[allow(dead_code)]
    metadata: Option<RelationshipMetadata>,
}

/// The main indexer struct that handles parsing and indexing of source code
pub struct SimpleIndexer {
    parser_factory: ParserFactory,
    import_resolver: ImportResolver,
    trait_resolver: TraitResolver,
    settings: Arc<Settings>,
    document_index: DocumentIndex,
    /// Optional fast symbol cache for O(1) lookups
    symbol_cache: Option<Arc<crate::storage::symbol_cache::ConcurrentSymbolCache>>,
    /// Unresolved relationships to be resolved in a second pass
    unresolved_relationships: Vec<UnresolvedRelationship>,
    /// Variable type information for method resolution
    variable_types: std::collections::HashMap<(FileId, String), String>,
    /// Trait symbols by file for relationship extraction
    trait_symbols_by_file:
        std::collections::HashMap<FileId, std::collections::HashMap<String, crate::SymbolKind>>,
    /// Method calls with rich receiver information for enhanced resolution
    method_calls_by_file: std::collections::HashMap<FileId, Vec<crate::parsing::MethodCall>>,
    /// Optional vector search engine
    vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
    /// Optional embedding generator
    embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
    /// Symbols pending vector processing (SymbolId, symbol_text)
    pending_embeddings: Vec<(SymbolId, String)>,
    /// Optional semantic search for documentation
    semantic_search: Option<Arc<Mutex<SimpleSemanticSearch>>>,
}

impl Default for SimpleIndexer {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleIndexer {
    pub fn new() -> Self {
        let settings = Arc::new(Settings::default());
        Self::with_settings(settings)
    }

    pub fn with_settings(settings: Arc<Settings>) -> Self {
        // Construct the full index path
        let index_base = if let Some(ref workspace_root) = settings.workspace_root {
            // If we have a workspace root, join it with the index path
            workspace_root.join(&settings.index_path)
        } else {
            // Otherwise use the index path as-is (relative to current directory)
            settings.index_path.clone()
        };

        // Tantivy data always goes under index_path/tantivy
        let tantivy_path = index_base.join("tantivy");

        let document_index =
            DocumentIndex::new(tantivy_path).expect("Failed to create Tantivy index");

        let debug = settings.debug;

        // Try to load symbol cache if it exists
        let symbol_cache = None; // Will be loaded lazily when index is opened

        let mut indexer = Self {
            parser_factory: ParserFactory::new(settings.clone()),
            import_resolver: ImportResolver::with_debug(debug),
            trait_resolver: TraitResolver::new(),
            settings,
            document_index,
            symbol_cache,
            unresolved_relationships: Vec::new(),
            variable_types: std::collections::HashMap::new(),
            trait_symbols_by_file: std::collections::HashMap::new(),
            method_calls_by_file: std::collections::HashMap::new(),
            vector_engine: None,
            embedding_generator: None,
            pending_embeddings: Vec::new(),
            semantic_search: None,
        };

        // Reconstruct TraitResolver state from stored relationships
        debug_print!(
            indexer,
            "Reconstructing trait resolver from stored relationships"
        );
        if let Err(e) = indexer.reconstruct_trait_resolver() {
            eprintln!("WARNING: Failed to reconstruct trait resolver: {e}");
        }

        // Try to load symbol cache for fast lookups
        if let Err(e) = indexer.load_symbol_cache() {
            debug_print!(indexer, "Could not load symbol cache: {e}");
        }

        indexer
    }

    /// Create indexer with lazy initialization for faster CLI startup
    pub fn with_settings_lazy(settings: Arc<Settings>, skip_trait_resolver: bool) -> Self {
        // Construct the full index path
        let index_base = if let Some(ref workspace_root) = settings.workspace_root {
            workspace_root.join(&settings.index_path)
        } else {
            settings.index_path.clone()
        };

        // Tantivy data always goes under index_path/tantivy
        let tantivy_path = index_base.join("tantivy");

        let document_index =
            DocumentIndex::new(tantivy_path).expect("Failed to create Tantivy index");

        let debug = settings.debug;

        let mut indexer = Self {
            parser_factory: ParserFactory::new(settings.clone()),
            import_resolver: ImportResolver::with_debug(debug),
            trait_resolver: TraitResolver::new(),
            settings,
            document_index,
            symbol_cache: None,
            unresolved_relationships: Vec::new(),
            variable_types: std::collections::HashMap::new(),
            trait_symbols_by_file: std::collections::HashMap::new(),
            method_calls_by_file: std::collections::HashMap::new(),
            vector_engine: None,
            embedding_generator: None,
            pending_embeddings: Vec::new(),
            semantic_search: None,
        };

        // Only reconstruct trait resolver if needed (for trait-related commands)
        if !skip_trait_resolver {
            debug_print!(
                indexer,
                "Reconstructing trait resolver from stored relationships"
            );
            if let Err(e) = indexer.reconstruct_trait_resolver() {
                eprintln!("WARNING: Failed to reconstruct trait resolver: {e}");
            }
        } else {
            debug_print!(
                indexer,
                "Skipping trait resolver reconstruction for faster startup"
            );
        }

        // Try to load symbol cache for fast lookups
        if let Err(e) = indexer.load_symbol_cache() {
            debug_print!(indexer, "Could not load symbol cache: {e}");
        }

        indexer
    }

    /// Create from loaded data (compatibility method)
    /// With Tantivy-only architecture, this just creates a new instance
    #[deprecated(note = "Use new() or with_settings() instead")]
    pub fn from_data(_data: ()) -> Self {
        Self::new()
    }

    /// Create from loaded data with custom settings (compatibility method)
    #[deprecated(note = "Use with_settings() instead")]
    pub fn from_data_with_settings(_data: (), settings: Arc<Settings>) -> Self {
        Self::with_settings(settings)
    }

    /// Get the settings
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Set the project root for module path calculation
    /// Enable vector search with the given engine and generator
    #[must_use = "Vector search configuration should be used"]
    pub fn with_vector_search(
        mut self,
        vector_engine: VectorSearchEngine,
        embedding_generator: Arc<dyn EmbeddingGenerator>,
    ) -> Self {
        self.vector_engine = Some(Arc::new(Mutex::new(vector_engine)));
        self.embedding_generator = Some(embedding_generator);
        self
    }

    /// Check if vector search is enabled
    #[must_use]
    pub fn has_vector_search(&self) -> bool {
        self.vector_engine.is_some() && self.embedding_generator.is_some()
    }

    /// Enable semantic search for documentation
    pub fn enable_semantic_search(&mut self) -> IndexResult<()> {
        match SimpleSemanticSearch::new() {
            Ok(search) => {
                self.semantic_search = Some(Arc::new(Mutex::new(search)));
                Ok(())
            }
            Err(e) => Err(IndexError::General(format!(
                "Failed to initialize semantic search: {e}"
            ))),
        }
    }

    /// Check if semantic search is enabled
    #[must_use]
    pub fn has_semantic_search(&self) -> bool {
        self.semantic_search.is_some()
    }

    /// Get the number of embeddings in semantic search
    pub fn semantic_search_embedding_count(&self) -> IndexResult<usize> {
        if let Some(semantic) = &self.semantic_search {
            Ok(semantic.lock().unwrap().embedding_count())
        } else {
            Err(IndexError::General(
                "Semantic search is not enabled".to_string(),
            ))
        }
    }

    /// Get semantic search metadata if available
    pub fn get_semantic_metadata(&self) -> Option<crate::semantic::SemanticMetadata> {
        if let Some(semantic) = &self.semantic_search {
            semantic.lock().unwrap().metadata().cloned()
        } else {
            None
        }
    }

    /// Save semantic search data to the given path
    pub fn save_semantic_search(
        &self,
        path: &Path,
    ) -> Result<(), crate::semantic::SemanticSearchError> {
        if let Some(semantic) = &self.semantic_search {
            semantic.lock().unwrap().save(path)
        } else {
            Ok(())
        }
    }

    /// Load and attach semantic search from the given path
    ///
    /// This is used during index loading to restore semantic search state.
    /// Returns Ok(true) if loaded successfully, Ok(false) if no data exists.
    pub fn load_semantic_search(&mut self, path: &Path, info: bool) -> IndexResult<bool> {
        use crate::semantic::{SemanticMetadata, SimpleSemanticSearch};

        // Check if semantic data exists
        if !SemanticMetadata::exists(path) {
            return Ok(false);
        }

        // Try to load semantic search
        match SimpleSemanticSearch::load(path) {
            Ok(semantic) => {
                let count = semantic.embedding_count();
                self.semantic_search = Some(Arc::new(Mutex::new(semantic)));
                if info {
                    eprintln!("Loaded semantic search with {count} embeddings");
                }
                Ok(true)
            }
            Err(e) => {
                // Don't fail the entire load, just warn
                eprintln!("Warning: Could not load semantic search: {e}");
                Ok(false)
            }
        }
    }

    /// Start a batch operation for Tantivy indexing
    pub fn start_tantivy_batch(&self) -> IndexResult<()> {
        self.document_index
            .start_batch()
            .map_err(|e| IndexError::TantivyError {
                operation: "start_batch".to_string(),
                cause: e.to_string(),
            })
    }

    /// Commit the current Tantivy batch
    pub fn commit_tantivy_batch(&mut self) -> IndexResult<()> {
        // First commit Tantivy batch
        self.document_index
            .commit_batch()
            .map_err(|e| IndexError::TantivyError {
                operation: "commit_batch".to_string(),
                cause: e.to_string(),
            })?;

        // Process pending embeddings if vector search is enabled
        match (&self.vector_engine, &self.embedding_generator) {
            (Some(engine), Some(generator)) if !self.pending_embeddings.is_empty() => {
                // Clone the Arc references to avoid borrow checker issues
                let engine = engine.clone();
                let generator = generator.clone();
                self.process_pending_embeddings(&engine, &generator)?;
            }
            _ => {} // No vector support or no pending embeddings
        }

        // Build or update symbol cache after batch commit
        // This happens alongside embedding cache for consistency
        if let Err(e) = self.build_symbol_cache() {
            // Non-fatal: we can continue without cache
            eprintln!("Warning: Failed to build symbol cache: {e}");
        }

        Ok(())
    }

    /// Begin a transaction (compatibility method)
    /// With Tantivy, transactions are handled internally by the batch system
    pub fn begin_transaction(&self) -> IndexTransaction {
        // Return a dummy transaction for compatibility
        IndexTransaction::new(&())
    }

    /// Commit a transaction (compatibility method)
    /// With Tantivy, this just commits the current batch
    pub fn commit_transaction(&mut self, mut transaction: IndexTransaction) -> IndexResult<()> {
        transaction.complete();
        self.commit_tantivy_batch()
    }

    /// Rollback a transaction (compatibility method)
    /// With Tantivy, uncommitted changes are automatically discarded
    pub fn rollback_transaction(&mut self, _transaction: IndexTransaction) {
        // No-op - Tantivy automatically discards uncommitted changes
    }

    /// Get the data for persistence (compatibility method)
    /// This method is no longer needed but kept for API compatibility
    #[deprecated(note = "Data is now stored directly in Tantivy")]
    pub fn data(&self) -> &() {
        &()
    }

    /// Get the data symbol count (compatibility method)
    pub fn data_symbol_count(&self) -> usize {
        self.symbol_count()
    }

    #[must_use = "The result of indexing a file should be checked"]
    pub fn index_file(&mut self, path: impl AsRef<Path>) -> IndexResult<crate::IndexingResult> {
        self.index_file_with_force(path, false)
    }

    #[must_use = "The result of indexing a file should be checked"]
    pub fn index_file_with_force(
        &mut self,
        path: impl AsRef<Path>,
        force: bool,
    ) -> IndexResult<crate::IndexingResult> {
        self.start_tantivy_batch()?;

        match self.index_file_internal(path, force) {
            Ok(result) => {
                self.commit_tantivy_batch()?;
                // Resolve relationships after committing
                self.resolve_cross_file_relationships()?;
                Ok(result)
            }
            Err(e) => {
                // Rollback is automatic - uncommitted changes are discarded
                Err(e)
            }
        }
    }

    /// Index a file without resolving relationships (for batch operations)
    pub fn index_file_no_resolve(
        &mut self,
        path: impl AsRef<Path>,
    ) -> IndexResult<crate::IndexingResult> {
        self.start_tantivy_batch()?;

        match self.index_file_internal(path, false) {
            Ok(result) => {
                self.commit_tantivy_batch()?;
                // Don't resolve relationships - caller will do it after all files
                Ok(result)
            }
            Err(e) => {
                // Rollback is automatic - uncommitted changes are discarded
                Err(e)
            }
        }
    }

    fn index_file_internal(
        &mut self,
        path: impl AsRef<Path>,
        force: bool,
    ) -> IndexResult<crate::IndexingResult> {
        let path = path.as_ref();
        let path_str = path.to_str().ok_or_else(|| IndexError::FileRead {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in path"),
        })?;

        // Read file and calculate hash
        let (content, content_hash) = self.read_file_with_hash(path)?;

        // Check if file already exists by querying Tantivy
        if let Ok(Some((file_id, existing_hash))) = self.document_index.get_file_info(path_str) {
            if !force && existing_hash == content_hash {
                // File hasn't changed, skip re-indexing
                return Ok(crate::IndexingResult::Cached(file_id));
            }

            // File has changed or force re-indexing
            // First, collect symbols that will be removed (for semantic search cleanup)
            let symbols_to_remove = if self.has_semantic_search() {
                self.document_index
                    .find_symbols_by_file(file_id)
                    .ok()
                    .map(|symbols| symbols.into_iter().map(|s| s.id).collect::<Vec<_>>())
            } else {
                None
            };

            // Use remove_file_documents to remove ALL documents for this file path
            self.document_index
                .remove_file_documents(path_str)
                .map_err(|e| IndexError::TantivyError {
                    operation: "remove_file_documents".to_string(),
                    cause: e.to_string(),
                })?;

            // Remove embeddings for the old symbols if semantic search is enabled
            if let Some(symbol_ids) = symbols_to_remove {
                if let Some(semantic) = &self.semantic_search {
                    semantic.lock().unwrap().remove_embeddings(&symbol_ids);
                }
            }
        }

        // Register or update file
        let file_id = self.register_file(path_str, content_hash)?;

        // Index the file content
        self.reindex_file_content(path, path_str, file_id, &content)?;

        Ok(crate::IndexingResult::Indexed(file_id))
    }

    /// Remove a file and all its symbols from the index
    pub fn remove_file(&mut self, path: impl AsRef<Path>) -> IndexResult<()> {
        let path = path.as_ref();
        let path_display = path.display();
        eprintln!("  remove_file called with path: {path_display}");
        let path_str = path.to_str().ok_or_else(|| IndexError::FileRead {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid UTF-8 in path"),
        })?;
        eprintln!("  Querying index for path: '{path_str}'");

        // Get the FileId for this file path
        let file_info =
            self.document_index
                .get_file_info(path_str)
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_file_info".to_string(),
                    cause: e.to_string(),
                })?;

        eprintln!("  get_file_info result: {file_info:?}");

        let symbols_to_remove = if let Some(info) = file_info {
            // Get all symbols for this file before removing
            self.document_index
                .find_symbols_by_file(info.0)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file".to_string(),
                    cause: e.to_string(),
                })?
        } else {
            // File not in index, nothing to remove
            eprintln!("  File not found in index: {path_str}");
            return Ok(());
        };

        // Remove ALL documents for this file from Tantivy
        self.document_index
            .remove_file_documents(path_str)
            .map_err(|e| IndexError::TantivyError {
                operation: "remove_file_documents".to_string(),
                cause: e.to_string(),
            })?;

        // Remove embeddings for the symbols if semantic search is enabled
        if !symbols_to_remove.is_empty() {
            if let Some(semantic) = &self.semantic_search {
                let symbol_ids: Vec<SymbolId> = symbols_to_remove.iter().map(|s| s.id).collect();
                semantic.lock().unwrap().remove_embeddings(&symbol_ids);
            }
        }

        let symbol_count = symbols_to_remove.len();
        eprintln!("  Removed {symbol_count} symbols from {path_str}");

        // Commit the changes to persist them
        self.document_index
            .commit_batch()
            .map_err(|e| IndexError::TantivyError {
                operation: "commit after removal".to_string(),
                cause: e.to_string(),
            })?;
        eprintln!("  Changes committed to index");

        // Rebuild symbol cache after file removal to remove stale entries
        if let Err(e) = self.build_symbol_cache() {
            eprintln!("Warning: Failed to rebuild symbol cache after file removal: {e}");
        }

        Ok(())
    }

    /// Read file content and calculate its hash
    fn read_file_with_hash(&self, path: &Path) -> IndexResult<(String, String)> {
        let content = fs::read_to_string(path).map_err(|e| IndexError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;

        let hash = calculate_hash(&content);
        Ok((content, hash))
    }

    /// Register a new file in the index
    fn register_file(&mut self, path_str: &str, content_hash: String) -> IndexResult<FileId> {
        // Get next file ID from Tantivy
        let file_counter =
            self.document_index
                .get_next_file_id()
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_next_file_id".to_string(),
                    cause: e.to_string(),
                })?;

        let file_id = FileId::new(file_counter).ok_or(IndexError::FileIdExhausted)?;

        // Update the file counter for next use
        self.document_index
            .store_metadata(
                crate::storage::MetadataKey::FileCounter,
                file_counter as u64,
            )
            .map_err(|e| IndexError::TantivyError {
                operation: "store_metadata".to_string(),
                cause: e.to_string(),
            })?;

        let timestamp = get_utc_timestamp();

        // Store file info in Tantivy
        self.document_index
            .store_file_info(file_id, path_str, &content_hash, timestamp)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_file_info".to_string(),
                cause: e.to_string(),
            })?;

        Ok(file_id)
    }

    /// Index or re-index file content
    fn reindex_file_content(
        &mut self,
        path: &Path,
        path_str: &str,
        file_id: FileId,
        content: &str,
    ) -> IndexResult<FileId> {
        debug_print!(
            self,
            "reindex_file_content called with path: {:?} (absolute: {})",
            path,
            path.is_absolute()
        );
        let language_id = self.detect_language(path)?;
        let parser_with_behavior = self.create_parser_with_behavior(language_id)?;
        let mut parser = parser_with_behavior.parser;
        let behavior = parser_with_behavior.behavior;
        let module_path = self.calculate_module_path(path);

        // Register the file with ImportResolver
        if let Some(ref mod_path) = module_path {
            debug_print!(
                self,
                "Registering file {:?} with module path: {}",
                path,
                mod_path
            );
            self.import_resolver
                .register_file(path.to_path_buf(), file_id, mod_path.clone());
        } else {
            debug_print!(self, "No module path for file {:?}", path);
        }

        let mut symbol_counter = self.get_next_symbol_counter()?;
        self.extract_and_store_symbols(
            &mut parser,
            content,
            file_id,
            path_str,
            &module_path,
            behavior.as_ref(),
            &mut symbol_counter,
        )?;
        self.extract_and_store_relationships(&mut parser, content, file_id, behavior.as_ref())?;
        self.update_symbol_counter(&symbol_counter)?;

        Ok(file_id)
    }

    /// Detect the programming language from file extension using the registry
    fn detect_language(&self, path: &Path) -> IndexResult<LanguageId> {
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        // Use the registry for language detection
        let registry = get_registry();
        let registry = registry
            .lock()
            .map_err(|e| IndexError::General(format!("Failed to acquire registry lock: {e}")))?;

        registry
            .get_by_extension(extension)
            .map(|def| def.id())
            .ok_or_else(|| IndexError::UnsupportedFileType {
                path: path.to_path_buf(),
                extension: extension.to_string(),
            })
    }

    /// Create a parser with its language-specific behavior
    fn create_parser_with_behavior(
        &self,
        language_id: LanguageId,
    ) -> IndexResult<crate::parsing::ParserWithBehavior> {
        // Use the registry-based method
        self.parser_factory
            .create_parser_with_behavior_from_registry(language_id)
    }

    /// Calculate module path relative to workspace root
    /// This is language-agnostic - just returns the relative path
    /// Language-specific parsers can override this in the symbol's module_path
    fn calculate_module_path(&self, path: &Path) -> Option<String> {
        // Use workspace_root from settings, or fall back to indexing.project_root
        let root = self.settings.workspace_root.as_ref().or(self
            .settings
            .indexing
            .project_root
            .as_ref())?;

        debug_print!(
            self,
            "Calculating module path for {:?} with root {:?}",
            path,
            root
        );

        // Use ImportResolver's module_path_from_file for Rust files
        let module_path = if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            // Make path absolute if it's relative
            let abs_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };
            let result = ImportResolver::module_path_from_file(&abs_path, root);
            debug_print!(self, "ImportResolver returned: {:?}", result);
            result
        } else {
            // For other languages, just return the relative path
            path.canonicalize()
                .ok()?
                .strip_prefix(root.canonicalize().ok()?)
                .ok()
                .and_then(|relative_path| relative_path.to_str().map(|s| s.to_string()))
        };

        debug_print!(self, "Module path for {:?}: {:?}", path, module_path);
        module_path
    }

    /// Get the next symbol counter from Tantivy
    fn get_next_symbol_counter(&self) -> IndexResult<SymbolCounter> {
        let next_id =
            self.document_index
                .get_next_symbol_id()
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_next_symbol_id".to_string(),
                    cause: e.to_string(),
                })?;

        // Create a counter starting from the next available ID
        // If next_id is 0 (shouldn't happen), start from 1
        let start_value = if next_id == 0 { 1 } else { next_id };
        Ok(SymbolCounter::from_value(start_value))
    }

    /// Extract symbols from content and store them in Tantivy
    fn extract_and_store_symbols(
        &mut self,
        parser: &mut Box<dyn crate::parsing::LanguageParser>,
        content: &str,
        file_id: FileId,
        path_str: &str,
        module_path: &Option<String>,
        behavior: &dyn crate::parsing::LanguageBehavior,
        symbol_counter: &mut SymbolCounter,
    ) -> IndexResult<()> {
        let symbols = parser.parse(content, file_id, symbol_counter);

        // Extract and register imports
        let imports = parser.find_imports(content, file_id);
        if !imports.is_empty() {
            debug_print!(
                self,
                "Found {} imports in file {:?}",
                imports.len(),
                file_id
            );
            for import in &imports {
                debug_print!(
                    self,
                    "  - Import: {} (alias: {:?}, glob: {})",
                    import.path,
                    import.alias,
                    import.is_glob
                );
            }
        }
        for import in imports {
            self.import_resolver.add_import(import);
        }

        // Track traits for later use in relationship extraction
        let mut trait_symbols: std::collections::HashMap<String, crate::SymbolKind> =
            std::collections::HashMap::new();

        for mut symbol in symbols {
            // Track trait symbols
            trait_symbols.insert(symbol.name.to_string(), symbol.kind);

            self.configure_symbol(&mut symbol, module_path, behavior);
            self.store_symbol(symbol, path_str)?;
        }

        // Store trait symbols for this file
        self.trait_symbols_by_file.insert(file_id, trait_symbols);

        Ok(())
    }

    /// Configure a symbol with module path and visibility
    ///
    /// TODO: Extract language-specific module path resolution into a dedicated trait.
    /// Current implementation violates single responsibility principle by embedding
    /// language-specific logic within the indexer. Each language parser should implement
    /// a `ModulePathResolver` trait that encapsulates its namespace/module conventions.
    /// This would eliminate the need for language enum matching and enable proper
    /// plugin architecture for new language support without modifying core indexing logic.
    fn configure_symbol(
        &self,
        symbol: &mut crate::Symbol,
        module_path: &Option<String>,
        behavior: &dyn crate::parsing::LanguageBehavior,
    ) {
        // Set module path if available
        if symbol.module_path.is_none() {
            if let Some(mod_path) = module_path {
                // Use behavior to format the module path according to language conventions
                let full_path = behavior.format_module_path(mod_path, &symbol.name);
                symbol.module_path = Some(full_path.into());
                debug_print!(
                    self,
                    "Set module path for symbol '{}': {:?}",
                    symbol.name,
                    symbol.module_path
                );
            }
        } else {
            debug_print!(
                self,
                "Symbol '{}' already has module path: {:?}",
                symbol.name,
                symbol.module_path
            );
        }

        // Parse visibility using language-specific behavior
        if let Some(sig) = &symbol.signature {
            symbol.visibility = behavior.parse_visibility(sig);
        }
    }

    /// Store a single symbol in Tantivy
    fn store_symbol(&mut self, symbol: crate::Symbol, path_str: &str) -> IndexResult<()> {
        // Index doc comment for semantic search if enabled
        if let (Some(semantic), Some(doc)) = (&self.semantic_search, &symbol.doc_comment) {
            if let Err(e) = semantic.lock().unwrap().index_doc_comment(symbol.id, doc) {
                eprintln!(
                    "WARNING: Failed to index doc comment for symbol {}: {}",
                    symbol.name, e
                );
            }
        }

        // Store the symbol in Tantivy
        self.document_index
            .index_symbol(&symbol, path_str)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_symbol".to_string(),
                cause: e.to_string(),
            })?;

        // If vector support is enabled, prepare for embedding
        if self.vector_engine.is_some() && self.embedding_generator.is_some() {
            let symbol_text =
                create_symbol_text(&symbol.name, symbol.kind, symbol.signature.as_deref());
            self.pending_embeddings.push((symbol.id, symbol_text));
        }

        Ok(())
    }

    /// Extract relationships from content and store them
    fn extract_and_store_relationships(
        &mut self,
        parser: &mut Box<dyn crate::parsing::LanguageParser>,
        content: &str,
        file_id: FileId,
        behavior: &dyn crate::parsing::LanguageBehavior,
    ) -> IndexResult<()> {
        // 1. Function/method calls
        let method_calls: Vec<MethodCall> = parser.find_method_calls(content);
        debug_print!(
            self,
            "Found {} method calls in file {:?}",
            method_calls.len(),
            file_id
        );

        // Debug: Show enhanced method call information before conversion
        for method_call in &method_calls {
            if let Some(receiver) = &method_call.receiver {
                if method_call.is_static {
                    debug_print!(
                        self,
                        "Static call: {}::{} in {}",
                        receiver,
                        method_call.method_name,
                        method_call.caller
                    );
                } else if receiver == "self" {
                    debug_print!(
                        self,
                        "Self call: self.{} in {}",
                        method_call.method_name,
                        method_call.caller
                    );
                } else {
                    debug_print!(
                        self,
                        "Instance call: {}.{} in {} (receiver will be lost in current format)",
                        receiver,
                        method_call.method_name,
                        method_call.caller
                    );
                }
            } else {
                debug_print!(
                    self,
                    "Plain call: {} in {}",
                    method_call.method_name,
                    method_call.caller
                );
            }
        }

        // Process method calls using MethodCall objects for enhanced resolution
        debug_print!(self, "Processing {} method calls", method_calls.len());
        for method_call in &method_calls {
            debug_print!(
                self,
                "Processing call: {} -> {}",
                method_call.caller,
                method_call.method_name
            );
            debug_print!(
                self,
                "Processing method call with enhanced data: {} -> {}",
                method_call.caller,
                method_call.method_name
            );

            // Store MethodCall for enhanced resolution during symbol resolution phase
            self.store_method_call_for_resolution(method_call, file_id);

            // Create metadata to store receiver information
            let metadata = method_call.receiver.as_ref().map(|receiver| {
                RelationshipMetadata::new()
                    .at_position(method_call.range.start_line, method_call.range.start_column)
                    .with_context(format!(
                        "receiver:{},static:{}",
                        receiver, method_call.is_static
                    ))
            });

            self.add_relationships_by_name(
                &method_call.caller,
                &method_call.method_name,
                file_id,
                RelationKind::Calls,
                metadata,
            )?;
        }

        // 2. Trait implementations
        let implementations = parser.find_implementations(content);
        for (type_name, trait_name, _range) in implementations {
            debug_print!(
                self,
                "Registering implementation: {} implements {}",
                type_name,
                trait_name
            );
            // Register with trait resolver
            self.trait_resolver.add_trait_impl(
                type_name.to_string(),
                trait_name.to_string(),
                file_id,
            );
            self.add_relationships_by_name(
                type_name,
                trait_name,
                file_id,
                RelationKind::Implements,
                None,
            )?;
        }

        // 2.5. Inherent methods (for complex method resolution)
        // TODO: Stage 4 will fix the trait signature to return Vec<(String, String, Range)>
        // For now, we'll use the trait method directly and handle the borrowing issue
        if behavior.supports_inherent_methods() {
            let inherent_methods = parser.find_inherent_methods(content);
            if !inherent_methods.is_empty() {
                // Group methods by type
                let mut methods_by_type: std::collections::HashMap<String, Vec<String>> =
                    std::collections::HashMap::new();
                for (type_name, method_name, _range) in inherent_methods {
                    debug_print!(
                        self,
                        "Found inherent method: {}::{}",
                        type_name,
                        method_name
                    );
                    methods_by_type
                        .entry(type_name.to_string())
                        .or_default()
                        .push(method_name.to_string());
                }

                // Register with trait resolver
                for (type_name, methods) in methods_by_type {
                    self.trait_resolver.add_inherent_methods(type_name, methods);
                }
            }
        }

        // 3. Type usage (in fields, parameters, returns)
        let uses = parser.find_uses(content);
        for (context_name, used_type, _range) in uses {
            self.add_relationships_by_name(
                context_name,
                used_type,
                file_id,
                RelationKind::Uses,
                None,
            )?;
        }

        // 4. Method definitions (trait defines methods)
        let defines = parser.find_defines(content);
        debug_print!(
            self,
            "Found {} defines for file {:?}",
            defines.len(),
            file_id
        );
        for (definer_name, method_name, _range) in defines {
            debug_print!(
                self,
                "Processing define: {} defines {}",
                definer_name,
                method_name
            );
            // Check if definer is a trait using our cached symbol kinds
            if let Some(symbol_kinds) = self.trait_symbols_by_file.get(&file_id) {
                debug_print!(self, "Found {} symbol kinds for file", symbol_kinds.len());
                // HashMap<String, _> can look up &str keys directly
                if let Some(kind) = symbol_kinds.get(definer_name) {
                    debug_print!(self, "Found kind {:?} for definer {}", kind, definer_name);
                    if *kind == crate::types::SymbolKind::Trait {
                        debug_print!(
                            self,
                            "Adding method '{}' to trait '{}'",
                            method_name,
                            definer_name
                        );
                        // Update trait resolver with method info
                        let existing_methods = self
                            .trait_resolver
                            .get_trait_methods(definer_name)
                            .unwrap_or_default();
                        let mut methods = existing_methods;
                        let method_name_str = method_name.to_string();
                        if !methods.contains(&method_name_str) {
                            methods.push(method_name_str);
                            self.trait_resolver
                                .add_trait_methods(definer_name.to_string(), methods);
                        }
                    }
                }
            }
            self.add_relationships_by_name(
                definer_name,
                method_name,
                file_id,
                RelationKind::Defines,
                None,
            )?;
        }

        // Variable type tracking for method resolution
        let var_types = parser.find_variable_types(content);
        for (var_name, type_name, _range) in var_types {
            self.variable_types
                .insert((file_id, var_name.to_string()), type_name.to_string());
        }

        Ok(())
    }

    /// Update the symbol counter in Tantivy metadata
    fn update_symbol_counter(&mut self, symbol_counter: &SymbolCounter) -> IndexResult<()> {
        self.document_index
            .store_metadata(
                crate::storage::MetadataKey::SymbolCounter,
                symbol_counter.current_count() as u64,
            )
            .map_err(|e| IndexError::TantivyError {
                operation: "store_metadata".to_string(),
                cause: e.to_string(),
            })
    }

    /// Check if two symbols are in the same module
    fn symbols_in_same_module(sym1: &Symbol, sym2: &Symbol) -> bool {
        match (&sym1.module_path, &sym2.module_path) {
            (Some(path1), Some(path2)) => path1 == path2,
            // If either symbol lacks module info, we can't determine
            _ => false,
        }
    }

    /// Check if a symbol is visible from another symbol's context
    fn is_symbol_visible_from(target: &Symbol, from: &Symbol) -> bool {
        use crate::Visibility;

        // Same module = always visible
        if Self::symbols_in_same_module(target, from) {
            return true;
        }

        // Different modules = target must be public
        target.visibility == Visibility::Public
    }

    /// TODO: Implement module proximity scoring for relationship resolution
    ///
    /// Purpose: Improve relationship resolution accuracy by preferring symbols
    /// in closer modules when multiple candidates exist.
    ///
    /// Description: This method calculates the proximity between two module paths
    /// to help disambiguate symbol references. When resolving relationships like
    /// function calls, symbols in the same or nearby modules should be preferred
    /// over distant ones.
    ///
    /// Returns:
    /// - 0: Same module (highest priority)
    /// - 1: Parent/child relationship
    /// - 2: Sibling modules (same parent)
    /// - 3+: More distant relationships
    ///
    /// Reference: See FIX_PLAN_RELATIONSHIPS.md for full implementation details
    #[allow(dead_code)]
    fn module_proximity(path1: Option<&str>, path2: Option<&str>) -> u32 {
        match (path1, path2) {
            (Some(p1), Some(p2)) => {
                if p1 == p2 {
                    return 0; // Same module
                }

                // Check if one is parent of the other
                if p1.starts_with(p2) || p2.starts_with(p1) {
                    return 1; // Parent/child relationship
                }

                // Check if they're siblings (same parent)
                let parts1: Vec<&str> = p1.split("::").collect();
                let parts2: Vec<&str> = p2.split("::").collect();

                if parts1.len() > 1 && parts2.len() > 1 {
                    // Compare parent paths
                    let parent1 = &parts1[..parts1.len() - 1].join("::");
                    let parent2 = &parts2[..parts2.len() - 1].join("::");

                    if parent1 == parent2 {
                        return 2; // Siblings
                    }
                }

                // Otherwise, they're distant
                3
            }
            // Missing module info = assume distant
            _ => 4,
        }
    }

    /// Check if a relationship between two symbol kinds is valid
    /// This is designed to be language-agnostic and permissive
    fn is_compatible_relationship(
        from_kind: crate::SymbolKind,
        to_kind: crate::SymbolKind,
        rel_kind: crate::RelationKind,
    ) -> bool {
        use crate::RelationKind::*;
        use crate::SymbolKind::*;

        match rel_kind {
            Calls | CalledBy => {
                // Executable code can call other executable code
                let callable = |k: &crate::SymbolKind| matches!(k, Function | Method | Macro);
                callable(&from_kind) && callable(&to_kind)
            }
            Implements | ImplementedBy => {
                // Types can implement interfaces/traits
                let implementor = |k: &crate::SymbolKind| matches!(k, Struct | Enum | Class);
                let interface = |k: &crate::SymbolKind| matches!(k, Trait | Interface);

                match rel_kind {
                    Implements => implementor(&from_kind) && interface(&to_kind),
                    ImplementedBy => interface(&from_kind) && implementor(&to_kind),
                    _ => unreachable!(),
                }
            }
            Uses | UsedBy => {
                // Most symbols can use/reference types and values
                // Be permissive here as different languages have different rules
                let can_use = |k: &crate::SymbolKind| {
                    matches!(
                        k,
                        Function | Method | Struct | Class | Trait | Interface | Module | Enum
                    )
                };
                let can_be_used = |k: &crate::SymbolKind| {
                    matches!(
                        k,
                        Struct
                            | Enum
                            | Class
                            | Trait
                            | Interface
                            | TypeAlias
                            | Constant
                            | Variable
                            | Function
                            | Method
                    )
                };

                match rel_kind {
                    Uses => can_use(&from_kind) && can_be_used(&to_kind),
                    UsedBy => can_be_used(&from_kind) && can_use(&to_kind),
                    _ => unreachable!(),
                }
            }
            Defines | DefinedIn => {
                // Containers can define members
                let container = |k: &crate::SymbolKind| {
                    matches!(k, Trait | Interface | Module | Struct | Enum | Class)
                };
                let member = |k: &crate::SymbolKind| {
                    matches!(k, Method | Function | Constant | Field | Variable)
                };

                match rel_kind {
                    Defines => container(&from_kind) && member(&to_kind),
                    DefinedIn => member(&from_kind) && container(&to_kind),
                    _ => unreachable!(),
                }
            }
            Extends | ExtendedBy => {
                // Types can extend other types (inheritance)
                let extendable = |k: &crate::SymbolKind| matches!(k, Class | Interface | Trait);
                extendable(&from_kind) && extendable(&to_kind)
            }
            References | ReferencedBy => {
                // Very permissive - almost anything can reference anything
                // This is a catch-all for general references
                true
            }
        }
    }

    /// Add a relationship to Tantivy
    fn add_relationship_internal(
        &mut self,
        from: SymbolId,
        to: SymbolId,
        rel: Relationship,
    ) -> IndexResult<()> {
        self.document_index
            .store_relationship(from, to, &rel)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_relationship".to_string(),
                cause: e.to_string(),
            })
    }

    /// Helper method to add relationships by symbol names
    /// Stores them as unresolved for later processing with import context
    fn add_relationships_by_name(
        &mut self,
        from_name: &str,
        to_name: &str,
        file_id: FileId,
        kind: RelationKind,
        metadata: Option<RelationshipMetadata>,
    ) -> IndexResult<()> {
        // Store as unresolved for later resolution when all symbols are committed
        // This allows us to:
        // 1. Wait until all symbols in the batch are searchable
        // 2. Use import context for accurate resolution
        debug_print!(
            self,
            "Adding unresolved relationship: {} -> {} (kind: {:?})",
            from_name,
            to_name,
            kind
        );
        self.unresolved_relationships.push(UnresolvedRelationship {
            from_name: from_name.into(),
            to_name: to_name.into(),
            file_id,
            kind,
            metadata,
        });

        Ok(())
    }

    // Query methods using Tantivy

    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        // Try cache first for O(1) lookup
        if let Some(ref cache) = self.symbol_cache {
            if let Some(id) = cache.lookup_by_name(name) {
                debug_print!(self, "Symbol '{}' found in cache (fast path)", name);
                return Some(id);
            }
            debug_print!(
                self,
                "Symbol '{}' not in cache, falling back to Tantivy",
                name
            );
        } else {
            debug_print!(self, "Symbol cache not loaded");
        }

        // Fallback to Tantivy
        self.document_index
            .find_symbols_by_name(name)
            .ok()
            .and_then(|symbols| symbols.first().map(|s| s.id))
    }

    pub fn find_symbols_by_name(&self, name: &str) -> Vec<Symbol> {
        // For now, still use Tantivy for full symbol retrieval
        // Cache only helps with ID lookups
        self.document_index
            .find_symbols_by_name(name)
            .unwrap_or_default()
    }

    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        self.document_index.find_symbol_by_id(id).ok().flatten()
    }

    pub fn get_called_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        // Query relationships where from_symbol_id = symbol_id and kind = Calls
        self.document_index
            .get_relationships_from(symbol_id, RelationKind::Calls)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(_, to_id, _)| self.get_symbol(to_id))
            .collect()
    }

    /// Returns called functions with receiver metadata for enhanced method call analysis.
    ///
    /// Provides receiver information (instance/static) from stored relationship metadata.
    pub fn get_called_functions_with_metadata(
        &self,
        symbol_id: SymbolId,
    ) -> Vec<(Symbol, Option<String>)> {
        self.document_index
            .get_relationships_from(symbol_id, RelationKind::Calls)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(_, to_id, rel)| {
                self.get_symbol(to_id).map(|symbol| {
                    // Extract receiver info from metadata context
                    let receiver_info = rel
                        .metadata
                        .and_then(|m| m.context)
                        .map(|ctx| ctx.to_string());
                    (symbol, receiver_info)
                })
            })
            .collect()
    }

    pub fn get_calling_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        // Query relationships where to_symbol_id = symbol_id and kind = Calls
        self.document_index
            .get_relationships_to(symbol_id, RelationKind::Calls)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(from_id, _, _)| self.get_symbol(from_id))
            .collect()
    }

    /// Returns calling functions with receiver metadata for enhanced method call analysis.
    ///
    /// Provides receiver information (instance/static) from stored relationship metadata.
    pub fn get_calling_functions_with_metadata(
        &self,
        symbol_id: SymbolId,
    ) -> Vec<(Symbol, Option<String>)> {
        self.document_index
            .get_relationships_to(symbol_id, RelationKind::Calls)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(from_id, _, rel)| {
                self.get_symbol(from_id).map(|symbol| {
                    // Extract receiver info from metadata context
                    let receiver_info = rel
                        .metadata
                        .and_then(|m| m.context)
                        .map(|ctx| ctx.to_string());
                    (symbol, receiver_info)
                })
            })
            .collect()
    }

    /// Get comprehensive context for a symbol including all relationships.
    ///
    /// Aggregates symbol data with configurable relationship information.
    pub fn get_symbol_context(
        &self,
        symbol_id: SymbolId,
        include: crate::symbol::context::ContextIncludes,
    ) -> Option<crate::symbol::context::SymbolContext> {
        use crate::symbol::context::{SymbolContext, SymbolRelationships};

        let symbol = self.get_symbol(symbol_id)?;
        let file_path = self
            .get_file_path(symbol.file_id)
            .unwrap_or_else(|| "<unknown>".to_string());

        let mut relationships = SymbolRelationships::default();

        // Load requested relationships using existing methods
        if include.contains(crate::symbol::context::ContextIncludes::IMPLEMENTATIONS) {
            match symbol.kind {
                SymbolKind::Trait => {
                    relationships.implemented_by = Some(self.get_implementations(symbol_id));
                }
                _ => {
                    // For types, find what traits they implement
                    // This would use existing relationship queries
                    let impls = self
                        .document_index
                        .get_relationships_from(symbol_id, RelationKind::Implements)
                        .ok()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|(_, to_id, _)| self.get_symbol(to_id))
                        .collect::<Vec<_>>();
                    if !impls.is_empty() {
                        relationships.implements = Some(impls);
                    }
                }
            }
        }

        if include.contains(crate::symbol::context::ContextIncludes::DEFINITIONS) {
            let deps = self.get_dependencies(symbol_id);
            if let Some(defines) = deps.get(&RelationKind::Defines) {
                relationships.defines = Some(defines.clone());
            }
        }

        if include.contains(crate::symbol::context::ContextIncludes::CALLS) {
            let calls = self.get_called_functions_with_metadata(symbol_id);
            if !calls.is_empty() {
                relationships.calls = Some(calls);
            }
        }

        if include.contains(crate::symbol::context::ContextIncludes::CALLERS) {
            let callers = self.get_calling_functions_with_metadata(symbol_id);
            if !callers.is_empty() {
                relationships.called_by = Some(callers);
            }
        }

        Some(SymbolContext {
            symbol,
            file_path,
            relationships,
        })
    }

    pub fn get_implementations(&self, trait_id: SymbolId) -> Vec<Symbol> {
        // Query relationships where to_symbol_id = trait_id and kind = Implements
        self.document_index
            .get_relationships_to(trait_id, RelationKind::Implements)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(from_id, _, _)| self.get_symbol(from_id))
            .collect()
    }

    pub fn get_all_symbols(&self) -> Vec<Symbol> {
        self.document_index
            .get_all_symbols(10000)
            .unwrap_or_default()
    }

    /// Get all dependencies of a symbol (what it depends on)
    pub fn get_dependencies(
        &self,
        symbol_id: SymbolId,
    ) -> std::collections::HashMap<RelationKind, Vec<Symbol>> {
        use std::collections::HashMap;
        let mut deps = HashMap::new();

        // Get all outgoing relationships
        for kind in &[
            RelationKind::Calls,
            RelationKind::Uses,
            RelationKind::Implements,
            RelationKind::Defines,
        ] {
            let symbols = self
                .document_index
                .get_relationships_from(symbol_id, *kind)
                .ok()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(_, to_id, _)| self.get_symbol(to_id))
                .collect::<Vec<_>>();

            if !symbols.is_empty() {
                deps.insert(*kind, symbols);
            }
        }

        deps
    }

    /// Get all dependents of a symbol (what depends on it)
    pub fn get_dependents(
        &self,
        symbol_id: SymbolId,
    ) -> std::collections::HashMap<RelationKind, Vec<Symbol>> {
        use std::collections::HashMap;
        let mut deps = HashMap::new();

        // Get all incoming relationships (skip Defines as it's not a true dependency)
        for kind in &[
            RelationKind::Calls,
            RelationKind::Uses,
            RelationKind::Implements,
        ] {
            let symbols = self
                .document_index
                .get_relationships_to(symbol_id, *kind)
                .ok()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(from_id, _, _)| self.get_symbol(from_id))
                .collect::<Vec<_>>();

            if !symbols.is_empty() {
                deps.insert(*kind, symbols);
            }
        }

        deps
    }

    /// Get impact radius - all symbols that would be affected by changing a symbol
    /// This is a simplified version that finds direct dependents only
    pub fn get_impact_radius(
        &self,
        symbol_id: SymbolId,
        max_depth: Option<usize>,
    ) -> Vec<SymbolId> {
        use std::collections::{HashSet, VecDeque};

        let depth = max_depth.unwrap_or(2); // Default depth of 2
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        let mut queue = VecDeque::new();

        // Start with the given symbol at depth 0
        queue.push_back((symbol_id, 0));
        visited.insert(symbol_id);

        while let Some((current_id, current_depth)) = queue.pop_front() {
            // Don't include the starting symbol in results
            if current_id != symbol_id {
                result.push(current_id);
            }

            // Stop if we've reached max depth
            if current_depth >= depth {
                continue;
            }

            // Find all symbols that depend on the current symbol
            for kind in &[
                RelationKind::Calls,
                RelationKind::Uses,
                RelationKind::Implements,
            ] {
                if let Ok(relationships) =
                    self.document_index.get_relationships_to(current_id, *kind)
                {
                    for (from_id, _, _) in relationships {
                        if visited.insert(from_id) {
                            queue.push_back((from_id, current_depth + 1));
                        }
                    }
                }
            }
        }

        result
    }

    pub fn symbol_count(&self) -> usize {
        self.document_index.count_symbols().unwrap_or(0)
    }

    /// Get import resolver for testing
    #[cfg(test)]
    pub fn import_resolver(&self) -> &ImportResolver {
        &self.import_resolver
    }

    pub fn get_symbols_by_file(&self, file_id: FileId) -> Vec<Symbol> {
        self.document_index
            .find_symbols_by_file(file_id)
            .unwrap_or_default()
    }

    pub fn file_count(&self) -> u32 {
        self.document_index.count_files().unwrap_or(0) as u32
    }

    pub fn relationship_count(&self) -> usize {
        self.document_index.count_relationships().unwrap_or(0)
    }

    pub fn get_file_path(&self, file_id: FileId) -> Option<String> {
        self.document_index.get_file_path(file_id).ok().flatten()
    }

    /// Get all indexed file paths - used by file watcher
    pub fn get_all_indexed_paths(&self) -> Vec<PathBuf> {
        self.document_index
            .get_all_indexed_paths()
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to get indexed paths: {e}");
                Vec::new()
            })
    }

    /// Search documentation using natural language query
    /// Returns symbols with their similarity scores, sorted by relevance
    pub fn semantic_search_docs(
        &self,
        query: &str,
        limit: usize,
    ) -> IndexResult<Vec<(Symbol, f32)>> {
        let semantic = self.semantic_search.as_ref().ok_or_else(|| {
            IndexError::General(
                "Semantic search is not enabled. Call enable_semantic_search() first.".to_string(),
            )
        })?;

        let results = semantic
            .lock()
            .unwrap()
            .search(query, limit)
            .map_err(|e| IndexError::General(format!("Semantic search failed: {e}")))?;

        // Convert SymbolIds to Symbols
        let mut symbol_results = Vec::with_capacity(results.len());
        for (symbol_id, score) in results {
            if let Some(symbol) = self.get_symbol(symbol_id) {
                symbol_results.push((symbol, score));
            }
        }

        Ok(symbol_results)
    }

    /// Search documentation with similarity threshold
    pub fn semantic_search_docs_with_threshold(
        &self,
        query: &str,
        limit: usize,
        threshold: f32,
    ) -> IndexResult<Vec<(Symbol, f32)>> {
        let semantic = self.semantic_search.as_ref().ok_or_else(|| {
            IndexError::General(
                "Semantic search is not enabled. Call enable_semantic_search() first.".to_string(),
            )
        })?;

        let results = semantic
            .lock()
            .unwrap()
            .search_with_threshold(query, limit, threshold)
            .map_err(|e| IndexError::General(format!("Semantic search failed: {e}")))?;

        // Convert SymbolIds to Symbols
        let mut symbol_results = Vec::with_capacity(results.len());
        for (symbol_id, score) in results {
            if let Some(symbol) = self.get_symbol(symbol_id) {
                symbol_results.push((symbol, score));
            }
        }

        Ok(symbol_results)
    }

    /// Clear the Tantivy index
    pub fn clear_tantivy_index(&mut self) -> IndexResult<()> {
        // Clear trait resolver data as well
        self.trait_resolver.clear();
        self.trait_symbols_by_file.clear();
        self.variable_types.clear();

        // Clear semantic search if enabled
        if let Some(ref semantic) = self.semantic_search {
            semantic.lock().unwrap().clear();
        }

        self.document_index
            .clear()
            .map_err(|e| IndexError::TantivyError {
                operation: "clear_index".to_string(),
                cause: e.to_string(),
            })
    }

    /// Search using full-text search
    #[must_use = "Search results should be used"]
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        kind_filter: Option<crate::types::SymbolKind>,
        module_filter: Option<&str>,
    ) -> IndexResult<Vec<SearchResult>> {
        self.document_index
            .search(query, limit, kind_filter, module_filter)
            .map_err(|e| IndexError::General(format!("Search failed: {e}")))
    }

    /// Get total number of indexed documents
    pub fn document_count(&self) -> IndexResult<u64> {
        self.document_index
            .document_count()
            .map_err(|e| IndexError::General(format!("Failed to get document count: {e}")))
    }

    #[must_use = "The indexing result should be checked for errors"]
    pub fn index_directory(
        &mut self,
        dir: impl AsRef<Path>,
        progress: bool,
        dry_run: bool,
    ) -> IndexResult<IndexStats> {
        self.index_directory_with_options(dir, progress, dry_run, false, None)
    }

    #[must_use = "The indexing result should be checked for errors"]
    pub fn index_directory_with_force(
        &mut self,
        dir: impl AsRef<Path>,
        progress: bool,
        dry_run: bool,
        force: bool,
    ) -> IndexResult<IndexStats> {
        self.index_directory_with_options(dir, progress, dry_run, force, None)
    }

    #[must_use = "The indexing result should be checked for errors"]
    pub fn index_directory_with_options(
        &mut self,
        dir: impl AsRef<Path>,
        progress: bool,
        dry_run: bool,
        force: bool,
        max_files: Option<usize>,
    ) -> IndexResult<IndexStats> {
        let walker = FileWalker::new(self.settings.clone());
        let files: Vec<_> = walker.walk(dir.as_ref()).collect();

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

        let mut stats = IndexStats::new();

        // Process files one at a time with individual batches
        let processed = Arc::new(AtomicUsize::new(0));

        for file_path in files {
            // Track files as they are processed

            {
                // Start a new batch for this file
                self.start_tantivy_batch()?;

                match self.index_file_internal(&file_path, force) {
                    Ok(result) => {
                        // Commit this file's symbols so they're searchable
                        self.commit_tantivy_batch()?;

                        let file_id = result.file_id();

                        // Only count as indexed if it wasn't from cache
                        if !result.is_cached() {
                            stats.files_indexed += 1;

                            // Update symbol count only for actually indexed files
                            let new_symbols = self
                                .document_index
                                .find_symbols_by_file(file_id)
                                .map(|symbols| symbols.len())
                                .unwrap_or(0);
                            stats.symbols_found += new_symbols;
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to index {}: {}", file_path.display(), e);
                        stats.files_failed += 1;
                        // Rollback is automatic
                    }
                }
            }

            if progress {
                let current = processed.fetch_add(1, Ordering::SeqCst) + 1;
                eprint!("\r{}", stats.progress_line(current, total_files));
            }
        }

        if progress {
            eprintln!(); // New line after progress
        }

        // Resolve cross-file relationships after all files are indexed
        if !dry_run {
            self.resolve_cross_file_relationships()?;
        }

        Ok(stats)
    }

    /// Reconstruct TraitResolver state from stored relationships
    fn reconstruct_trait_resolver(&mut self) -> IndexResult<()> {
        // Reconstruct trait implementations
        self.reconstruct_trait_implementations()?;

        // Reconstruct trait method definitions
        self.reconstruct_trait_methods()?;

        Ok(())
    }

    /// Reconstruct trait implementations from stored relationships
    fn reconstruct_trait_implementations(&mut self) -> IndexResult<()> {
        let implements_relationships = self
            .document_index
            .get_all_relationships_by_kind(RelationKind::Implements)
            .map_err(|e| IndexError::TantivyError {
                operation: "get_all_relationships_by_kind".to_string(),
                cause: e.to_string(),
            })?;

        debug_print!(
            self,
            "Found {} implements relationships",
            implements_relationships.len()
        );

        for (type_id, trait_id, _) in implements_relationships {
            // Get symbol names
            let type_symbol = self
                .document_index
                .find_symbol_by_id(type_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbol_by_id".to_string(),
                    cause: e.to_string(),
                })?;
            let trait_symbol = self
                .document_index
                .find_symbol_by_id(trait_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbol_by_id".to_string(),
                    cause: e.to_string(),
                })?;

            if let (Some(type_sym), Some(trait_sym)) = (type_symbol, trait_symbol) {
                debug_print!(self, "{} implements {}", type_sym.name, trait_sym.name);
                self.trait_resolver.add_trait_impl(
                    type_sym.name.to_string(),
                    trait_sym.name.to_string(),
                    type_sym.file_id,
                );
            }
        }

        Ok(())
    }

    /// Reconstruct trait method definitions from stored relationships
    fn reconstruct_trait_methods(&mut self) -> IndexResult<()> {
        let defines_relationships = self
            .document_index
            .get_all_relationships_by_kind(RelationKind::Defines)
            .map_err(|e| IndexError::TantivyError {
                operation: "get_all_relationships_by_kind".to_string(),
                cause: e.to_string(),
            })?;

        // Group methods by trait
        let mut trait_methods: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for (definer_id, method_id, _) in defines_relationships {
            let definer_symbol =
                self.document_index
                    .find_symbol_by_id(definer_id)
                    .map_err(|e| IndexError::TantivyError {
                        operation: "find_symbol_by_id".to_string(),
                        cause: e.to_string(),
                    })?;
            let method_symbol = self
                .document_index
                .find_symbol_by_id(method_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbol_by_id".to_string(),
                    cause: e.to_string(),
                })?;

            if let (Some(definer), Some(method)) = (definer_symbol, method_symbol) {
                if definer.kind == crate::types::SymbolKind::Trait {
                    debug_print!(
                        self,
                        "Trait {} defines method {}",
                        definer.name,
                        method.name
                    );
                    trait_methods
                        .entry(definer.name.to_string())
                        .or_default()
                        .push(method.name.to_string());
                }
            }
        }

        // Update trait resolver
        for (trait_name, methods) in trait_methods {
            self.trait_resolver.add_trait_methods(trait_name, methods);
        }

        Ok(())
    }

    /// Stores MethodCall objects for enhanced resolution during symbol resolution phase.
    ///
    /// Enables precise method resolution by preserving receiver and static call information.
    fn store_method_call_for_resolution(
        &mut self,
        method_call: &crate::parsing::MethodCall,
        file_id: FileId,
    ) {
        debug_print!(
            self,
            "Storing method call for enhanced resolution: {} calls {} (static: {}, receiver: {:?})",
            method_call.caller,
            method_call.method_name,
            method_call.is_static,
            method_call.receiver
        );

        self.method_calls_by_file
            .entry(file_id)
            .or_default()
            .push(method_call.clone());
    }

    /// Resolves method calls using enhanced MethodCall data with fallback to legacy resolution.
    ///
    /// Matches caller/method names to stored MethodCall objects for precise resolution.
    /// Falls back to string-based resolution when no MethodCall data is available.
    fn resolve_method_call_enhanced(
        &self,
        call_target: &str,
        caller_name: &str,
        file_id: FileId,
        context: &ResolutionContext,
    ) -> Option<SymbolId> {
        // Try to find corresponding MethodCall object for enhanced resolution
        if let Some(method_calls) = self.method_calls_by_file.get(&file_id) {
            for method_call in method_calls {
                if method_call.caller == caller_name && method_call.method_name == call_target {
                    debug_print!(
                        self,
                        "Found MethodCall object for {}->{}! Using enhanced resolution",
                        caller_name,
                        call_target
                    );
                    return self.resolve_method_call(method_call, file_id, context);
                }
            }
        }

        // Fallback to legacy string-based resolution
        debug_print!(
            self,
            "No MethodCall object found for {}->{}. Using legacy resolution",
            caller_name,
            call_target
        );
        self.resolve_method_call_legacy(call_target, file_id, context)
    }

    /// Legacy method call resolution using string patterns (for backward compatibility)
    fn resolve_method_call_legacy(
        &self,
        call_target: &str,
        file_id: FileId,
        context: &ResolutionContext,
    ) -> Option<SymbolId> {
        // Check for receiver@method pattern
        let (receiver, method_name) = match call_target.find('@') {
            Some(pos) => (&call_target[..pos], &call_target[pos + 1..]),
            None => {
                debug_print!(
                    self,
                    "No receiver found, resolving '{}' as regular function",
                    call_target
                );
                let result = context.resolve(call_target);
                debug_print!(self, "Regular function resolution result: {:?}", result);
                return result;
            }
        };

        debug_print!(
            self,
            "Legacy resolution: receiver={}, method={}",
            receiver,
            method_name
        );

        // Look up receiver's type
        let type_name = self.variable_types.get(&(file_id, receiver.to_string()))?;

        debug_print!(self, "Found type for {}: {}", receiver, type_name);

        // Check if method comes from a trait
        match self
            .trait_resolver
            .resolve_method_trait(type_name, method_name)
        {
            Some(trait_name) => {
                debug_print!(
                    self,
                    "Method {} comes from trait {}",
                    method_name,
                    trait_name
                );
                let trait_method = format!("{trait_name}::{method_name}");
                let result = context
                    .resolve(&trait_method)
                    .or_else(|| context.resolve(method_name));
                debug_print!(self, "Resolution result for {}: {:?}", method_name, result);
                result
            }
            None => {
                if self
                    .trait_resolver
                    .is_inherent_method(type_name, method_name)
                {
                    debug_print!(
                        self,
                        "Method {} is inherent on type {}",
                        method_name,
                        type_name
                    );
                    let type_method = format!("{type_name}::{method_name}");
                    let result = context
                        .resolve(&type_method)
                        .or_else(|| context.resolve(method_name));
                    debug_print!(
                        self,
                        "Inherent method resolution result for {}: {:?}",
                        method_name,
                        result
                    );
                    result
                } else {
                    debug_print!(
                        self,
                        "Method {} not found on type {}",
                        method_name,
                        type_name
                    );
                    let result = context.resolve(method_name);
                    debug_print!(
                        self,
                        "Direct resolution result for {}: {:?}",
                        method_name,
                        result
                    );
                    result
                }
            }
        }
    }

    /// Resolve a method call using MethodCall struct with rich receiver information
    fn resolve_method_call(
        &self,
        method_call: &crate::parsing::MethodCall,
        file_id: FileId,
        context: &ResolutionContext,
    ) -> Option<SymbolId> {
        // If no receiver, treat as regular function call
        let receiver = match &method_call.receiver {
            Some(recv) => recv,
            None => {
                debug_print!(
                    self,
                    "No receiver found, resolving '{}' as regular function",
                    method_call.method_name
                );
                let result = context.resolve(&method_call.method_name);
                debug_print!(self, "Regular function resolution result: {:?}", result);
                return result;
            }
        };

        debug_print!(
            self,
            "Resolving method call: receiver={}, method={}, is_static={}",
            receiver,
            method_call.method_name,
            method_call.is_static
        );

        // Handle static methods differently - they don't need receiver type lookup
        if method_call.is_static {
            debug_print!(
                self,
                "Static method call: {}::{}",
                receiver,
                method_call.method_name
            );
            // For static calls, receiver is the type name, try Type::method format
            let static_method = format!("{}::{}", receiver, method_call.method_name);
            let result = context
                .resolve(&static_method)
                .or_else(|| context.resolve(&method_call.method_name));
            debug_print!(
                self,
                "Static method resolution result for {}: {:?}",
                method_call.method_name,
                result
            );
            return result;
        }

        // For instance methods, look up receiver's type
        let type_name = self.variable_types.get(&(file_id, receiver.to_string()))?;

        debug_print!(self, "Found type for {}: {}", receiver, type_name);

        // Check if method comes from a trait
        match self
            .trait_resolver
            .resolve_method_trait(type_name, &method_call.method_name)
        {
            Some(trait_name) => {
                debug_print!(
                    self,
                    "Method {} comes from trait {}",
                    method_call.method_name,
                    trait_name
                );
                // Try trait::method resolution first
                let trait_method = format!("{}::{}", trait_name, method_call.method_name);
                let result = context
                    .resolve(&trait_method)
                    .or_else(|| context.resolve(&method_call.method_name));
                debug_print!(
                    self,
                    "Resolution result for {}: {:?}",
                    method_call.method_name,
                    result
                );
                result
            }
            None => {
                // Could be an inherent method or not exist
                if self
                    .trait_resolver
                    .is_inherent_method(type_name, &method_call.method_name)
                {
                    debug_print!(
                        self,
                        "Method {} is inherent on type {}",
                        method_call.method_name,
                        type_name
                    );
                    // Try Type::method format for inherent methods
                    let type_method = format!("{}::{}", type_name, method_call.method_name);
                    let result = context
                        .resolve(&type_method)
                        .or_else(|| context.resolve(&method_call.method_name));
                    debug_print!(
                        self,
                        "Inherent method resolution result for {}: {:?}",
                        method_call.method_name,
                        result
                    );
                    result
                } else {
                    debug_print!(
                        self,
                        "Method {} not found on type {}",
                        method_call.method_name,
                        type_name
                    );
                    // Last resort - try to resolve just the method name
                    let result = context.resolve(&method_call.method_name);
                    debug_print!(
                        self,
                        "Direct resolution result for {}: {:?}",
                        method_call.method_name,
                        result
                    );
                    result
                }
            }
        }
    }

    /// Build resolution context for a file with all available symbols
    fn build_resolution_context(&self, file_id: FileId) -> IndexResult<ResolutionContext> {
        let mut context = ResolutionContext::new(file_id);

        // 1. Add imported symbols
        if let Some(imports) = self.import_resolver.imports_by_file.get(&file_id) {
            for import in imports {
                // Resolve the import to actual symbols
                if let Some(symbol_id) = self.import_resolver.resolve_symbol(
                    &import.path, // Pass full path, not just last segment
                    file_id,
                    &self.document_index,
                ) {
                    let name = if let Some(alias) = &import.alias {
                        alias.clone()
                    } else {
                        import
                            .path
                            .split("::")
                            .last()
                            .unwrap_or(&import.path)
                            .to_string()
                    };
                    debug_print!(
                        self,
                        "Adding import to context: name='{}', symbol_id={:?}",
                        name,
                        symbol_id
                    );
                    context.add_import(name, symbol_id, import.alias.is_some());
                }
            }
        }

        // 2. Add module-level symbols from current file
        let file_symbols = self
            .document_index
            .find_symbols_by_file(file_id)
            .map_err(|e| IndexError::TantivyError {
                operation: "find_symbols_by_file".to_string(),
                cause: e.to_string(),
            })?;

        for symbol in file_symbols {
            // Only add top-level symbols (functions, structs, etc. not local variables)
            match symbol.kind {
                crate::SymbolKind::Function |
                crate::SymbolKind::Method |  // Methods are also callable
                crate::SymbolKind::Struct |
                crate::SymbolKind::Trait |
                crate::SymbolKind::Enum |
                crate::SymbolKind::Constant => {
                    context.add_module_symbol(symbol.name.to_string(), symbol.id);
                }
                _ => {}
            }
        }

        // 3. Add public crate symbols
        // For now, we'll add all public symbols from the crate
        // In a real implementation, this would be more selective
        let all_symbols =
            self.document_index
                .get_all_symbols(10000)
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_all_symbols".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in all_symbols {
            if symbol.visibility == crate::Visibility::Public && symbol.file_id != file_id {
                context.add_crate_symbol(symbol.name.to_string(), symbol.id);
            }
        }

        Ok(context)
    }

    /// Resolve cross-file relationships using imports
    fn resolve_cross_file_relationships(&mut self) -> IndexResult<()> {
        // Process all unresolved relationships
        let unresolved = std::mem::take(&mut self.unresolved_relationships);

        debug_print!(
            self,
            "resolve_cross_file_relationships: {} unresolved relationships",
            unresolved.len()
        );

        if unresolved.is_empty() {
            return Ok(());
        }

        // Start a batch for relationship updates
        self.start_tantivy_batch()?;

        let mut resolved_count = 0;
        let mut skipped_count = 0;
        let mut external_symbols_created = 0u32;
        let _total_unresolved = unresolved.len();

        // Group relationships by file for efficient context building
        let mut relationships_by_file: std::collections::HashMap<
            FileId,
            Vec<UnresolvedRelationship>,
        > = std::collections::HashMap::new();
        for rel in unresolved {
            relationships_by_file
                .entry(rel.file_id)
                .or_default()
                .push(rel);
        }

        // Process each file's relationships with its resolution context
        for (file_id, file_relationships) in relationships_by_file {
            // Build resolution context for this file
            let context = self.build_resolution_context(file_id)?;

            for rel in file_relationships {
                debug_print!(
                    self,
                    "Processing relationship: {} -> {} (kind: {:?}, file: {:?})",
                    rel.from_name,
                    rel.to_name,
                    rel.kind,
                    rel.file_id
                );

                // Find 'from' symbols - these should be in the current file
                let all_from_symbols = self
                    .document_index
                    .find_symbols_by_name(&rel.from_name)
                    .map_err(|e| IndexError::TantivyError {
                        operation: "find_symbols_by_name".to_string(),
                        cause: e.to_string(),
                    })?;

                // Filter to only symbols from the current file
                let from_symbols: Vec<_> = all_from_symbols
                    .into_iter()
                    .filter(|s| s.file_id == file_id)
                    .collect();

                debug_print!(
                    self,
                    "Found {} from_symbols in current file",
                    from_symbols.len()
                );

                // Use ResolutionContext to resolve the target symbol (except for Defines)
                let to_symbol_id = if rel.kind == RelationKind::Defines {
                    // For defines relationships, we're looking for extension methods/properties
                    // that are defined on a type (from_name defines to_name)
                    debug_print!(
                        self,
                        "Processing Defines relationship: {} defines {}",
                        rel.from_name, rel.to_name
                    );
                    
                    // Find all symbols with the target name (method/property being defined)
                    let method_symbols = self
                        .document_index
                        .find_symbols_by_name(&rel.to_name)
                        .map_err(|e| IndexError::TantivyError {
                            operation: "find_symbols_by_name".to_string(),
                            cause: e.to_string(),
                        })?;

                    // For extension methods/properties, we need to find the symbol that:
                    // 1. Has the right name (rel.to_name)
                    // 2. Is in the current file
                    // 3. Is a method or property
                    // 4. Is defined in an extension context (if we can determine that)
                    
                    let mut candidates: Vec<_> = method_symbols
                        .into_iter()
                        .filter(|s| {
                            s.file_id == file_id && 
                            (s.kind == crate::types::SymbolKind::Method || 
                             s.kind == crate::types::SymbolKind::Field ||
                             s.kind == crate::types::SymbolKind::Function)
                        })
                        .collect();

                    debug_print!(
                        self,
                        "Found {} candidates for '{}' in file",
                        candidates.len(), rel.to_name
                    );

                    // Sort by line number for consistent ordering
                    candidates.sort_by_key(|s| s.range.start_line);

                    // For Swift extensions, we want to find the method/property that
                    // corresponds to this specific defines relationship.
                    // Since we may have multiple methods with the same name,
                    // we need to match based on the defining type (from_name).
                    
                    // First check if there's a from_symbol that matches our from_name
                    let from_type = from_symbols.iter()
                        .find(|s| s.name.as_ref() == rel.from_name.as_ref() && 
                                  (s.kind == crate::types::SymbolKind::Struct || 
                                   s.kind == crate::types::SymbolKind::Class ||
                                   s.kind == crate::types::SymbolKind::Enum ||
                                   s.kind == crate::types::SymbolKind::Trait ||
                                   s.kind == crate::types::SymbolKind::Interface));
                    
                    if let Some(_type_symbol) = from_type {
                        // We found the type being extended, return the first matching method
                        debug_print!(
                            self,
                            "Found type '{}' being extended with '{}'",
                            rel.from_name, rel.to_name
                        );
                        candidates.first().map(|s| s.id)
                    } else {
                        // Type might be defined in another file, still try to link the method
                        debug_print!(
                            self,
                            "Type '{}' not found in current file, but linking extension method '{}'",
                            rel.from_name, rel.to_name
                        );
                        candidates.first().map(|s| s.id)
                    }
                } else if rel.to_name.contains("::") {
                    // Handle qualified paths like String::new
                    let parts: Vec<&str> = rel.to_name.split("::").collect();
                    if parts.len() == 2 {
                        // Try to resolve the type first, then find the method
                        if let Some(_type_id) = context.resolve(parts[0]) {
                            // Find the method on this type
                            // For now, just resolve the method name
                            context.resolve(parts[1])
                        } else {
                            None
                        }
                    } else {
                        context.resolve(&rel.to_name)
                    }
                } else if rel.to_name.starts_with("self.") {
                    // Handle self.method() calls
                    let method_name = &rel.to_name[5..]; // Skip "self."
                    // Look for the method in the current module
                    context.resolve(method_name)
                } else if rel.kind == RelationKind::Calls && from_symbols.len() == 1 {
                    debug_print!(self, "Resolving as method call: '{}'", rel.to_name);
                    self.resolve_method_call_enhanced(
                        &rel.to_name,
                        &rel.from_name,
                        file_id,
                        &context,
                    )
                } else {
                    debug_print!(self, "Resolving '{}' using context", rel.to_name);
                    let result = context.resolve(&rel.to_name);
                    debug_print!(self, "Resolution result: {:?}", result);
                    result
                };

                let to_symbol_id = match to_symbol_id {
                    Some(id) => {
                        debug_print!(self, "Resolved target symbol to: {:?}", id);
                        id
                    }
                    None => {
                        // Check if this looks like an external/stdlib symbol
                        // Common Swift stdlib/framework symbols
                        let is_external = matches!(
                            rel.to_name.as_ref(),
                            "print" | "sqrt" | "abs" | "min" | "max" | "pow" | 
                            "Date" | "UUID" | "String" | "Int" | "Double" | "Bool" |
                            "Array" | "Dictionary" | "Set" | "Optional" |
                            "Task" | "MainActor" | "async" | "await"
                        ) || rel.to_name.starts_with("NS") // Foundation types
                          || rel.to_name.starts_with("UI") // UIKit types
                          || rel.to_name.starts_with("CG"); // Core Graphics types
                        
                        if is_external {
                            debug_print!(
                                self,
                                "Symbol '{}' appears to be external/stdlib - creating placeholder",
                                rel.to_name
                            );
                            
                            // Create or find an external placeholder symbol
                            // Use FileId(0) as a special marker for external symbols
                            let external_file_id = FileId(0);
                            
                            // Check if we already have this external symbol
                            let existing_external = self
                                .document_index
                                .find_symbols_by_name(&rel.to_name)
                                .ok()
                                .and_then(|symbols| {
                                    symbols.into_iter()
                                        .find(|s| s.file_id == external_file_id)
                                        .map(|s| s.id)
                                });
                            
                            if let Some(existing_id) = existing_external {
                                debug_print!(
                                    self, 
                                    "Found existing external symbol: {:?}", 
                                    existing_id
                                );
                                existing_id
                            } else {
                                // Create a new external symbol placeholder
                                // We use a special ID generation scheme for external symbols
                                // to avoid conflicts with real symbols
                                let external_id = SymbolId(1_000_000 + external_symbols_created);
                                external_symbols_created += 1;
                                
                                let external_symbol = crate::symbol::Symbol::new(
                                    external_id,
                                    rel.to_name.as_ref(),
                                    crate::types::SymbolKind::Function, // Default to function
                                    external_file_id,
                                    crate::types::Range::new(0, 0, 0, 0), // Dummy range
                                )
                                .with_module_path("external")
                                .with_visibility(crate::symbol::Visibility::Public);
                                
                                // Store the external symbol with a special path
                                if let Err(e) = self.document_index.index_symbol(
                                    &external_symbol, 
                                    "<external>"
                                ) {
                                    debug_print!(
                                        self,
                                        "Failed to add external symbol: {}",
                                        e
                                    );
                                    skipped_count += 1;
                                    continue;
                                }
                                
                                debug_print!(
                                    self,
                                    "Created external symbol '{}' with ID: {:?}",
                                    rel.to_name, external_symbol.id
                                );
                                
                                external_symbol.id
                            }
                        } else {
                            debug_print!(
                                self,
                                "Failed to resolve target symbol '{}' - skipping",
                                rel.to_name
                            );
                            // Symbol not in scope and not external - skip this relationship
                            skipped_count += 1;
                            continue;
                        }
                    }
                };

                // Get the full symbol data
                // For external symbols, we might have just created them and they won't be findable yet
                // Check if this is an external symbol ID (>= 1,000,000)
                let to_symbol = if to_symbol_id.value() >= 1_000_000 {
                    // This is an external symbol we just created
                    // Create a placeholder symbol object for it
                    debug_print!(self, "Using external symbol placeholder for ID: {:?}", to_symbol_id);
                    crate::symbol::Symbol::new(
                        to_symbol_id,
                        rel.to_name.as_ref(),
                        crate::types::SymbolKind::Function,
                        FileId(0),
                        crate::types::Range::new(0, 0, 0, 0),
                    )
                    .with_module_path("external")
                    .with_visibility(crate::symbol::Visibility::Public)
                } else {
                    // Normal symbol - look it up
                    debug_print!(self, "Looking up symbol by ID: {:?}", to_symbol_id);
                    match self
                        .document_index
                        .find_symbol_by_id(to_symbol_id)
                        .map_err(|e| IndexError::TantivyError {
                            operation: "find_symbol_by_id".to_string(),
                            cause: e.to_string(),
                        })? {
                        Some(symbol) => {
                            debug_print!(self, "Found target symbol: {}", symbol.name);
                            symbol
                        }
                        None => {
                            debug_print!(self, "Target symbol not found in index - skipping");
                            skipped_count += 1;
                            continue;
                        }
                    }
                };

                // Process with our filtering logic
                debug_print!(self, "Processing {} from symbols", from_symbols.len());
                for from_symbol in &from_symbols {
                    debug_print!(
                        self,
                        "Checking relationship from {} to {}",
                        from_symbol.name,
                        to_symbol.name
                    );

                    // Check symbol kind compatibility
                    if !Self::is_compatible_relationship(from_symbol.kind, to_symbol.kind, rel.kind)
                    {
                        debug_print!(
                            self,
                            "Incompatible relationship: {} ({:?}) -> {} ({:?}) for {:?}",
                            from_symbol.name,
                            from_symbol.kind,
                            to_symbol.name,
                            to_symbol.kind,
                            rel.kind
                        );
                        skipped_count += 1;
                        continue;
                    }

                    // Check visibility (skip for Defines - a type can always see its own methods)
                    if rel.kind != RelationKind::Defines {
                        debug_print!(
                            self,
                            "Checking visibility: {} (vis: {:?}, module: {:?}) from {} (module: {:?})",
                            to_symbol.name,
                            to_symbol.visibility,
                            to_symbol.module_path,
                            from_symbol.name,
                            from_symbol.module_path
                        );
                        if !Self::is_symbol_visible_from(&to_symbol, from_symbol) {
                            debug_print!(
                                self,
                                "Symbol not visible: {} not visible from {}",
                                to_symbol.name,
                                from_symbol.name
                            );
                            skipped_count += 1;
                            continue;
                        }
                    }

                    // Add the relationship with preserved metadata
                    debug_print!(
                        self,
                        "Adding relationship: {} -> {} (kind: {:?})",
                        from_symbol.name,
                        to_symbol.name,
                        rel.kind
                    );
                    debug_print!(
                        self,
                        "Adding relationship: {} ({:?}) -> {} ({:?})",
                        from_symbol.name,
                        from_symbol.id,
                        to_symbol.name,
                        to_symbol.id
                    );
                    let mut relationship = Relationship::new(rel.kind);
                    if let Some(ref metadata) = rel.metadata {
                        relationship = relationship.with_metadata(metadata.clone());
                    }
                    self.add_relationship_internal(from_symbol.id, to_symbol.id, relationship)?;
                    resolved_count += 1;
                }
            }
        }

        // Commit the batch with all the relationships
        self.commit_tantivy_batch()?;

        debug_print!(
            self,
            "Relationship resolution complete - resolved: {}, skipped: {}, total: {}",
            resolved_count,
            skipped_count,
            _total_unresolved
        );

        Ok(())
    }

    /// Process pending embeddings after a successful Tantivy commit
    fn process_pending_embeddings(
        &mut self,
        vector_engine: &Arc<Mutex<VectorSearchEngine>>,
        embedding_generator: &Arc<dyn EmbeddingGenerator>,
    ) -> IndexResult<()> {
        if self.pending_embeddings.is_empty() {
            return Ok(());
        }

        // Extract texts for embedding generation
        let texts: Vec<&str> = self
            .pending_embeddings
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();

        // Generate embeddings
        let embeddings = embedding_generator
            .generate_embeddings(&texts)
            .map_err(|e| IndexError::General(format!("Vector embedding generation failed: {e}")))?;

        // Validate embedding count matches input
        if embeddings.len() != texts.len() {
            return Err(IndexError::General(format!(
                "Embedding count mismatch: expected {}, got {}",
                texts.len(),
                embeddings.len()
            )));
        }

        // Create vector IDs and embeddings pairs
        let mut vectors = Vec::with_capacity(self.pending_embeddings.len());
        for (i, (symbol_id, _)) in self.pending_embeddings.iter().enumerate() {
            // Convert SymbolId to VectorId (both wrap u32)
            if let Some(vector_id) = crate::vector::VectorId::new(symbol_id.value()) {
                vectors.push((vector_id, embeddings[i].clone()));
            }
        }

        // Index vectors
        vector_engine
            .lock()
            .map_err(|_| IndexError::General("Vector engine mutex poisoned".to_string()))?
            .index_vectors(&vectors)
            .map_err(|e| IndexError::General(format!("Vector indexing failed: {e}")))?;

        // Clear pending embeddings
        self.pending_embeddings.clear();

        Ok(())
    }

    /// Build or rebuild the symbol cache from current index
    pub fn build_symbol_cache(&mut self) -> IndexResult<()> {
        let cache_path = self.get_cache_path();

        // Get all symbols from the index (use the existing public method)
        let all_symbols = self.get_all_symbols();

        // Build the cache file
        crate::storage::symbol_cache::SymbolHashCache::build_from_symbols(
            &cache_path,
            all_symbols.iter(),
        )
        .map_err(|e| IndexError::General(format!("Failed to build symbol cache: {e}")))?;

        // Load the cache for immediate use
        self.load_symbol_cache()?;

        debug_print!(
            self,
            "Built symbol cache with {} symbols",
            all_symbols.len()
        );
        Ok(())
    }

    /// Load symbol cache if it exists
    pub fn load_symbol_cache(&mut self) -> IndexResult<()> {
        let cache_path = self.get_cache_path();

        if cache_path.exists() {
            match crate::storage::symbol_cache::SymbolHashCache::open(&cache_path) {
                Ok(cache) => {
                    self.symbol_cache = Some(Arc::new(
                        crate::storage::symbol_cache::ConcurrentSymbolCache::new(cache),
                    ));
                    debug_print!(self, "Loaded symbol cache from {}", cache_path.display());
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Warning: Failed to load symbol cache: {e}");
                    self.symbol_cache = None;
                    Ok(()) // Non-fatal, continue without cache
                }
            }
        } else {
            debug_print!(self, "No symbol cache found at {}", cache_path.display());
            Ok(())
        }
    }

    /// Get the path for the symbol cache file
    fn get_cache_path(&self) -> PathBuf {
        let index_base = if let Some(ref workspace_root) = self.settings.workspace_root {
            workspace_root.join(&self.settings.index_path)
        } else {
            self.settings.index_path.clone()
        };

        index_base.join("symbol_cache.bin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::types::SymbolCounter;
    use crate::{FileId, RelationKind, Symbol, SymbolKind, Visibility};

    #[test]
    fn test_trait_implementations_resolution() {
        // Test the relationship resolution bug directly by creating symbols manually
        use tempfile::TempDir;

        // Create a temporary directory for the test index
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("test_index");

        let settings = Settings {
            debug: true,
            index_path,
            ..Default::default()
        };
        let mut indexer = SimpleIndexer::with_settings(Arc::new(settings));

        let file_id = FileId(1);
        let file_path = "test.rs";

        // Start transaction to get proper symbol IDs
        indexer.start_tantivy_batch().unwrap();

        // Get proper symbol IDs using the counter
        let mut counter = indexer.get_next_symbol_counter().unwrap();
        let trait_id = counter.next_id();
        let struct_id = counter.next_id();

        // Create symbols with proper IDs
        let trait_symbol = Symbol {
            id: trait_id,
            name: "MyTrait".into(),
            kind: SymbolKind::Trait,
            range: crate::Range::new(0, 0, 0, 0),
            file_id,
            visibility: Visibility::Public,
            doc_comment: None,
            signature: None,
            module_path: Some("test".into()),
        };

        let struct_symbol = Symbol {
            id: struct_id,
            name: "MyStruct".into(),
            kind: SymbolKind::Struct,
            range: crate::Range::new(1, 0, 1, 0),
            file_id,
            visibility: Visibility::Public,
            doc_comment: None,
            signature: None,
            module_path: Some("test".into()),
        };

        // Store symbols
        indexer
            .store_symbol(trait_symbol.clone(), file_path)
            .unwrap();
        indexer
            .store_symbol(struct_symbol.clone(), file_path)
            .unwrap();

        // Register the implementation relationship
        indexer
            .add_relationships_by_name(
                "MyStruct",
                "MyTrait",
                file_id,
                RelationKind::Implements,
                None,
            )
            .unwrap();

        // Commit
        indexer.commit_tantivy_batch().unwrap();

        // Debug: Check unresolved relationships
        eprintln!(
            "Unresolved relationships before resolution: {:?}",
            indexer.unresolved_relationships
        );
        assert_eq!(indexer.unresolved_relationships.len(), 1);
        let unresolved = &indexer.unresolved_relationships[0];
        assert_eq!(unresolved.from_name.as_ref(), "MyStruct");
        assert_eq!(unresolved.to_name.as_ref(), "MyTrait");

        // Resolve relationships - THIS IS WHERE THE BUG HAPPENS
        // First, let's see what symbols are in the index
        if indexer.settings.debug {
            let all_symbols = indexer.document_index.get_all_symbols(100).unwrap();
            for sym in &all_symbols {
                debug_print!(
                    indexer,
                    "Symbol in index - ID {:?}: name='{}', kind={:?}",
                    sym.id,
                    sym.name,
                    sym.kind
                );
            }
        }

        indexer.resolve_cross_file_relationships().unwrap();

        // Find the trait
        let found_trait = indexer
            .find_symbols_by_name("MyTrait")
            .into_iter()
            .find(|s| s.kind == SymbolKind::Trait)
            .expect("Should find MyTrait");

        eprintln!("Created symbols: trait_id={trait_id:?}, struct_id={struct_id:?}");
        eprintln!("Found trait with ID: {:?}", found_trait.id);

        // THIS SHOULD WORK: get_implementations should return MyStruct
        let implementations = indexer.get_implementations(found_trait.id);

        eprintln!("Found {} implementations", implementations.len());
        for impl_sym in &implementations {
            eprintln!("  Implementation: {} ({:?})", impl_sym.name, impl_sym.kind);
        }

        // This assertion will fail, exposing the bug
        assert_eq!(
            implementations.len(),
            1,
            "Should find MyStruct implements MyTrait"
        );
        if !implementations.is_empty() {
            assert_eq!(implementations[0].name.as_ref(), "MyStruct");
        }
    }

    #[test]
    fn test_symbol_module_paths() {
        use std::fs;
        use tempfile::TempDir;

        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create src directory
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Create a simple Rust file
        let test_file = src_dir.join("test.rs");
        fs::write(
            &test_file,
            r#"
pub fn hello() {}
pub struct World;
"#,
        )
        .unwrap();

        // Create indexer with proper settings
        let settings = Arc::new(Settings {
            workspace_root: Some(project_root.to_path_buf()),
            index_path: project_root.join(".test_index"),
            ..Settings::default()
        });

        let mut indexer = SimpleIndexer::with_settings(settings);

        // Index the file
        indexer.index_file(&test_file).unwrap();

        // Find symbols and check their module paths
        let hello_symbols = indexer
            .document_index
            .find_symbols_by_name("hello")
            .unwrap();
        assert_eq!(hello_symbols.len(), 1);
        assert_eq!(
            hello_symbols[0].module_path.as_ref().map(|s| s.as_ref()),
            Some("crate::test::hello")
        );

        let world_symbols = indexer
            .document_index
            .find_symbols_by_name("World")
            .unwrap();
        assert_eq!(world_symbols.len(), 1);
        assert_eq!(
            world_symbols[0].module_path.as_ref().map(|s| s.as_ref()),
            Some("crate::test::World")
        );
    }

    #[test]
    fn test_simple_import_resolution() {
        use std::fs;
        use tempfile::TempDir;

        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create src directory
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Create config.rs with a function
        let config_file = src_dir.join("config.rs");
        fs::write(
            &config_file,
            r#"
pub fn create_config() -> String {
    "config".to_string()
}
"#,
        )
        .unwrap();

        // Create another.rs with a different function of same name
        let another_file = src_dir.join("another.rs");
        fs::write(
            &another_file,
            r#"
pub fn create_config() -> i32 {
    42
}
"#,
        )
        .unwrap();

        // Create main.rs that imports only from config
        let main_file = src_dir.join("main.rs");
        fs::write(
            &main_file,
            r#"
use crate::config::create_config;

fn main() {
    let cfg = create_config();
}
"#,
        )
        .unwrap();

        // Create indexer with proper settings
        let settings = Arc::new(Settings {
            workspace_root: Some(project_root.to_path_buf()),
            index_path: project_root.join(".test_index"),
            ..Settings::default()
        });

        let mut indexer = SimpleIndexer::with_settings(settings);

        // Index all files
        indexer.index_file(&config_file).unwrap();
        indexer.index_file(&another_file).unwrap();
        indexer.index_file(&main_file).unwrap();

        // Check that symbols have correct module paths
        let config_funcs = indexer
            .document_index
            .find_symbols_by_name("create_config")
            .unwrap();
        assert_eq!(config_funcs.len(), 2);

        // Verify module paths
        let config_create = config_funcs
            .iter()
            .find(|s| {
                s.module_path.as_ref().map(|m| m.as_ref()) == Some("crate::config::create_config")
            })
            .expect("Should find crate::config::create_config");
        let _another_create = config_funcs
            .iter()
            .find(|s| {
                s.module_path.as_ref().map(|m| m.as_ref()) == Some("crate::another::create_config")
            })
            .expect("Should find crate::another::create_config");

        // Verify import was registered
        let main_symbols = indexer.document_index.find_symbols_by_name("main").unwrap();
        assert_eq!(main_symbols.len(), 1);
        let main_file_id = main_symbols[0].file_id;

        // Test that resolve_symbol works correctly
        let resolved = indexer.import_resolver.resolve_symbol(
            "create_config",
            main_file_id,
            &indexer.document_index,
        );

        // Should resolve to config::create_config, not another::create_config
        assert_eq!(resolved, Some(config_create.id));
    }

    #[test]
    fn test_symbols_in_same_module() {
        let mut symbol_counter = SymbolCounter::new();

        let sym1 = Symbol::new(
            symbol_counter.next_id(),
            "test1",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        )
        .with_module_path("crate::module_a");

        let sym2 = Symbol::new(
            symbol_counter.next_id(),
            "test2",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        )
        .with_module_path("crate::module_a");

        let sym3 = Symbol::new(
            symbol_counter.next_id(),
            "test3",
            SymbolKind::Function,
            FileId::new(2).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        )
        .with_module_path("crate::module_b");

        let sym4 = Symbol::new(
            symbol_counter.next_id(),
            "test4",
            SymbolKind::Function,
            FileId::new(2).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        ); // No module path

        assert!(SimpleIndexer::symbols_in_same_module(&sym1, &sym2));
        assert!(!SimpleIndexer::symbols_in_same_module(&sym1, &sym3));
        assert!(!SimpleIndexer::symbols_in_same_module(&sym1, &sym4));
        assert!(!SimpleIndexer::symbols_in_same_module(&sym4, &sym4)); // Both have no module
    }

    #[test]
    fn test_is_symbol_visible_from() {
        let mut symbol_counter = SymbolCounter::new();

        let pub_sym = Symbol::new(
            symbol_counter.next_id(),
            "public_fn",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        )
        .with_module_path("crate::module_a")
        .with_visibility(Visibility::Public);

        let priv_sym = Symbol::new(
            symbol_counter.next_id(),
            "private_fn",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        )
        .with_module_path("crate::module_a")
        .with_visibility(Visibility::Private);

        let other_module_sym = Symbol::new(
            symbol_counter.next_id(),
            "other_fn",
            SymbolKind::Function,
            FileId::new(2).unwrap(),
            crate::Range::new(0, 0, 0, 0),
        )
        .with_module_path("crate::module_b");

        // Same module - both visible
        assert!(SimpleIndexer::is_symbol_visible_from(&pub_sym, &priv_sym));
        assert!(SimpleIndexer::is_symbol_visible_from(&priv_sym, &pub_sym));

        // Different modules - only public visible
        assert!(SimpleIndexer::is_symbol_visible_from(
            &pub_sym,
            &other_module_sym
        ));
        assert!(!SimpleIndexer::is_symbol_visible_from(
            &priv_sym,
            &other_module_sym
        ));
    }

    #[test]
    fn test_import_resolution() {
        use std::fs;
        use tempfile::TempDir;

        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create test files
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // utils.rs with a function
        let utils_path = src_dir.join("utils.rs");
        fs::write(
            &utils_path,
            r#"
pub fn helper_function() -> i32 {
    42
}
"#,
        )
        .unwrap();

        // main.rs that imports the function
        let main_path = src_dir.join("main.rs");
        fs::write(
            &main_path,
            r#"
use crate::utils::helper_function;

fn main() {
    let result = helper_function();
}
"#,
        )
        .unwrap();

        // Create indexer
        let settings = Arc::new(Settings {
            workspace_root: Some(project_root.to_path_buf()),
            index_path: PathBuf::from(".test_import"),
            ..Settings::default()
        });

        let mut indexer = SimpleIndexer::with_settings(settings);

        // Index files
        indexer.index_file(&utils_path).unwrap();
        indexer.index_file(&main_path).unwrap();

        // Resolve relationships
        indexer.resolve_cross_file_relationships().unwrap();

        // Verify that main calls helper_function
        let main_symbols = indexer.document_index.find_symbols_by_name("main").unwrap();
        assert_eq!(main_symbols.len(), 1);

        let helper_symbols = indexer
            .document_index
            .find_symbols_by_name("helper_function")
            .unwrap();
        assert_eq!(helper_symbols.len(), 1);

        // Check that import was registered
        let file_id = main_symbols[0].file_id;
        let resolved = indexer.import_resolver.resolve_symbol(
            "helper_function",
            file_id,
            &indexer.document_index,
        );

        assert!(
            resolved.is_some(),
            "Should resolve helper_function through imports"
        );
        assert_eq!(resolved.unwrap(), helper_symbols[0].id);
    }

    // Test import-based resolution - should now work with our fixes
    #[test]
    fn test_import_based_relationship_resolution() {
        use std::fs;
        use tempfile::TempDir;

        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create test files
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // config.rs with a struct
        let config_path = src_dir.join("config.rs");
        fs::write(
            &config_path,
            r#"
pub struct Config {
    value: String,
}

impl Config {
    pub fn new() -> Self {
        Config { value: String::new() }
    }
}
"#,
        )
        .unwrap();

        // another.rs with a different struct that also has new()
        let another_path = src_dir.join("another.rs");
        fs::write(
            &another_path,
            r#"
pub struct Another {
    data: i32,
}

impl Another {
    pub fn new() -> Self {
        Another { data: 0 }
    }
}
"#,
        )
        .unwrap();

        // main.rs that imports only Config - using a direct function instead
        let main_path = src_dir.join("main.rs");
        fs::write(
            &main_path,
            r#"
use crate::config::create_config;

fn main() {
    let c = create_config();  // Should link to config::create_config
}
"#,
        )
        .unwrap();

        // Update config.rs to have a function
        fs::write(
            &config_path,
            r#"
pub fn create_config() -> Config {
    Config { value: String::new() }
}

pub struct Config {
    value: String,
}
"#,
        )
        .unwrap();

        // Update another.rs to also have a create function
        fs::write(
            &another_path,
            r#"
pub fn create_config() -> Another {
    Another { data: 0 }
}

pub struct Another {
    data: i32,
}
"#,
        )
        .unwrap();

        // Create indexer with debug enabled
        let settings = Arc::new(Settings {
            workspace_root: Some(project_root.to_path_buf()),
            index_path: PathBuf::from(".test_import_resolution"),
            debug: true,
            ..Settings::default()
        });

        let mut indexer = SimpleIndexer::with_settings(settings);

        // Index files
        // Use index_file_no_resolve to batch indexing without resolving relationships
        indexer.index_file_no_resolve(&config_path).unwrap();
        indexer.index_file_no_resolve(&another_path).unwrap();
        indexer.index_file_no_resolve(&main_path).unwrap();

        // Debug: Check unresolved relationships before resolution
        eprintln!(
            "Unresolved relationships before resolution: {:?}",
            indexer.unresolved_relationships
        );

        // Now resolve all relationships after all files are indexed
        indexer.resolve_cross_file_relationships().unwrap();

        // Verify correct resolution
        let main_symbols = indexer.document_index.find_symbols_by_name("main").unwrap();
        assert_eq!(main_symbols.len(), 1);

        // Get relationships from main
        let main_id = main_symbols[0].id;
        let relationships = indexer
            .document_index
            .get_relationships_from(main_id, RelationKind::Calls)
            .unwrap();

        eprintln!("Relationships from main: {relationships:?}");

        // Should have exactly one relationship (to create_config)
        assert_eq!(
            relationships.len(),
            1,
            "main should call exactly one create_config function"
        );

        // Verify it's config::create_config, not another::create_config
        let (_, target_id, _) = &relationships[0]; // Second element is to_id
        let target_symbol = indexer
            .document_index
            .find_symbol_by_id(*target_id)
            .unwrap()
            .unwrap();

        assert_eq!(target_symbol.name.as_ref(), "create_config");
        assert_eq!(
            target_symbol.module_path.as_ref().map(|s| s.as_ref()),
            Some("crate::config::create_config")
        );
    }

    #[test]
    fn test_module_proximity() {
        // Same module
        assert_eq!(
            SimpleIndexer::module_proximity(Some("crate::module_a"), Some("crate::module_a")),
            0
        );

        // Parent/child
        assert_eq!(
            SimpleIndexer::module_proximity(
                Some("crate::module_a"),
                Some("crate::module_a::submodule")
            ),
            1
        );
        assert_eq!(
            SimpleIndexer::module_proximity(
                Some("crate::module_a::submodule"),
                Some("crate::module_a")
            ),
            1
        );

        // Siblings
        assert_eq!(
            SimpleIndexer::module_proximity(Some("crate::module_a"), Some("crate::module_b")),
            2
        );
        assert_eq!(
            SimpleIndexer::module_proximity(
                Some("crate::storage::memory"),
                Some("crate::storage::tantivy")
            ),
            2
        );

        // Distant
        assert_eq!(
            SimpleIndexer::module_proximity(
                Some("crate::module_a::sub"),
                Some("crate::module_b::other")
            ),
            3
        );

        // Missing module info
        assert_eq!(
            SimpleIndexer::module_proximity(None, Some("crate::module_a")),
            4
        );
        assert_eq!(
            SimpleIndexer::module_proximity(Some("crate::module_a"), None),
            4
        );
        assert_eq!(SimpleIndexer::module_proximity(None, None), 4);
    }

    #[test]
    fn test_is_compatible_relationship_calls() {
        // Valid call relationships - executable code calling executable code
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Function,
            RelationKind::Calls
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Method,
            SymbolKind::Function,
            RelationKind::Calls
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Method,
            RelationKind::Calls
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Macro,
            SymbolKind::Function,
            RelationKind::Calls
        ));

        // Invalid call relationships - non-executable code
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Struct,
            SymbolKind::Function,
            RelationKind::Calls
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Trait,
            SymbolKind::Method,
            RelationKind::Calls
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Struct,
            RelationKind::Calls
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Constant,
            SymbolKind::Function,
            RelationKind::Calls
        ));
    }

    #[test]
    fn test_is_compatible_relationship_implements() {
        // Valid implements relationships - types implementing interfaces
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Struct,
            SymbolKind::Trait,
            RelationKind::Implements
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Enum,
            SymbolKind::Trait,
            RelationKind::Implements
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Class,
            SymbolKind::Interface,
            RelationKind::Implements
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Class,
            SymbolKind::Trait,
            RelationKind::Implements
        ));

        // Invalid implements relationships
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Trait,
            RelationKind::Implements
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Struct,
            SymbolKind::Function,
            RelationKind::Implements
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Trait,
            SymbolKind::Struct,
            RelationKind::Implements
        ));
    }

    #[test]
    fn test_is_compatible_relationship_uses() {
        // Valid uses relationships - language agnostic
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Struct,
            RelationKind::Uses
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Method,
            SymbolKind::Enum,
            RelationKind::Uses
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Class,
            SymbolKind::Interface,
            RelationKind::Uses
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Module,
            SymbolKind::TypeAlias,
            RelationKind::Uses
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Constant,
            RelationKind::Uses
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Method,
            SymbolKind::Variable,
            RelationKind::Uses
        ));

        // Invalid uses relationships - what can't use things
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Constant,
            SymbolKind::Struct,
            RelationKind::Uses
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Variable,
            SymbolKind::Class,
            RelationKind::Uses
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Field,
            SymbolKind::Function,
            RelationKind::Uses
        ));
    }

    #[test]
    fn test_is_compatible_relationship_defines() {
        // Valid defines relationships - containers defining members
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Trait,
            SymbolKind::Method,
            RelationKind::Defines
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Module,
            SymbolKind::Function,
            RelationKind::Defines
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Struct,
            SymbolKind::Field,
            RelationKind::Defines
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Class,
            SymbolKind::Method,
            RelationKind::Defines
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Interface,
            SymbolKind::Method,
            RelationKind::Defines
        ));
        assert!(SimpleIndexer::is_compatible_relationship(
            SymbolKind::Enum,
            SymbolKind::Constant,
            RelationKind::Defines
        ));

        // Invalid defines relationships - non-containers
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Function,
            SymbolKind::Method,
            RelationKind::Defines
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Method,
            SymbolKind::Function,
            RelationKind::Defines
        ));
        assert!(!SimpleIndexer::is_compatible_relationship(
            SymbolKind::Variable,
            SymbolKind::Field,
            RelationKind::Defines
        ));
    }

    // ===== Stage 3 Baseline Tests =====
    // These tests capture the CURRENT behavior of configure_symbol
    // to ensure refactoring doesn't change functionality

    #[test]
    fn test_configure_symbol_baseline_rust() {
        use crate::parsing::rust_behavior::RustBehavior;
        use tempfile::TempDir;

        // Create temp directory for test
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings {
            workspace_root: Some(temp_dir.path().to_path_buf()),
            ..Settings::default()
        };

        let indexer = SimpleIndexer::with_settings(Arc::new(settings));
        let behavior = RustBehavior::new();
        let mut symbol_counter = SymbolCounter::new();

        // Test case 1: Rust with public function
        let mut symbol = Symbol {
            id: symbol_counter.next_id(),
            name: "test_function".into(),
            kind: SymbolKind::Function,
            signature: Some("pub fn test_function() -> Result<()>".into()),
            module_path: None,
            file_id: FileId(1),
            range: crate::Range::new(0, 10, 0, 20),
            visibility: Visibility::Private,
            doc_comment: None,
        };

        let module_path = Some("crate::module".to_string());
        indexer.configure_symbol(&mut symbol, &module_path, &behavior);

        // CURRENT BEHAVIOR: Rust adds symbol name to module path
        assert_eq!(
            symbol.module_path.as_deref(),
            Some("crate::module::test_function")
        );

        // CURRENT BEHAVIOR: Updates visibility to Public because signature contains "pub "
        assert_eq!(symbol.visibility, Visibility::Public);
    }

    #[test]
    fn test_configure_symbol_baseline_python() {
        use crate::parsing::python_behavior::PythonBehavior;
        use tempfile::TempDir;

        // Create temp directory for test
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings {
            workspace_root: Some(temp_dir.path().to_path_buf()),
            ..Settings::default()
        };

        let indexer = SimpleIndexer::with_settings(Arc::new(settings));
        let behavior = PythonBehavior::new();
        let mut symbol_counter = SymbolCounter::new();

        let mut symbol = Symbol {
            id: symbol_counter.next_id(),
            name: "test_function".into(),
            kind: SymbolKind::Function,
            signature: Some("def test_function():".into()),
            module_path: None,
            file_id: FileId(1),
            range: crate::Range::new(0, 10, 0, 20),
            visibility: Visibility::Public,
            doc_comment: None,
        };

        let module_path = Some("test_module".to_string());
        indexer.configure_symbol(&mut symbol, &module_path, &behavior);

        // Python doesn't add symbol name to module path
        assert_eq!(symbol.module_path.as_deref(), Some("test_module"));

        // Python doesn't have visibility parsing in configure_symbol
        assert_eq!(symbol.visibility, Visibility::Public);
    }

    #[test]
    fn test_configure_symbol_php_baseline() {
        use crate::parsing::php_behavior::PhpBehavior;
        use tempfile::TempDir;

        // Create temp directory for test
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings {
            workspace_root: Some(temp_dir.path().to_path_buf()),
            ..Settings::default()
        };

        let indexer = SimpleIndexer::with_settings(Arc::new(settings));
        let behavior = PhpBehavior::new();
        let mut symbol_counter = SymbolCounter::new();

        let mut symbol = Symbol {
            id: symbol_counter.next_id(),
            name: "testFunction".into(),
            kind: SymbolKind::Function,
            signature: Some("public function testFunction()".into()),
            module_path: None,
            file_id: FileId(1),
            range: crate::Range::new(0, 10, 0, 20),
            visibility: Visibility::Public,
            doc_comment: None,
        };

        let module_path = Some("App\\Utils".to_string());
        indexer.configure_symbol(&mut symbol, &module_path, &behavior);

        // PHP doesn't add symbol name to module path
        assert_eq!(symbol.module_path.as_deref(), Some("App\\Utils"));

        // PHP doesn't have visibility parsing in configure_symbol
        assert_eq!(symbol.visibility, Visibility::Public);
    }

    #[test]
    fn test_configure_symbol_different_languages() {
        use crate::parsing::{
            php_behavior::PhpBehavior, python_behavior::PythonBehavior, rust_behavior::RustBehavior,
        };

        use tempfile::TempDir;

        // Create temp directory for test
        let temp_dir = TempDir::new().unwrap();
        let settings = Settings {
            workspace_root: Some(temp_dir.path().to_path_buf()),
            ..Settings::default()
        };

        let indexer = SimpleIndexer::with_settings(Arc::new(settings));
        let rust_behavior = RustBehavior::new();
        let python_behavior = PythonBehavior::new();
        let php_behavior = PhpBehavior::new();
        let module_path = Some("test_module".to_string());

        // Use SymbolCounter for proper ID generation
        let mut symbol_counter = SymbolCounter::new();

        // Test symbols with language-appropriate signatures
        let mut rust_symbol = Symbol {
            id: symbol_counter.next_id(),
            name: "test".into(),
            kind: SymbolKind::Function,
            signature: Some("pub fn test()".into()),
            module_path: None,
            file_id: FileId(1),
            range: crate::Range::new(0, 10, 0, 20),
            visibility: Visibility::Private,
            doc_comment: None,
        };

        let mut python_symbol = Symbol {
            id: symbol_counter.next_id(),
            name: "test".into(),
            kind: SymbolKind::Function,
            signature: Some("def test():".into()), // Python signature
            module_path: None,
            file_id: FileId(2),
            range: crate::Range::new(0, 10, 0, 20),
            visibility: Visibility::Private,
            doc_comment: None,
        };

        let mut php_symbol = Symbol {
            id: symbol_counter.next_id(),
            name: "test".into(),
            kind: SymbolKind::Function,
            signature: Some("public function test()".into()), // PHP signature
            module_path: None,
            file_id: FileId(3),
            range: crate::Range::new(0, 10, 0, 20),
            visibility: Visibility::Private,
            doc_comment: None,
        };

        // Configure each symbol with its behavior
        indexer.configure_symbol(&mut rust_symbol, &module_path, &rust_behavior);
        indexer.configure_symbol(&mut python_symbol, &module_path, &python_behavior);
        indexer.configure_symbol(&mut php_symbol, &module_path, &php_behavior);

        // Verify different behaviors
        assert_eq!(
            rust_symbol.module_path.as_deref(),
            Some("test_module::test")
        );
        assert_eq!(python_symbol.module_path.as_deref(), Some("test_module"));
        assert_eq!(php_symbol.module_path.as_deref(), Some("test_module"));

        // Visibility parsed according to each language's rules
        assert_eq!(rust_symbol.visibility, Visibility::Public); // Rust: "pub " means public
        assert_eq!(python_symbol.visibility, Visibility::Public); // Python: no underscore prefix means public
        assert_eq!(php_symbol.visibility, Visibility::Public); // PHP: "public function" means public
    }
}
