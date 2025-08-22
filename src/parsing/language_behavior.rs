//! Language-specific behavior abstraction
//!
//! This module provides the LanguageBehavior trait which encapsulates
//! all language-specific logic that was previously hardcoded in SimpleIndexer.
//! Each language implements this trait to define its specific conventions.
//!
//! # Architecture
//!
//! The LanguageBehavior trait is part of a larger refactoring to achieve true
//! language modularity in the codanna indexing system. It works in conjunction
//! with:
//!
//! - `LanguageParser`: Handles AST parsing for each language
//! - `ParserFactory`: Creates parser-behavior pairs
//! - `SimpleIndexer`: Uses behaviors to process symbols without language-specific code
//!
//! # Example Usage
//!
//! ```rust
//! use codanna::parsing::{ParserFactory, Language};
//! use codanna::types::{FileId, SymbolCounter};
//! use codanna::Settings;
//! use std::sync::Arc;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create settings
//! let settings = Arc::new(Settings::default());
//!
//! // Create factory and get parser-behavior pair
//! let factory = ParserFactory::new(settings);
//! let mut pair = factory.create_parser_with_behavior(Language::Rust)?;
//!
//! // Prepare parsing context
//! let code = "fn main() { println!(\"Hello\"); }";
//! let file_id = FileId::new(1).ok_or("Invalid file ID")?;
//! let mut counter = SymbolCounter::new();
//!
//! // Parse code with the parser
//! let mut symbols = pair.parser.parse(code, file_id, &mut counter);
//!
//! // Process symbols with the behavior
//! for symbol in &mut symbols {
//!     pair.behavior.configure_symbol(symbol, Some("crate::module"));
//! }
//!
//! println!("Parsed {} symbols", symbols.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Implementing a New Language
//!
//! To add support for a new language:
//!
//! 1. Create a parser implementing `LanguageParser`
//! 2. Create a behavior implementing `LanguageBehavior`
//! 3. Register both in `ParserFactory`
//! 4. (Future) Register in the language registry for auto-discovery

use crate::parsing::resolution::{
    GenericInheritanceResolver, GenericResolutionContext, InheritanceResolver, ResolutionScope,
    ScopeLevel,
};
use crate::relationship::RelationKind;
use crate::storage::DocumentIndex;
use crate::{FileId, IndexError, IndexResult, Symbol, SymbolId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// Trait for language-specific behavior and configuration
///
/// This trait extracts all language-specific logic from the indexer,
/// making the system truly language-agnostic. Each language parser
/// is paired with a behavior implementation that knows how to:
/// - Format module paths according to language conventions
/// - Parse visibility from signatures
/// - Validate node types using tree-sitter metadata
///
/// # Design Principles
///
/// 1. **Zero allocation where possible**: Methods return static strings or reuse inputs
/// 2. **Language agnostic core**: The indexer should never check language types
/// 3. **Extensible**: New languages can be added without modifying existing code
/// 4. **Type safe**: Use tree-sitter's ABI-15 for compile-time validation
pub trait LanguageBehavior: Send + Sync {
    /// Format a module path according to language conventions
    ///
    /// # Examples
    /// - Rust: `"crate::module::submodule"`
    /// - Python: `"module.submodule"`
    /// - PHP: `"\\Namespace\\Subnamespace"`
    /// - Go: `"module/submodule"`
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String;

    /// Parse visibility from a symbol's signature
    ///
    /// # Examples
    /// - Rust: `"pub fn foo()"` -> Public
    /// - Python: `"def _foo()"` -> Module (single underscore)
    /// - PHP: `"private function foo()"` -> Private
    /// - Go: `"func foo()"` -> Public
    fn parse_visibility(&self, signature: &str) -> Visibility;

    /// Get the module separator for this language
    ///
    /// # Examples
    /// - Rust: `"::"`
    /// - Python: `"."`
    /// - PHP: `"\\"`
    /// - Go: `"/"`
    fn module_separator(&self) -> &'static str;

    /// Check if this language supports trait/interface concepts
    fn supports_traits(&self) -> bool {
        false
    }

    /// Check if this language supports inherent methods
    /// (methods defined directly on types, not through traits)
    fn supports_inherent_methods(&self) -> bool {
        false
    }

    /// Get the tree-sitter Language for ABI-15 metadata access
    fn get_language(&self) -> Language;

    /// Validate that a node kind exists in this language's grammar
    /// Uses ABI-15 to check if the node type is valid
    fn validate_node_kind(&self, node_kind: &str) -> bool {
        self.get_language().id_for_node_kind(node_kind, true) != 0
    }

    /// Get the ABI version of the language grammar
    fn get_abi_version(&self) -> usize {
        self.get_language().abi_version()
    }

    /// Configure a symbol with language-specific rules
    ///
    /// This is the main entry point for applying language-specific
    /// configuration to a symbol during indexing.
    fn configure_symbol(&self, symbol: &mut Symbol, module_path: Option<&str>) {
        // Apply module path formatting
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }

        // Apply visibility parsing
        if let Some(ref sig) = symbol.signature {
            symbol.visibility = self.parse_visibility(sig);
        }
    }

    /// Calculate the module path from a file path according to language conventions
    ///
    /// This method converts a file system path to a language-specific module path.
    /// Each language has different conventions for how file paths map to module/namespace paths.
    ///
    /// # Examples
    /// - Rust: `"src/foo/bar.rs"` → `"crate::foo::bar"`
    /// - Python: `"src/package/module.py"` → `"package.module"`
    /// - PHP: `"src/Namespace/Class.php"` → `"\\Namespace\\Class"`
    /// - Go: `"src/module/submodule.go"` → `"module/submodule"`
    ///
    /// # Default Implementation
    /// Returns None by default. Languages should override this if they have
    /// specific module path conventions.
    fn module_path_from_file(&self, _file_path: &Path, _project_root: &Path) -> Option<String> {
        None
    }

    /// Resolve an import path to a symbol ID using language-specific conventions
    ///
    /// This method handles the language-specific logic for resolving import paths
    /// to actual symbols in the index. Each language has different import semantics
    /// and path formats.
    ///
    /// # Examples
    /// - Rust: `"crate::foo::Bar"` → looks for Bar in module crate::foo
    /// - Python: `"package.module.Class"` → looks for Class in package.module
    /// - PHP: `"\\App\\Controllers\\UserController"` → looks for UserController in \\App\\Controllers
    /// - Go: `"module/submodule"` → looks for submodule in module
    ///
    /// # Default Implementation
    /// The default implementation:
    /// 1. Splits the path using the language's module separator
    /// 2. Extracts the symbol name (last segment)
    /// 3. Searches for symbols with that name
    /// 4. Matches against the full module path
    fn resolve_import_path(
        &self,
        import_path: &str,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Split the path using this language's separator
        let separator = self.module_separator();
        let segments: Vec<&str> = import_path.split(separator).collect();

        if segments.is_empty() {
            return None;
        }

        // The symbol name is the last segment
        let symbol_name = segments.last()?;

        // Find symbols with this name
        let candidates = document_index
            .find_symbols_by_name(symbol_name, None)
            .ok()?;

        // Find the one with matching full module path
        for candidate in &candidates {
            if let Some(module_path) = &candidate.module_path {
                if module_path.as_ref() == import_path {
                    return Some(candidate.id);
                }
            }
        }

        None
    }

    // ========== New Resolution Methods (v0.4.1) ==========

    /// Create a language-specific resolution context
    ///
    /// Returns a resolution scope that implements the language's scoping rules.
    /// Default implementation returns a generic context that works for most languages.
    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(GenericResolutionContext::new(file_id))
    }

    /// Create a language-specific inheritance resolver
    ///
    /// Returns an inheritance resolver that handles the language's inheritance model.
    /// Default implementation returns a generic resolver.
    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(GenericInheritanceResolver::new())
    }

    /// Add an import to the language's import tracking
    ///
    /// Default implementation is a no-op. Languages should override to track imports.
    fn add_import(&self, _import: crate::parsing::Import) {
        // Default: no-op
    }

    /// Register a file with its module path
    ///
    /// Default implementation is a no-op. Languages should override to track files.
    fn register_file(&self, _path: PathBuf, _file_id: FileId, _module_path: String) {
        // Default: no-op
    }

    /// Add a trait/interface implementation
    ///
    /// Default implementation is a no-op. Languages with traits/interfaces should override.
    fn add_trait_impl(&self, _type_name: String, _trait_name: String, _file_id: FileId) {
        // Default: no-op for languages without traits
    }

    /// Add inherent methods for a type
    ///
    /// Default implementation is a no-op. Languages with inherent methods should override.
    fn add_inherent_methods(&self, _type_name: String, _methods: Vec<String>) {
        // Default: no-op for languages without inherent methods
    }

    /// Add methods that a trait/interface defines
    ///
    /// Default implementation is a no-op. Languages with traits/interfaces should override.
    fn add_trait_methods(&self, _trait_name: String, _methods: Vec<String>) {
        // Default: no-op
    }

    /// Resolve which trait/interface provides a method
    ///
    /// Returns the trait/interface name if the method comes from one, None if inherent.
    fn resolve_method_trait(&self, _type_name: &str, _method: &str) -> Option<&str> {
        None
    }

    /// Format a method call for this language
    ///
    /// Default uses the module separator (e.g., Type::method for Rust, Type.method for others)
    fn format_method_call(&self, receiver: &str, method: &str) -> String {
        format!("{}{}{}", receiver, self.module_separator(), method)
    }

    /// Get the inheritance relationship name for this language
    ///
    /// Returns "implements" for languages with interfaces, "extends" for inheritance.
    fn inheritance_relation_name(&self) -> &'static str {
        if self.supports_traits() {
            "implements"
        } else {
            "extends"
        }
    }

    /// Map language-specific relationship to generic RelationKind
    ///
    /// Allows languages to define how their concepts map to the generic relationship types.
    fn map_relationship(&self, language_specific: &str) -> RelationKind {
        match language_specific {
            "extends" => RelationKind::Extends,
            "implements" => RelationKind::Implements,
            "inherits" => RelationKind::Extends,
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
            _ => RelationKind::References,
        }
    }

    /// Build a complete resolution context for a file
    ///
    /// This is the main entry point for resolution context creation.
    /// This language-agnostic implementation:
    /// 1. Adds imports tracked by the behavior
    /// 2. Adds resolvable symbols from the current file
    /// 3. Adds visible symbols from other files
    ///
    /// Each language controls behavior through its overrides of:
    /// - `get_imports_for_file()` - what imports are available
    /// - `resolve_import()` - how imports resolve to symbols
    /// - `is_resolvable_symbol()` - what symbols can be resolved
    /// - `is_symbol_visible_from_file()` - cross-file visibility rules
    fn build_resolution_context(
        &self,
        file_id: FileId,
        document_index: &DocumentIndex,
    ) -> IndexResult<Box<dyn ResolutionScope>> {
        // Create language-specific resolution context
        let mut context = self.create_resolution_context(file_id);

        // 1. Add imported symbols using behavior's tracked imports
        let imports = self.get_imports_for_file(file_id);
        for import in imports {
            if let Some(symbol_id) = self.resolve_import(&import, document_index) {
                // Use alias if provided, otherwise use the last segment of the path
                let name = if let Some(alias) = &import.alias {
                    alias.clone()
                } else {
                    // Let the language determine the separator
                    import
                        .path
                        .split(self.module_separator())
                        .last()
                        .unwrap_or(&import.path)
                        .to_string()
                };

                // Add as imported symbol (higher priority than module symbols)
                context.add_symbol(name, symbol_id, ScopeLevel::Package);
            }
        }

        // 2. Add file's module-level symbols
        let file_symbols =
            document_index
                .find_symbols_by_file(file_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in file_symbols {
            if self.is_resolvable_symbol(&symbol) {
                context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Module);
            }
        }

        // 3. Add visible symbols from other files (public/exported symbols)
        // Note: This is expensive, so we limit to a reasonable number
        let all_symbols =
            document_index
                .get_all_symbols(10000)
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_all_symbols".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in all_symbols {
            // Skip symbols from the current file (already added above)
            if symbol.file_id == file_id {
                continue;
            }

            // Check if this symbol is visible from the current file
            if self.is_symbol_visible_from_file(&symbol, file_id) {
                // Add as global symbol (lower priority)
                context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Global);
            }
        }

        Ok(context)
    }

    /// Check if a symbol should be resolvable (added to resolution context)
    ///
    /// Languages override this to filter which symbols are available for resolution.
    /// For example, local variables might not be resolvable from other scopes.
    ///
    /// Default implementation includes common top-level symbols.
    fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool {
        use crate::SymbolKind;

        // Check scope_context first if available
        if let Some(ref scope_context) = symbol.scope_context {
            use crate::symbol::ScopeContext;
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember => {
                    // Class members might be resolvable depending on visibility
                    matches!(symbol.visibility, Visibility::Public)
                }
            }
        } else {
            // Fallback to symbol kind for backward compatibility
            matches!(
                symbol.kind,
                SymbolKind::Function
                    | SymbolKind::Method
                    | SymbolKind::Struct
                    | SymbolKind::Trait
                    | SymbolKind::Interface
                    | SymbolKind::Class
                    | SymbolKind::TypeAlias
                    | SymbolKind::Enum
                    | SymbolKind::Constant
            )
        }
    }

    /// Check if a symbol is visible from another file
    ///
    /// Languages implement their visibility rules here.
    /// For example, Rust checks pub, Python might check __all__, etc.
    ///
    /// Default implementation checks basic visibility.
    fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Different file: check visibility
        matches!(symbol.visibility, Visibility::Public)
    }

    /// Get imports for a file
    ///
    /// Returns the list of imports that were registered for this file.
    /// Languages should track imports when add_import() is called.
    ///
    /// Default implementation returns empty (no imports).
    fn get_imports_for_file(&self, _file_id: FileId) -> Vec<crate::parsing::Import> {
        Vec::new()
    }

    /// Resolve an import to a symbol ID
    ///
    /// Takes an import and resolves it to an actual symbol in the index.
    /// Languages implement their specific import resolution logic here.
    ///
    /// Default implementation tries basic name matching.
    fn resolve_import(
        &self,
        import: &crate::parsing::Import,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Get the importing module path for context
        let importing_module = self.get_module_path_for_file(import.file_id);

        // Use enhanced resolution with module context
        self.resolve_import_path_with_context(
            &import.path,
            importing_module.as_deref(),
            document_index,
        )
    }

    /// Check if an import path matches a symbol's module path
    ///
    /// This allows each language to implement custom matching rules.
    /// For example, Rust needs to handle relative imports where
    /// "helpers::func" should match "crate::module::helpers::func"
    /// when imported from "crate::module".
    ///
    /// # Arguments
    /// * `import_path` - The import path as written in source
    /// * `symbol_module_path` - The full module path of the symbol
    /// * `importing_module` - The module doing the importing (if known)
    ///
    /// # Default Implementation
    /// Exact match only. Languages should override for relative imports.
    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        import_path == symbol_module_path
    }

    /// Get the module path for a file from behavior state
    ///
    /// Default implementation returns None. Languages with state tracking
    /// should override to return the module path.
    fn get_module_path_for_file(&self, _file_id: FileId) -> Option<String> {
        None
    }

    /// Enhanced import path resolution with module context
    ///
    /// This is separate from resolve_import_path for backward compatibility.
    /// The default implementation uses import_matches_symbol for matching.
    fn resolve_import_path_with_context(
        &self,
        import_path: &str,
        importing_module: Option<&str>,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Split the path using this language's separator
        let separator = self.module_separator();
        let segments: Vec<&str> = import_path.split(separator).collect();

        if segments.is_empty() {
            return None;
        }

        // The symbol name is the last segment
        let symbol_name = segments.last()?;

        // Find symbols with this name (using index for performance)
        let candidates = document_index
            .find_symbols_by_name(symbol_name, None)
            .ok()?;

        // Find the one with matching module path using language-specific rules
        for candidate in &candidates {
            if let Some(module_path) = &candidate.module_path {
                if self.import_matches_symbol(import_path, module_path.as_ref(), importing_module) {
                    return Some(candidate.id);
                }
            }
        }

        None
    }
}

/// Language metadata from ABI-15
#[derive(Debug, Clone)]
pub struct LanguageMetadata {
    pub abi_version: usize,
    pub node_kind_count: usize,
    pub field_count: usize,
}

impl LanguageMetadata {
    /// Create metadata from a tree-sitter Language
    pub fn from_language(language: Language) -> Self {
        Self {
            abi_version: language.abi_version(),
            node_kind_count: language.node_kind_count(),
            field_count: language.field_count(),
        }
    }
}
