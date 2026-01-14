//! Go-specific language behavior implementation

use crate::Visibility;
use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::resolution::{InheritanceResolver, ResolutionScope};
use crate::types::FileId;
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::{GoInheritanceResolver, GoResolutionContext};

/// Go language behavior implementation
#[derive(Clone)]
pub struct GoBehavior {
    state: BehaviorState,
}

impl GoBehavior {
    /// Create a new Go behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }
}

impl Default for GoBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for GoBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for GoBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("go")
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // Go uses file paths as module paths, not including the symbol name
        // All symbols in the same file share the same module path for visibility
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_go::LANGUAGE.into()
    }
    fn module_separator(&self) -> &'static str {
        "/"
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            Some(".".to_string())
        } else {
            Some(components.join("/"))
        }
    }

    // Go uses directory-based packages, needs custom handling
    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        use crate::parsing::paths::strip_extension;

        // Convert file path to Go package path relative to project root
        // e.g., pkg/utils/helpers.go -> pkg/utils

        // Get relative path from project root
        let relative_path = file_path
            .strip_prefix(project_root)
            .ok()
            .or_else(|| file_path.strip_prefix("./").ok())
            .unwrap_or(file_path);

        let path = relative_path.to_str()?;

        // Remove Go file extension using passed extensions and get directory
        let path_clean = path.trim_start_matches("./");
        let module_path = strip_extension(path_clean, extensions);

        // Get directory path (Go packages are directories)
        let dir_path = if let Some(parent) = Path::new(module_path).parent() {
            parent.to_str().unwrap_or("")
        } else {
            "" // Root package
        };

        // Convert empty path to current directory marker
        if dir_path.is_empty() {
            Some(".".to_string())
        } else {
            Some(dir_path.to_string())
        }
    }

    /// Parse visibility from Go symbol signature using capitalization rules
    ///
    /// In Go, visibility is determined by the first character of the identifier:
    /// - Uppercase first letter = Public/Exported (accessible outside package)
    /// - Lowercase first letter = Private/Unexported (package-private)
    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Go uses capitalization for visibility
        // Extract the symbol name from the signature and check if it starts with uppercase

        // Try to extract the symbol name from different signature patterns
        let symbol_name = if let Some(func_start) = signature.find("func ") {
            // Function signature: "func FunctionName(" or "func (receiver) MethodName("
            let after_func = &signature[func_start + 5..].trim_start();
            if after_func.starts_with('(') {
                // Method with receiver: "func (r *Type) MethodName("
                if let Some(receiver_end) = after_func.find(") ") {
                    let after_receiver = &after_func[receiver_end + 2..].trim_start();
                    after_receiver.split('(').next().unwrap_or("").trim()
                } else {
                    ""
                }
            } else {
                // Regular function: "func FunctionName("
                after_func.split('(').next().unwrap_or("").trim()
            }
        } else if let Some(type_start) = signature.find("type ") {
            // Type signature: "type TypeName struct" or "type TypeName interface"
            let after_type = &signature[type_start + 5..];
            after_type.split_whitespace().next().unwrap_or("")
        } else if let Some(var_start) = signature.find("var ") {
            // Variable signature: "var VariableName type"
            let after_var = &signature[var_start + 4..];
            after_var.split_whitespace().next().unwrap_or("")
        } else if let Some(const_start) = signature.find("const ") {
            // Constant signature: "const ConstantName = value"
            let after_const = &signature[const_start + 6..];
            after_const.split_whitespace().next().unwrap_or("")
        } else {
            // Fallback: take the first word that looks like an identifier
            signature
                .split_whitespace()
                .find(|word| word.chars().next().is_some_and(|c| c.is_alphabetic()))
                .unwrap_or("")
        };

        // Go visibility: uppercase first letter = public, lowercase = private
        if let Some(first_char) = symbol_name.chars().next() {
            if first_char.is_uppercase() {
                Visibility::Public
            } else {
                Visibility::Private
            }
        } else {
            Visibility::Private
        }
    }

    /// Go uses interfaces instead of traits
    ///
    /// Go's interface system provides similar functionality to traits
    /// but uses structural typing (duck typing) rather than explicit implementation.
    fn supports_traits(&self) -> bool {
        false // Go has interfaces, not traits (traits are a Rust concept)
    }

    /// Go supports methods on types (inherent methods)
    ///
    /// Methods can be defined on any named type using receiver syntax:
    /// `func (r ReceiverType) MethodName() {}`
    fn supports_inherent_methods(&self) -> bool {
        true // Go has methods on types
    }

    // Go-specific resolution overrides

    /// Create a Go-specific resolution context for symbol resolution
    ///
    /// Returns a GoResolutionContext that handles Go's package-based scoping,
    /// import resolution, and module system integration.
    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(GoResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(GoInheritanceResolver::new())
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // Go uses interface implementation (implicit)
        // There's no explicit "extends" or "implements" in Go
        "implements"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;

        match language_specific {
            "extends" => RelationKind::Extends,
            "implements" => RelationKind::Implements,
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
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

    // Go-specific: Symbol resolution rules
    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // Go allows forward references for functions, types, and constants at package level
        let package_level_symbol = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Struct
                | SymbolKind::Interface
                | SymbolKind::Constant
                | SymbolKind::TypeAlias
        );

        if package_level_symbol {
            return true;
        }

        // Methods are always resolvable within their file
        if matches!(symbol.kind, SymbolKind::Method) {
            return true;
        }

        // Check scope_context for other symbols
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember { .. } => {
                    // Struct/interface members are resolvable if exported (uppercase)
                    matches!(symbol.visibility, Visibility::Public)
                }
            }
        } else {
            // Fallback for symbols without scope_context
            matches!(symbol.kind, SymbolKind::TypeAlias | SymbolKind::Variable)
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Apply Go-specific module path formatting
        if let Some(path) = module_path {
            // Go uses package paths, not including symbol names
            symbol.module_path = Some(path.to_string().into());
        }

        // Apply Go visibility parsing based on capitalization
        if let Some(ref sig) = symbol.signature {
            symbol.visibility = self.parse_visibility(sig);
        }

        // Set Go-specific symbol properties
        // Go symbols are package-scoped by default
        if symbol.module_path.is_none() {
            symbol.module_path = Some(".".to_string().into()); // Current package
        }
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // Helper function to resolve relative path to absolute module path for Go
        fn resolve_relative_path(import_path: &str, importing_mod: &str) -> String {
            if import_path.starts_with("./") {
                // Same directory import
                let relative = import_path.trim_start_matches("./");

                if importing_mod.is_empty() || importing_mod == "." {
                    relative.to_string()
                } else {
                    format!("{importing_mod}/{relative}")
                }
            } else if import_path.starts_with("../") {
                // Parent directory import
                // Start with the importing module parts as owned strings
                let mut module_parts: Vec<String> =
                    importing_mod.split('/').map(|s| s.to_string()).collect();

                let mut path_remaining: &str = import_path;

                // Navigate up for each '../'
                while path_remaining.starts_with("../") {
                    if !module_parts.is_empty() {
                        module_parts.pop();
                    }
                    // If we've gone above the module root, we just continue
                    // This handles cases like ../../../some/path from a shallow module
                    path_remaining = &path_remaining[3..];
                }

                // Add the remaining path
                if !path_remaining.is_empty() {
                    module_parts.extend(
                        path_remaining
                            .split('/')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                    );
                }

                module_parts.join("/")
            } else {
                // Not a relative path, return as-is
                import_path.to_string()
            }
        }

        // Case 1: Exact match (most common case, check first for performance)
        if import_path == symbol_module_path {
            return true;
        }

        // Case 2: Only do complex matching if we have the importing module context
        if let Some(importing_mod) = importing_module {
            // Go import resolution:
            // - Relative imports start with './' or '../'
            // - Absolute imports are package paths like "fmt", "github.com/user/repo/package"

            if import_path.starts_with("./") || import_path.starts_with("../") {
                // Resolve relative path to absolute module path
                let resolved = resolve_relative_path(import_path, importing_mod);

                // Check if it matches exactly
                if resolved == symbol_module_path {
                    return true;
                }
            }
            // else: absolute package imports like "fmt", "github.com/user/repo"
            // These should match exactly (no complex resolution needed for Go packages)
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Visibility;
    use crate::parsing::registry::LanguageId;
    use std::path::Path;

    #[test]
    fn test_module_separator() {
        let behavior = GoBehavior::new();
        assert_eq!(behavior.module_separator(), "/");
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = GoBehavior::new();
        let project_root = Path::new("/home/user/project");
        let extensions = &["go"];

        // Test basic Go file
        let file_path = Path::new("/home/user/project/pkg/utils/helper.go");
        assert_eq!(
            behavior.module_path_from_file(file_path, project_root, extensions),
            Some("pkg/utils".to_string())
        );

        // Test root level file
        let file_path = Path::new("/home/user/project/main.go");
        assert_eq!(
            behavior.module_path_from_file(file_path, project_root, extensions),
            Some(".".to_string())
        );

        // Test nested package
        let file_path = Path::new("/home/user/project/internal/api/server.go");
        assert_eq!(
            behavior.module_path_from_file(file_path, project_root, extensions),
            Some("internal/api".to_string())
        );
    }

    #[test]
    fn test_format_module_path() {
        let behavior = GoBehavior::new();
        // Go doesn't append symbol names to module paths like Rust does
        assert_eq!(
            behavior.format_module_path("pkg/utils", "Helper"),
            "pkg/utils"
        );
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = GoBehavior::new();

        // Test function signatures
        assert_eq!(
            behavior.parse_visibility("func PublicFunction() error"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("func privateFunction() error"),
            Visibility::Private
        );

        // Test method signatures
        assert_eq!(
            behavior.parse_visibility("func (s *Server) HandleRequest() error"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("func (s *Server) handleInternal() error"),
            Visibility::Private
        );

        // Test type signatures
        assert_eq!(
            behavior.parse_visibility("type PublicStruct struct"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("type privateStruct struct"),
            Visibility::Private
        );

        // Test variable signatures
        assert_eq!(
            behavior.parse_visibility("var GlobalVar string"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("var localVar string"),
            Visibility::Private
        );

        // Test constant signatures
        assert_eq!(
            behavior.parse_visibility("const MaxRetries = 3"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("const timeout = 30"),
            Visibility::Private
        );
    }

    #[test]
    fn test_supports_traits() {
        let behavior = GoBehavior::new();
        assert!(!behavior.supports_traits()); // Go has interfaces, not traits
    }

    #[test]
    fn test_supports_inherent_methods() {
        let behavior = GoBehavior::new();
        assert!(behavior.supports_inherent_methods()); // Go has methods on types
    }

    #[test]
    fn test_import_matches_symbol() {
        let behavior = GoBehavior::new();

        // Test exact matches
        assert!(behavior.import_matches_symbol("fmt", "fmt", None));
        assert!(behavior.import_matches_symbol(
            "github.com/user/repo",
            "github.com/user/repo",
            None
        ));

        // Test relative imports
        assert!(behavior.import_matches_symbol("./utils", "pkg/utils", Some("pkg")));
        assert!(behavior.import_matches_symbol("../shared", "pkg/shared", Some("pkg/api")));

        // Test non-matches
        assert!(!behavior.import_matches_symbol("fmt", "strings", None));
        assert!(!behavior.import_matches_symbol("./utils", "pkg/other", Some("pkg")));
    }

    #[test]
    fn test_configure_symbol() {
        use crate::{FileId, Range, Symbol, SymbolId, SymbolKind, Visibility};

        let behavior = GoBehavior::new();

        // Test function with public signature
        let mut symbol = Symbol {
            id: SymbolId::new(1).unwrap(),
            name: "PublicFunction".into(),
            kind: SymbolKind::Function,
            signature: Some("func PublicFunction() error".into()),
            module_path: None,
            file_id: FileId::new(1).unwrap(),
            range: Range {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 10,
            },
            file_path: "<unknown>".into(),
            doc_comment: None,
            visibility: Visibility::Private, // Will be updated by configure_symbol
            scope_context: None,
            language_id: Some(LanguageId::new("go")),
        };

        behavior.configure_symbol(&mut symbol, Some("pkg/utils"));

        assert_eq!(
            symbol.module_path.as_ref().map(|s| s.as_ref()),
            Some("pkg/utils")
        );
        assert_eq!(symbol.visibility, Visibility::Public); // Should be public due to capitalization

        // Test variable with private signature
        let mut symbol = Symbol {
            id: SymbolId::new(2).unwrap(),
            name: "privateVar".into(),
            kind: SymbolKind::Variable,
            signature: Some("var privateVar string".into()),
            module_path: None,
            file_id: FileId::new(1).unwrap(),
            range: Range {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 10,
            },
            file_path: "<unknown>".into(),
            doc_comment: None,
            visibility: Visibility::Public, // Will be updated by configure_symbol
            scope_context: None,
            language_id: Some(LanguageId::new("go")),
        };

        behavior.configure_symbol(&mut symbol, None);

        assert_eq!(symbol.module_path.as_ref().map(|s| s.as_ref()), Some(".")); // Default to current package
        assert_eq!(symbol.visibility, Visibility::Private); // Should be private due to lowercase
    }
}
