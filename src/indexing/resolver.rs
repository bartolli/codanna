//! Import and module path resolution for cross-file relationship building
//! 
//! This module handles:
//! - Tracking import statements (`use` declarations)
//! - Resolving module paths to actual symbols
//! - Building cross-file relationships

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::{FileId, SymbolId, SymbolStore};

/// Represents an import statement in a file
#[derive(Debug, Clone)]
pub struct Import {
    /// The path being imported (e.g., "std::collections::HashMap")
    pub path: String,
    /// The alias if any (e.g., "use foo::Bar as Baz")
    pub alias: Option<String>,
    /// Location in the file where this import appears
    pub file_id: FileId,
    /// Whether this is a glob import (e.g., "use foo::*")
    pub is_glob: bool,
}

/// Tracks module structure and imports across files
#[derive(Debug)]
pub struct ImportResolver {
    /// Maps file paths to their module paths
    file_to_module: HashMap<PathBuf, String>,
    /// Maps module paths to file paths
    module_to_file: HashMap<String, PathBuf>,
    /// Import statements by file
    imports_by_file: HashMap<FileId, Vec<Import>>,
    /// Maps file paths to FileIds
    path_to_file_id: HashMap<PathBuf, FileId>,
}

impl ImportResolver {
    pub fn new() -> Self {
        Self {
            file_to_module: HashMap::new(),
            module_to_file: HashMap::new(),
            imports_by_file: HashMap::new(),
            path_to_file_id: HashMap::new(),
        }
    }
    
    /// Register a file with its module path
    pub fn register_file(&mut self, file_path: PathBuf, file_id: FileId, module_path: String) {
        self.file_to_module.insert(file_path.clone(), module_path.clone());
        self.module_to_file.insert(module_path, file_path.clone());
        self.path_to_file_id.insert(file_path, file_id);
    }
    
    /// Add an import statement for a file
    pub fn add_import(&mut self, import: Import) {
        self.imports_by_file
            .entry(import.file_id)
            .or_insert_with(Vec::new)
            .push(import);
    }
    
    /// Resolve a symbol reference to its actual definition
    /// 
    /// Given a symbol name used in a file, this tries to resolve it to the actual
    /// symbol definition by checking:
    /// 1. Direct imports in the file
    /// 2. Glob imports
    /// 3. Prelude items (for Rust)
    pub fn resolve_symbol(
        &self,
        name: &str,
        from_file: FileId,
        symbol_store: &SymbolStore,
    ) -> Option<SymbolId> {
        // Check if there's a direct import for this name
        if let Some(imports) = self.imports_by_file.get(&from_file) {
            for import in imports {
                // Handle aliased imports
                if let Some(alias) = &import.alias {
                    if alias == name {
                        // The import path might be like "crate::foo::Bar"
                        // We need to find the symbol "Bar" in the appropriate module
                        return self.resolve_import_path(&import.path, symbol_store);
                    }
                }
                
                // Handle direct imports (e.g., "use foo::Bar" and we're looking for "Bar")
                if let Some(last_segment) = import.path.split("::").last() {
                    if last_segment == name && !import.is_glob {
                        return self.resolve_import_path(&import.path, symbol_store);
                    }
                }
                
                // Handle glob imports (e.g., "use foo::*")
                if import.is_glob {
                    // Try to find the symbol in the glob-imported module
                    let module_path = &import.path;
                    if let Some(symbol_id) = self.find_symbol_in_module(name, module_path, symbol_store) {
                        return Some(symbol_id);
                    }
                }
            }
        }
        
        // TODO: Handle prelude items and other implicit imports
        
        None
    }
    
    /// Resolve an import path to a symbol
    fn resolve_import_path(&self, path: &str, symbol_store: &SymbolStore) -> Option<SymbolId> {
        // Split the path into segments
        let segments: Vec<&str> = path.split("::").collect();
        if segments.is_empty() {
            return None;
        }
        
        // The last segment is the symbol name
        let symbol_name = segments.last()?;
        
        // Build the module path (all segments except the last)
        let module_path = segments[..segments.len() - 1].join("::");
        
        // Find the symbol in the module
        self.find_symbol_in_module(symbol_name, &module_path, symbol_store)
    }
    
    /// Find a symbol by name within a specific module
    fn find_symbol_in_module(
        &self,
        name: &str,
        module_path: &str,
        symbol_store: &SymbolStore,
    ) -> Option<SymbolId> {
        // Find all symbols with this name
        let candidates = symbol_store.find_by_name(name);
        
        // Filter by module path
        for symbol in candidates {
            if let Some(symbol_module) = &symbol.module_path {
                if symbol_module.as_ref() == module_path {
                    return Some(symbol.id);
                }
            }
        }
        
        None
    }
    
    /// Get the module path for a file
    pub fn get_module_path(&self, file_path: &Path) -> Option<&str> {
        self.file_to_module.get(file_path).map(|s| s.as_str())
    }
    
    /// Build module path from file path (for Rust projects)
    /// 
    /// Converts a file path like "src/foo/bar.rs" to a module path like "crate::foo::bar"
    pub fn module_path_from_file(file_path: &Path, project_root: &Path) -> Option<String> {
        let relative_path = file_path.strip_prefix(project_root).ok()?;
        
        // Remove the "src/" prefix if present
        let path_without_src = relative_path
            .strip_prefix("src/")
            .unwrap_or(relative_path);
        
        // Remove the file extension
        let path_str = path_without_src.to_str()?;
        let path_without_ext = if path_str.ends_with(".rs") {
            &path_str[..path_str.len() - 3]
        } else {
            path_str
        };
        
        // Handle special cases for mod.rs files BEFORE converting separators
        let module_path = if path_without_ext.ends_with("/mod") {
            // foo/mod.rs -> foo
            path_without_ext[..path_without_ext.len() - 4].to_string()
        } else {
            path_without_ext.to_string()
        };
        
        // Convert path separators to module separators
        let module_path = module_path.replace('/', "::");
        
        // Handle special cases
        let module_path = if module_path == "main" {
            "crate".to_string()
        } else if module_path == "lib" {
            "crate".to_string()
        } else if module_path.is_empty() {
            "crate".to_string()
        } else {
            format!("crate::{}", module_path)
        };
        
        Some(module_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_module_path_from_file() {
        let root = Path::new("/project");
        
        // Test main.rs
        let main_path = Path::new("/project/src/main.rs");
        assert_eq!(
            ImportResolver::module_path_from_file(main_path, root),
            Some("crate".to_string())
        );
        
        // Test lib.rs
        let lib_path = Path::new("/project/src/lib.rs");
        assert_eq!(
            ImportResolver::module_path_from_file(lib_path, root),
            Some("crate".to_string())
        );
        
        // Test regular module
        let module_path = Path::new("/project/src/foo/bar.rs");
        assert_eq!(
            ImportResolver::module_path_from_file(module_path, root),
            Some("crate::foo::bar".to_string())
        );
        
        // Test mod.rs
        let mod_path = Path::new("/project/src/foo/mod.rs");
        assert_eq!(
            ImportResolver::module_path_from_file(mod_path, root),
            Some("crate::foo".to_string())
        );
    }
}