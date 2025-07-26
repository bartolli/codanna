//! Tantivy-only implementation of SimpleIndexer
//! This version uses Tantivy as the single source of truth for all data

use crate::{
    FileId, SymbolId, Relationship, RelationKind, Symbol,
    Settings, Visibility,
    IndexError, IndexResult,
};
use crate::storage::{DocumentIndex, SearchResult};
use crate::parsing::{Language, ParserFactory};
use crate::indexing::{FileWalker, IndexStats, ImportResolver, IndexTransaction, calculate_hash, get_utc_timestamp};
use std::path::{Path, PathBuf};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Compatibility struct for transaction support
#[derive(Debug)]
pub struct TantivyTransaction;

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
}

#[derive(Debug)]
pub struct SimpleIndexer {
    parser_factory: ParserFactory,
    #[allow(dead_code)]
    import_resolver: ImportResolver,
    settings: Arc<Settings>,
    project_root: Option<PathBuf>,
    document_index: DocumentIndex,
    /// Unresolved relationships to be resolved in a second pass
    unresolved_relationships: Vec<UnresolvedRelationship>,
}

impl SimpleIndexer {
    pub fn new() -> Self {
        let settings = Arc::new(Settings::default());
        Self::with_settings(settings)
    }
    
    pub fn with_settings(settings: Arc<Settings>) -> Self {
        let project_root = settings.workspace_root.clone()
            .or_else(|| settings.indexing.project_root.clone());
            
        // Use the configured index path for Tantivy
        let tantivy_path = if settings.index_path.exists() || settings.index_path.parent().map_or(false, |p| p.exists()) {
            settings.index_path.join("tantivy")
        } else if let Some(ref root) = project_root {
            // Fallback to project root if index path doesn't exist
            root.join(".codanna").join("tantivy")
        } else {
            PathBuf::from(".codanna/tantivy")
        };
        
        let document_index = DocumentIndex::new(tantivy_path)
            .expect("Failed to create Tantivy index");
        
        Self {
            parser_factory: ParserFactory::new(settings.clone()),
            import_resolver: ImportResolver::new(),
            settings,
            project_root,
            document_index,
            unresolved_relationships: Vec::new(),
        }
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
    pub fn set_project_root(&mut self, root: PathBuf) {
        self.project_root = Some(root);
    }
    
    /// Start a batch operation for Tantivy indexing
    pub fn start_tantivy_batch(&self) -> IndexResult<()> {
        self.document_index.start_batch()
            .map_err(|e| IndexError::TantivyError {
                operation: "start_batch".to_string(),
                cause: e.to_string(),
            })
    }
    
    /// Commit the current Tantivy batch
    pub fn commit_tantivy_batch(&self) -> IndexResult<()> {
        self.document_index.commit_batch()
            .map_err(|e| IndexError::TantivyError {
                operation: "commit_batch".to_string(),
                cause: e.to_string(),
            })
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
    pub fn index_file_with_force(&mut self, path: impl AsRef<Path>, force: bool) -> IndexResult<crate::IndexingResult> {
        self.start_tantivy_batch()?;
        
        match self.index_file_internal(path, force) {
            Ok(result) => {
                self.commit_tantivy_batch()?;
                Ok(result)
            }
            Err(e) => {
                // Rollback is automatic - uncommitted changes are discarded
                Err(e)
            }
        }
    }
    
    fn index_file_internal(&mut self, path: impl AsRef<Path>, force: bool) -> IndexResult<crate::IndexingResult> {
        let path = path.as_ref();
        let path_str = path.to_str().ok_or_else(|| {
            IndexError::FileRead {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in path"),
            }
        })?;
        
        // Read file and calculate hash
        let (content, content_hash) = self.read_file_with_hash(path)?;
        
        // Check if file already exists by querying Tantivy
        if let Ok(Some((file_id, existing_hash))) = self.document_index.get_file_info(path_str) {
            if !force && existing_hash == content_hash {
                // File hasn't changed, skip re-indexing
                return Ok(crate::IndexingResult::Cached(file_id));
            }
            
            // File has changed or force re-indexing, remove old symbols
            self.remove_file_symbols(file_id)?;
        }
        
        // Register or update file
        let file_id = self.register_file(path_str, content_hash)?;
        
        // Index the file content
        self.reindex_file_content(path, path_str, file_id, &content)?;
        
        Ok(crate::IndexingResult::Indexed(file_id))
    }
    
    /// Read file content and calculate its hash
    fn read_file_with_hash(&self, path: &Path) -> IndexResult<(String, String)> {
        let content = fs::read_to_string(path)
            .map_err(|e| IndexError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;
        
        let hash = calculate_hash(&content);
        Ok((content, hash))
    }
    
    /// Register a new file in the index
    fn register_file(&mut self, path_str: &str, content_hash: String) -> IndexResult<FileId> {
        // Get next file ID from Tantivy
        let file_counter = self.document_index.get_next_file_id()
            .map_err(|e| IndexError::TantivyError {
                operation: "get_next_file_id".to_string(),
                cause: e.to_string(),
            })?;
            
        let file_id = FileId::new(file_counter)
            .ok_or(IndexError::FileIdExhausted)?;
            
        let timestamp = get_utc_timestamp();
        
        // Store file info in Tantivy
        self.document_index.store_file_info(file_id, path_str, &content_hash, timestamp)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_file_info".to_string(),
                cause: e.to_string(),
            })?;
        
        Ok(file_id)
    }
    
    /// Remove all symbols from a file
    fn remove_file_symbols(&mut self, file_id: FileId) -> IndexResult<()> {
        // Get all symbols for this file from Tantivy
        let symbols = self.document_index.find_symbols_by_file(file_id)
            .map_err(|e| IndexError::TantivyError {
                operation: "find_symbols_by_file".to_string(),
                cause: e.to_string(),
            })?;
        
        // Remove each symbol and its relationships from Tantivy
        for symbol in symbols {
            self.document_index.delete_symbol(symbol.id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "delete_symbol".to_string(),
                    cause: e.to_string(),
                })?;
            
            // Also remove relationships
            self.document_index.delete_relationships_for_symbol(symbol.id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "delete_relationships".to_string(),
                    cause: e.to_string(),
                })?;
        }
        
        Ok(())
    }
    
    /// Index or re-index file content
    fn reindex_file_content(&mut self, path: &Path, path_str: &str, file_id: FileId, content: &str) -> IndexResult<FileId> {
        let language = self.detect_language(path)?;
        let mut parser = self.create_parser(language)?;
        let module_path = self.calculate_module_path(path);
        
        let symbol_counter = self.get_next_symbol_counter()?;
        self.extract_and_store_symbols(&mut parser, content, file_id, path_str, &module_path, language, symbol_counter)?;
        self.extract_and_store_relationships(&mut parser, content, file_id)?;
        self.update_symbol_counter(symbol_counter)?;
        
        Ok(file_id)
    }
    
    /// Detect the programming language from file extension
    fn detect_language(&self, path: &Path) -> IndexResult<Language> {
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        
        Language::from_extension(extension)
            .ok_or_else(|| IndexError::UnsupportedFileType {
                path: path.to_path_buf(),
                extension: extension.to_string(),
            })
    }
    
    /// Create a parser for the given language
    fn create_parser(&self, language: Language) -> IndexResult<Box<dyn crate::parsing::LanguageParser>> {
        self.parser_factory.create_parser(language)
    }
    
    /// Calculate module path relative to project root
    fn calculate_module_path(&self, path: &Path) -> Option<String> {
        self.project_root.as_ref().and_then(|root| {
            path.strip_prefix(root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        })
    }
    
    /// Get the next symbol counter from Tantivy
    fn get_next_symbol_counter(&self) -> IndexResult<u32> {
        self.document_index.get_next_symbol_id()
            .map_err(|e| IndexError::TantivyError {
                operation: "get_next_symbol_id".to_string(),
                cause: e.to_string(),
            })
    }
    
    /// Extract symbols from content and store them in Tantivy
    fn extract_and_store_symbols(
        &mut self,
        parser: &mut Box<dyn crate::parsing::LanguageParser>,
        content: &str,
        file_id: FileId,
        path_str: &str,
        module_path: &Option<String>,
        language: Language,
        mut symbol_counter: u32,
    ) -> IndexResult<()> {
        let symbols = parser.parse(content, file_id, &mut symbol_counter);
        
        for mut symbol in symbols {
            self.configure_symbol(&mut symbol, module_path, language);
            self.store_symbol(symbol, path_str)?;
        }
        
        Ok(())
    }
    
    /// Configure a symbol with module path and visibility
    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: &Option<String>, language: Language) {
        // Set module path if available
        if symbol.module_path.is_none() {
            symbol.module_path = module_path.clone().map(Into::into);
        }
        
        // Determine visibility based on language rules
        if language == Language::Rust {
            // In Rust, items are private by default unless marked pub
            if let Some(sig) = &symbol.signature {
                if sig.contains("pub ") {
                    symbol.visibility = Visibility::Public;
                }
            }
        }
    }
    
    /// Store a single symbol in Tantivy
    fn store_symbol(&mut self, symbol: crate::Symbol, path_str: &str) -> IndexResult<()> {
        self.document_index.index_symbol(&symbol, path_str)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_symbol".to_string(),
                cause: e.to_string(),
            })
    }
    
    /// Extract relationships from content and store them
    fn extract_and_store_relationships(
        &mut self,
        parser: &mut Box<dyn crate::parsing::LanguageParser>,
        content: &str,
        file_id: FileId,
    ) -> IndexResult<()> {
        // 1. Function/method calls
        let calls = parser.find_calls(content);
        for (caller_name, callee_name, _range) in calls {
            self.add_relationships_by_name(&caller_name, &callee_name, file_id, RelationKind::Calls)?;
        }
        
        // 2. Trait implementations
        let implementations = parser.find_implementations(content);
        for (type_name, trait_name, _range) in implementations {
            self.add_relationships_by_name(&type_name, &trait_name, file_id, RelationKind::Implements)?;
        }
        
        // 3. Type usage (in fields, parameters, returns)
        let uses = parser.find_uses(content);
        for (context_name, used_type, _range) in uses {
            self.add_relationships_by_name(&context_name, &used_type, file_id, RelationKind::Uses)?;
        }
        
        // 4. Method definitions (trait defines methods)
        let defines = parser.find_defines(content);
        for (definer_name, method_name, _range) in defines {
            self.add_relationships_by_name(&definer_name, &method_name, file_id, RelationKind::Defines)?;
        }
        
        Ok(())
    }
    
    /// Update the symbol counter in Tantivy metadata
    fn update_symbol_counter(&mut self, symbol_counter: u32) -> IndexResult<()> {
        self.document_index.store_metadata(crate::storage::MetadataKey::SymbolCounter, symbol_counter as u64)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_metadata".to_string(),
                cause: e.to_string(),
            })
    }
    
    /// Add a relationship to Tantivy
    fn add_relationship_internal(&mut self, from: SymbolId, to: SymbolId, rel: Relationship) -> IndexResult<()> {
        self.document_index.store_relationship(from, to, &rel)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_relationship".to_string(),
                cause: e.to_string(),
            })
    }
    
    /// Helper method to add relationships by symbol names
    fn add_relationships_by_name(&mut self, from_name: &str, to_name: &str, file_id: FileId, kind: RelationKind) -> IndexResult<()> {
        // Extract the last component of the name for simple matching
        // e.g., "std::fmt::Debug" -> "Debug"
        let simple_to_name = to_name.split("::").last().unwrap_or(to_name);
        
        // Find symbols by name in Tantivy
        let from_symbols = self.document_index.find_symbols_by_name(from_name)
            .map_err(|e| IndexError::TantivyError {
                operation: "find_symbols_by_name".to_string(),
                cause: e.to_string(),
            })?;
        
        let to_symbols = self.document_index.find_symbols_by_name(simple_to_name)
            .map_err(|e| IndexError::TantivyError {
                operation: "find_symbols_by_name".to_string(),
                cause: e.to_string(),
            })?;
        
        // Add relationships for matching symbols
        // For 'from' symbols, we only consider those in the current file
        // For 'to' symbols, we consider all matches (cross-file references)
        for from_symbol in &from_symbols {
            if from_symbol.file_id == file_id {
                for to_symbol in &to_symbols {
                    self.add_relationship_internal(from_symbol.id, to_symbol.id, Relationship::new(kind))?;
                }
            }
        }
        
        // If no 'to' symbols found, store as unresolved for later resolution
        if to_symbols.is_empty() && !from_symbols.is_empty() {
            // Store unresolved relationship for later cross-file resolution
            self.unresolved_relationships.push(UnresolvedRelationship {
                from_name: from_name.into(),
                to_name: simple_to_name.into(),
                file_id,
                kind,
            });
        }
        
        Ok(())
    }
    
    // Query methods using Tantivy
    
    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        self.document_index.find_symbols_by_name(name)
            .ok()
            .and_then(|symbols| symbols.first().map(|s| s.id))
    }
    
    pub fn find_symbols_by_name(&self, name: &str) -> Vec<Symbol> {
        self.document_index.find_symbols_by_name(name)
            .unwrap_or_default()
    }
    
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        self.document_index.find_symbol_by_id(id)
            .ok()
            .flatten()
    }
    
    pub fn get_called_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        // Query relationships where from_symbol_id = symbol_id and kind = Calls
        self.document_index.get_relationships_from(symbol_id, RelationKind::Calls)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(_, to_id, _)| self.get_symbol(to_id))
            .collect()
    }
    
    pub fn get_calling_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        // Query relationships where to_symbol_id = symbol_id and kind = Calls
        self.document_index.get_relationships_to(symbol_id, RelationKind::Calls)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(from_id, _, _)| self.get_symbol(from_id))
            .collect()
    }
    
    pub fn get_implementations(&self, trait_id: SymbolId) -> Vec<Symbol> {
        // Query relationships where to_symbol_id = trait_id and kind = Implements
        self.document_index.get_relationships_to(trait_id, RelationKind::Implements)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(from_id, _, _)| self.get_symbol(from_id))
            .collect()
    }
    
    pub fn get_all_symbols(&self) -> Vec<Symbol> {
        self.document_index.get_all_symbols(10000)
            .unwrap_or_default()
    }
    
    /// Get all dependencies of a symbol (what it depends on)
    pub fn get_dependencies(&self, symbol_id: SymbolId) -> std::collections::HashMap<RelationKind, Vec<Symbol>> {
        use std::collections::HashMap;
        let mut deps = HashMap::new();
        
        // Get all outgoing relationships
        for kind in &[RelationKind::Calls, RelationKind::Uses, RelationKind::Implements, RelationKind::Defines] {
            let symbols = self.document_index.get_relationships_from(symbol_id, *kind)
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
    pub fn get_dependents(&self, symbol_id: SymbolId) -> std::collections::HashMap<RelationKind, Vec<Symbol>> {
        use std::collections::HashMap;
        let mut deps = HashMap::new();
        
        // Get all incoming relationships
        for kind in &[RelationKind::Calls, RelationKind::Uses, RelationKind::Implements, RelationKind::Defines] {
            let symbols = self.document_index.get_relationships_to(symbol_id, *kind)
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
    pub fn get_impact_radius(&self, symbol_id: SymbolId, max_depth: Option<usize>) -> Vec<SymbolId> {
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
            for kind in &[RelationKind::Calls, RelationKind::Uses, RelationKind::Implements] {
                if let Ok(relationships) = self.document_index.get_relationships_to(current_id, *kind) {
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
        self.document_index.count_symbols()
            .unwrap_or(0)
    }
    
    pub fn file_count(&self) -> u32 {
        self.document_index.count_files()
            .unwrap_or(0) as u32
    }
    
    pub fn relationship_count(&self) -> usize {
        self.document_index.count_relationships()
            .unwrap_or(0)
    }
    
    pub fn get_file_path(&self, file_id: FileId) -> Option<String> {
        self.document_index.get_file_path(file_id)
            .ok()
            .flatten()
    }
    
    /// Clear the Tantivy index
    pub fn clear_tantivy_index(&self) -> IndexResult<()> {
        self.document_index.clear()
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
        self.document_index.search(query, limit, kind_filter, module_filter)
            .map_err(|e| IndexError::General(format!("Search failed: {}", e)))
    }
    
    /// Get total number of indexed documents
    pub fn document_count(&self) -> IndexResult<u64> {
        self.document_index.document_count()
            .map_err(|e| IndexError::General(format!("Failed to get document count: {}", e)))
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
        let mut stats = IndexStats::new();
        
        // Process files one at a time with individual batches
        let processed = Arc::new(AtomicUsize::new(0));
        
        for file_path in files {
            // Track files as they are processed
            
            if !dry_run {
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
                        }
                        
                        // Update symbol count
                        let new_symbols = self.document_index.find_symbols_by_file(file_id)
                            .map(|symbols| symbols.len())
                            .unwrap_or(0);
                        stats.symbols_found += new_symbols;
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
    
    /// Resolve cross-file relationships using imports
    fn resolve_cross_file_relationships(&mut self) -> IndexResult<()> {
        // Process all unresolved relationships
        let unresolved = std::mem::take(&mut self.unresolved_relationships);
        
        if unresolved.is_empty() {
            return Ok(());
        }
        
        // Start a batch for relationship updates
        self.start_tantivy_batch()?;
        
        let mut _resolved_count = 0;
        for rel in unresolved {
            // Try to find the target symbol again (it might have been indexed in a later file)
            let to_symbols = self.document_index.find_symbols_by_name(&rel.to_name)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_name".to_string(),
                    cause: e.to_string(),
                })?;
            
            if !to_symbols.is_empty() {
                // Found the target symbol(s), create relationships
                let from_symbols = self.document_index.find_symbols_by_name(&rel.from_name)
                    .map_err(|e| IndexError::TantivyError {
                        operation: "find_symbols_by_name".to_string(),
                        cause: e.to_string(),
                    })?;
                
                for from_symbol in &from_symbols {
                    if from_symbol.file_id == rel.file_id {
                        for to_symbol in &to_symbols {
                            self.add_relationship_internal(from_symbol.id, to_symbol.id, Relationship::new(rel.kind))?;
                            _resolved_count += 1;
                        }
                    }
                }
            }
            // If still not found, the symbol might be from an external crate or not indexed
        }
        
        
        // Commit the batch with all the relationships
        self.commit_tantivy_batch()?;
        
        Ok(())
    }
}

impl Default for SimpleIndexer {
    fn default() -> Self {
        Self::new()
    }
}