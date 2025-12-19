//! Rust-specific language behavior implementation

use super::resolution::{RustResolutionContext, RustTraitResolver};
use crate::FileId;
use crate::Visibility;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::{InheritanceResolver, LanguageBehavior, ResolutionScope};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tree_sitter::Language;

/// Rust language behavior implementation
#[derive(Clone)]
pub struct RustBehavior {
    language: Language,
    state: BehaviorState,
    trait_resolver: Arc<RwLock<RustTraitResolver>>,
}

impl RustBehavior {
    /// Create a new Rust behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
            state: BehaviorState::new(),
            trait_resolver: Arc::new(RwLock::new(RustTraitResolver::new())),
        }
    }
}

impl StatefulBehavior for RustBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for RustBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for RustBehavior {
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        format!("{base_path}::{symbol_name}")
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        if signature.contains("pub(crate)") {
            Visibility::Crate
        } else if signature.contains("pub(super)") {
            Visibility::Module
        } else if signature.contains("pub ") || signature.starts_with("pub ") {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn supports_traits(&self) -> bool {
        true
    }

    fn supports_inherent_methods(&self) -> bool {
        true
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Get relative path from project root
        let relative_path = file_path.strip_prefix(project_root).ok()?;

        // Remove the "src/" prefix if present
        let path_without_src = relative_path.strip_prefix("src/").unwrap_or(relative_path);

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

        // Handle special cases - main, lib, and empty paths all map to crate root
        let module_path = if module_path == "main" || module_path == "lib" || module_path.is_empty()
        {
            "crate".to_string()
        } else {
            format!("crate::{module_path}")
        };

        Some(module_path)
    }

    // Override resolution methods to use Rust-specific implementations

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(RustResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        // Clone the current state of the trait resolver
        // This is a snapshot at this point in time
        let resolver = self.trait_resolver.read().unwrap();
        Box::new(resolver.clone())
    }

    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // Check scope_context first if available
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember { .. } => {
                    // Rust-specific: trait methods and impl methods should be resolvable
                    // even if they're private, for within-file resolution
                    matches!(symbol.kind, SymbolKind::Method)
                        || matches!(symbol.visibility, crate::Visibility::Public)
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

    fn add_trait_impl(&self, type_name: String, trait_name: String, file_id: FileId) {
        // Activate the actual functionality from RustTraitResolver
        let mut resolver = self.trait_resolver.write().unwrap();
        resolver.add_trait_impl(type_name, trait_name, file_id);
    }

    fn add_inherent_methods(&self, type_name: String, methods: Vec<String>) {
        // Activate the actual functionality from RustTraitResolver
        let mut resolver = self.trait_resolver.write().unwrap();
        resolver.add_inherent_methods(type_name, methods);
    }

    fn add_trait_methods(&self, trait_name: String, methods: Vec<String>) {
        // Activate the actual functionality from RustTraitResolver
        let mut resolver = self.trait_resolver.write().unwrap();
        resolver.add_trait_methods(trait_name, methods);
    }

    fn resolve_method_trait(&self, _type_name: &str, _method: &str) -> Option<&str> {
        // Note: Due to the lifetime constraints of returning &str from a RwLock,
        // we can't implement this directly. The actual resolution happens through
        // create_inheritance_resolver() which returns a snapshot of the resolver.
        // This will be addressed when we change the API to return String instead of &str.
        None
    }

    fn format_method_call(&self, receiver: &str, method: &str) -> String {
        // Rust uses :: for associated functions and . for methods
        // Since we don't have context here, default to method syntax
        format!("{receiver}.{method}")
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // Rust uses "implements" for traits
        "implements"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;
        match language_specific {
            "implements" => RelationKind::Implements,
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
            "references" => RelationKind::References,
            _ => RelationKind::References,
        }
    }

    // Override import tracking methods to use state

    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.register_file_with_state(path, file_id, module_path);
    }

    fn add_import(&self, import: crate::parsing::Import) {
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<crate::parsing::Import> {
        self.get_imports_from_state(file_id)
    }

    // Override visibility check for Rust-specific semantics
    fn is_symbol_visible_from_file(&self, symbol: &crate::Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Different file: check visibility based on Rust rules
        match symbol.visibility {
            Visibility::Public => true,
            Visibility::Crate => {
                // pub(crate) is visible from anywhere in the same crate
                // For now, assume all files are in the same crate
                // TODO: In the future, check if files are in same crate based on Cargo.toml
                true
            }
            Visibility::Module => {
                // pub(super) is visible from parent module and siblings
                // This requires module hierarchy analysis
                // For now, be conservative and return false
                false
            }
            Visibility::Private => false,
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // Case 1: Exact match (most common case, check first for performance)
        if import_path == symbol_module_path {
            return true;
        }

        // Case 1b: Handle crate:: prefix mismatch
        // Import might be "crate::foo::Bar" but symbol might be stored as "foo::Bar"
        if let Some(without_crate) = import_path.strip_prefix("crate::") {
            // Remove "crate::" prefix
            if without_crate == symbol_module_path {
                return true;
            }
        }

        // Case 1c: Reverse case - symbol has crate:: but import doesn't
        if symbol_module_path.starts_with("crate::") && !import_path.starts_with("crate::") {
            let symbol_without_crate = &symbol_module_path[7..];
            if import_path == symbol_without_crate {
                return true;
            }
        }

        // Case 1d: Common re-export pattern
        // Allow importing a re-exported name from a higher-level module:
        // import_path:  crate::parsing::Name
        // symbol_path:  crate::parsing::something::Name
        // Heuristic: same trailing name, symbol path starts with import prefix + '::'
        if let Some((import_prefix, import_name)) = import_path.rsplit_once("::") {
            // Only apply when both sides end with the same name
            if symbol_module_path.ends_with(&format!("::{import_name}")) {
                // Direct prefix check
                if symbol_module_path.starts_with(&format!("{import_prefix}::")) {
                    tracing::debug!(
                        "[rust] re-export heuristic matched (direct): import='{import_path}', symbol='{symbol_module_path}'"
                    );
                    return true;
                }

                // crate:: prefix normalization (import has crate::, symbol doesn't)
                if let Some(without_crate) = import_prefix.strip_prefix("crate::") {
                    if symbol_module_path.starts_with(&format!("{without_crate}::")) {
                        tracing::debug!(
                            "[rust] re-export heuristic matched (import had crate::): import='{import_path}', symbol='{symbol_module_path}'"
                        );
                        return true;
                    }
                }

                // crate:: prefix normalization (symbol has crate::, import doesn't)
                if symbol_module_path.starts_with("crate::")
                    && !import_prefix.starts_with("crate::")
                {
                    let symbol_without_crate = &symbol_module_path[7..];
                    if symbol_without_crate.starts_with(&format!("{import_prefix}::")) {
                        tracing::debug!(
                            "[rust] re-export heuristic matched (symbol had crate::): import='{import_path}', symbol='{symbol_module_path}'"
                        );
                        return true;
                    }
                }
            }
        }

        // Case 2: Handle super:: imports
        if import_path.starts_with("super::") {
            if let Some(importing_mod) = importing_module {
                let relative_path = import_path.strip_prefix("super::").unwrap(); // Safe: we checked starts_with

                // super:: means go up one level from the importing module
                // Example: In crate::parsing::rust, super::LanguageBehavior -> crate::parsing::LanguageBehavior
                if let Some(parent) = importing_mod.rsplit_once("::") {
                    let candidate = format!("{}::{}", parent.0, relative_path);
                    if candidate == symbol_module_path {
                        return true;
                    }

                    // Re-export heuristic for super:: imports:
                    // If the symbol lives deeper under the parent module but has the same tail name,
                    // consider it a match (common re-export pattern)
                    if symbol_module_path.ends_with(&format!("::{relative_path}"))
                        && (symbol_module_path.starts_with(&format!("{}::", parent.0))
                            || symbol_module_path == parent.0)
                    {
                        tracing::debug!(
                            "[rust] re-export heuristic matched (super): import='{import_path}', symbol='{symbol_module_path}'"
                        );
                        return true;
                    }
                }
            }
        }

        // Case 3: Only do complex matching if we have the importing module context
        if let Some(importing_mod) = importing_module {
            // Check if it's a relative import (doesn't start with crate:: or std libs)
            if !import_path.starts_with("crate::")
                && !import_path.starts_with("std::")
                && !import_path.starts_with("core::")
                && !import_path.starts_with("alloc::")
                && !import_path.starts_with("super::")
            {
                // Try as relative to importing module
                // Example: helpers::func in crate::module -> crate::module::helpers::func
                let candidate = format!("{importing_mod}::{import_path}");
                if candidate == symbol_module_path {
                    return true;
                }

                // Re-export heuristic for relative import under importing module
                if let Some((base, name)) = candidate.rsplit_once("::") {
                    if symbol_module_path.ends_with(&format!("::{name}"))
                        && (symbol_module_path.starts_with(&format!("{base}::"))
                            || symbol_module_path == base)
                    {
                        tracing::debug!(
                            "[rust] re-export heuristic matched (relative): import='{import_path}', symbol='{symbol_module_path}'"
                        );
                        return true;
                    }
                }

                // Try as sibling module (same parent)
                // Example: In crate::module::submodule, helpers::func -> crate::module::helpers::func
                if let Some(parent) = importing_mod.rsplit_once("::") {
                    let sibling = format!("{}::{}", parent.0, import_path);
                    if sibling == symbol_module_path {
                        return true;
                    }

                    // Re-export heuristic for sibling resolution
                    if let Some((base, name)) = sibling.rsplit_once("::") {
                        if symbol_module_path.ends_with(&format!("::{name}"))
                            && (symbol_module_path.starts_with(&format!("{base}::"))
                                || symbol_module_path == base)
                        {
                            tracing::debug!(
                                "[rust] re-export heuristic matched (sibling): import='{import_path}', symbol='{symbol_module_path}'"
                            );
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_module_path() {
        let behavior = RustBehavior::new();
        assert_eq!(
            behavior.format_module_path("crate::module", "function"),
            "crate::module::function"
        );
    }

    #[test]
    fn test_import_matches_symbol_reexport_cases() {
        let behavior = RustBehavior::new();

        // Exact match
        assert!(behavior.import_matches_symbol(
            "crate::parsing::LanguageBehavior",
            "crate::parsing::LanguageBehavior",
            Some("crate::parsing::rust")
        ));

        // crate:: prefix mismatch (import has crate::, symbol doesn't)
        assert!(behavior.import_matches_symbol("crate::foo::Bar", "foo::Bar", Some("crate::foo")));

        // Reverse crate:: mismatch (symbol has crate::, import doesn't)
        assert!(behavior.import_matches_symbol("foo::Bar", "crate::foo::Bar", Some("crate::foo")));

        // Re-export under deeper module (direct heuristic)
        assert!(behavior.import_matches_symbol(
            "crate::parsing::LanguageBehavior",
            "crate::parsing::language_behavior::LanguageBehavior",
            Some("crate::parsing::rust")
        ));

        // super:: import resolves to parent; symbol lives deeper under parent
        assert!(behavior.import_matches_symbol(
            "super::TypeScriptBehavior",
            "crate::parsing::typescript::behavior::TypeScriptBehavior",
            Some("crate::parsing::typescript::parser")
        ));

        // Relative import from module; symbol under submodule
        assert!(behavior.import_matches_symbol(
            "LanguageBehavior",
            "crate::parsing::language_behavior::LanguageBehavior",
            Some("crate::parsing")
        ));

        // Sibling import pattern; symbol under deeper sibling
        assert!(behavior.import_matches_symbol(
            "LanguageBehavior",
            "crate::parsing::language_behavior::LanguageBehavior",
            Some("crate::parsing::rust")
        ));
    }

    #[test]
    fn test_import_matches_symbol_negative_cases() {
        let behavior = RustBehavior::new();

        // Different tail name should not match
        assert!(!behavior.import_matches_symbol(
            "crate::parsing::Foo",
            "crate::parsing::language_behavior::Bar",
            Some("crate::parsing::rust")
        ));

        // Prefix mismatch should not match (import prefix doesn't match symbol prefix)
        assert!(!behavior.import_matches_symbol(
            "crate::utils::Helper",
            "crate::parsing::utils::Helper",
            Some("crate::utils")
        ));

        // super:: import but symbol lives outside parent module
        assert!(!behavior.import_matches_symbol(
            "super::Foo",
            "crate::x::Foo",
            Some("crate::a::b::c")
        ));

        // Relative import from module; symbol under unrelated module
        assert!(!behavior.import_matches_symbol(
            "helpers::func",
            "crate::other::helpers::func",
            Some("crate::module")
        ));

        // crate:: mismatch with different path should not match
        assert!(!behavior.import_matches_symbol(
            "crate::foo::Bar",
            "crate::bar::Bar",
            Some("crate::foo")
        ));

        // Sibling heuristic should not over-match across unrelated bases
        assert!(!behavior.import_matches_symbol(
            "LanguageBehavior",
            "crate::other::language_behavior::LanguageBehavior",
            Some("crate::parsing::rust")
        ));
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = RustBehavior::new();

        assert_eq!(
            behavior.parse_visibility("pub fn foo()"),
            Visibility::Public
        );
        assert_eq!(behavior.parse_visibility("fn foo()"), Visibility::Private);
        assert_eq!(
            behavior.parse_visibility("pub(crate) fn foo()"),
            Visibility::Crate
        );
        assert_eq!(
            behavior.parse_visibility("pub(super) fn foo()"),
            Visibility::Module
        );
    }

    #[test]
    fn test_module_separator() {
        let behavior = RustBehavior::new();
        assert_eq!(behavior.module_separator(), "::");
    }

    #[test]
    fn test_supports_features() {
        let behavior = RustBehavior::new();
        assert!(behavior.supports_traits());
        assert!(behavior.supports_inherent_methods());
    }

    #[test]
    fn test_abi_version() {
        let behavior = RustBehavior::new();
        // Rust should be on ABI-15
        assert_eq!(behavior.get_abi_version(), 15);
    }

    #[test]
    fn test_validate_node_kinds() {
        let behavior = RustBehavior::new();

        // Valid Rust node kinds
        assert!(behavior.validate_node_kind("function_item"));
        assert!(behavior.validate_node_kind("struct_item"));
        assert!(behavior.validate_node_kind("impl_item"));
        assert!(behavior.validate_node_kind("trait_item"));

        // Invalid node kind
        assert!(!behavior.validate_node_kind("made_up_node"));
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = RustBehavior::new();
        let root = Path::new("/project");

        // Test main.rs
        let main_path = Path::new("/project/src/main.rs");
        assert_eq!(
            behavior.module_path_from_file(main_path, root),
            Some("crate".to_string())
        );

        // Test lib.rs
        let lib_path = Path::new("/project/src/lib.rs");
        assert_eq!(
            behavior.module_path_from_file(lib_path, root),
            Some("crate".to_string())
        );

        // Test regular module
        let module_path = Path::new("/project/src/foo/bar.rs");
        assert_eq!(
            behavior.module_path_from_file(module_path, root),
            Some("crate::foo::bar".to_string())
        );

        // Test mod.rs
        let mod_path = Path::new("/project/src/foo/mod.rs");
        assert_eq!(
            behavior.module_path_from_file(mod_path, root),
            Some("crate::foo".to_string())
        );

        // Test nested module
        let nested_path = Path::new("/project/src/a/b/c.rs");
        assert_eq!(
            behavior.module_path_from_file(nested_path, root),
            Some("crate::a::b::c".to_string())
        );

        // Test file outside src
        let outside_path = Path::new("/project/tests/integration.rs");
        assert_eq!(
            behavior.module_path_from_file(outside_path, root),
            Some("crate::tests::integration".to_string())
        );
    }
}
