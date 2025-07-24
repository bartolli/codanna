use crate::{
    SymbolStore, DependencyGraph, RustParser, 
    FileId, SymbolId, Relationship, RelationKind
};
use std::path::Path;
use std::fs;
use std::collections::HashMap;

pub struct SimpleIndexer {
    symbol_store: SymbolStore,
    graph: DependencyGraph,
    parser: RustParser,
    file_counter: u32,
    file_map: HashMap<String, FileId>,
}

impl SimpleIndexer {
    pub fn new() -> Self {
        Self {
            symbol_store: SymbolStore::new(),
            graph: DependencyGraph::new(),
            parser: RustParser::new().expect("Failed to create parser"),
            file_counter: 1,
            file_map: HashMap::new(),
        }
    }
    
    pub fn index_file(&mut self, path: impl AsRef<Path>) -> Result<FileId, String> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy().to_string();
        
        // Check if we've already indexed this file
        if let Some(&file_id) = self.file_map.get(&path_str) {
            return Ok(file_id);
        }
        
        // Read file content
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        
        // Create file ID
        let file_id = FileId::new(self.file_counter)
            .ok_or_else(|| "Failed to create file ID".to_string())?;
        self.file_counter += 1;
        self.file_map.insert(path_str, file_id);
        
        // Parse symbols
        let symbols = self.parser.parse(&content, file_id);
        
        // Store symbols and add to graph
        for symbol in symbols {
            let symbol_id = symbol.id;
            self.symbol_store.insert(symbol);
            self.graph.add_symbol(symbol_id);
        }
        
        // Find and store relationships (function calls)
        let calls = self.parser.find_calls(&content);
        for (caller_name, callee_name, _range) in calls {
            // Find symbols by name
            let callers = self.symbol_store.find_by_name(&caller_name);
            let callees = self.symbol_store.find_by_name(&callee_name);
            
            // Add relationships for all matching symbols
            for caller in &callers {
                for callee in &callees {
                    if caller.file_id == file_id {
                        self.graph.add_relationship(
                            caller.id,
                            callee.id,
                            Relationship::new(RelationKind::Calls),
                        );
                    }
                }
            }
        }
        
        Ok(file_id)
    }
    
    pub fn find_symbol(&self, name: &str) -> Option<SymbolId> {
        self.symbol_store.find_by_name(name)
            .into_iter()
            .next()
            .map(|s| s.id)
    }
    
    pub fn get_symbol(&self, id: SymbolId) -> Option<crate::Symbol> {
        self.symbol_store.get(id)
    }
    
    pub fn find_symbols_by_name(&self, name: &str) -> Vec<crate::Symbol> {
        self.symbol_store.find_by_name(name)
    }
    
    pub fn get_called_functions(&self, symbol_id: SymbolId) -> Vec<crate::Symbol> {
        self.graph.get_relationships(symbol_id, RelationKind::Calls)
            .into_iter()
            .filter_map(|id| self.symbol_store.get(id))
            .collect()
    }
    
    pub fn get_calling_functions(&self, symbol_id: SymbolId) -> Vec<crate::Symbol> {
        self.graph.get_incoming_relationships(symbol_id, RelationKind::Calls)
            .into_iter()
            .filter_map(|id| self.symbol_store.get(id))
            .collect()
    }
    
    pub fn get_all_symbols(&self) -> Vec<crate::Symbol> {
        self.symbol_store.iter().collect()
    }
    
    pub fn symbol_count(&self) -> usize {
        self.symbol_store.len()
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
        let mut indexer = SimpleIndexer::new();
        let file_id = indexer.index_file("tests/fixtures/simple.rs").unwrap();
        
        assert!(file_id.value() > 0);
        assert!(indexer.symbol_count() > 0);
        
        // Should find the add function
        let add_symbol = indexer.find_symbol("add");
        assert!(add_symbol.is_some());
        
        let symbol = indexer.get_symbol(add_symbol.unwrap()).unwrap();
        assert_eq!(symbol.name.as_ref(), "add");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }
    
    #[test]
    fn test_index_file_with_function_calls() {
        let mut indexer = SimpleIndexer::new();
        indexer.index_file("tests/fixtures/calls.rs").unwrap();
        
        // Find main function
        let main_id = indexer.find_symbol("main").unwrap();
        let main_fn = indexer.get_symbol(main_id).unwrap();
        assert_eq!(main_fn.name.as_ref(), "main");
        
        // Check what main calls
        let called = indexer.get_called_functions(main_id);
        let called_names: Vec<&str> = called.iter()
            .map(|s| s.name.as_ref())
            .collect();
        
        assert!(called_names.contains(&"process_batch"));
        
        // Find helper function
        let helper_id = indexer.find_symbol("helper").unwrap();
        
        // Check what calls helper
        let callers = indexer.get_calling_functions(helper_id);
        let caller_names: Vec<&str> = callers.iter()
            .map(|s| s.name.as_ref())
            .collect();
        
        assert!(caller_names.contains(&"process_single"));
    }
    
    #[test]
    fn test_find_multiple_symbols() {
        let mut indexer = SimpleIndexer::new();
        indexer.index_file("tests/fixtures/types.rs").unwrap();
        
        // Should find both struct and trait named Operation
        let symbols = indexer.find_symbols_by_name("Operation");
        assert_eq!(symbols.len(), 1); // Only the trait is named Operation
        assert_eq!(symbols[0].kind, SymbolKind::Trait);
        
        // Should find Addition struct
        let addition = indexer.find_symbol("Addition");
        assert!(addition.is_some());
    }
    
    #[test]
    fn test_method_extraction() {
        let mut indexer = SimpleIndexer::new();
        indexer.index_file("tests/fixtures/simple.rs").unwrap();
        
        // Should find methods from impl blocks
        let new_method = indexer.find_symbol("new");
        assert!(new_method.is_some(), "Could not find 'new' method");
        
        let symbol = indexer.get_symbol(new_method.unwrap()).unwrap();
        assert_eq!(symbol.kind, SymbolKind::Method);
        
        // Also check other methods
        let distance = indexer.find_symbol("distance");
        assert!(distance.is_some());
        assert_eq!(indexer.get_symbol(distance.unwrap()).unwrap().kind, SymbolKind::Method);
    }
}