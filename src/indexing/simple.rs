use crate::{
    SymbolStore, DependencyGraph, 
    FileId, SymbolId, Relationship, RelationKind, Symbol, IndexData,
    Settings, Visibility,
};
use crate::parsing::{Language, ParserFactory, RustParser};
use crate::indexing::{FileWalker, IndexStats, ImportResolver};
use std::path::{Path, PathBuf};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SimpleIndexer {
    pub symbol_store: SymbolStore,
    pub graph: DependencyGraph,
    parser_factory: ParserFactory,
    import_resolver: ImportResolver,
    data: IndexData,
    settings: Arc<Settings>,
    project_root: Option<PathBuf>,
}

impl SimpleIndexer {
    pub fn new() -> Self {
        let settings = Arc::new(Settings::default());
        Self::with_settings(settings)
    }
    
    pub fn with_settings(settings: Arc<Settings>) -> Self {
        Self {
            symbol_store: SymbolStore::new(),
            graph: DependencyGraph::new(),
            parser_factory: ParserFactory::new(settings.clone()),
            import_resolver: ImportResolver::new(),
            data: IndexData::new(),
            settings,
            project_root: None,
        }
    }
    
    /// Create from loaded data
    pub fn from_data(data: IndexData) -> Self {
        Self::from_data_with_settings(data, Arc::new(Settings::default()))
    }
    
    /// Create from loaded data with custom settings
    pub fn from_data_with_settings(data: IndexData, settings: Arc<Settings>) -> Self {
        let mut indexer = Self::with_settings(settings);
        indexer.data = data;
        
        // Rebuild in-memory structures
        for symbol in &indexer.data.symbols {
            indexer.symbol_store.insert(symbol.clone());
            indexer.graph.add_symbol(symbol.id);
        }
        
        for (from, to, rel) in &indexer.data.relationships {
            indexer.graph.add_relationship(*from, *to, rel.clone());
        }
        
        // TODO: Rebuild import resolver from stored data
        
        indexer
    }
    
    /// Get the data for persistence
    pub fn data(&self) -> &IndexData {
        &self.data
    }
    
    /// Set the project root for module path calculation
    pub fn set_project_root(&mut self, root: PathBuf) {
        self.project_root = Some(root);
    }
    
    pub fn index_file(&mut self, path: impl AsRef<Path>) -> Result<FileId, String> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy().to_string();
        
        // Check if we've already indexed this file
        if let Some(&file_id) = self.data.file_map.get(&path_str) {
            return Ok(file_id);
        }
        
        // Detect language from file extension
        let language = Language::from_path(path)
            .ok_or_else(|| format!("Unsupported file type: {}", path.display()))?;
        
        // Create parser for this language
        let mut parser = self.parser_factory.create_parser(language)?;
        
        // Read file content
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        
        // Create file ID
        let file_id = FileId::new(self.data.file_counter)
            .ok_or_else(|| "Failed to create file ID".to_string())?;
        self.data.file_counter += 1;
        self.data.file_map.insert(path_str, file_id);
        
        // Determine module path for this file
        let module_path = if let Some(root) = &self.project_root {
            ImportResolver::module_path_from_file(path, root)
        } else {
            // Try to guess project root from path
            let root = path.ancestors()
                .find(|p| p.join("Cargo.toml").exists() || p.join("src").exists())
                .unwrap_or_else(|| path.parent().unwrap_or(path));
            ImportResolver::module_path_from_file(path, root)
        }.unwrap_or_else(|| "crate".to_string());
        
        // Register file with the import resolver
        self.import_resolver.register_file(
            path.to_path_buf(),
            file_id,
            module_path.clone()
        );
        
        // Parse symbols
        let mut symbols = parser.parse(&content, file_id);
        
        // Update symbols with module path and visibility
        for symbol in &mut symbols {
            symbol.module_path = Some(format!("{}::{}", module_path, symbol.name).into());
            
            // TODO: Parse actual visibility from AST
            // For now, assume pub items in lib.rs/mod.rs are public
            if path.ends_with("lib.rs") || path.ends_with("mod.rs") {
                symbol.visibility = Visibility::Public;
            }
        }
        
        // Extract imports if this is a Rust file
        if language == Language::Rust {
            if let Ok(mut rust_parser) = RustParser::new() {
                let imports = rust_parser.extract_imports(&content, file_id);
                for import in imports {
                    self.import_resolver.add_import(import);
                }
            }
        }
        
        // Store symbols and add to graph
        for symbol in symbols {
            let symbol_id = symbol.id;
            self.symbol_store.insert(symbol.clone());
            self.graph.add_symbol(symbol_id);
            // Also store for serialization
            self.data.symbols.push(symbol);
        }
        
        // Find and store relationships (function calls)
        let calls = parser.find_calls(&content);
        for (caller_name, callee_name, _range) in calls {
            // Find symbols by name
            let callers = self.symbol_store.find_by_name(&caller_name);
            let callees = self.symbol_store.find_by_name(&callee_name);
            
            // Add relationships for all matching symbols
            for caller in &callers {
                for callee in &callees {
                    if caller.file_id == file_id {
                        let rel = Relationship::new(RelationKind::Calls);
                        self.graph.add_relationship(
                            caller.id,
                            callee.id,
                            rel.clone(),
                        );
                        // Store for serialization
                        self.data.relationships.push((caller.id, callee.id, rel));
                    }
                }
            }
        }
        
        // Find and store trait implementations
        let implementations = parser.find_implementations(&content);
        for (type_name, trait_name, _range) in implementations {
            // Find symbols by name
            let types = self.symbol_store.find_by_name(&type_name);
            let traits = self.symbol_store.find_by_name(&trait_name);
            
            // Add implements relationships
            for type_symbol in &types {
                for trait_symbol in &traits {
                    if type_symbol.file_id == file_id && trait_symbol.kind == crate::SymbolKind::Trait {
                        let rel = Relationship::new(RelationKind::Implements);
                        self.graph.add_relationship(
                            type_symbol.id,
                            trait_symbol.id,
                            rel.clone(),
                        );
                        // Store for serialization
                        self.data.relationships.push((type_symbol.id, trait_symbol.id, rel));
                    }
                }
            }
        }
        
        // Find and store type uses (struct fields and function params/returns)
        let uses = parser.find_uses(&content);
        for (user_name, used_name, _range) in uses {
            // Find symbols by name
            let users = self.symbol_store.find_by_name(&user_name);
            let used = self.symbol_store.find_by_name(&used_name);
            
            // Add uses relationships
            for user_symbol in &users {
                for used_symbol in &used {
                    // Don't create self-references or cross-file references for now
                    if user_symbol.file_id == file_id && 
                       user_symbol.id != used_symbol.id {
                        let rel = Relationship::new(RelationKind::Uses);
                        self.graph.add_relationship(
                            user_symbol.id,
                            used_symbol.id,
                            rel.clone(),
                        );
                        // Store for serialization
                        self.data.relationships.push((user_symbol.id, used_symbol.id, rel));
                    }
                }
            }
        }
        
        // Find and store defines relationships (traits defining methods, impl blocks defining methods)
        let defines = parser.find_defines(&content);
        for (definer_name, defined_name, _range) in defines {
            // Find symbols by name
            let definers = self.symbol_store.find_by_name(&definer_name);
            let defined = self.symbol_store.find_by_name(&defined_name);
            
            // Add defines relationships
            for definer_symbol in &definers {
                for defined_symbol in &defined {
                    if definer_symbol.file_id == file_id && 
                       defined_symbol.file_id == file_id {
                        let rel = Relationship::new(RelationKind::Defines);
                        self.graph.add_relationship(
                            definer_symbol.id,
                            defined_symbol.id,
                            rel.clone(),
                        );
                        // Store for serialization
                        self.data.relationships.push((definer_symbol.id, defined_symbol.id, rel));
                    }
                }
            }
        }
        
        Ok(file_id)
    }
    
    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        self.symbol_store.find_by_name(name).first().map(|s| s.id)
    }
    
    pub fn find_symbols_by_name(&self, name: &str) -> Vec<Symbol> {
        self.symbol_store.find_by_name(name)
    }
    
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        self.symbol_store.get(id)
    }
    
    pub fn get_called_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        self.graph.get_relationships(symbol_id, RelationKind::Calls)
            .into_iter()
            .filter_map(|id| self.symbol_store.get(id))
            .collect()
    }
    
    pub fn get_calling_functions(&self, symbol_id: SymbolId) -> Vec<Symbol> {
        self.graph.get_incoming_relationships(symbol_id, RelationKind::Calls)
            .into_iter()
            .filter_map(|id| self.symbol_store.get(id))
            .collect()
    }
    
    pub fn get_implementations(&self, trait_id: SymbolId) -> Vec<Symbol> {
        self.graph.get_incoming_relationships(trait_id, RelationKind::Implements)
            .into_iter()
            .filter_map(|id| self.symbol_store.get(id))
            .collect()
    }
    
    pub fn get_all_symbols(&self) -> Vec<Symbol> {
        self.symbol_store.iter().collect()
    }
    
    pub fn symbol_count(&self) -> usize {
        self.symbol_store.len()
    }
    
    /// Get the data symbol count (for debugging)
    pub fn data_symbol_count(&self) -> usize {
        self.data.symbols.len()
    }
    
    /// Get the file path for a given FileId
    pub fn get_file_path(&self, file_id: FileId) -> Option<&str> {
        // Find the path in the file_map by searching for the FileId
        self.data.file_map.iter()
            .find(|(_, &id)| id == file_id)
            .map(|(path, _)| path.as_str())
    }
    
    /// Index all files in a directory recursively
    pub fn index_directory(
        &mut self, 
        root: &Path, 
        show_progress: bool,
        dry_run: bool,
        max_files: Option<usize>,
    ) -> Result<IndexStats, String> {
        // Set project root for module path calculation
        self.set_project_root(root.to_path_buf());
        
        let mut stats = IndexStats::new();
        let walker = FileWalker::new(self.settings.clone());
        
        // Collect all files to index
        let files: Vec<_> = if let Some(max) = max_files {
            walker.walk(root).take(max).collect()
        } else {
            walker.walk(root).collect()
        };
        
        let total_files = files.len();
        if total_files == 0 {
            stats.stop_timing();
            return Ok(stats);
        }
        
        if dry_run {
            println!("Would index {} files:", total_files);
            for (i, file) in files.iter().enumerate() {
                if i < 20 {
                    println!("  {}", file.display());
                } else if i == 20 {
                    println!("  ... and {} more files", total_files - 20);
                    break;
                }
            }
            stats.stop_timing();
            return Ok(stats);
        }
        
        let _processed = AtomicUsize::new(0);
        let last_progress = AtomicUsize::new(0);
        let progress_interval = 100; // Update every 100 files
        
        // Process files in parallel chunks
        let _chunk_size = self.settings.indexing.parallel_threads.max(1) * 4;
        
        // Since we need mutable access to self, we can't parallelize directly
        // For now, process sequentially with progress reporting
        for (i, file_path) in files.iter().enumerate() {
            match self.index_file(file_path) {
                Ok(file_id) => {
                    stats.files_indexed += 1;
                    
                    // Update symbol count
                    let new_symbols = self.symbol_store.find_by_file(file_id).len();
                    stats.symbols_found += new_symbols;
                }
                Err(e) => {
                    stats.add_error(file_path.clone(), e);
                }
            }
            
            // Progress reporting
            if show_progress {
                let current = i + 1;
                let last = last_progress.load(Ordering::Relaxed);
                
                if current - last >= progress_interval || current == total_files {
                    last_progress.store(current, Ordering::Relaxed);
                    eprint!("\r{}", stats.progress_line(current, total_files));
                    if current == total_files {
                        eprintln!(); // Final newline
                    }
                }
            }
        }
        
        // After indexing all files, resolve cross-file relationships
        if !dry_run && stats.files_indexed > 0 {
            self.resolve_cross_file_relationships();
        }
        
        stats.stop_timing();
        Ok(stats)
    }
    
    /// Resolve cross-file relationships using imports
    fn resolve_cross_file_relationships(&mut self) {
        // Get all unresolved relationships (where we only matched by name within the same file)
        let relationships_to_check: Vec<_> = self.data.relationships.clone();
        
        for (from_id, _to_id, rel) in relationships_to_check {
            // Get the symbol that's making the reference
            let _from_symbol = match self.symbol_store.get(from_id) {
                Some(s) => s,
                None => continue,
            };
            
            // For each relationship, try to resolve it across files
            match rel.kind {
                RelationKind::Calls => {
                    // Already handled in find_calls - could be improved with import resolution
                }
                RelationKind::Uses | RelationKind::Implements => {
                    // These relationships might reference types from other files
                    // TODO: Use import resolver to find the actual symbol
                }
                _ => {}
            }
        }
        
        // TODO: This is a placeholder. Full implementation would:
        // 1. For each unresolved symbol reference
        // 2. Use import_resolver.resolve_symbol() to find the actual symbol
        // 3. Create new relationships between symbols across files
    }
}

impl Default for SimpleIndexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolKind;
    
    #[test]
    fn test_index_simple_file() {
        // Get the path relative to the project root
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let test_file = std::path::Path::new(manifest_dir).join("tests/fixtures/simple.rs");
        
        let mut indexer = SimpleIndexer::new();
        let file_id = indexer.index_file(test_file).unwrap();
        
        assert!(file_id.value() > 0);
        assert!(indexer.symbol_count() > 0);
        
        // Should find the add function
        let add_symbol = indexer.find_symbol("add");
        assert!(add_symbol.is_some());
        
        let symbol = indexer.get_symbol(add_symbol.unwrap()).unwrap();
        assert_eq!(symbol.name.as_ref(), "add");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }
}