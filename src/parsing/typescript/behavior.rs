//! TypeScript-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::paths::strip_extension;
use crate::parsing::resolution::{InheritanceResolver, ResolutionScope};
use crate::project_resolver::persist::{ResolutionPersistence, ResolutionRules};
use crate::types::FileId;
use crate::{SymbolId, Visibility};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tree_sitter::Language;

use super::resolution::{TypeScriptInheritanceResolver, TypeScriptResolutionContext};

/// TypeScript language behavior implementation
#[derive(Clone)]
pub struct TypeScriptBehavior {
    state: BehaviorState,
}

impl TypeScriptBehavior {
    /// Create a new TypeScript behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }

    /// Load project resolution rules for a file from the persisted index
    ///
    /// Uses a thread-local cache to avoid repeated disk reads.
    /// Cache is invalidated after 1 second to pick up changes.
    fn load_project_rules_for_file(&self, file_id: FileId) -> Option<ResolutionRules> {
        thread_local! {
            static RULES_CACHE: RefCell<Option<(Instant, crate::project_resolver::persist::ResolutionIndex)>> = const { RefCell::new(None) };
        }

        RULES_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();

            // Check if cache is fresh (< 1 second old)
            let needs_reload = if let Some((timestamp, _)) = *cache {
                timestamp.elapsed() >= Duration::from_secs(1)
            } else {
                true
            };

            // Load fresh from disk if needed
            if needs_reload {
                let persistence =
                    ResolutionPersistence::new(Path::new(crate::init::local_dir_name()));
                if let Ok(index) = persistence.load("typescript") {
                    *cache = Some((Instant::now(), index));
                } else {
                    // No index file exists yet - that's OK
                    return None;
                }
            }

            // Get rules for the file
            if let Some((_, ref index)) = *cache {
                // Get the file path for this FileId from our behavior state
                if let Some(file_path) = self.state.get_file_path(file_id) {
                    // Find the config that applies to this file
                    if let Some(config_path) = index.get_config_for_file(&file_path) {
                        return index.rules.get(config_path).cloned();
                    }
                }

                // Fallback: return any rules we have (for tests without proper file registration)
                index.rules.values().next().cloned()
            } else {
                None
            }
        })
    }
}

impl Default for TypeScriptBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for TypeScriptBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for TypeScriptBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("typescript")
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Preserve parser-derived visibility (export detection), only set module path.
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // TypeScript uses file paths as module paths, not including the symbol name
        // All symbols in the same file share the same module path for visibility
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        // Use tsconfig infrastructure to compute canonical module paths
        // This ensures symbols use the SAME path format as enhanced imports

        // Load the resolution index to find which tsconfig governs this file
        let persistence = ResolutionPersistence::new(Path::new(crate::init::local_dir_name()));
        let index = persistence.load("typescript").ok()?;

        // get_config_for_file() expects a relative path (relative to workspace root)
        let relative_to_workspace = file_path
            .strip_prefix(project_root)
            .ok()
            .unwrap_or(file_path);

        // Find which tsconfig applies to this file
        let config_path = index.get_config_for_file(relative_to_workspace)?;
        tracing::debug!(
            "[typescript] module_path_from_file relative_to_workspace={relative_to_workspace:?} config_path={config_path:?}"
        );

        // Get the tsconfig's directory (the project root for this file)
        let tsconfig_dir = project_root.join(config_path.parent()?);
        tracing::debug!("[typescript] module_path_from_file tsconfig_dir={tsconfig_dir:?}");

        // Compute path relative to the tsconfig's directory
        let relative_path = file_path.strip_prefix(&tsconfig_dir).ok()?;
        let path = relative_path.to_str()?;

        // Remove file extensions using the provided extensions list
        let path_without_ext = strip_extension(path.trim_start_matches("./"), extensions);

        // Handle /index suffix (directory imports)
        let module_path = path_without_ext.trim_end_matches("/index");

        // Replace path separators with module separators
        let result = module_path.replace('/', ".");

        tracing::debug!(
            "[typescript] module_path_from_file file_path={file_path:?} -> module_path={result}"
        );

        Some(result)
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // TypeScript visibility modifiers
        if signature.contains("export ") || signature.contains("export default") {
            Visibility::Public
        } else if signature.contains("private ") || signature.contains("#") {
            Visibility::Private
        } else if signature.contains("protected ") {
            // TypeScript has protected but Rust's Visibility enum doesn't
            // Map protected to Module visibility as a reasonable approximation
            Visibility::Module
        } else {
            // Default visibility for TypeScript symbols
            // Module-level symbols are private by default unless exported
            Visibility::Private
        }
    }

    fn supports_traits(&self) -> bool {
        true // TypeScript has interfaces
    }

    fn supports_inherent_methods(&self) -> bool {
        true // TypeScript has class methods
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            None
        } else {
            Some(components.join("."))
        }
    }

    // TypeScript-specific resolution overrides

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(TypeScriptResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(TypeScriptInheritanceResolver::new())
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // TypeScript uses both "extends" and "implements"
        // Default to "extends" as it's more general
        "extends"
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
        // Store the ORIGINAL import path (including path aliases like @/components)
        // Enhancement will happen on-demand during resolution in build_resolution_context_with_cache()
        // This preserves the semantic information about whether a path was a tsconfig alias or a relative import
        tracing::debug!(
            "[typescript] add_import path='{}' alias={:?} file_id={:?}",
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
    /// Uses tsconfig path aliases via TypeScriptProjectEnhancer.
    /// Returns (scope, enhanced_imports) where enhanced_imports have path aliases resolved.
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
        // Extensions available for stripping from import paths
        let _ = extensions;
        use crate::parsing::ScopeLevel;
        use crate::parsing::resolution::{ImportBinding, ImportOrigin, ProjectResolutionEnhancer};

        // Helper to normalize relative imports
        fn normalize_import(import_path: &str, importing_mod: &str) -> String {
            if import_path.starts_with("./") {
                let rel = import_path.trim_start_matches("./").replace('/', ".");
                if importing_mod.is_empty() {
                    rel
                } else {
                    // Get parent of importing module
                    let parts: Vec<&str> = importing_mod.split('.').collect();
                    let parent = parts[..parts.len().saturating_sub(1)].join(".");
                    if parent.is_empty() {
                        rel
                    } else {
                        format!("{parent}.{rel}")
                    }
                }
            } else if import_path.starts_with("../") {
                // Just use the path as-is for now
                import_path.replace('/', ".")
            } else {
                // External or absolute
                import_path.replace('/', ".")
            }
        }

        let mut context = TypeScriptResolutionContext::new(file_id);

        let importing_module = self.get_module_path_for_file(file_id);

        // Load project rules for path alias enhancement
        let maybe_enhancer = self
            .load_project_rules_for_file(file_id)
            .map(super::resolution::TypeScriptProjectEnhancer::new);

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

            // Enhance import path if we have tsconfig rules
            let target_module = if let Some(ref enhancer) = maybe_enhancer {
                if let Some(enhanced_path) = enhancer.enhance_import_path(&import.path, file_id) {
                    // Tsconfig alias - convert enhanced path to module format
                    enhanced_path.trim_start_matches("./").replace('/', ".")
                } else {
                    // Regular import - normalize relative to importing module
                    normalize_import(&import.path, &importing_module.clone().unwrap_or_default())
                }
            } else {
                normalize_import(&import.path, &importing_module.clone().unwrap_or_default())
            };

            // Collect enhanced import with resolved path
            enhanced_imports.push(crate::parsing::Import {
                path: target_module.clone(),
                file_id: import.file_id,
                alias: import.alias.clone(),
                is_glob: import.is_glob,
                is_type_only: import.is_type_only,
            });

            // Look up candidates by local_name and match module_path
            let mut resolved_symbol: Option<SymbolId> = None;
            let candidates = cache.lookup_candidates(&local_name);
            for id in candidates {
                if let Some(symbol) = cache.get(id) {
                    if let Some(ref module_path) = symbol.module_path {
                        if module_path.as_ref() == target_module
                            || target_module.ends_with(module_path.as_ref())
                            || module_path.ends_with(&target_module)
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

        // Add local symbols from this file
        for sym_id in cache.symbols_in_file(file_id) {
            if let Some(symbol) = cache.get(sym_id) {
                if self.is_resolvable_symbol(&symbol) {
                    context.add_symbol(symbol.name.to_string(), symbol.id, ScopeLevel::Module);
                    if let Some(ref module_path) = symbol.module_path {
                        context.add_symbol(module_path.to_string(), symbol.id, ScopeLevel::Module);
                    }
                }
            }
        }

        (Box::new(context), enhanced_imports)
    }

    // TypeScript-specific: Support hoisting
    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // TypeScript hoists function declarations and class declarations
        // They can be used before their definition in the file
        let hoisted = matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
        );

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
            matches!(
                symbol.kind,
                SymbolKind::TypeAlias | SymbolKind::Constant | SymbolKind::Variable
            )
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
                // Same directory import
                let relative = import_path.trim_start_matches("./");
                let normalized = normalize_path(relative);

                if importing_mod.is_empty() {
                    normalized
                } else {
                    format!("{importing_mod}.{normalized}")
                }
            } else if import_path.starts_with("../") {
                // Parent directory import
                // Start with the importing module parts as owned strings
                let mut module_parts: Vec<String> =
                    importing_mod.split('.').map(|s| s.to_string()).collect();

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
                // Not a relative path, return as-is
                import_path.to_string()
            }
        }

        // Helper function to check if path matches with optional index resolution
        fn matches_with_index(candidate: &str, target: &str) -> bool {
            candidate == target || format!("{candidate}.index") == target
        }

        // Case 1: Exact match (most common case, check first for performance)
        if import_path == symbol_module_path {
            return true;
        }

        // Case 2: Only do complex matching if we have the importing module context
        if let Some(importing_mod) = importing_module {
            // TypeScript import resolution differs from Rust:
            // - Relative imports start with './' or '../'
            // - Absolute imports are package names or path aliases

            if import_path.starts_with("./") || import_path.starts_with("../") {
                // Resolve relative path to absolute module path
                let resolved = resolve_relative_path(import_path, importing_mod);

                // Check if it matches (with or without index)
                if matches_with_index(&resolved, symbol_module_path) {
                    return true;
                }
            }
            // else: bare module imports and scoped packages
            // These need exact match for now (TODO: implement proper resolution)
        }

        false
    }
}
