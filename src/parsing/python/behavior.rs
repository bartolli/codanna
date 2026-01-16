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

    /// Compute Python module path from file path using resolution cache.
    ///
    /// Uses cached resolution rules from PythonProvider to map file paths to modules.
    /// Falls back to convention-based path stripping if no cache is available.
    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        use crate::parsing::paths::strip_extension;
        use crate::project_resolver::persist::ResolutionPersistence;
        use std::cell::RefCell;
        use std::time::{Duration, Instant};

        // Thread-local cache with 1-second TTL (per Java/Swift pattern)
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
                if let Ok(index) = persistence.load("python") {
                    *cache_ref = Some((Instant::now(), index));
                } else {
                    *cache_ref = None;
                }
            }

            // Get module path from cached rules
            // Returns: (module_path, fallback_project_dir)
            if let Some((_, ref index)) = *cache_ref {
                // Canonicalize file path for matching
                if let Ok(canon_file) = file_path.canonicalize() {
                    // Find config that applies to this file
                    if let Some(config_path) = index.get_config_for_file(&canon_file) {
                        // Get project directory (parent of config file) for fallback
                        let project_dir = config_path.parent().map(|p| p.to_path_buf());

                        if let Some(rules) = index.rules.get(config_path) {
                            // Sort source roots by length (longest first) for proper prefix matching
                            let mut roots: Vec<_> = rules.paths.keys().collect();
                            roots.sort_by_key(|k| std::cmp::Reverse(k.len()));

                            // Extract module from file path using source roots
                            for root_path in roots {
                                let root = std::path::Path::new(root_path);
                                let canon_root =
                                    root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

                                if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                                    // Convert to module path
                                    let Some(path_str) = relative.to_str() else {
                                        continue;
                                    };
                                    let path_without_ext = strip_extension(path_str, extensions);

                                    // Handle __init__.py - it represents the package itself
                                    let module_path = if path_without_ext.ends_with("/__init__") {
                                        path_without_ext
                                            .strip_suffix("/__init__")
                                            .unwrap_or(path_without_ext)
                                    } else {
                                        path_without_ext
                                    };

                                    // Convert path separators to dots
                                    let module_path = module_path.replace(['/', '\\'], ".");

                                    // Handle special cases
                                    if module_path.is_empty() || module_path == "__init__" {
                                        return (None, project_dir);
                                    } else if module_path == "__main__" || module_path == "main" {
                                        return (Some("__main__".to_string()), None);
                                    } else {
                                        return (Some(module_path), None);
                                    }
                                }
                            }
                        }

                        // Config matched but no source root matched - return project dir for fallback
                        return (None, project_dir);
                    }
                }
            }

            (None, None)
        });

        // Return cached result if found
        if let (Some(module_path), _) = &cached_result {
            return Some(module_path.clone());
        }

        // Use project directory from config if available, otherwise workspace root
        let fallback_root = cached_result.1.as_deref().unwrap_or(project_root);

        // Fallback: convention-based path stripping
        let relative_path = file_path.strip_prefix(fallback_root).ok()?;
        let path_str = relative_path.to_str()?;

        // Remove common Python source directories if present
        let path_without_src = path_str
            .strip_prefix("src/")
            .or_else(|| path_str.strip_prefix("lib/"))
            .or_else(|| path_str.strip_prefix("app/"))
            .or_else(|| path_str.strip_prefix("python/"))
            .unwrap_or(path_str);

        // Remove extension using passed extensions from settings.toml
        let path_without_ext = strip_extension(path_without_src, extensions);

        // Handle __init__.py - it represents the package itself
        let module_path = if path_without_ext.ends_with("/__init__") {
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
            None
        } else if module_path == "__main__" || module_path == "main" {
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

    /// Build resolution context for parallel pipeline.
    ///
    /// Python-specific: handles `from X import Y` by extracting module portion
    /// and matching against symbol module_path.
    fn build_resolution_context_with_pipeline_cache(
        &self,
        file_id: FileId,
        imports: &[crate::parsing::Import],
        cache: &dyn crate::parsing::PipelineSymbolCache,
        extensions: &[&str],
    ) -> (
        Box<dyn crate::parsing::ResolutionScope>,
        Vec<crate::parsing::Import>,
    ) {
        use crate::SymbolId;
        use crate::parsing::ScopeLevel;
        use crate::parsing::resolution::{ImportBinding, ImportOrigin};

        let _ = extensions; // Available for future use

        let mut context = super::resolution::PythonResolutionContext::new(file_id);
        let importing_module = self.get_module_path_for_file(file_id);

        let mut enhanced_imports = Vec::with_capacity(imports.len());

        for import in imports {
            // 1. Extract module and symbol from import path
            // For "from pydantic.v1.error_wrappers import ValidationError":
            //   module_part = "pydantic.v1.error_wrappers"
            //   symbol_name = "ValidationError"
            let (module_part, symbol_name) = if let Some(pos) = import.path.rfind('.') {
                (
                    import.path[..pos].to_string(),
                    import.path[pos + 1..].to_string(),
                )
            } else {
                // Simple import like "import os"
                (String::new(), import.path.clone())
            };

            // 2. Get local name (alias or symbol_name)
            let local_name = import.alias.clone().unwrap_or_else(|| symbol_name.clone());

            // 3. Resolve relative imports or use module portion
            let target_module = if import.path.starts_with('.') {
                // Relative import: resolve against importing module
                self.resolve_python_relative_import(
                    &import.path,
                    importing_module.as_deref().unwrap_or(""),
                )
            } else {
                // Absolute import: use extracted module portion (NOT full path)
                module_part.clone()
            };

            // 4. Collect enhanced import - keep full path for Tier 2 matching
            // Python includes symbol name in path: "module.symbol"
            enhanced_imports.push(crate::parsing::Import {
                path: import.path.clone(),
                file_id: import.file_id,
                alias: import.alias.clone(),
                is_glob: import.is_glob,
                is_type_only: import.is_type_only,
            });

            // 5. Lookup candidates by symbol name and match by module_path
            let mut resolved_symbol: Option<SymbolId> = None;
            let candidates = cache.lookup_candidates(&symbol_name);

            for id in candidates {
                if let Some(symbol) = cache.get(id) {
                    if let Some(ref sym_module) = symbol.module_path {
                        let sym_mod = sym_module.as_ref();

                        // Python match: exact module match preferred
                        // For "from pydantic.v1.error_wrappers import ValidationError":
                        //   target_module = "pydantic.v1.error_wrappers"
                        //   sym_module should equal "pydantic.v1.error_wrappers"
                        if sym_mod == target_module {
                            // Exact match - best
                            resolved_symbol = Some(id);
                            break;
                        }

                        // Handle suffix matches for relative/short imports
                        if !target_module.is_empty() {
                            // target_module ends with sym_module (relative import resolved)
                            if target_module.ends_with(sym_mod)
                                && (target_module.len() == sym_mod.len()
                                    || target_module
                                        .chars()
                                        .nth(target_module.len() - sym_mod.len() - 1)
                                        == Some('.'))
                            {
                                resolved_symbol = Some(id);
                                break;
                            }

                            // sym_module ends with target_module (short import)
                            if sym_mod.ends_with(&target_module)
                                && (sym_mod.len() == target_module.len()
                                    || sym_mod.chars().nth(sym_mod.len() - target_module.len() - 1)
                                        == Some('.'))
                            {
                                resolved_symbol = Some(id);
                                break;
                            }
                        }
                    }
                }
            }

            // 6. Register binding
            let origin = if resolved_symbol.is_some() {
                ImportOrigin::Internal
            } else {
                ImportOrigin::External
            };

            context.register_import_binding(ImportBinding {
                import: import.clone(),
                exposed_name: local_name.clone(),
                origin,
                resolved_symbol,
            });

            if let Some(symbol_id) = resolved_symbol {
                context.add_symbol(local_name, symbol_id, ScopeLevel::Package);
            }
        }

        // 7. Populate context with enhanced imports
        context.populate_imports(&enhanced_imports);

        // 8. Add local symbols from this file
        for sym_id in cache.symbols_in_file(file_id) {
            if let Some(symbol) = cache.get(sym_id) {
                if self.is_resolvable_symbol(&symbol) {
                    context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Module);
                    if let Some(ref module_path) = symbol.module_path {
                        context.add_symbol(module_path.to_string(), symbol.id, ScopeLevel::Global);
                    }
                }
            }
        }

        (Box::new(context), enhanced_imports)
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

            // Handle Python "from X import Y" pattern
            // import_path = "X.Y" (full path including symbol name)
            // symbol_module_path = "X" (just the module where symbol is defined)
            //
            // Check: import_path starts with symbol_module_path + "."
            // AND the remaining part is just the symbol name (no more dots)
            let prefix = format!("{symbol_module_path}.");
            if import_path.starts_with(&prefix) {
                let remainder = &import_path[prefix.len()..];
                // remainder should be just the symbol name (no dots)
                if !remainder.contains('.') {
                    tracing::trace!(
                        target: "pipeline",
                        "[python] module prefix match: {import_path} starts with {prefix}, symbol={remainder}"
                    );
                    return true;
                }
            }

            // Handle short imports (just symbol name)
            if !import_path.contains('.') {
                // Simple name, might match if it's the last segment of symbol path
                if symbol_module_path.ends_with(&format!(".{import_path}"))
                    || symbol_module_path == import_path
                {
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
