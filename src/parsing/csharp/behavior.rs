//! C# language behavior implementation
//!
//! This module defines how C# code is processed during indexing, including:
//! - Module path calculation (namespace handling)
//! - Import resolution (using directives)
//! - Relationship mapping (method calls, implementations)
//! - Symbol visibility rules
//! - Caller name normalization
//!
//! The behavior system provides language-specific logic that complements
//! the generic parser, allowing for proper C# semantics.

use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::paths::strip_extension;
use crate::parsing::resolution::ResolutionScope;
use crate::symbol::ScopeContext;
use crate::types::FileId;
use crate::{Symbol, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::CSharpResolutionContext;

/// C# language behavior implementation
///
/// Provides C#-specific logic for code analysis including namespace resolution,
/// using directive handling, and symbol visibility rules.
///
/// # Architecture
///
/// The behavior maintains state about processed files, imports, and module paths
/// to enable proper cross-file resolution of C# symbols.
#[derive(Clone)]
pub struct CSharpBehavior {
    state: BehaviorState,
}

impl CSharpBehavior {
    /// Create a new C# behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }

    /// Get the fully qualified class name containing this symbol (C# implementation)
    pub fn get_containing_class(&self, symbol: &Symbol) -> Option<String> {
        if let Some(ScopeContext::ClassMember {
            class_name: Some(class),
        }) = &symbol.scope_context
        {
            if let Some(pkg) = &symbol.module_path {
                if pkg.is_empty() {
                    return Some(class.to_string());
                }
                return Some(format!("{pkg}.{class}"));
            }
            return Some(class.to_string());
        }
        None
    }

    /// Check if symbol is visible from another file (C# visibility rules)
    ///
    /// C# visibility: private, protected, internal, protected internal, private protected, public
    /// For now, simplified implementation (similar to backward-compatible mode)
    pub fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // C# visibility check - simplified for now
        // TODO: Implement full C# visibility rules (internal, protected, etc.)
        match symbol.visibility {
            Visibility::Private => false,
            Visibility::Public => true,
            _ => true, // Be permissive for other levels
        }
    }
}

impl Default for CSharpBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for CSharpBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for CSharpBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("csharp")
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Set namespace as module path for C# symbols
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // C# uses namespaces as module paths, not including the symbol name
        // All symbols in the same namespace share the same module path
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            None
        } else {
            Some(components.join("."))
        }
    }

    fn module_separator(&self) -> &'static str {
        "." // C# uses dots for namespace separation
    }

    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        use crate::project_resolver::persist::ResolutionPersistence;
        use std::cell::RefCell;
        use std::time::{Duration, Instant};

        // Thread-local cache with 1-second TTL (per Go/Python/TypeScript pattern)
        thread_local! {
            static RULES_CACHE: RefCell<Option<(Instant, crate::project_resolver::persist::ResolutionIndex)>> = const { RefCell::new(None) };
        }

        // Try cached resolution first
        let cached_result = RULES_CACHE.with(|cache| {
            let mut cache_ref = cache.borrow_mut();

            // Check if cache needs reload (>1 second old or empty)
            let needs_reload = cache_ref
                .as_ref()
                .map(|(ts, _)| ts.elapsed() >= Duration::from_secs(1))
                .unwrap_or(true);

            // Load from disk if needed
            if needs_reload {
                let persistence =
                    ResolutionPersistence::new(std::path::Path::new(crate::init::local_dir_name()));
                if let Ok(index) = persistence.load("csharp") {
                    *cache_ref = Some((Instant::now(), index));
                } else {
                    *cache_ref = None;
                }
            }

            // Get module path from cached rules
            if let Some((_, ref index)) = *cache_ref {
                // Canonicalize file path for matching
                if let Ok(canon_file) = file_path.canonicalize() {
                    // Find config that applies to this file
                    if let Some(config_path) = index.get_config_for_file(&canon_file) {
                        if let Some(rules) = index.rules.get(config_path) {
                            // Get baseUrl (RootNamespace from .csproj)
                            if let Some(ref base_url) = rules.base_url {
                                // Find matching source root
                                for root_path in rules.paths.keys() {
                                    let root = std::path::Path::new(root_path);
                                    let canon_root =
                                        root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

                                    if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                                        // Get directory path (C# namespaces follow folder structure)
                                        let relative_str = relative.to_str()?;
                                        let path_without_ext =
                                            strip_extension(relative_str, extensions);
                                        let dir_path = if let Some(parent) =
                                            Path::new(path_without_ext).parent()
                                        {
                                            parent.to_str().unwrap_or("")
                                        } else {
                                            ""
                                        };

                                        // Combine baseUrl with relative directory (dots for C#)
                                        if dir_path.is_empty() {
                                            return Some(base_url.clone());
                                        } else {
                                            let namespace_suffix =
                                                dir_path.replace(['/', '\\'], ".");
                                            return Some(format!("{base_url}.{namespace_suffix}"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            None
        });

        // Return cached result if found
        if cached_result.is_some() {
            return cached_result;
        }

        // Fallback: directory-based path (original behavior)
        let relative_path = file_path
            .strip_prefix(project_root)
            .ok()
            .or_else(|| file_path.strip_prefix("./").ok())
            .unwrap_or(file_path);

        let path = relative_path.to_str()?;
        let path_without_prefix = path
            .trim_start_matches("./")
            .trim_start_matches("src/")
            .trim_start_matches("lib/");

        let module_path = strip_extension(path_without_prefix, extensions);
        let namespace_path = module_path.replace(['/', '\\'], ".");

        Some(namespace_path)
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // C# visibility modifiers in order of precedence
        if signature.contains("public ") {
            Visibility::Public
        } else if signature.contains("private ") {
            Visibility::Private
        } else if signature.contains("protected ") {
            // Map protected to Module visibility as closest approximation
            Visibility::Module
        } else if signature.contains("internal ") {
            // Internal is assembly-level visibility, map to Module
            Visibility::Module
        } else {
            // Default C# visibility depends on context:
            // - Top-level types: internal
            // - Class members: private
            // We'll default to private as most conservative
            Visibility::Private
        }
    }

    fn supports_traits(&self) -> bool {
        true // C# has interfaces
    }

    fn supports_inherent_methods(&self) -> bool {
        true // C# has class and struct methods
    }

    // C#-specific resolution
    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(CSharpResolutionContext::new(file_id))
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // C# uses both ":" for inheritance and explicit implements
        "inherits"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;

        match language_specific {
            "inherits" | "extends" => RelationKind::Extends,
            "implements" => RelationKind::Implements,
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
            _ => RelationKind::References,
        }
    }

    // Use state-based import tracking
    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.register_file_with_state(path, file_id, module_path);
    }

    fn add_import(&self, import: crate::parsing::Import) {
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<crate::parsing::Import> {
        self.get_imports_from_state(file_id)
    }

    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // C# symbols that are always resolvable
        let always_resolvable = matches!(
            symbol.kind,
            SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Method
                | SymbolKind::Field
        );

        if always_resolvable {
            return true;
        }

        // Check scope context
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember { .. } => {
                    matches!(symbol.visibility, Visibility::Public | Visibility::Module)
                }
            }
        } else {
            // Fallback for symbols without scope context
            matches!(
                symbol.kind,
                SymbolKind::TypeAlias | SymbolKind::Constant | SymbolKind::Variable
            )
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        // C# namespace matching
        // Exact match or symbol is in a sub-namespace of the import
        import_path == symbol_module_path
            || symbol_module_path.starts_with(&format!("{import_path}."))
    }
}
