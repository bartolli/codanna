//! Resolution context for accurate symbol resolution
//! 
//! Tracks what symbols are available in different scopes to enable
//! accurate resolution of symbol references.

use std::collections::HashMap;
use crate::{SymbolId, FileId};

/// Represents a symbol that can be referenced in a scope
#[derive(Debug, Clone)]
pub struct ScopedSymbol {
    /// The symbol's unique identifier
    pub id: SymbolId,
    /// The name as it appears in this scope (might be aliased)
    pub name: String,
    /// Whether this symbol is directly imported or in scope
    pub is_imported: bool,
}

/// Context for resolving symbols within a specific scope
#[derive(Debug)]
pub struct ResolutionContext {
    /// Current file being processed
    pub file_id: FileId,
    
    /// Local variables and parameters in current scope
    /// Key: variable name, Value: symbol info
    local_scope: HashMap<String, ScopedSymbol>,
    
    /// Symbols imported into this file
    /// Key: symbol name (or alias), Value: symbol info
    imported_symbols: HashMap<String, ScopedSymbol>,
    
    /// Symbols defined at module level in current file
    /// Key: symbol name, Value: symbol info
    module_symbols: HashMap<String, ScopedSymbol>,
    
    /// Public symbols visible from the crate
    /// Key: symbol name, Value: symbol info
    crate_symbols: HashMap<String, ScopedSymbol>,
}

impl ResolutionContext {
    /// Create a new resolution context for a file
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scope: HashMap::new(),
            imported_symbols: HashMap::new(),
            module_symbols: HashMap::new(),
            crate_symbols: HashMap::new(),
        }
    }
    
    /// Add a local variable or parameter to the current scope
    pub fn add_local(&mut self, name: String, symbol_id: SymbolId) {
        self.local_scope.insert(name.clone(), ScopedSymbol {
            id: symbol_id,
            name,
            is_imported: false,
        });
    }
    
    /// Add an imported symbol
    pub fn add_import(&mut self, name: String, symbol_id: SymbolId, _is_aliased: bool) {
        self.imported_symbols.insert(name.clone(), ScopedSymbol {
            id: symbol_id,
            name,
            is_imported: true,
        });
    }
    
    /// Add a module-level symbol from the current file
    pub fn add_module_symbol(&mut self, name: String, symbol_id: SymbolId) {
        self.module_symbols.insert(name.clone(), ScopedSymbol {
            id: symbol_id,
            name,
            is_imported: false,
        });
    }
    
    /// Add a crate-level public symbol
    pub fn add_crate_symbol(&mut self, name: String, symbol_id: SymbolId) {
        self.crate_symbols.insert(name.clone(), ScopedSymbol {
            id: symbol_id,
            name,
            is_imported: false,
        });
    }
    
    /// Clear local scope (e.g., when exiting a function)
    pub fn clear_local_scope(&mut self) {
        self.local_scope.clear();
    }
    
    /// Resolve a symbol name following Rust's scoping rules
    /// Returns the resolved symbol ID if found
    /// 
    /// Resolution order:
    /// 1. Local scope (variables, parameters)
    /// 2. Imported symbols
    /// 3. Module-level symbols
    /// 4. Crate public symbols
    /// 5. TODO: Prelude items
    pub fn resolve(&self, name: &str) -> Option<SymbolId> {
        // 1. Check local scope first
        if let Some(symbol) = self.local_scope.get(name) {
            return Some(symbol.id);
        }
        
        // 2. Check imported symbols
        if let Some(symbol) = self.imported_symbols.get(name) {
            return Some(symbol.id);
        }
        
        // 3. Check module-level symbols
        if let Some(symbol) = self.module_symbols.get(name) {
            return Some(symbol.id);
        }
        
        // 4. Check crate public symbols
        if let Some(symbol) = self.crate_symbols.get(name) {
            return Some(symbol.id);
        }
        
        // 5. TODO: Check prelude items (would need standard library symbols)
        
        None
    }
    
    /// Get all symbols in current scope (for debugging)
    pub fn all_symbols_in_scope(&self) -> Vec<&ScopedSymbol> {
        self.local_scope.values()
            .chain(self.imported_symbols.values())
            .chain(self.module_symbols.values())
            .chain(self.crate_symbols.values())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resolution_order() {
        let file_id = FileId::new(1).unwrap();
        let mut ctx = ResolutionContext::new(file_id);
        
        // Add symbols with same name at different scopes
        let local_id = SymbolId::new(1).unwrap();
        let import_id = SymbolId::new(2).unwrap();
        let module_id = SymbolId::new(3).unwrap();
        let crate_id = SymbolId::new(4).unwrap();
        
        ctx.add_crate_symbol("test".to_string(), crate_id);
        ctx.add_module_symbol("test".to_string(), module_id);
        ctx.add_import("test".to_string(), import_id, false);
        ctx.add_local("test".to_string(), local_id);
        
        // Should resolve to local scope first
        assert_eq!(ctx.resolve("test"), Some(local_id));
        
        // Clear local scope
        ctx.clear_local_scope();
        
        // Should now resolve to imported symbol
        assert_eq!(ctx.resolve("test"), Some(import_id));
    }
    
    #[test]
    fn test_aliased_imports() {
        let file_id = FileId::new(1).unwrap();
        let mut ctx = ResolutionContext::new(file_id);
        
        let symbol_id = SymbolId::new(1).unwrap();
        
        // Add aliased import (e.g., use foo::Bar as Baz)
        ctx.add_import("Baz".to_string(), symbol_id, true);
        
        // Should resolve the alias
        assert_eq!(ctx.resolve("Baz"), Some(symbol_id));
        
        // Original name should not resolve
        assert_eq!(ctx.resolve("Bar"), None);
    }
    
    #[test]
    fn test_scope_isolation() {
        let file_id = FileId::new(1).unwrap();
        let mut ctx = ResolutionContext::new(file_id);
        
        let local_id = SymbolId::new(1).unwrap();
        let module_id = SymbolId::new(2).unwrap();
        
        // Add module symbol
        ctx.add_module_symbol("foo".to_string(), module_id);
        
        // Add local with same name
        ctx.add_local("foo".to_string(), local_id);
        
        // Should resolve to local
        assert_eq!(ctx.resolve("foo"), Some(local_id));
        
        // Clear local scope
        ctx.clear_local_scope();
        
        // Should now resolve to module symbol
        assert_eq!(ctx.resolve("foo"), Some(module_id));
    }
}