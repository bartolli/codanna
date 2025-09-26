//! Go-specific resolution and inheritance implementation
//!
//! This module implements Go's scoping and resolution rules:
//! - Package-level scope (functions, types, variables, constants)
//! - Function/method scope (parameters and local variables)
//! - Block scope (variables declared in blocks)
//! - Imported package symbols
//! - Interface implementation tracking (implicit in Go)

use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::storage::DocumentIndex;
use crate::{FileId, SymbolId};
use std::collections::HashMap;

/// Information extracted from go.mod file
///
/// Contains module metadata including name, Go version, dependencies,
/// and replace directives used for import resolution.
#[derive(Debug, Clone, Default)]
pub struct GoModInfo {
    /// Module name from 'module' directive
    pub module_name: Option<String>,

    /// Go version from 'go' directive  
    pub go_version: Option<String>,

    /// Dependencies from 'require' directives (module -> version)
    pub dependencies: HashMap<String, String>,

    /// Replace directives (from -> to)
    pub replacements: HashMap<String, String>,
}

/// Type information for Go type system resolution
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Type name (e.g., "int", "MyStruct", "Stack\[T\]")
    pub name: String,

    /// Symbol ID if this type has an associated symbol
    pub symbol_id: Option<SymbolId>,

    /// Package path where this type is defined
    pub package_path: Option<String>,

    /// Whether this type is exported (public)
    pub is_exported: bool,

    /// Type category
    pub category: TypeCategory,

    /// Generic type parameters if this is a generic type
    pub generic_params: Vec<String>,

    /// Constraints for generic parameters
    pub constraints: HashMap<String, String>,
}

/// Categories of types in Go
#[derive(Debug, Clone, PartialEq)]
pub enum TypeCategory {
    /// Built-in types (int, string, bool, etc.)
    BuiltIn,

    /// User-defined struct types
    Struct,

    /// User-defined interface types
    Interface,

    /// Type aliases (type MyInt int)
    Alias,

    /// Generic type parameters (T, K, V, etc.)
    Generic,

    /// Instantiated generic types (Stack\[string\])
    GenericInstance,
}

/// Registry for tracking all types available in the current Go context
#[derive(Debug, Default)]
pub struct TypeRegistry {
    /// All registered types by name
    types: HashMap<String, TypeInfo>,

    /// Built-in Go types (initialized once)
    built_in_types: HashMap<String, TypeInfo>,

    /// Generic type contexts (stack for nested scopes)
    generic_contexts: Vec<HashMap<String, TypeInfo>>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            types: HashMap::new(),
            built_in_types: Self::init_built_in_types(),
            generic_contexts: Vec::new(),
        };

        // Copy built-in types to main registry
        for (name, type_info) in &registry.built_in_types {
            registry.types.insert(name.clone(), type_info.clone());
        }

        registry
    }

    /// Initialize Go built-in types
    fn init_built_in_types() -> HashMap<String, TypeInfo> {
        let mut types = HashMap::new();

        // Boolean type
        types.insert(
            "bool".to_string(),
            TypeInfo {
                name: "bool".to_string(),
                symbol_id: None,
                package_path: None,
                is_exported: true,
                category: TypeCategory::BuiltIn,
                generic_params: Vec::new(),
                constraints: HashMap::new(),
            },
        );

        // String types
        for name in &["string", "byte", "rune"] {
            types.insert(
                name.to_string(),
                TypeInfo {
                    name: name.to_string(),
                    symbol_id: None,
                    package_path: None,
                    is_exported: true,
                    category: TypeCategory::BuiltIn,
                    generic_params: Vec::new(),
                    constraints: HashMap::new(),
                },
            );
        }

        // Numeric types
        for name in &[
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "float32",
            "float64",
            "complex64",
            "complex128",
            "uintptr",
        ] {
            types.insert(
                name.to_string(),
                TypeInfo {
                    name: name.to_string(),
                    symbol_id: None,
                    package_path: None,
                    is_exported: true,
                    category: TypeCategory::BuiltIn,
                    generic_params: Vec::new(),
                    constraints: HashMap::new(),
                },
            );
        }

        // Special built-in types
        for name in &["error", "any"] {
            types.insert(
                name.to_string(),
                TypeInfo {
                    name: name.to_string(),
                    symbol_id: None,
                    package_path: None,
                    is_exported: true,
                    category: TypeCategory::BuiltIn,
                    generic_params: Vec::new(),
                    constraints: HashMap::new(),
                },
            );
        }

        // Add comparable (Go 1.18+ constraint)
        types.insert(
            "comparable".to_string(),
            TypeInfo {
                name: "comparable".to_string(),
                symbol_id: None,
                package_path: None,
                is_exported: true,
                category: TypeCategory::Interface,
                generic_params: Vec::new(),
                constraints: HashMap::new(),
            },
        );

        types
    }

    /// Check if a type name represents a Go built-in type
    pub fn is_built_in_type(&self, type_name: &str) -> bool {
        self.built_in_types.contains_key(type_name)
    }

    /// Register a user-defined type
    pub fn register_type(&mut self, type_info: TypeInfo) {
        self.types.insert(type_info.name.clone(), type_info);
    }

    /// Resolve a type by name with scope-aware lookup
    pub fn resolve_type(&self, type_name: &str) -> Option<&TypeInfo> {
        // 1. Check generic contexts (most specific scope first)
        for context in self.generic_contexts.iter().rev() {
            if let Some(type_info) = context.get(type_name) {
                return Some(type_info);
            }
        }

        // 2. Check registered types (including built-ins)
        self.types.get(type_name)
    }

    /// Enter a new generic scope (for functions/types with type parameters)
    pub fn enter_generic_scope(&mut self) {
        self.generic_contexts.push(HashMap::new());
    }

    /// Exit the current generic scope
    pub fn exit_generic_scope(&mut self) {
        self.generic_contexts.pop();
    }

    /// Add a generic type parameter to current scope
    pub fn add_generic_parameter(&mut self, param_name: String, constraint: Option<String>) {
        if let Some(current_context) = self.generic_contexts.last_mut() {
            current_context.insert(
                param_name.clone(),
                TypeInfo {
                    name: param_name,
                    symbol_id: None,
                    package_path: None,
                    is_exported: false, // Type parameters are scoped
                    category: TypeCategory::Generic,
                    generic_params: Vec::new(),
                    constraints: constraint
                        .map(|c| {
                            let mut constraints = HashMap::new();
                            constraints.insert("constraint".to_string(), c);
                            constraints
                        })
                        .unwrap_or_default(),
                },
            );
        }
    }

    /// Get all types that implement a given interface
    ///
    /// This method requires an inheritance resolver to perform actual method compatibility checking.
    /// If no resolver is provided, it returns all struct types as potential candidates.
    pub fn find_types_implementing(
        &self,
        interface_name: &str,
        inheritance_resolver: Option<&GoInheritanceResolver>,
    ) -> Vec<&TypeInfo> {
        // Find all types that could implement this interface
        self.types
            .values()
            .filter(|type_info| {
                // Only structs can implement interfaces in Go
                if !matches!(type_info.category, TypeCategory::Struct) {
                    return false;
                }

                // If inheritance resolver is available, check method compatibility
                if let Some(resolver) = inheritance_resolver {
                    resolver.check_struct_implements_interface(&type_info.name, interface_name)
                } else {
                    // Fallback: assume all structs could potentially implement the interface
                    true
                }
            })
            .collect()
    }

    /// Check if a type implements an interface (requires inheritance resolver)
    pub fn type_implements_interface(
        &self,
        type_name: &str,
        interface_name: &str,
        inheritance_resolver: Option<&GoInheritanceResolver>,
    ) -> bool {
        // First check if the type is a struct
        if let Some(type_info) = self.types.get(type_name) {
            if !matches!(type_info.category, TypeCategory::Struct) {
                return false;
            }
        } else {
            return false;
        }

        // Use inheritance resolver for method compatibility checking
        if let Some(resolver) = inheritance_resolver {
            resolver.check_struct_implements_interface(type_name, interface_name)
        } else {
            // Without resolver, we cannot determine compatibility
            false
        }
    }
}

/// Go-specific resolution context handling Go's scoping rules
///
/// Go has the following scoping rules:
/// 1. Package scope - functions, types, variables, constants at package level
/// 2. Function scope - parameters and local variables within functions
/// 3. Block scope - variables declared within blocks (if, for, etc.)
/// 4. Imported symbols - symbols from imported packages
pub struct GoResolutionContext {
    /// File ID for this resolution context
    file_id: FileId,

    /// Local scope (function parameters, local variables, block variables)
    local_scope: HashMap<String, SymbolId>,

    /// Package-level symbols (functions, types, variables, constants)
    package_symbols: HashMap<String, SymbolId>,

    /// Imported symbols from other packages
    imported_symbols: HashMap<String, SymbolId>,

    /// Track nested scopes (blocks, functions, etc.)
    scope_stack: Vec<ScopeType>,

    /// Import tracking (path -> alias)
    imports: Vec<(String, Option<String>)>,

    /// Type registry for type resolution
    type_registry: TypeRegistry,
}

impl GoResolutionContext {
    /// Create a new Go resolution context for the specified file
    ///
    /// Initializes package-level scoping for Go's module system and imports
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scope: HashMap::new(),
            package_symbols: HashMap::new(),
            imported_symbols: HashMap::new(),
            scope_stack: Vec::new(),
            imports: Vec::new(),
            type_registry: TypeRegistry::new(),
        }
    }

    /// Add an import (import statement)
    pub fn add_import(&mut self, path: String, alias: Option<String>) {
        self.imports.push((path, alias));
    }

    /// Add an imported symbol to the context
    ///
    /// This is called when an import is resolved to add the symbol to the imported symbols.
    /// In Go, all imports are available in both type and value contexts.
    pub fn add_import_symbol(&mut self, name: String, symbol_id: SymbolId, _is_type_only: bool) {
        // In Go, all symbols are available in both type and value contexts
        self.imported_symbols.insert(name, symbol_id);
    }

    /// Add a symbol with proper scope context
    ///
    /// This method uses the symbol's scope_context to determine proper placement.
    /// In Go: package-level symbols, function parameters, and local variables.
    pub fn add_symbol_with_context(
        &mut self,
        name: String,
        symbol_id: SymbolId,
        scope_context: Option<&crate::symbol::ScopeContext>,
    ) {
        use crate::symbol::ScopeContext;

        match scope_context {
            Some(ScopeContext::Local { .. }) => {
                // Local variables and function parameters
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::ClassMember) => {
                // Struct/interface members - treat as local within the type
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Parameter) => {
                // Function parameters are local
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Module) | Some(ScopeContext::Global) => {
                // Package-level declarations (functions, types, variables, constants)
                self.package_symbols.insert(name, symbol_id);
            }
            Some(ScopeContext::Package) => {
                // Imported symbols
                self.imported_symbols.insert(name, symbol_id);
            }
            None => {
                // Default to package scope for Go (most symbols are package-level)
                self.package_symbols.insert(name, symbol_id);
            }
        }
    }

    /// Get the current file's module path for package comparison
    fn get_current_module_path(&self, document_index: &DocumentIndex) -> Option<String> {
        // Try to find a symbol from this file to get its module path
        if let Ok(file_symbols) = document_index.find_symbols_by_file(self.file_id) {
            if let Some(symbol) = file_symbols.first() {
                return symbol.module_path.as_ref().map(|s| s.as_ref().to_string());
            }
        }
        None
    }

    /// Resolve local package symbols (symbols in the same package)
    ///
    /// This method resolves symbols that are declared in other files
    /// within the same Go package. It properly compares module paths to ensure
    /// only symbols from the same package are considered.
    pub fn resolve_local_package_symbols(
        &self,
        symbol_name: &str,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Get the current file's module path
        let current_module_path = self.get_current_module_path(document_index)?;

        // In Go, all symbols in the same package are accessible
        // Look for symbols with matching name and package path
        if let Ok(candidates) = document_index.find_symbols_by_name(symbol_name, Some("Go")) {
            for candidate in candidates {
                // Skip symbols from the current file (already handled in local scope)
                if candidate.file_id == self.file_id {
                    continue;
                }

                // Check if symbol is in the same package (same module path)
                if let Some(ref candidate_module_path) = candidate.module_path {
                    let candidate_path = candidate_module_path.as_ref();

                    // In Go, symbols in the same package are accessible regardless of visibility
                    // But exported symbols take precedence
                    if candidate_path == current_module_path {
                        // Same package - return the symbol
                        // In Go, all symbols in the same package are accessible
                        match candidate.visibility {
                            crate::Visibility::Public => return Some(candidate.id),
                            crate::Visibility::Private => return Some(candidate.id),
                            crate::Visibility::Crate => return Some(candidate.id),
                            crate::Visibility::Module => return Some(candidate.id),
                        }
                    }
                }
            }
        }
        None
    }

    /// Resolve imported package symbols (symbols from imported packages)
    ///
    /// This method resolves symbols that come from explicitly imported packages.
    /// It handles various Go import patterns with enhanced resolution:
    /// - Standard library: "fmt", "strings", "net/http"
    /// - External modules: "github.com/user/repo/package"
    /// - Local modules: "myproject/internal/utils"
    /// - Relative imports: "./utils", "../common"
    /// - Vendor directory imports: resolved via vendor/
    pub fn resolve_imported_package_symbols(
        &self,
        package_name: &str,
        symbol_name: &str,
        document_index: &DocumentIndex,
        current_package_path: Option<&str>,
        project_root: Option<&str>,
    ) -> Option<SymbolId> {
        // Look through imports to find the package
        for (import_path, alias) in &self.imports {
            let effective_name = alias.as_deref().unwrap_or_else(|| {
                // If no alias, use the last component of the import path
                import_path.split('/').next_back().unwrap_or(import_path)
            });

            if effective_name == package_name {
                // 1. Check if it's a relative import
                if (import_path.starts_with("./") || import_path.starts_with("../"))
                    && current_package_path.is_some()
                {
                    if let Some(current_path) = current_package_path {
                        if let Some(resolved_path) =
                            self.resolve_relative_import(import_path, current_path)
                        {
                            return self.resolve_symbol_in_package(
                                &resolved_path,
                                symbol_name,
                                document_index,
                            );
                        }
                    }
                }

                // 2. Check vendor directory if project root is available
                if let Some(root) = project_root {
                    if let Some(vendor_symbol) =
                        self.resolve_vendor_import(import_path, root, document_index)
                    {
                        return Some(vendor_symbol);
                    }
                }

                // 3. Standard resolution for absolute imports
                return self.resolve_symbol_in_package(import_path, symbol_name, document_index);
            }
        }
        None
    }

    /// Resolve relative imports (./pkg, ../pkg)
    ///
    /// Handle Go relative imports which are uncommon but valid.
    /// Relative imports are resolved relative to the importing package's directory.
    pub fn resolve_relative_import(
        &self,
        import_path: &str,
        current_package_path: &str,
    ) -> Option<String> {
        if !import_path.starts_with("./") && !import_path.starts_with("../") {
            return None;
        }

        let current_parts: Vec<&str> = current_package_path.split('/').collect();
        let import_parts: Vec<&str> = import_path.split('/').collect();

        // Count the number of ".." in the import path
        let up_count = import_parts
            .iter()
            .filter(|&part| *part == ".." || *part == "../")
            .count();

        // Calculate how many parts to keep from the current path
        // Special case: if going up would leave us at just the module name,
        // and we have more to go up, we go to root (outside the module)
        let keep_count = if (up_count == current_parts.len() - 1 && current_parts.len() >= 3)
            || up_count >= current_parts.len()
        {
            0 // Go to root when traversing up to/beyond module level
        } else {
            current_parts.len() - up_count
        };

        // Start with the parts we keep from the current path
        let mut resolved_parts: Vec<&str> = current_parts.into_iter().take(keep_count).collect();

        // Add non-".." parts from the import path
        for part in import_parts {
            match part {
                "." | "./" => continue,   // Current directory
                ".." | "../" => continue, // Already handled above
                _ if !part.is_empty() => resolved_parts.push(part),
                _ => continue,
            }
        }

        Some(resolved_parts.join("/"))
    }

    /// Check for imports in vendor directory
    ///
    /// Vendor directories contain vendored dependencies and have higher
    /// priority than external modules in Go module resolution.
    pub fn resolve_vendor_import(
        &self,
        import_path: &str,
        project_root: &str,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Construct vendor path: project_root/vendor/import_path
        let vendor_path = format!("{project_root}/vendor/{import_path}");

        // Look for symbols from this vendor path
        if let Ok(candidates) = document_index.find_symbols_by_name("*", Some("Go")) {
            for candidate in candidates {
                if let Some(ref module_path) = candidate.module_path {
                    let module_str: &str = module_path.as_ref();
                    if module_str.starts_with(&vendor_path)
                        || (module_str.contains("vendor/") && module_str.ends_with(&import_path))
                    {
                        return Some(candidate.id);
                    }
                }
            }
        }

        None
    }

    /// Parse go.mod file for module information
    ///
    /// Extract module name, Go version, dependencies, and replace directives
    /// from a go.mod file.
    pub fn parse_go_mod(&self, go_mod_path: &str) -> Option<GoModInfo> {
        use std::fs;

        let content = fs::read_to_string(go_mod_path).ok()?;
        let mut info = GoModInfo::default();
        let mut in_require_block = false;

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            // Parse module directive
            if line.starts_with("module ") {
                if let Some(module_name) = line.strip_prefix("module ") {
                    info.module_name = Some(module_name.trim().to_string());
                }
            }
            // Parse go directive
            else if line.starts_with("go ") {
                if let Some(go_version) = line.strip_prefix("go ") {
                    info.go_version = Some(go_version.trim().to_string());
                }
            }
            // Parse replace directives
            else if line.starts_with("replace ") {
                if let Some(replace_part) = line.strip_prefix("replace ") {
                    if let Some((from, to)) = replace_part.split_once(" => ") {
                        info.replacements
                            .insert(from.trim().to_string(), to.trim().to_string());
                    }
                }
            }
            // Parse require directive - handle both inline and block forms
            else if line.starts_with("require ") {
                if line.ends_with("(") {
                    // Start of require block
                    in_require_block = true;
                } else {
                    // Inline require
                    if let Some(require_part) = line.strip_prefix("require ") {
                        let parts: Vec<&str> = require_part.split_whitespace().collect();
                        if parts.len() >= 2 {
                            info.dependencies
                                .insert(parts[0].to_string(), parts[1].to_string());
                        }
                    }
                }
            }
            // Handle require block content
            else if in_require_block {
                if line == ")" {
                    in_require_block = false;
                } else {
                    // Parse dependency line in block
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        info.dependencies
                            .insert(parts[0].to_string(), parts[1].to_string());
                    }
                }
            }
        }

        Some(info)
    }

    /// Apply module replacements from go.mod
    ///
    /// Handle go.mod replace directives to map import paths to local or
    /// alternative module paths.
    pub fn apply_module_replacements(&self, import_path: &str, go_mod_info: &GoModInfo) -> String {
        // Check for exact replacement
        if let Some(replacement) = go_mod_info.replacements.get(import_path) {
            return replacement.clone();
        }

        // Check for prefix replacements
        for (from_pattern, to_replacement) in &go_mod_info.replacements {
            if import_path.starts_with(from_pattern) {
                let suffix = &import_path[from_pattern.len()..];
                return format!("{to_replacement}{suffix}");
            }
        }

        // No replacement found
        import_path.to_string()
    }

    /// Handle Go module paths and go.mod resolution
    ///
    /// Resolve Go module paths using go.mod file information
    ///
    /// This method implements Go module resolution logic with go.mod parsing
    /// and module replacement support.
    pub fn handle_go_module_paths(
        &self,
        module_path: &str,
        document_index: &DocumentIndex,
    ) -> Option<String> {
        // 1. Check if the path is a standard library package
        if self.is_standard_library_package(module_path) {
            return Some(module_path.to_string());
        }

        // 2. Look for go.mod file in the project
        // This would typically walk up from the current file to find go.mod
        if let Some(go_mod_info) = self.find_and_parse_go_mod(document_index) {
            // Apply any replacements from go.mod
            let resolved_path = self.apply_module_replacements(module_path, &go_mod_info);

            // 3. Check if it's a local module (starts with module name)
            if let Some(ref module_name) = go_mod_info.module_name {
                if resolved_path.starts_with(module_name) {
                    return Some(resolved_path);
                }
            }

            return Some(resolved_path);
        }

        // 3. Fallback to assuming the module path is valid
        Some(module_path.to_string())
    }

    /// Find and parse go.mod file in the project
    ///
    /// Searches for go.mod files in the indexed codebase and finds the nearest one
    /// relative to the current file. Caches the parsed information for performance.
    fn find_and_parse_go_mod(&self, document_index: &DocumentIndex) -> Option<GoModInfo> {
        // Get all indexed paths to find go.mod files
        let all_paths = document_index.get_all_indexed_paths().ok()?;

        // Find all go.mod files in the indexed codebase
        let go_mod_files: Vec<_> = all_paths
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name == "go.mod")
                    .unwrap_or(false)
            })
            .collect();

        if go_mod_files.is_empty() {
            return None;
        }

        // Get the current file's path to find the nearest go.mod
        let current_file_path = document_index.get_file_path(self.file_id).ok()??;
        let current_path = std::path::Path::new(&current_file_path);

        // Find the nearest go.mod file by walking up the directory tree
        let mut nearest_go_mod: Option<&std::path::PathBuf> = None;
        let mut nearest_distance = usize::MAX;

        for go_mod_path in &go_mod_files {
            // Check if this go.mod is in a parent directory of the current file
            if let Some(go_mod_parent) = go_mod_path.parent() {
                if current_path.starts_with(go_mod_parent) {
                    // Calculate the distance (number of directory levels)
                    let distance = current_path
                        .strip_prefix(go_mod_parent)
                        .ok()?
                        .components()
                        .count();

                    if distance < nearest_distance {
                        nearest_distance = distance;
                        nearest_go_mod = Some(go_mod_path);
                    }
                }
            }
        }

        // If no go.mod found in parent directories, use the first one found
        if nearest_go_mod.is_none() && !go_mod_files.is_empty() {
            nearest_go_mod = Some(go_mod_files[0]);
        }

        // Parse the nearest go.mod file
        if let Some(go_mod_path) = nearest_go_mod {
            if let Some(go_mod_str) = go_mod_path.to_str() {
                return self.parse_go_mod(go_mod_str);
            }
        }

        None
    }

    /// Check if a package is part of the Go standard library
    ///
    /// This method identifies Go standard library packages that don't
    /// need explicit module resolution.
    pub fn is_standard_library_package(&self, package_path: &str) -> bool {
        // Common Go standard library packages
        // In practice, this would be a more comprehensive list or
        // determined by checking the Go installation
        const STDLIB_PACKAGES: &[&str] = &[
            "fmt",
            "strings",
            "strconv",
            "io",
            "os",
            "time",
            "context",
            "encoding/json",
            "net/http",
            "net/url",
            "path/filepath",
            "regexp",
            "sort",
            "sync",
            "errors",
            "log",
            "math",
            "bytes",
            "bufio",
            "crypto",
            "database/sql",
            "reflect",
            "runtime",
        ];

        STDLIB_PACKAGES
            .iter()
            .any(|&pkg| package_path == pkg || package_path.starts_with(&format!("{pkg}/")))
    }

    /// Resolve a symbol within a specific package
    ///
    /// Helper method to find symbols that belong to a given package path.
    fn resolve_symbol_in_package(
        &self,
        package_path: &str,
        symbol_name: &str,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        if let Ok(candidates) = document_index.find_symbols_by_name(symbol_name, Some("Go")) {
            for candidate in candidates {
                if let Some(ref module_path) = candidate.module_path {
                    let module_str: &str = module_path.as_ref();

                    // Check for exact match or last component match
                    if (module_str == package_path
                        || module_str.split('/').next_back()
                            == Some(package_path.split('/').next_back().unwrap_or(package_path)))
                        && candidate.visibility == crate::Visibility::Public
                    {
                        return Some(candidate.id);
                    }
                }
            }
        }
        None
    }

    /// Register a user-defined type
    pub fn register_type(&mut self, type_info: TypeInfo) {
        self.type_registry.register_type(type_info);
    }

    /// Resolve a type by name
    pub fn resolve_type(&self, type_name: &str) -> Option<&TypeInfo> {
        self.type_registry.resolve_type(type_name)
    }

    /// Check if a type is a built-in Go type
    pub fn is_built_in_type(&self, type_name: &str) -> bool {
        self.type_registry.is_built_in_type(type_name)
    }

    /// Enter a generic scope for function/type with type parameters
    pub fn enter_generic_scope(&mut self) {
        self.type_registry.enter_generic_scope();
    }

    /// Exit the current generic scope
    pub fn exit_generic_scope(&mut self) {
        self.type_registry.exit_generic_scope();
    }

    /// Add a generic type parameter to the current scope
    pub fn add_generic_parameter(&mut self, param_name: String, constraint: Option<String>) {
        self.type_registry
            .add_generic_parameter(param_name, constraint);
    }

    /// Parse generic type parameters from a signature like "[T any, K comparable]"
    pub fn parse_and_register_generic_params(&mut self, generic_part: &str) {
        // Remove brackets and split by comma
        let cleaned = generic_part.trim_start_matches('[').trim_end_matches(']');
        if cleaned.is_empty() {
            return;
        }

        for param in cleaned.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }

            // Parse "T any", "K comparable", "V SomeInterface", etc.
            let parts: Vec<&str> = param.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let param_name = parts[0].to_string();
            let constraint = if parts.len() > 1 {
                Some(parts[1..].join(" "))
            } else {
                None
            };

            self.add_generic_parameter(param_name, constraint);
        }
    }
}

impl ResolutionScope for GoResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                // Local scope: function parameters, local variables, block variables
                self.local_scope.insert(name, symbol_id);
            }
            ScopeLevel::Module | ScopeLevel::Global => {
                // Package-level symbols: functions, types, variables, constants
                self.package_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Package => {
                // Imported symbols from other packages
                self.imported_symbols.insert(name, symbol_id);
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Go resolution order:
        // 1. Local scope (function parameters, local variables, block variables)
        // 2. Package scope (functions, types, variables, constants)
        // 3. Imported symbols (from other packages)

        // 1. Check local scope first (most specific)
        if let Some(&id) = self.local_scope.get(name) {
            return Some(id);
        }

        // 2. Check package-level symbols
        if let Some(&id) = self.package_symbols.get(name) {
            return Some(id);
        }

        // 3. Check imported symbols
        self.imported_symbols.get(name).copied()
    }

    fn clear_local_scope(&mut self) {
        // Clear local variables and parameters when exiting scope
        self.local_scope.clear();
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);
        // No special handling needed for Go scope entry
    }

    fn exit_scope(&mut self) {
        self.scope_stack.pop();
        // Clear local scope when exiting function scope
        if matches!(
            self.scope_stack.last(),
            None | Some(ScopeType::Module | ScopeType::Global)
        ) {
            self.clear_local_scope();
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut symbols = Vec::new();

        // Add all symbols with their appropriate scope levels
        for (name, &id) in &self.local_scope {
            symbols.push((name.clone(), id, ScopeLevel::Local));
        }
        for (name, &id) in &self.package_symbols {
            symbols.push((name.clone(), id, ScopeLevel::Module));
        }
        for (name, &id) in &self.imported_symbols {
            symbols.push((name.clone(), id, ScopeLevel::Package));
        }

        symbols
    }
}

/// Go interface implementation resolution system
///
/// In Go, interface implementation is implicit - any type that has all the methods
/// of an interface automatically implements that interface. This resolver tracks:
/// - Type method definitions
/// - Interface method requirements
/// - Implicit interface implementations
pub struct GoInheritanceResolver {
    /// Maps struct names to interfaces they implement (implicit)
    /// Key: "StructName", Value: Vec<"InterfaceName">
    struct_implements: HashMap<String, Vec<String>>,

    /// Maps interface names to interfaces they embed
    /// Key: "InterfaceName", Value: Vec<"EmbeddedInterfaceName">
    interface_embeds: HashMap<String, Vec<String>>,

    /// Tracks methods on types (structs and interfaces)
    /// Key: "TypeName", Value: Vec<"method_name">
    type_methods: HashMap<String, Vec<String>>,
}

impl Default for GoInheritanceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl GoInheritanceResolver {
    /// Create a new Go inheritance resolver
    ///
    /// Tracks Go's implicit interface implementation (structural typing)
    /// and interface embedding relationships for type compatibility checking.
    pub fn new() -> Self {
        Self {
            struct_implements: HashMap::new(),
            interface_embeds: HashMap::new(),
            type_methods: HashMap::new(),
        }
    }

    /// Check if a type is an interface
    ///
    /// This method determines whether a type is an interface based on:
    /// 1. Explicit tracking via interface_embeds
    /// 2. Common Go naming conventions (interfaces often start with 'I' or end with 'er')
    /// 3. Exclusion principle (if it's not a struct, it might be an interface)
    pub fn is_interface(&self, type_name: &str) -> bool {
        // 1. Explicitly tracked interfaces
        if self.interface_embeds.contains_key(type_name) {
            return true;
        }

        // 2. Check if it's known to be a struct (has implementation relationships)
        if self.struct_implements.contains_key(type_name) {
            return false; // Definitely a struct
        }

        // 3. Common Go interface naming conventions (only if not explicitly tracked as struct)
        if type_name.starts_with('I')
            && type_name.len() > 1
            && type_name.chars().nth(1).unwrap().is_uppercase()
        {
            return true; // IReader, IWriter, etc.
        }

        // Simple heuristics for common interface patterns
        // But be more conservative - only for single words ending in "er" or "able"
        let word_count = type_name
            .split(|c: char| c.is_uppercase() && c != type_name.chars().next().unwrap())
            .count();
        if word_count == 1 && (type_name.ends_with("er") || type_name.ends_with("able")) {
            return true; // Reader, Writer, Comparable, etc.
        }

        // 4. Default to false for unknown types
        false
    }
}

impl InheritanceResolver for GoInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        match kind {
            "embeds" => {
                // Interface embeds another interface
                self.interface_embeds.entry(child).or_default().push(parent);
            }
            "implements" => {
                // Struct implements interface (implicit in Go)
                self.struct_implements
                    .entry(child)
                    .or_default()
                    .push(parent);
            }
            _ => {
                // Go doesn't have explicit inheritance like "extends"
                // Handle as interface embedding by default
                self.interface_embeds.entry(child).or_default().push(parent);
            }
        }
    }

    fn resolve_method(&self, type_name: &str, method_name: &str) -> Option<String> {
        // Check if the type has this method directly
        if let Some(methods) = self.type_methods.get(type_name) {
            if methods.iter().any(|m| m == method_name) {
                return Some(type_name.to_string());
            }
        }

        // For structs: check implemented interfaces
        if let Some(interfaces) = self.struct_implements.get(type_name) {
            for interface in interfaces {
                if let Some(methods) = self.type_methods.get(interface) {
                    if methods.iter().any(|m| m == method_name) {
                        return Some(interface.clone());
                    }
                }
                // Recursively check embedded interfaces
                if let Some(resolved) = self.resolve_method(interface, method_name) {
                    return Some(resolved);
                }
            }
        }

        // For interfaces: check embedded interfaces
        if let Some(embedded) = self.interface_embeds.get(type_name) {
            for embedded_interface in embedded {
                if let Some(resolved) = self.resolve_method(embedded_interface, method_name) {
                    return Some(resolved);
                }
            }
        }

        None
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut chain = vec![type_name.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(type_name.to_string());

        // For structs: add implemented interfaces
        if let Some(interfaces) = self.struct_implements.get(type_name) {
            for interface in interfaces {
                if visited.insert(interface.clone()) {
                    chain.push(interface.clone());
                    // Get interface's embedded interfaces
                    for embedded in self.get_inheritance_chain(interface) {
                        if visited.insert(embedded.clone()) {
                            chain.push(embedded);
                        }
                    }
                }
            }
        }

        // For interfaces: add embedded interfaces
        if let Some(embedded) = self.interface_embeds.get(type_name) {
            for embedded_interface in embedded {
                if visited.insert(embedded_interface.clone()) {
                    chain.push(embedded_interface.clone());
                    // Recursively get embedded interface's chain
                    for ancestor in self.get_inheritance_chain(embedded_interface) {
                        if visited.insert(ancestor.clone()) {
                            chain.push(ancestor);
                        }
                    }
                }
            }
        }

        chain
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        // Check if struct implements interface
        if let Some(interfaces) = self.struct_implements.get(child) {
            if interfaces.contains(&parent.to_string()) {
                return true;
            }
            // Check if any implemented interface embeds parent
            for interface in interfaces {
                if self.is_subtype(interface, parent) {
                    return true;
                }
            }
        }

        // Check interface embedding
        if let Some(embedded) = self.interface_embeds.get(child) {
            if embedded.contains(&parent.to_string()) {
                return true;
            }
            // Recursive check for embedded interfaces
            for embedded_interface in embedded {
                if self.is_subtype(embedded_interface, parent) {
                    return true;
                }
            }
        }

        false
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods
            .entry(type_name)
            .or_default()
            .extend(methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut all_methods = Vec::new();
        let mut visited = std::collections::HashSet::new();

        // Helper to collect methods recursively
        fn collect_methods(
            resolver: &GoInheritanceResolver,
            type_name: &str,
            all_methods: &mut Vec<String>,
            visited: &mut std::collections::HashSet<String>,
        ) {
            if !visited.insert(type_name.to_string()) {
                return;
            }

            // Add this type's methods
            if let Some(methods) = resolver.type_methods.get(type_name) {
                for method in methods {
                    if !all_methods.contains(method) {
                        all_methods.push(method.clone());
                    }
                }
            }

            // For structs: check implemented interfaces
            if let Some(interfaces) = resolver.struct_implements.get(type_name) {
                for interface in interfaces {
                    collect_methods(resolver, interface, all_methods, visited);
                }
            }

            // For interfaces: check embedded interfaces
            if let Some(embedded) = resolver.interface_embeds.get(type_name) {
                for embedded_interface in embedded {
                    collect_methods(resolver, embedded_interface, all_methods, visited);
                }
            }
        }

        collect_methods(self, type_name, &mut all_methods, &mut visited);
        all_methods
    }
}

/// Extension methods for GoInheritanceResolver for Go-specific operations
impl GoInheritanceResolver {
    /// Register that a struct implements an interface (implicit in Go)
    pub fn add_struct_implements(&mut self, struct_name: String, interface_name: String) {
        self.struct_implements
            .entry(struct_name)
            .or_default()
            .push(interface_name);
    }

    /// Register that an interface embeds other interfaces
    pub fn add_interface_embeds(&mut self, interface_name: String, embedded: Vec<String>) {
        self.interface_embeds.insert(interface_name, embedded);
    }

    /// Get all interfaces that a struct implements (directly and indirectly)
    pub fn get_all_interfaces(&self, struct_name: &str) -> Vec<String> {
        let mut interfaces = Vec::new();
        let mut visited = std::collections::HashSet::new();

        // Get directly implemented interfaces
        if let Some(direct) = self.struct_implements.get(struct_name) {
            for interface in direct {
                if visited.insert(interface.clone()) {
                    interfaces.push(interface.clone());
                    // Get embedded interfaces
                    for embedded in self.get_inheritance_chain(interface) {
                        if visited.insert(embedded.clone()) {
                            interfaces.push(embedded);
                        }
                    }
                }
            }
        }

        interfaces
    }

    /// Check if a struct type implements an interface
    ///
    /// This performs structural compatibility checking - in Go, a type implements
    /// an interface if it has all the methods required by the interface.
    pub fn check_struct_implements_interface(
        &self,
        struct_name: &str,
        interface_name: &str,
    ) -> bool {
        // Get methods required by the interface
        let interface_methods = self.get_all_methods(interface_name);

        // Get methods available on the struct
        let struct_methods = self.get_all_methods(struct_name);

        // Check if struct has all required interface methods
        for interface_method in &interface_methods {
            if !struct_methods.contains(interface_method) {
                return false;
            }
        }

        // If struct has all interface methods, it implements the interface
        !interface_methods.is_empty()
            && interface_methods.iter().all(|m| struct_methods.contains(m))
    }

    /// Discover all implicit interface implementations
    ///
    /// This method scans all known types and determines which structs
    /// implicitly implement which interfaces based on method signatures.
    pub fn discover_implementations(&mut self) -> Vec<(String, String)> {
        let mut implementations = Vec::new();

        // Get all struct names and interface names
        let struct_names: Vec<String> = self.struct_implements.keys().cloned().collect();
        let interface_names: Vec<String> = self
            .interface_embeds
            .keys()
            .cloned()
            .chain(
                self.type_methods
                    .keys()
                    .filter(|name| self.is_interface(name))
                    .cloned(),
            )
            .collect();

        // Check each struct against each interface
        for struct_name in &struct_names {
            for interface_name in &interface_names {
                if self.check_struct_implements_interface(struct_name, interface_name) {
                    // Register this implementation if not already known
                    if !self
                        .struct_implements
                        .get(struct_name)
                        .map(|impls| impls.contains(interface_name))
                        .unwrap_or(false)
                    {
                        self.add_struct_implements(struct_name.clone(), interface_name.clone());
                        implementations.push((struct_name.clone(), interface_name.clone()));
                    }
                }
            }
        }

        implementations
    }

    /// Find all types (structs) that implement a given interface
    pub fn find_implementations_of(&self, interface_name: &str) -> Vec<String> {
        let mut implementations = Vec::new();

        for (struct_name, interfaces) in &self.struct_implements {
            if interfaces.contains(&interface_name.to_string()) {
                implementations.push(struct_name.clone());
            }
        }

        // Also check if any other structs could implement this interface
        // based on their method sets (not yet explicitly tracked)
        for struct_name in self.type_methods.keys() {
            if !self.is_interface(struct_name)
                && !implementations.contains(struct_name)
                && self.check_struct_implements_interface(struct_name, interface_name)
            {
                implementations.push(struct_name.clone());
            }
        }

        implementations
    }

    /// Register methods for a type (struct or interface)
    pub fn register_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods.insert(type_name, methods);
    }

    /// Check if a type has a specific method
    pub fn type_has_method(&self, type_name: &str, method_name: &str) -> bool {
        if let Some(methods) = self.type_methods.get(type_name) {
            methods.contains(&method_name.to_string())
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_package_scope() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Add package-level function
        context.add_symbol(
            "MyFunction".to_string(),
            SymbolId::new(1).unwrap(),
            ScopeLevel::Module,
        );

        // Function should be resolvable at package scope
        assert_eq!(
            context.resolve("MyFunction"),
            Some(SymbolId::new(1).unwrap())
        );
    }

    #[test]
    fn test_local_scope() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Add local variable
        context.add_symbol(
            "localVar".to_string(),
            SymbolId::new(1).unwrap(),
            ScopeLevel::Local,
        );

        // Should be resolvable
        assert_eq!(context.resolve("localVar"), Some(SymbolId::new(1).unwrap()));

        // Clear local scope
        context.clear_local_scope();

        // Should no longer be resolvable
        assert_eq!(context.resolve("localVar"), None);
    }

    #[test]
    fn test_struct_implements_interface() {
        let mut resolver = GoInheritanceResolver::new();

        // Struct implements interface (implicit in Go)
        resolver.add_inheritance(
            "MyStruct".to_string(),
            "MyInterface".to_string(),
            "implements",
        );

        // Check subtype relationship
        assert!(resolver.is_subtype("MyStruct", "MyInterface"));

        // Check inheritance chain
        let chain = resolver.get_inheritance_chain("MyStruct");
        assert!(chain.contains(&"MyInterface".to_string()));
    }

    #[test]
    fn test_interface_embedding() {
        let mut resolver = GoInheritanceResolver::new();

        // Interface embeds multiple interfaces
        resolver.add_interface_embeds(
            "CompositeInterface".to_string(),
            vec!["Reader".to_string(), "Writer".to_string()],
        );

        // Check inheritance chain
        let chain = resolver.get_inheritance_chain("CompositeInterface");
        assert!(chain.contains(&"Reader".to_string()));
        assert!(chain.contains(&"Writer".to_string()));

        // Check subtype relationships
        assert!(resolver.is_subtype("CompositeInterface", "Reader"));
        assert!(resolver.is_subtype("CompositeInterface", "Writer"));
    }

    #[test]
    fn test_go_resolution_context_package_resolution() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test adding imports
        context.add_import("fmt".to_string(), None);
        context.add_import(
            "github.com/user/repo/utils".to_string(),
            Some("utils".to_string()),
        );

        assert_eq!(context.imports.len(), 2);
        assert_eq!(context.imports[0], ("fmt".to_string(), None));
        assert_eq!(
            context.imports[1],
            (
                "github.com/user/repo/utils".to_string(),
                Some("utils".to_string())
            )
        );
    }

    #[test]
    fn test_standard_library_detection() {
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test common standard library packages
        assert!(context.is_standard_library_package("fmt"));
        assert!(context.is_standard_library_package("strings"));
        assert!(context.is_standard_library_package("net/http"));
        assert!(context.is_standard_library_package("encoding/json"));

        // Test non-standard library packages
        assert!(!context.is_standard_library_package("github.com/user/repo"));
        assert!(!context.is_standard_library_package("myproject/internal/utils"));
        assert!(!context.is_standard_library_package("unknown_package"));
    }

    #[test]
    fn test_go_module_path_handling() {
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Create a minimal DocumentIndex for testing
        let temp_dir = std::env::temp_dir().join("codanna_test_go_resolution");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let document_index = DocumentIndex::new(&temp_dir).unwrap();

        // Test standard library path handling
        let stdlib_result = context.handle_go_module_paths("fmt", &document_index);
        assert_eq!(stdlib_result, Some("fmt".to_string()));

        let stdlib_subpackage = context.handle_go_module_paths("net/http", &document_index);
        assert_eq!(stdlib_subpackage, Some("net/http".to_string()));

        // Test external module path handling
        let external_result =
            context.handle_go_module_paths("github.com/user/repo", &document_index);
        assert_eq!(external_result, Some("github.com/user/repo".to_string()));

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).unwrap_or(());
    }

    #[test]
    fn test_resolve_imported_package_symbols() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Set up imports
        context.add_import("fmt".to_string(), None);
        context.add_import(
            "github.com/user/repo/utils".to_string(),
            Some("utils".to_string()),
        );

        // Create a minimal DocumentIndex for testing
        let temp_dir = std::env::temp_dir().join("codanna_test_go_resolution_2");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let document_index = DocumentIndex::new(&temp_dir).unwrap();

        // The actual resolution would require symbols in the index
        // For now, test that the method handles the imports correctly
        let result =
            context.resolve_imported_package_symbols("fmt", "Println", &document_index, None, None);
        // Result will be None since we don't have actual symbols, but it should not panic
        assert!(result.is_none());

        let result = context.resolve_imported_package_symbols(
            "utils",
            "Helper",
            &document_index,
            None,
            None,
        );
        assert!(result.is_none());

        // Test non-existent package
        let result = context.resolve_imported_package_symbols(
            "nonexistent",
            "Symbol",
            &document_index,
            None,
            None,
        );
        assert!(result.is_none());

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).unwrap_or(());
    }

    #[test]
    fn test_resolution_scope_with_package_context() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test adding symbols at different scope levels
        context.add_symbol(
            "LocalVar".to_string(),
            SymbolId::new(1).unwrap(),
            crate::parsing::ScopeLevel::Local,
        );

        context.add_symbol(
            "PackageFunc".to_string(),
            SymbolId::new(2).unwrap(),
            crate::parsing::ScopeLevel::Module,
        );

        context.add_symbol(
            "ImportedSymbol".to_string(),
            SymbolId::new(3).unwrap(),
            crate::parsing::ScopeLevel::Package,
        );

        // Test resolution order: local -> package -> imported
        assert_eq!(context.resolve("LocalVar"), Some(SymbolId::new(1).unwrap()));
        assert_eq!(
            context.resolve("PackageFunc"),
            Some(SymbolId::new(2).unwrap())
        );
        assert_eq!(
            context.resolve("ImportedSymbol"),
            Some(SymbolId::new(3).unwrap())
        );
        assert_eq!(context.resolve("NonExistent"), None);

        // Test that local scope has higher priority
        context.add_symbol(
            "ConflictingName".to_string(),
            SymbolId::new(4).unwrap(),
            crate::parsing::ScopeLevel::Local,
        );
        context.add_symbol(
            "ConflictingName".to_string(),
            SymbolId::new(5).unwrap(),
            crate::parsing::ScopeLevel::Module,
        );

        assert_eq!(
            context.resolve("ConflictingName"),
            Some(SymbolId::new(4).unwrap())
        );
    }

    #[test]
    fn test_relative_import_resolution() {
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test current directory import
        let result = context.resolve_relative_import("./utils", "myproject/internal");
        assert_eq!(result, Some("myproject/internal/utils".to_string()));

        // Test parent directory import
        let result = context.resolve_relative_import("../common", "myproject/internal");
        assert_eq!(result, Some("myproject/common".to_string()));

        // Test multiple parent directories
        let result = context.resolve_relative_import("../../shared", "myproject/pkg/internal");
        assert_eq!(result, Some("shared".to_string()));

        // Test complex relative path
        let result = context.resolve_relative_import("../lib/utils", "myproject/cmd");
        assert_eq!(result, Some("myproject/lib/utils".to_string()));

        // Test non-relative path (should return None)
        let result = context.resolve_relative_import("fmt", "myproject/internal");
        assert_eq!(result, None);

        let result = context.resolve_relative_import("github.com/user/repo", "myproject/internal");
        assert_eq!(result, None);
    }

    #[test]
    fn test_go_mod_parsing() {
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test parsing module info from content (simulate file read)
        let go_mod_content = r#"
module github.com/mycompany/myproject

go 1.21

require (
    github.com/gin-gonic/gin v1.9.1
    github.com/lib/pq v1.10.7
)

replace github.com/old/module => ../local/module
replace github.com/another/module => github.com/fork/module v1.2.3
"#;

        // Create a temporary go.mod file for testing
        let temp_dir = std::env::temp_dir().join("codanna_test_go_mod");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let go_mod_path = temp_dir.join("go.mod");
        std::fs::write(&go_mod_path, go_mod_content).unwrap();

        let result = context.parse_go_mod(go_mod_path.to_str().unwrap());
        assert!(result.is_some());

        let info = result.unwrap();
        assert_eq!(
            info.module_name,
            Some("github.com/mycompany/myproject".to_string())
        );
        assert_eq!(info.go_version, Some("1.21".to_string()));

        // Check dependencies
        assert!(info.dependencies.contains_key("github.com/gin-gonic/gin"));
        assert_eq!(info.dependencies["github.com/gin-gonic/gin"], "v1.9.1");

        // Check replacements
        assert!(info.replacements.contains_key("github.com/old/module"));
        assert_eq!(
            info.replacements["github.com/old/module"],
            "../local/module"
        );

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).unwrap_or(());
    }

    #[test]
    fn test_module_replacements() {
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        let mut go_mod_info = GoModInfo::default();
        go_mod_info.replacements.insert(
            "github.com/old/module".to_string(),
            "../local/module".to_string(),
        );
        go_mod_info.replacements.insert(
            "github.com/company".to_string(),
            "github.com/fork".to_string(),
        );

        // Test exact replacement
        let result = context.apply_module_replacements("github.com/old/module", &go_mod_info);
        assert_eq!(result, "../local/module");

        // Test prefix replacement
        let result = context.apply_module_replacements("github.com/company/repo/pkg", &go_mod_info);
        assert_eq!(result, "github.com/fork/repo/pkg");

        // Test no replacement
        let result = context.apply_module_replacements("github.com/other/module", &go_mod_info);
        assert_eq!(result, "github.com/other/module");
    }

    #[test]
    fn test_type_registry_built_ins() {
        let registry = TypeRegistry::new();

        // Test built-in type detection
        assert!(registry.is_built_in_type("int"));
        assert!(registry.is_built_in_type("string"));
        assert!(registry.is_built_in_type("bool"));
        assert!(registry.is_built_in_type("float64"));
        assert!(registry.is_built_in_type("error"));
        assert!(registry.is_built_in_type("any"));
        assert!(registry.is_built_in_type("comparable"));

        // Test non-built-in types
        assert!(!registry.is_built_in_type("MyStruct"));
        assert!(!registry.is_built_in_type("CustomInterface"));

        // Test built-in type resolution
        let int_type = registry.resolve_type("int");
        assert!(int_type.is_some());
        assert_eq!(int_type.unwrap().category, TypeCategory::BuiltIn);

        let string_type = registry.resolve_type("string");
        assert!(string_type.is_some());
        assert_eq!(string_type.unwrap().category, TypeCategory::BuiltIn);
    }

    #[test]
    fn test_type_registry_user_defined() {
        let mut registry = TypeRegistry::new();

        // Register a user-defined struct
        let struct_info = TypeInfo {
            name: "Person".to_string(),
            symbol_id: Some(SymbolId::new(1).unwrap()),
            package_path: Some("myproject/models".to_string()),
            is_exported: true,
            category: TypeCategory::Struct,
            generic_params: Vec::new(),
            constraints: HashMap::new(),
        };
        registry.register_type(struct_info);

        // Test resolution
        let person_type = registry.resolve_type("Person");
        assert!(person_type.is_some());
        let person = person_type.unwrap();
        assert_eq!(person.category, TypeCategory::Struct);
        assert!(person.is_exported);
        assert_eq!(person.package_path, Some("myproject/models".to_string()));

        // Register a generic type
        let generic_info = TypeInfo {
            name: "Stack".to_string(),
            symbol_id: Some(SymbolId::new(2).unwrap()),
            package_path: Some("myproject/containers".to_string()),
            is_exported: true,
            category: TypeCategory::Struct,
            generic_params: vec!["T".to_string()],
            constraints: HashMap::new(),
        };
        registry.register_type(generic_info);

        let stack_type = registry.resolve_type("Stack");
        assert!(stack_type.is_some());
        let stack = stack_type.unwrap();
        assert_eq!(stack.generic_params, vec!["T".to_string()]);
    }

    #[test]
    fn test_type_registry_generic_scopes() {
        let mut registry = TypeRegistry::new();

        // Enter a generic scope
        registry.enter_generic_scope();
        registry.add_generic_parameter("T".to_string(), Some("any".to_string()));
        registry.add_generic_parameter("K".to_string(), Some("comparable".to_string()));

        // Test resolution within scope
        let t_type = registry.resolve_type("T");
        assert!(t_type.is_some());
        assert_eq!(t_type.unwrap().category, TypeCategory::Generic);

        let k_type = registry.resolve_type("K");
        assert!(k_type.is_some());
        assert_eq!(k_type.unwrap().category, TypeCategory::Generic);

        // Exit scope
        registry.exit_generic_scope();

        // Should no longer be resolvable
        assert!(registry.resolve_type("T").is_none());
        assert!(registry.resolve_type("K").is_none());

        // But built-ins should still work
        assert!(registry.resolve_type("int").is_some());
    }

    #[test]
    fn test_go_resolution_context_type_integration() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test built-in type checks
        assert!(context.is_built_in_type("string"));
        assert!(context.is_built_in_type("int"));
        assert!(!context.is_built_in_type("MyType"));

        // Register a user-defined type
        let type_info = TypeInfo {
            name: "User".to_string(),
            symbol_id: Some(SymbolId::new(1).unwrap()),
            package_path: Some("myapp/models".to_string()),
            is_exported: true,
            category: TypeCategory::Struct,
            generic_params: Vec::new(),
            constraints: HashMap::new(),
        };
        context.register_type(type_info);

        // Test type resolution
        let user_type = context.resolve_type("User");
        assert!(user_type.is_some());
        assert_eq!(user_type.unwrap().category, TypeCategory::Struct);

        // Test generic parameter parsing
        context.enter_generic_scope();
        context.parse_and_register_generic_params("[T any, K comparable]");

        assert!(context.resolve_type("T").is_some());
        assert!(context.resolve_type("K").is_some());

        context.exit_generic_scope();
        assert!(context.resolve_type("T").is_none());
    }

    #[test]
    fn test_interface_implementation_detection() {
        let mut resolver = GoInheritanceResolver::new();

        // Register interface methods
        resolver.register_type_methods("Writer".to_string(), vec!["Write".to_string()]);

        // Register struct methods
        resolver.register_type_methods(
            "FileWriter".to_string(),
            vec!["Write".to_string(), "Close".to_string()],
        );

        // Test interface detection heuristics
        assert!(resolver.is_interface("Writer")); // ends with "er"
        assert!(resolver.is_interface("IReader")); // starts with "I"
        assert!(resolver.is_interface("Comparable")); // ends with "able"

        // Register FileWriter as having implementations to mark it as a struct
        resolver.add_struct_implements("FileWriter".to_string(), "Writer".to_string());
        assert!(!resolver.is_interface("FileWriter")); // now registered as struct

        // Test implementation checking
        assert!(resolver.check_struct_implements_interface("FileWriter", "Writer"));
        assert!(resolver.type_has_method("FileWriter", "Write"));
        assert!(resolver.type_has_method("FileWriter", "Close"));
        assert!(!resolver.type_has_method("FileWriter", "Read"));

        // Test finding implementations
        let implementations = resolver.find_implementations_of("Writer");
        // Should contain FileWriter since we registered it as implementing Writer
        assert!(implementations.contains(&"FileWriter".to_string()));
    }

    #[test]
    fn test_generic_parameter_parsing() {
        let mut context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test simple generic parsing
        context.enter_generic_scope();
        context.parse_and_register_generic_params("[T any]");
        assert!(context.resolve_type("T").is_some());
        context.exit_generic_scope();

        // Test complex generic parsing with constraints
        let mut test_context = GoResolutionContext::new(FileId::new(1).unwrap());
        test_context.enter_generic_scope();
        test_context.parse_and_register_generic_params("[T any, K comparable, V Serializable]");

        // All parameters should be registered
        assert!(test_context.resolve_type("T").is_some());
        assert!(test_context.resolve_type("K").is_some());
        assert!(test_context.resolve_type("V").is_some());
    }

    #[test]
    fn test_inheritance_resolver_comprehensive() {
        let mut resolver = GoInheritanceResolver::new();

        // Set up a complex inheritance hierarchy
        resolver.add_interface_embeds(
            "ReadWriter".to_string(),
            vec!["Reader".to_string(), "Writer".to_string()],
        );

        resolver.register_type_methods("Reader".to_string(), vec!["Read".to_string()]);
        resolver.register_type_methods("Writer".to_string(), vec!["Write".to_string()]);
        resolver.register_type_methods(
            "File".to_string(),
            vec!["Read".to_string(), "Write".to_string(), "Close".to_string()],
        );

        // Test method resolution
        assert_eq!(
            resolver.resolve_method("File", "Read"),
            Some("File".to_string())
        );
        assert_eq!(
            resolver.resolve_method("File", "Write"),
            Some("File".to_string())
        );
        assert!(resolver.resolve_method("File", "NonExistent").is_none());

        // Test inheritance chains
        let chain = resolver.get_inheritance_chain("ReadWriter");
        assert!(chain.contains(&"ReadWriter".to_string()));
        assert!(chain.contains(&"Reader".to_string()));
        assert!(chain.contains(&"Writer".to_string()));

        // Test all methods aggregation
        let all_methods = resolver.get_all_methods("ReadWriter");
        assert!(all_methods.contains(&"Read".to_string()));
        assert!(all_methods.contains(&"Write".to_string()));
    }

    #[test]
    fn test_method_compatibility_checking() {
        let mut registry = TypeRegistry::new();
        let mut resolver = GoInheritanceResolver::new();

        // Register a struct type
        let struct_info = TypeInfo {
            name: "MyStruct".to_string(),
            symbol_id: Some(SymbolId::new(1).unwrap()),
            package_path: Some("test/pkg".to_string()),
            is_exported: true,
            category: TypeCategory::Struct,
            generic_params: Vec::new(),
            constraints: HashMap::new(),
        };
        registry.register_type(struct_info);

        // Register interface and struct methods
        resolver.register_type_methods("MyInterface".to_string(), vec!["Method1".to_string()]);
        resolver.register_type_methods(
            "MyStruct".to_string(),
            vec!["Method1".to_string(), "Method2".to_string()],
        );

        // Test method compatibility checking without resolver
        let implementations = registry.find_types_implementing("MyInterface", None);
        assert_eq!(implementations.len(), 1); // Should return all structs as candidates

        // Test with resolver - should check method compatibility
        let implementations = registry.find_types_implementing("MyInterface", Some(&resolver));
        assert_eq!(implementations.len(), 1); // MyStruct implements MyInterface

        // Test type_implements_interface
        assert!(registry.type_implements_interface("MyStruct", "MyInterface", Some(&resolver)));
        assert!(!registry.type_implements_interface(
            "MyStruct",
            "NonExistentInterface",
            Some(&resolver)
        ));
        assert!(!registry.type_implements_interface(
            "NonExistentStruct",
            "MyInterface",
            Some(&resolver)
        ));
    }

    #[test]
    fn test_same_package_symbol_resolution() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let document_index = DocumentIndex::new(temp_dir.path()).unwrap();
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test with empty index - should return None
        let result = context.resolve_local_package_symbols("TestSymbol", &document_index);
        assert!(result.is_none());

        // Test get_current_module_path with empty index
        let module_path = context.get_current_module_path(&document_index);
        assert!(module_path.is_none());
    }

    #[test]
    fn test_go_mod_file_search() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let document_index = DocumentIndex::new(temp_dir.path()).unwrap();
        let context = GoResolutionContext::new(FileId::new(1).unwrap());

        // Test with empty index - should return None
        let go_mod_info = context.find_and_parse_go_mod(&document_index);
        assert!(go_mod_info.is_none());

        // The actual test with real go.mod files would require setting up the index
        // with file entries, which is more complex for a unit test
    }

    #[test]
    fn test_enhanced_type_registry_methods() {
        let registry = TypeRegistry::new();
        let resolver = GoInheritanceResolver::new();

        // Test finding types implementing an interface with no types registered
        let implementations = registry.find_types_implementing("SomeInterface", Some(&resolver));
        assert!(implementations.is_empty());

        // Test type_implements_interface with no types
        assert!(!registry.type_implements_interface("NonExistent", "Interface", Some(&resolver)));
        assert!(!registry.type_implements_interface("NonExistent", "Interface", None));
    }
}
