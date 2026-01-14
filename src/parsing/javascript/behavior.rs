//! JavaScript-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::paths::strip_extension;
use crate::parsing::resolution::{InheritanceResolver, ResolutionScope};
use crate::project_resolver::persist::ResolutionPersistence;
use crate::types::FileId;
use crate::{SymbolId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::{JavaScriptInheritanceResolver, JavaScriptResolutionContext};

/// Normalize a JavaScript import path to a module path
///
/// Handles relative imports (./foo, ../bar) and strips JS extensions.
/// Returns a dot-separated module path that matches how modules are stored.
/// Extensions come from settings.toml - no hardcoded values.
fn normalize_js_import(import_path: &str, importing_mod: &str, extensions: &[&str]) -> String {
    fn parent_module(m: &str) -> String {
        let mut parts: Vec<&str> = if m.is_empty() {
            Vec::new()
        } else {
            m.split('.').collect()
        };
        if !parts.is_empty() {
            parts.pop();
        }
        parts.join(".")
    }

    let result = if import_path.starts_with("./") {
        let base = parent_module(importing_mod);
        let rel = import_path.trim_start_matches("./").replace('/', ".");
        if base.is_empty() {
            rel
        } else {
            format!("{base}.{rel}")
        }
    } else if import_path.starts_with("../") {
        let base_owned = parent_module(importing_mod);
        let mut parts: Vec<&str> = base_owned.split('.').collect();
        let mut rest = import_path;
        while rest.starts_with("../") {
            if !parts.is_empty() {
                parts.pop();
            }
            rest = &rest[3..];
        }
        let rest = rest.trim_start_matches("./").replace('/', ".");
        let mut combined = parts.join(".");
        if !rest.is_empty() {
            combined = if combined.is_empty() {
                rest
            } else {
                format!("{combined}.{rest}")
            };
        }
        combined
    } else {
        import_path.replace('/', ".")
    };

    // Strip extensions from settings.toml to match module paths
    strip_extension(&result, extensions).to_string()
}

/// JavaScript language behavior implementation
#[derive(Clone)]
pub struct JavaScriptBehavior {
    state: BehaviorState,
}

impl JavaScriptBehavior {
    /// Create a new JavaScript behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }
}

impl Default for JavaScriptBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for JavaScriptBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for JavaScriptBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("javascript")
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Preserve parser-derived visibility (export detection), only set module path.
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // JavaScript uses file paths as module paths, not including the symbol name
        // All symbols in the same file share the same module path for visibility
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_javascript::LANGUAGE.into()
    }

    fn module_separator(&self) -> &'static str {
        "."
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            None
        } else {
            Some(components.join("."))
        }
    }

    // JavaScript uses jsconfig for module resolution, needs custom handling
    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        // Use jsconfig infrastructure to compute canonical module paths
        // This ensures symbols use the SAME path format as enhanced imports

        // Load the resolution index to find which jsconfig governs this file
        let persistence = ResolutionPersistence::new(Path::new(crate::init::local_dir_name()));

        // Try jsconfig-based resolution first
        if let Ok(index) = persistence.load("javascript") {
            // get_config_for_file() expects a relative path (relative to workspace root)
            let relative_to_workspace = file_path
                .strip_prefix(project_root)
                .ok()
                .unwrap_or(file_path);

            // Find which jsconfig applies to this file
            if let Some(config_path) = index.get_config_for_file(relative_to_workspace) {
                tracing::debug!(
                    "[javascript] module_path_from_file relative_to_workspace={relative_to_workspace:?} config_path={config_path:?}"
                );

                // Get the jsconfig's directory (the project root for this file)
                if let Some(parent) = config_path.parent() {
                    let jsconfig_dir = project_root.join(parent);
                    tracing::debug!(
                        "[javascript] module_path_from_file jsconfig_dir={jsconfig_dir:?}"
                    );

                    // Compute path relative to the jsconfig's directory
                    if let Ok(relative_path) = file_path.strip_prefix(&jsconfig_dir) {
                        if let Some(path) = relative_path.to_str() {
                            // Strip extension using the provided extensions list
                            let path_without_ext =
                                strip_extension(path.trim_start_matches("./"), extensions);

                            // Handle /index suffix (directory imports)
                            let module_path = path_without_ext.trim_end_matches("/index");

                            let result = module_path.replace('/', ".");

                            tracing::debug!(
                                "[javascript] module_path_from_file file_path={file_path:?} -> module_path={result}"
                            );

                            return Some(result);
                        }
                    }
                }
            }
        }

        // Fallback: simple path-based module resolution
        let relative_path = file_path.strip_prefix(project_root).ok()?;
        let path = relative_path.to_str()?;

        // Strip extension using the provided extensions list
        let path_without_ext = strip_extension(path.trim_start_matches("./"), extensions);

        // Handle /index suffix (directory imports)
        let module_path = path_without_ext.trim_end_matches("/index");

        // Replace path separators with module separators
        let result = module_path.replace('/', ".");

        tracing::debug!(
            "[javascript] module_path_from_file file_path={file_path:?} -> module_path={result}"
        );

        Some(result)
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // JavaScript visibility modifiers
        if signature.contains("export ") || signature.contains("export default") {
            Visibility::Public
        } else if signature.contains("private ") || signature.contains("#") {
            Visibility::Private
        } else {
            // Default visibility for JavaScript symbols
            // Module-level symbols are private by default unless exported
            Visibility::Private
        }
    }

    fn supports_traits(&self) -> bool {
        false // JavaScript doesn't have interfaces
    }

    fn supports_inherent_methods(&self) -> bool {
        true // JavaScript has class methods
    }

    // JavaScript-specific resolution overrides

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(JavaScriptResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(JavaScriptInheritanceResolver::new())
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // JavaScript only uses "extends" for class inheritance
        "extends"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;

        match language_specific {
            "extends" => RelationKind::Extends,
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
        // Store the import path as-is for resolution
        tracing::debug!(
            "[javascript] add_import path='{}' alias={:?} file_id={:?}",
            import.path,
            import.alias,
            import.file_id
        );
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<crate::parsing::Import> {
        self.get_imports_from_state(file_id)
    }

    /// Build resolution context for parallel pipeline (no Tantivy).
    ///
    /// Uses jsconfig path aliases via JavaScriptProjectEnhancer.
    /// Returns (scope, enhanced_imports) where enhanced_imports have path aliases resolved.
    ///
    /// CRITICAL: Symbols from pipeline have `module_path: None`.
    /// We compute module_path on-the-fly from `symbol.file_path` using rules.
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
        use crate::parsing::ScopeLevel;
        use crate::parsing::resolution::{ImportBinding, ImportOrigin, ProjectResolutionEnhancer};
        use std::path::PathBuf;

        let mut context = JavaScriptResolutionContext::new(file_id);

        // Helper to compute module_path from file_path using rules
        // Rules contain jsconfig paths; module_path_from_file extracts canonical module path
        let compute_module_path = |file_path: &str| -> Option<String> {
            let path = PathBuf::from(file_path);
            // project_root is unused by JavaScript's module_path_from_file (uses rules instead)
            self.module_path_from_file(&path, &PathBuf::new(), extensions)
        };

        // Compute importing module from current file
        let importing_module = cache
            .symbols_in_file(file_id)
            .first()
            .and_then(|id| cache.get(*id))
            .and_then(|sym| compute_module_path(&sym.file_path));

        // Load project rules for path alias enhancement
        let maybe_enhancer = self
            .load_project_rules_for_file(file_id)
            .map(super::resolution::JavaScriptProjectEnhancer::new);

        // Build enhanced imports with path aliases resolved
        let mut enhanced_imports = Vec::with_capacity(imports.len());

        for import in imports {
            // Get the local name to bind (alias or last path segment)
            let local_name = import.alias.clone().unwrap_or_else(|| {
                import
                    .path
                    .split('/')
                    .next_back()
                    .or_else(|| import.path.split('.').next_back())
                    .unwrap_or(&import.path)
                    .to_string()
            });

            // Enhance import path if we have jsconfig rules
            let target_module = if let Some(ref enhancer) = maybe_enhancer {
                if let Some(enhanced_path) = enhancer.enhance_import_path(&import.path, file_id) {
                    // Jsconfig alias - convert enhanced path to module format
                    enhanced_path.trim_start_matches("./").replace('/', ".")
                } else {
                    // Regular import - normalize relative to importing module
                    normalize_js_import(
                        &import.path,
                        &importing_module.clone().unwrap_or_default(),
                        extensions,
                    )
                }
            } else {
                normalize_js_import(
                    &import.path,
                    &importing_module.clone().unwrap_or_default(),
                    extensions,
                )
            };

            // Collect enhanced import with resolved path
            enhanced_imports.push(crate::parsing::Import {
                path: target_module.clone(),
                file_id: import.file_id,
                alias: import.alias.clone(),
                is_glob: import.is_glob,
                is_type_only: import.is_type_only,
            });

            // Look up candidates by local_name and match computed module_path
            let mut resolved_symbol: Option<SymbolId> = None;
            let candidates = cache.lookup_candidates(&local_name);
            for id in candidates {
                if let Some(symbol) = cache.get(id) {
                    // Compute module_path from file_path using rules
                    if let Some(computed_module) = compute_module_path(&symbol.file_path) {
                        if computed_module == target_module
                            || target_module.ends_with(&computed_module)
                            || computed_module.ends_with(&target_module)
                        {
                            resolved_symbol = Some(id);
                            break;
                        }
                    }
                }
            }

            // Determine origin
            let origin = if resolved_symbol.is_some() {
                ImportOrigin::Internal
            } else {
                ImportOrigin::External
            };

            // Register binding
            context.register_import_binding(ImportBinding {
                import: import.clone(),
                exposed_name: local_name.clone(),
                origin,
                resolved_symbol,
            });

            if let (ImportOrigin::Internal, Some(symbol_id)) = (origin, resolved_symbol) {
                context.add_symbol(local_name.clone(), symbol_id, ScopeLevel::Module);
            }
        }

        // Populate context with enhanced imports
        context.populate_imports(&enhanced_imports);

        // Add local symbols from this file with computed module_path
        for sym_id in cache.symbols_in_file(file_id) {
            if let Some(symbol) = cache.get(sym_id) {
                if self.is_resolvable_symbol(&symbol) {
                    context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Module);
                    // Compute module_path from file_path using rules
                    if let Some(computed_module) = compute_module_path(&symbol.file_path) {
                        context.add_symbol(computed_module, symbol.id, ScopeLevel::Module);
                    }
                }
            }
        }

        (Box::new(context), enhanced_imports)
    }

    // JavaScript-specific: Support hoisting
    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // JavaScript hoists function declarations and class declarations
        let hoisted = matches!(symbol.kind, SymbolKind::Function | SymbolKind::Class);

        if hoisted {
            return true;
        }

        // Methods are always resolvable within their file
        if matches!(symbol.kind, SymbolKind::Method) {
            return true;
        }

        // Check scope_context for non-hoisted symbols
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember { .. } => {
                    // Class members are resolvable if public or exported
                    matches!(symbol.visibility, Visibility::Public)
                }
            }
        } else {
            // Fallback for symbols without scope_context
            matches!(symbol.kind, SymbolKind::Constant | SymbolKind::Variable)
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
        self.state.get_file_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // Helper function to normalize path separators to dots
        fn normalize_path(path: &str) -> String {
            path.replace('/', ".")
        }

        // Helper function to resolve relative path to absolute module path
        fn resolve_relative_path(import_path: &str, importing_mod: &str) -> String {
            if import_path.starts_with("./") {
                let relative = import_path.trim_start_matches("./");
                let normalized = normalize_path(relative);

                if importing_mod.is_empty() {
                    normalized
                } else {
                    format!("{importing_mod}.{normalized}")
                }
            } else if import_path.starts_with("../") {
                let mut module_parts: Vec<String> =
                    importing_mod.split('.').map(|s| s.to_string()).collect();

                let mut path_remaining: &str = import_path;

                while path_remaining.starts_with("../") {
                    if !module_parts.is_empty() {
                        module_parts.pop();
                    }
                    path_remaining = &path_remaining[3..];
                }

                if !path_remaining.is_empty() {
                    let normalized = normalize_path(path_remaining);
                    module_parts.extend(
                        normalized
                            .split('.')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                    );
                }

                module_parts.join(".")
            } else {
                import_path.to_string()
            }
        }

        // Helper function to check if path matches with optional index resolution
        fn matches_with_index(candidate: &str, target: &str) -> bool {
            candidate == target || format!("{candidate}.index") == target
        }

        // Case 1: Exact match
        if import_path == symbol_module_path {
            return true;
        }

        // Case 2: Complex matching with importing module context
        if let Some(importing_mod) = importing_module {
            if import_path.starts_with("./") || import_path.starts_with("../") {
                let resolved = resolve_relative_path(import_path, importing_mod);

                if matches_with_index(&resolved, symbol_module_path) {
                    return true;
                }
            }
        }

        false
    }
}
