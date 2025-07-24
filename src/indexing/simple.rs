use crate::{
    SymbolStore, DependencyGraph, RustParser, 
    FileId, SymbolId, Relationship, RelationKind, Symbol, IndexData
};
use std::path::Path;
use std::fs;

pub struct SimpleIndexer {
    pub symbol_store: SymbolStore,
    pub graph: DependencyGraph,
    parser: RustParser,
    data: IndexData,
}

impl SimpleIndexer {
    pub fn new() -> Self {
        Self {
            symbol_store: SymbolStore::new(),
            graph: DependencyGraph::new(),
            parser: RustParser::new().expect("Failed to create parser"),
            data: IndexData::new(),
        }
    }
    
    /// Create from loaded data
    pub fn from_data(data: IndexData) -> Self {
        let mut indexer = Self::new();
        indexer.data = data;
        
        // Rebuild in-memory structures
        for symbol in &indexer.data.symbols {
            indexer.symbol_store.insert(symbol.clone());
            indexer.graph.add_symbol(symbol.id);
        }
        
        for (from, to, rel) in &indexer.data.relationships {
            indexer.graph.add_relationship(*from, *to, rel.clone());
        }
        
        indexer
    }
    
    /// Get the data for persistence
    pub fn data(&self) -> &IndexData {
        &self.data
    }
    
    pub fn index_file(&mut self, path: impl AsRef<Path>) -> Result<FileId, String> {
        let path = path.as_ref();
        let path_str = path.to_string_lossy().to_string();
        
        // Check if we've already indexed this file
        if let Some(&file_id) = self.data.file_map.get(&path_str) {
            return Ok(file_id);
        }
        
        // Read file content
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        
        // Create file ID
        let file_id = FileId::new(self.data.file_counter)
            .ok_or_else(|| "Failed to create file ID".to_string())?;
        self.data.file_counter += 1;
        self.data.file_map.insert(path_str, file_id);
        
        // Parse symbols
        let symbols = self.parser.parse(&content, file_id);
        
        // Store symbols and add to graph
        for symbol in symbols {
            let symbol_id = symbol.id;
            self.symbol_store.insert(symbol.clone());
            self.graph.add_symbol(symbol_id);
            // Also store for serialization
            self.data.symbols.push(symbol);
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
        let implementations = self.parser.find_implementations(&content);
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
        let uses = self.parser.find_uses(&content);
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
        let defines = self.parser.find_defines(&content);
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