//! Import and module path resolution for cross-file relationship building
//! 
//! This module handles:
//! - Tracking import statements (`use` declarations)
//! - Resolving module paths to actual symbols
//! - Building cross-file relationships

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::{FileId, SymbolId};
use crate::storage::DocumentIndex;

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
    pub imports_by_file: HashMap<FileId, Vec<Import>>,
    /// Maps file paths to FileIds
    path_to_file_id: HashMap<PathBuf, FileId>,
}

impl Default for ImportResolver {
    fn default() -> Self {
        Self::new()
    }
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
            .or_default()
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
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        eprintln!("DEBUG ImportResolver: Trying to resolve '{}' from file {:?}", name, from_file);
        
        // Check if there's a direct import for this name
        if let Some(imports) = self.imports_by_file.get(&from_file) {
            eprintln!("DEBUG ImportResolver: Found {} imports in file", imports.len());
            for import in imports {
                // Handle aliased imports
                if let Some(alias) = &import.alias {
                    if alias == name {
                        // The import path might be like "crate::foo::Bar"
                        // We need to find the symbol "Bar" in the appropriate module
                        return self.resolve_import_path(&import.path, document_index);
                    }
                }
                
                // Handle direct imports (e.g., "use foo::Bar" and we're looking for "Bar")
                if let Some(last_segment) = import.path.split("::").last() {
                    eprintln!("DEBUG ImportResolver: Checking import path '{}', last segment: '{}'", import.path, last_segment);
                    if last_segment == name && !import.is_glob {
                        eprintln!("DEBUG ImportResolver: Match! Resolving import path '{}'", import.path);
                        return self.resolve_import_path(&import.path, document_index);
                    }
                }
                
                // Handle glob imports (e.g., "use foo::*")
                if import.is_glob {
                    // Try to find the symbol in the glob-imported module
                    let module_path = &import.path;
                    if let Some(symbol_id) = self.find_symbol_in_module(name, module_path, document_index) {
                        return Some(symbol_id);
                    }
                }
            }
        }
        
        // TODO: Handle prelude items and other implicit imports
        
        None
    }
    
    /// Resolve an import path to a symbol
    fn resolve_import_path(&self, path: &str, document_index: &DocumentIndex) -> Option<SymbolId> {
        eprintln!("DEBUG resolve_import_path: Resolving path '{}'", path);
        
        // Split the path into segments
        let segments: Vec<&str> = path.split("::").collect();
        if segments.is_empty() {
            return None;
        }
        
        // The last segment is the symbol name
        let symbol_name = segments.last()?;
        
        // Build the module path (all segments except the last)
        let module_path = segments[..segments.len() - 1].join("::");
        
        eprintln!("DEBUG resolve_import_path: Looking for symbol '{}' in module '{}'", symbol_name, module_path);
        
        // Find the symbol in the module
        self.find_symbol_in_module(symbol_name, &module_path, document_index)
    }
    
    /// Find a symbol by name within a specific module
    fn find_symbol_in_module(
        &self,
        name: &str,
        module_path: &str,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Use Tantivy to find symbols with this name
        let candidates = document_index.find_symbols_by_name(name).ok()?;
        
        // Filter by module path
        candidates.into_iter()
            .find(|symbol| {
                symbol.module_path
                    .as_ref()
                    .map(|m| m.as_ref() == module_path)
                    .unwrap_or(false)
            })
            .map(|symbol| symbol.id)
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
        let path_without_ext = path_str.strip_suffix(".rs").unwrap_or(path_str);
        
        // Handle special cases for mod.rs files BEFORE converting separators
        let module_path = if let Some(stripped) = path_without_ext.strip_suffix("/mod") {
            // foo/mod.rs -> foo
            stripped.to_string()
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
    
    #[test]
    fn test_import_registration() {
        let mut resolver = ImportResolver::new();
        
        // Register files
        let file_id_1 = FileId::new(1).unwrap();
        let file_id_2 = FileId::new(2).unwrap();
        
        resolver.register_file(PathBuf::from("src/module_a.rs"), file_id_1, "crate::module_a".to_string());
        resolver.register_file(PathBuf::from("src/main.rs"), file_id_2, "crate".to_string());
        
        // Add an import
        resolver.add_import(Import {
            path: "crate::module_a::ConfigA".to_string(),
            alias: None,
            file_id: file_id_2,
            is_glob: false,
        });
        
        // Verify file registration
        assert_eq!(resolver.get_module_path(Path::new("src/module_a.rs")), Some("crate::module_a"));
        assert_eq!(resolver.get_module_path(Path::new("src/main.rs")), Some("crate"));
        
        // Verify import was stored
        assert!(resolver.imports_by_file.contains_key(&file_id_2));
        assert_eq!(resolver.imports_by_file[&file_id_2].len(), 1);
    }
}