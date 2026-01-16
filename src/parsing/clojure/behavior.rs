//! Clojure-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::ResolutionScope;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::storage::DocumentIndex;
use crate::{FileId, SymbolId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// Clojure language behavior implementation
#[derive(Clone)]
pub struct ClojureBehavior {
    language: Language,
    state: BehaviorState,
}

impl ClojureBehavior {
    /// Create a new Clojure behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_clojure_orchard::LANGUAGE.into(),
            state: BehaviorState::new(),
        }
    }
}

impl StatefulBehavior for ClojureBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for ClojureBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for ClojureBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("clojure")
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Apply default behavior: set module_path and parse visibility
        if let Some(path) = module_path {
            symbol.module_path = Some(path.to_string().into());
        }

        if let Some(ref sig) = symbol.signature {
            symbol.visibility = self.parse_visibility(sig);
        }
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(crate::parsing::clojure::ClojureResolutionContext::new(
            file_id,
        ))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn crate::parsing::InheritanceResolver> {
        // Clojure doesn't have traditional inheritance, use generic resolver
        Box::new(crate::parsing::GenericInheritanceResolver::new())
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // Clojure uses namespace as module path
        base_path.to_string()
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Clojure visibility rules:
        // 1. defn- = Private
        // 2. ^:private metadata = Private
        // 3. Names starting with - = Private (convention)
        // 4. Everything else = Public
        if signature.contains("defn-")
            || signature.contains("^:private")
            || signature.contains("^{:private true}")
        {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn module_separator(&self) -> &'static str {
        "." // Clojure uses dots for namespaces
    }

    fn supports_traits(&self) -> bool {
        true // Clojure has protocols
    }

    fn supports_inherent_methods(&self) -> bool {
        false // Clojure doesn't have inherent methods like Rust
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Convert src/my/namespace/core.clj -> my.namespace.core
        let relative = file_path.strip_prefix(project_root).ok()?;

        // Remove src/ prefix if present
        let path_str = relative.to_string_lossy();
        let without_src = path_str
            .strip_prefix("src/")
            .or_else(|| path_str.strip_prefix("src\\"))
            .unwrap_or(&path_str);

        // Remove extension and convert path separators to dots
        let without_ext = without_src
            .strip_suffix(".clj")
            .or_else(|| without_src.strip_suffix(".cljc"))
            .or_else(|| without_src.strip_suffix(".cljs"))
            .or_else(|| without_src.strip_suffix(".edn"))
            .unwrap_or(without_src);

        // Convert slashes to dots, underscores to hyphens (Clojure convention)
        let module_path = without_ext
            .replace('/', ".")
            .replace('\\', ".")
            .replace('_', "-");

        if module_path.is_empty() {
            None
        } else {
            Some(module_path)
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

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        // Exact match
        if import_path == symbol_module_path {
            return true;
        }

        // Check if import path is a prefix of symbol module path
        if symbol_module_path.starts_with(&format!("{import_path}.")) {
            return true;
        }

        // Check if symbol is in the imported namespace
        if let Some(last_dot) = import_path.rfind('.') {
            let ns_part = &import_path[..last_dot];
            if ns_part == symbol_module_path {
                return true;
            }
        }

        false
    }

    fn build_resolution_context(
        &self,
        file_id: FileId,
        document_index: &DocumentIndex,
    ) -> crate::error::IndexResult<Box<dyn crate::parsing::ResolutionScope>> {
        use crate::error::IndexError;
        use crate::parsing::clojure::ClojureResolutionContext;

        let mut context = ClojureResolutionContext::new(file_id);

        // Add imported symbols
        let imports = self.get_imports_for_file(file_id);
        for import in imports {
            if let Some(symbol_id) = self.resolve_import(&import, document_index) {
                let name = if let Some(alias) = &import.alias {
                    alias.clone()
                } else {
                    // Use last segment of path as default name
                    import
                        .path
                        .rsplit('.')
                        .next()
                        .unwrap_or(&import.path)
                        .to_string()
                };

                context.add_symbol(name, symbol_id, crate::parsing::ScopeLevel::Package);
            }
        }

        // Add file's namespace-level symbols
        let file_symbols =
            document_index
                .find_symbols_by_file(file_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in file_symbols {
            if self.is_resolvable_symbol(&symbol) {
                context.add_symbol(
                    symbol.name.to_string(),
                    symbol.id,
                    crate::parsing::ScopeLevel::Module,
                );
            }
        }

        Ok(Box::new(context))
    }

    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;

        // Clojure resolves functions, macros, vars, protocols, records
        matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Variable
                | SymbolKind::Macro
                | SymbolKind::Interface
                | SymbolKind::Struct
                | SymbolKind::Method
                | SymbolKind::Module
        )
    }

    fn resolve_import(
        &self,
        import: &crate::parsing::Import,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        let importing_module = self.get_module_path_for_file(import.file_id);

        self.resolve_import_path_with_context(
            &import.path,
            importing_module.as_deref(),
            document_index,
        )
    }

    fn is_symbol_visible_from_file(&self, symbol: &crate::Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Check visibility
        match symbol.visibility {
            Visibility::Public => true,
            Visibility::Private => false,
            Visibility::Module => {
                // Same module/namespace
                if let Some(symbol_module) = &symbol.module_path {
                    if let Some(from_module) = self.get_module_path_for_file(from_file) {
                        return symbol_module.as_ref() == from_module;
                    }
                }
                false
            }
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_module_path() {
        let behavior = ClojureBehavior::new();
        assert_eq!(
            behavior.format_module_path("my.namespace.core", "my-fn"),
            "my.namespace.core"
        );
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = ClojureBehavior::new();

        // Public functions
        assert_eq!(
            behavior.parse_visibility("(defn my-fn [x] ...)"),
            Visibility::Public
        );

        // Private functions
        assert_eq!(
            behavior.parse_visibility("(defn- private-fn [x] ...)"),
            Visibility::Private
        );

        // Private via metadata
        assert_eq!(
            behavior.parse_visibility("(def ^:private secret 42)"),
            Visibility::Private
        );
    }

    #[test]
    fn test_module_separator() {
        let behavior = ClojureBehavior::new();
        assert_eq!(behavior.module_separator(), ".");
    }

    #[test]
    fn test_supports_features() {
        let behavior = ClojureBehavior::new();
        assert!(behavior.supports_traits()); // protocols
        assert!(!behavior.supports_inherent_methods());
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = ClojureBehavior::new();
        let root = Path::new("/project");

        // Test regular module
        let module_path = Path::new("/project/src/my/namespace/core.clj");
        assert_eq!(
            behavior.module_path_from_file(module_path, root),
            Some("my.namespace.core".to_string())
        );

        // Test with underscores (should become hyphens)
        let underscore_path = Path::new("/project/src/my_app/some_module.clj");
        assert_eq!(
            behavior.module_path_from_file(underscore_path, root),
            Some("my-app.some-module".to_string())
        );

        // Test ClojureScript
        let cljs_path = Path::new("/project/src/app/main.cljs");
        assert_eq!(
            behavior.module_path_from_file(cljs_path, root),
            Some("app.main".to_string())
        );
    }
}
