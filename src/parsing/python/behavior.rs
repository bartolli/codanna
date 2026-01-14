//! Python-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::ResolutionScope;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::{FileId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// Python language behavior implementation
#[derive(Clone)]
pub struct PythonBehavior {
    language: Language,
    state: BehaviorState,
}

impl PythonBehavior {
    /// Create a new Python behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_python::LANGUAGE.into(),
            state: BehaviorState::new(),
        }
    }

    /// Resolve Python relative imports (., .., etc.)
    fn resolve_python_relative_import(&self, import_path: &str, from_module: &str) -> String {
        let dots = import_path.chars().take_while(|&c| c == '.').count();
        let remaining = &import_path[dots..];

        // Split the current module path
        let mut parts: Vec<_> = from_module.split('.').collect();

        // In Python relative imports:
        // . = current package (same directory)
        // .. = parent package (go up 1 level from current package)
        // ... = grandparent package (go up 2 levels from current package)
        //
        // From module parent.child:
        // .sibling gives parent.sibling (same package as child)
        // ..sibling gives sibling (parent of parent.child is root)
        //
        // The key insight: we go up 'dots' levels from the current module
        for _ in 0..dots {
            if !parts.is_empty() {
                parts.pop();
            }
        }

        // Add the remaining path if any
        if !remaining.is_empty() {
            // Skip the leading dot if present
            let remaining = remaining.trim_start_matches('.');
            if !remaining.is_empty() {
                // Split the remaining path and add each part
                for part in remaining.split('.') {
                    if !part.is_empty() {
                        parts.push(part);
                    }
                }
            }
        }

        parts.join(".")
    }
}

impl StatefulBehavior for PythonBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for PythonBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for PythonBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("python")
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Apply default behavior: set module_path and parse visibility
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.clone().into());

            // If this is the synthetic module symbol, set its display name to the last segment
            // (e.g., examples.python.module_calls_test -> module_calls_test)
            if symbol.kind == crate::types::SymbolKind::Module {
                let short = full_path.rsplit('.').next().unwrap_or(full_path.as_str());
                symbol.name = crate::types::compact_string(short);
            }
        } else if symbol.kind == crate::types::SymbolKind::Module {
            // No module path available (e.g., root __init__.py). Avoid '<' '>' which
            // get stripped by the analyzer, to keep the name searchable via exact term.
            symbol.name = crate::types::compact_string("module");
        }

        if let Some(ref sig) = symbol.signature {
            symbol.visibility = self.parse_visibility(sig);
        }
    }
    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(crate::parsing::python::PythonResolutionContext::new(
            file_id,
        ))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn crate::parsing::InheritanceResolver> {
        Box::new(crate::parsing::python::PythonInheritanceResolver::new())
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // Python typically uses file paths as module paths, not including the symbol name
        base_path.to_string()
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Python uses naming conventions for visibility
        // Check for special/dunder methods first
        if signature.contains("__init__")
            || signature.contains("__str__")
            || signature.contains("__repr__")
            || signature.contains("__eq__")
            || signature.contains("__hash__")
            || signature.contains("__call__")
        {
            // Dunder methods are public
            Visibility::Public
        } else if signature.contains("def __") || signature.contains("class __") {
            // Double underscore (not dunder) = private (name mangling)
            Visibility::Private
        } else if signature.contains("def _") || signature.contains("class _") {
            // Single underscore = module-level/protected
            Visibility::Module
        } else {
            // Everything else is public in Python
            Visibility::Public
        }
    }

    fn module_separator(&self) -> &'static str {
        "."
    }

    fn supports_traits(&self) -> bool {
        false // Python doesn't have traits, it has inheritance and mixins
    }

    fn supports_inherent_methods(&self) -> bool {
        false // Python methods are always on classes, not separate
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            None
        } else {
            Some(components.join("."))
        }
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn normalize_caller_name(&self, name: &str, file_id: FileId) -> String {
        if name == "<module>" {
            if let Some(module_path) = self.get_module_path_for_file(file_id) {
                // Return the last path segment so it matches a token in the name field
                module_path
                    .rsplit('.')
                    .next()
                    .unwrap_or("module")
                    .to_string()
            } else {
                // No module path known (e.g., root __init__.py)
                "module".to_string()
            }
        } else {
            name.to_string()
        }
    }

    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        use crate::parsing::paths::strip_extension;

        // Get relative path from project root
        let relative_path = file_path.strip_prefix(project_root).ok()?;

        // Convert path to string
        let path_str = relative_path.to_str()?;

        // Remove common Python source directories if present
        let path_without_src = path_str
            .strip_prefix("src/")
            .or_else(|| path_str.strip_prefix("lib/"))
            .or_else(|| path_str.strip_prefix("app/"))
            .unwrap_or(path_str);

        // Remove extension using passed extensions from settings.toml
        let path_without_ext = strip_extension(path_without_src, extensions);

        // Handle __init__.py - it represents the package itself
        let module_path = if path_without_ext.ends_with("/__init__") {
            // Remove /__init__ to get the package path
            path_without_ext
                .strip_suffix("/__init__")
                .unwrap_or(path_without_ext)
                .to_string()
        } else {
            path_without_ext.to_string()
        };

        // Convert path separators to Python module separators
        let module_path = module_path.replace('/', ".");

        // Handle special cases
        if module_path.is_empty() || module_path == "__init__" {
            // Root __init__.py or empty path
            None
        } else if module_path == "__main__" || module_path == "main" {
            // __main__.py is the entry point
            Some("__main__".to_string())
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
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // 1. Always check exact match first (performance)
        if import_path == symbol_module_path {
            tracing::debug!("[python] exact match: {import_path} == {symbol_module_path}");
            return true;
        }

        // 2. Handle Python-specific import patterns
        if let Some(importing_mod) = importing_module {
            tracing::debug!(
                "[python] import_matches_symbol: import='{import_path}', symbol='{symbol_module_path}', from='{importing_mod}'"
            );
            // Handle relative imports starting with dots
            if import_path.starts_with('.') {
                let resolved = self.resolve_python_relative_import(import_path, importing_mod);
                if resolved == symbol_module_path {
                    return true;
                }
            }

            // Handle absolute imports that might be partial
            // e.g., "module.func" might match "package.module.func"
            if !import_path.contains('.') {
                // Simple module name, might be imported directly
                if symbol_module_path.ends_with(&format!(".{import_path}")) {
                    return true;
                }
            } else {
                // Multi-part import path
                // Check if it's a suffix of the symbol path
                if symbol_module_path.ends_with(import_path) {
                    return true;
                }
            }
        }

        false
    }

    // Python-specific: Check if a symbol should be added to resolution context
    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // Python resolves functions, classes, and module-level variables
        let resolvable_kind = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Method
        );

        if !resolvable_kind {
            return false;
        }

        // Check scope context
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global => true,
                ScopeContext::Local { hoisted, .. } => {
                    // In Python, nothing is truly hoisted
                    // But functions defined at module level are available
                    !hoisted && symbol.kind == SymbolKind::Function
                }
                ScopeContext::ClassMember { .. } => {
                    // Class members are resolvable through the class
                    true
                }
                ScopeContext::Parameter => false,
                ScopeContext::Package => true,
            }
        } else {
            // Default to resolvable for module-level symbols
            true
        }
    }

    // Python-specific: Check visibility based on naming conventions
    fn is_symbol_visible_from_file(&self, symbol: &crate::Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Python uses naming conventions for visibility:
        // - __name (double underscore): Private (name mangling)
        // - _name (single underscore): Module-level/protected
        // - name (no underscore): Public

        // Check the actual symbol name, not just the visibility field
        let name = symbol.name.as_ref();

        // Special methods like __init__, __str__ are always public
        if name.starts_with("__") && name.ends_with("__") {
            return true;
        }

        // Private names are not visible outside the module
        if name.starts_with("__") {
            return false;
        }

        // Module-level (_name) symbols are visible but discouraged
        // Let's be permissive and allow them
        if name.starts_with('_') {
            return true;
        }

        // Public names are always visible
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_module_path() {
        let behavior = PythonBehavior::new();
        assert_eq!(
            behavior.format_module_path("module.submodule", "function"),
            "module.submodule"
        );
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = PythonBehavior::new();

        // Public functions
        assert_eq!(behavior.parse_visibility("def foo():"), Visibility::Public);
        assert_eq!(
            behavior.parse_visibility("class MyClass:"),
            Visibility::Public
        );

        // Protected/module-level
        assert_eq!(
            behavior.parse_visibility("def _internal():"),
            Visibility::Module
        );
        assert_eq!(
            behavior.parse_visibility("class _InternalClass:"),
            Visibility::Module
        );

        // Private (name mangling)
        assert_eq!(
            behavior.parse_visibility("def __private():"),
            Visibility::Private
        );

        // Special methods should be public
        assert_eq!(
            behavior.parse_visibility("def __init__(self):"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("def __str__(self):"),
            Visibility::Public
        );
    }

    #[test]
    fn test_module_separator() {
        let behavior = PythonBehavior::new();
        assert_eq!(behavior.module_separator(), ".");
    }

    #[test]
    fn test_supports_features() {
        let behavior = PythonBehavior::new();
        assert!(!behavior.supports_traits());
        assert!(!behavior.supports_inherent_methods());
    }

    #[test]
    fn test_validate_node_kinds() {
        let behavior = PythonBehavior::new();

        // Valid Python node kinds
        assert!(behavior.validate_node_kind("function_definition"));
        assert!(behavior.validate_node_kind("class_definition"));
        assert!(behavior.validate_node_kind("module"));

        // Invalid node kind
        assert!(!behavior.validate_node_kind("struct_item")); // Rust-specific
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = PythonBehavior::new();
        let root = Path::new("/project");
        let extensions = &["py", "pyi"];

        // Test regular module
        let module_path = Path::new("/project/src/package/module.py");
        assert_eq!(
            behavior.module_path_from_file(module_path, root, extensions),
            Some("package.module".to_string())
        );

        // Test __init__.py (represents the package)
        let init_path = Path::new("/project/src/package/__init__.py");
        assert_eq!(
            behavior.module_path_from_file(init_path, root, extensions),
            Some("package".to_string())
        );

        // Test nested module
        let nested_path = Path::new("/project/src/package/subpackage/module.py");
        assert_eq!(
            behavior.module_path_from_file(nested_path, root, extensions),
            Some("package.subpackage.module".to_string())
        );

        // Test __main__.py
        let main_path = Path::new("/project/__main__.py");
        assert_eq!(
            behavior.module_path_from_file(main_path, root, extensions),
            Some("__main__".to_string())
        );

        // Test root __init__.py (should return None)
        let root_init = Path::new("/project/__init__.py");
        assert_eq!(
            behavior.module_path_from_file(root_init, root, extensions),
            None
        );

        // Test without src directory
        let no_src_path = Path::new("/project/mypackage/mymodule.py");
        assert_eq!(
            behavior.module_path_from_file(no_src_path, root, extensions),
            Some("mypackage.mymodule".to_string())
        );

        // Test .pyi stub file
        let stub_path = Path::new("/project/typings/module.pyi");
        assert_eq!(
            behavior.module_path_from_file(stub_path, root, extensions),
            Some("typings.module".to_string())
        );
    }
}
