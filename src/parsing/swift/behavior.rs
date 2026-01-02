//! Swift-specific language behavior implementation

use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::{Import, InheritanceResolver, LanguageBehavior, ResolutionScope};
use crate::types::compact_string;
use crate::{FileId, Symbol, SymbolKind, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::{SwiftInheritanceResolver, SwiftResolutionContext};

/// Language behavior for Swift
#[derive(Clone)]
pub struct SwiftBehavior {
    state: BehaviorState,
}

impl SwiftBehavior {
    /// Create a new behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }
}

impl Default for SwiftBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for SwiftBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for SwiftBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("swift")
    }

    fn configure_symbol(&self, symbol: &mut Symbol, module_path: Option<&str>) {
        // Set module path
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }

        // Visibility is already set by parser's determine_visibility() from AST
        // Do not overwrite it here - tree-sitter provides accurate visibility_modifier nodes

        // For file modules, use the last segment of the path as the name
        if symbol.kind == SymbolKind::Module {
            if let Some(path) = module_path {
                if let Some(name) = path.rsplit('.').next() {
                    if !name.is_empty() {
                        symbol.name = compact_string(name);
                    }
                }
            }
        }
    }

    /// Check if symbol is visible from another file
    ///
    /// Swift visibility rules:
    /// - open/public: visible from any module
    /// - internal (Module): visible within same module only
    /// - fileprivate/private (Private): visible within same file only
    ///
    /// Note: We map fileprivate and private both to Visibility::Private
    /// since we don't track declaration boundaries within files.
    fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Check visibility level
        match symbol.visibility {
            Visibility::Public => true, // open/public - visible everywhere
            Visibility::Module => {
                // internal - visible within same module
                // For now, treat same module as visible (indexer handles module boundaries)
                true
            }
            Visibility::Private => false, // private/fileprivate - file-scoped only
            Visibility::Crate => true,    // Not used in Swift, but treat as visible
        }
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(SwiftResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(SwiftInheritanceResolver::new())
    }

    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        if base_path.is_empty() {
            symbol_name.to_string()
        } else if symbol_name == "<file>" {
            base_path.to_string()
        } else {
            format!("{base_path}.{symbol_name}")
        }
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // This is a fallback method required by the trait.
        // Primary visibility detection happens in parser's determine_visibility() via AST.
        // This is only used if signature-based detection is needed as a backup.
        if signature.contains("open ") || signature.contains("public ") {
            Visibility::Public
        } else if signature.contains("private ") || signature.contains("fileprivate ") {
            Visibility::Private
        } else {
            // Swift default is internal
            Visibility::Module
        }
    }

    fn module_separator(&self) -> &'static str {
        "."
    }

    /// Convert file path to Swift module path
    ///
    /// Uses cached resolution rules from SwiftProvider to map file paths to modules.
    /// Falls back to convention-based path stripping if no cache is available.
    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        use crate::project_resolver::persist::ResolutionPersistence;
        use std::cell::RefCell;
        use std::time::{Duration, Instant};

        // Thread-local cache with 1-second TTL (per Java/TypeScript pattern)
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
                if let Ok(index) = persistence.load("swift") {
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
                            // Extract module from file path using source roots
                            for root_path in rules.paths.keys() {
                                let root = std::path::Path::new(root_path);
                                let canon_root =
                                    root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

                                if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                                    // Convert path to module: MyModule/Types/User.swift -> MyModule.Types
                                    if let Some(parent) = relative.parent() {
                                        let module_path =
                                            parent.to_string_lossy().replace(['/', '\\'], ".");
                                        return Some(module_path);
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

        // Fallback: convention-based path stripping
        let relative = file_path.strip_prefix(project_root).ok()?;
        let mut path = relative.to_string_lossy().replace('\\', "/");

        // Remove .swift extension
        if path.ends_with(".swift") {
            path.truncate(path.len() - 6);
        }

        // Strip common Swift source directories
        let path = path
            .trim_start_matches("Sources/")
            .trim_start_matches("Source/")
            .trim_start_matches("src/")
            .trim_start_matches("Tests/");

        // Convert path separators to dots
        let module_path = path.replace('/', ".");

        Some(module_path)
    }

    fn get_language(&self) -> Language {
        tree_sitter_swift::LANGUAGE.into()
    }

    fn supports_traits(&self) -> bool {
        true // Swift has protocols
    }

    fn supports_inherent_methods(&self) -> bool {
        false // Swift methods are defined in class/struct body, not separate impl blocks
    }

    // Import tracking methods using state
    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.register_file_with_state(path, file_id, module_path);
    }

    fn add_import(&self, import: Import) {
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<Import> {
        self.get_imports_from_state(file_id)
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
        self.state.get_file_path(file_id)
    }

    /// Build resolution context for parallel pipeline (no Tantivy).
    ///
    /// Swift doesn't have path aliases like TypeScript/JavaScript.
    /// Imports are module-level (e.g., `import Foundation`).
    ///
    /// CRITICAL: Symbols from pipeline have `module_path: None`.
    /// We compute module_path on-the-fly from `symbol.file_path` using rules.
    fn build_resolution_context_with_pipeline_cache(
        &self,
        file_id: FileId,
        imports: &[crate::parsing::Import],
        cache: &dyn crate::parsing::PipelineSymbolCache,
    ) -> (
        Box<dyn crate::parsing::ResolutionScope>,
        Vec<crate::parsing::Import>,
    ) {
        use crate::parsing::ScopeLevel;
        use crate::parsing::resolution::{ImportBinding, ImportOrigin};
        use std::path::PathBuf;

        let mut context = SwiftResolutionContext::new(file_id);

        // Helper to compute module_path from file_path using rules
        // Rules contain source roots; module_path_from_file extracts module name
        let compute_module_path = |file_path: &str| -> Option<String> {
            let path = PathBuf::from(file_path);
            // project_root is unused by Swift's module_path_from_file (uses rules instead)
            self.module_path_from_file(&path, &PathBuf::new())
        };

        // Build enhanced imports (Swift imports are already module-level, no transformation)
        let mut enhanced_imports = Vec::with_capacity(imports.len());

        for import in imports {
            // For Swift, the import path is the module name (e.g., "Foundation")
            let local_name = import.alias.clone().unwrap_or_else(|| import.path.clone());

            // Collect enhanced import (no transformation needed for Swift)
            enhanced_imports.push(crate::parsing::Import {
                path: import.path.clone(),
                file_id: import.file_id,
                alias: import.alias.clone(),
                is_glob: import.is_glob,
                is_type_only: import.is_type_only,
            });

            // Look up candidates by module name and match computed module_path
            let mut resolved_symbol: Option<crate::SymbolId> = None;
            let candidates = cache.lookup_candidates(&local_name);
            for id in candidates {
                if let Some(symbol) = cache.get(id) {
                    // Compute module_path from file_path using rules
                    if let Some(computed_module) = compute_module_path(&symbol.file_path) {
                        // Swift: import Foundation matches Foundation.* symbols
                        if computed_module == import.path
                            || computed_module.starts_with(&format!("{}.", import.path))
                        {
                            resolved_symbol = Some(id);
                            break;
                        }
                    }
                }
            }

            // Determine origin (consistent with TypeScript/JavaScript pattern)
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
                context.add_symbol(local_name.clone(), symbol_id, ScopeLevel::Global);
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
                        context.add_symbol(computed_module, symbol.id, ScopeLevel::Global);
                    }
                }
            }
        }

        (Box::new(context), enhanced_imports)
    }

    /// Check if a symbol can participate in resolution
    fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool {
        use crate::symbol::ScopeContext;

        // Swift resolves types, functions, and properties
        let resolvable_kind = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Interface  // Protocol
                | SymbolKind::Method
                | SymbolKind::Field      // Property
                | SymbolKind::Enum
                | SymbolKind::Constant
                | SymbolKind::TypeAlias
        );

        if !resolvable_kind {
            return false;
        }

        // Check scope context - exclude local variables and parameters
        if let Some(ref scope_context) = symbol.scope_context {
            matches!(
                scope_context,
                ScopeContext::Module | ScopeContext::Global | ScopeContext::ClassMember { .. }
            )
        } else {
            true // No scope context = resolvable
        }
    }

    /// Initialize resolution context with imports for this file
    ///
    /// Called by the indexer to populate the resolution context with
    /// import bindings from BehaviorState before symbol resolution.
    fn initialize_resolution_context(&self, context: &mut dyn ResolutionScope, file_id: FileId) {
        // Downcast to SwiftResolutionContext to access Swift-specific methods
        if context
            .as_any_mut()
            .downcast_mut::<SwiftResolutionContext>()
            .is_some()
        {
            // Get imports for this file from BehaviorState
            let imports = self.state.get_imports_for_file(file_id);

            // Populate imports into the context
            context.populate_imports(&imports);
        }
    }

    /// Check if import path matches a symbol's module path
    ///
    /// Swift imports are module-level: `import Foundation`
    /// A symbol in Foundation.String matches import "Foundation"
    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        // Exact match: import MyModule matches MyModule
        if import_path == symbol_module_path {
            return true;
        }

        // Module prefix match: import Foundation matches Foundation.String
        if symbol_module_path.starts_with(import_path)
            && symbol_module_path
                .get(import_path.len()..)
                .is_some_and(|s| s.starts_with('.'))
        {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_visibility_fallback() {
        let behavior = SwiftBehavior::new();

        // Test fallback signature parsing (not primary detection)
        assert_eq!(
            behavior.parse_visibility("public class Foo"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("open class Foo"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("private var x"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("fileprivate func bar()"),
            Visibility::Private
        );
        assert_eq!(behavior.parse_visibility("class Foo"), Visibility::Module); // Default
    }

    #[test]
    fn test_format_module_path() {
        let behavior = SwiftBehavior::new();

        assert_eq!(
            behavior.format_module_path("MyApp", "MyClass"),
            "MyApp.MyClass"
        );
        assert_eq!(behavior.format_module_path("", "MyClass"), "MyClass");
        assert_eq!(behavior.format_module_path("MyApp", "<file>"), "MyApp");
    }

    #[test]
    fn test_module_separator() {
        let behavior = SwiftBehavior::new();
        assert_eq!(behavior.module_separator(), ".");
    }
}
