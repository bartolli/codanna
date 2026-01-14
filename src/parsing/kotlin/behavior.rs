//! Kotlin-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::ResolutionScope;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::paths::strip_extension;
use crate::parsing::{Import, InheritanceResolver};
use crate::symbol::ScopeContext;
use crate::types::compact_string;
use crate::{FileId, Symbol, SymbolKind, Visibility};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tree_sitter::Language;

/// Language behavior for Kotlin
#[derive(Clone)]
pub struct KotlinBehavior {
    language: Language,
    state: BehaviorState,
    expression_types: Arc<RwLock<HashMap<FileId, HashMap<String, String>>>>,
}

impl KotlinBehavior {
    /// Create a new behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_kotlin::language(),
            state: BehaviorState::new(),
            expression_types: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the fully qualified class name containing this symbol
    ///
    /// Extracts the class name from ClassMember scope context and combines
    /// with module_path (package) to form fully qualified name.
    pub fn get_containing_class(&self, symbol: &Symbol) -> Option<String> {
        if let Some(ScopeContext::ClassMember {
            class_name: Some(class),
        }) = &symbol.scope_context
        {
            // Combine package + class for fully qualified name
            if let Some(pkg) = &symbol.module_path {
                if pkg.is_empty() {
                    return Some(class.to_string()); // Default package
                }
                return Some(format!("{pkg}.{class}")); // Qualified
            }
            return Some(class.to_string()); // No package info
        }
        None
    }

    /// Check if symbol is visible from another file with full inheritance support
    ///
    /// Enhanced visibility check that supports:
    /// - Inheritance-based protected access (requires inheritance resolver)
    ///
    /// Kotlin visibility levels:
    /// - private: file-scoped
    /// - internal: module-scoped (mapped to Crate)
    /// - protected: subclasses only (mapped to Module)
    /// - public: everywhere
    pub fn is_symbol_visible_from_context(
        &self,
        symbol: &Symbol,
        from_file: FileId,
        accessing_class: Option<&str>,
        inheritance: &dyn InheritanceResolver,
    ) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Check visibility modifiers
        match symbol.visibility {
            Visibility::Private => false, // Private symbols are file-scoped
            Visibility::Crate => {
                // Kotlin internal - module-scoped
                // For now, be permissive (would need module boundary tracking)
                true
            }
            Visibility::Module => {
                // Kotlin protected - accessible to subclasses
                // Check inheritance if context available
                if let Some(accessing) = accessing_class {
                    if let Some(containing) = self.get_containing_class(symbol) {
                        return inheritance.is_subtype(accessing, &containing);
                    }
                }

                // No context for inheritance check: be permissive
                true
            }
            Visibility::Public => true, // Public symbols visible everywhere
        }
    }

    /// Check if symbol is visible from another file (backward compatible)
    ///
    /// Delegates to is_symbol_visible_from_context() without inheritance context.
    /// For full inheritance-based protected checks, use is_symbol_visible_from_context().
    pub fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Delegate to enhanced method without inheritance context
        let resolver = super::KotlinInheritanceResolver::new();
        self.is_symbol_visible_from_context(symbol, from_file, None, &resolver)
    }
}

impl StatefulBehavior for KotlinBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for KotlinBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for KotlinBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("kotlin")
    }

    fn configure_symbol(&self, symbol: &mut Symbol, module_path: Option<&str>) {
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }

        if let Some(signature) = &symbol.signature {
            symbol.visibility = self.parse_visibility(signature);
        }

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

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(crate::parsing::kotlin::KotlinResolutionContext::new(
            file_id,
        ))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(crate::parsing::kotlin::KotlinInheritanceResolver::new())
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
        let trimmed = signature.trim();

        if trimmed.contains("private") {
            Visibility::Private
        } else if trimmed.contains("protected") {
            Visibility::Module // Map protected to module-level
        } else if trimmed.contains("internal") {
            Visibility::Crate // Map internal to crate-level
        } else {
            Visibility::Public // Kotlin default
        }
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

    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        let relative = file_path.strip_prefix(project_root).ok()?;
        let path = relative.to_string_lossy().replace('\\', "/");

        // Strip file extension using the provided extensions list
        let path_without_ext = strip_extension(&path, extensions);

        // Convert path to package notation: src/main/kotlin/com/example/MyClass -> com.example.MyClass
        // Strip common Kotlin source directories
        let path_stripped = path_without_ext
            .trim_start_matches("src/main/kotlin/")
            .trim_start_matches("src/main/java/")
            .trim_start_matches("src/test/kotlin/")
            .trim_start_matches("src/test/java/")
            .trim_start_matches("src/");

        // Convert path separators to dots
        let module_path = path_stripped.replace('/', ".");

        Some(module_path)
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn supports_traits(&self) -> bool {
        true // Kotlin has interfaces
    }

    // Override import tracking methods to use state
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

    fn register_expression_types(&self, file_id: FileId, entries: &[(String, String)]) {
        if entries.is_empty() {
            return;
        }

        let mut map = HashMap::with_capacity(entries.len());
        for (expr, ty) in entries {
            map.insert(expr.clone(), ty.clone());
        }
        self.expression_types.write().insert(file_id, map);
        tracing::debug!(
            "[kotlin] registered {} expression types for file {:?}",
            entries.len(),
            file_id
        );
    }

    fn initialize_resolution_context(&self, context: &mut dyn ResolutionScope, file_id: FileId) {
        if let Some(kotlin_ctx) = context
            .as_any_mut()
            .downcast_mut::<crate::parsing::kotlin::KotlinResolutionContext>()
        {
            if let Some(entries) = self.expression_types.write().remove(&file_id) {
                kotlin_ctx.set_expression_types(entries);
            }
        }
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

        // Handle wildcard imports (import com.example.*)
        if let Some(base) = import_path.strip_suffix(".*") {
            if let Some(stripped) = symbol_module_path.strip_prefix(base) {
                // Check that it's a direct child (not nested deeper)
                if let Some(remainder) = stripped.strip_prefix('.') {
                    // No more dots = direct child
                    return !remainder.contains('.');
                }
            }
        }

        // Handle partial matches (import com.example.MyClass matches symbol com.example.MyClass.InnerClass)
        if let Some(remainder) = symbol_module_path.strip_prefix(import_path) {
            return remainder.is_empty() || remainder.starts_with('.');
        }

        false
    }

    fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool {
        use crate::symbol::ScopeContext;

        // Kotlin resolves classes, functions, properties, etc.
        let resolvable_kind = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Method
                | SymbolKind::Field
                | SymbolKind::Enum
        );

        if !resolvable_kind {
            return false;
        }

        // Check scope context
        if let Some(ref scope_context) = symbol.scope_context {
            matches!(
                scope_context,
                ScopeContext::Module
                    | ScopeContext::Global
                    | ScopeContext::ClassMember { .. }
                    | ScopeContext::Package
            )
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_visibility() {
        let behavior = KotlinBehavior::new();

        assert_eq!(
            behavior.parse_visibility("private fun test()"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("protected fun test()"),
            Visibility::Module
        );
        assert_eq!(
            behavior.parse_visibility("internal fun test()"),
            Visibility::Crate
        );
        assert_eq!(behavior.parse_visibility("fun test()"), Visibility::Public);
        assert_eq!(
            behavior.parse_visibility("public fun test()"),
            Visibility::Public
        );
    }

    #[test]
    fn test_format_module_path() {
        let behavior = KotlinBehavior::new();

        assert_eq!(
            behavior.format_module_path("com.example", "MyClass"),
            "com.example.MyClass"
        );
        assert_eq!(behavior.format_module_path("", "MyClass"), "MyClass");
    }

    #[test]
    fn test_import_matches_symbol() {
        let behavior = KotlinBehavior::new();

        // Exact match
        assert!(behavior.import_matches_symbol("com.example.MyClass", "com.example.MyClass", None));

        // Wildcard match
        assert!(behavior.import_matches_symbol("com.example.*", "com.example.MyClass", None));

        // Wildcard should not match nested
        assert!(!behavior.import_matches_symbol("com.example.*", "com.example.sub.MyClass", None));

        // Partial match
        assert!(behavior.import_matches_symbol(
            "com.example.MyClass",
            "com.example.MyClass.InnerClass",
            None
        ));
    }

    #[test]
    fn test_module_separator() {
        let behavior = KotlinBehavior::new();
        assert_eq!(behavior.module_separator(), ".");
    }

    #[test]
    fn test_supports_traits() {
        let behavior = KotlinBehavior::new();
        assert!(behavior.supports_traits());
    }

    #[test]
    fn test_protected_visibility_with_inheritance() {
        use crate::types::{FileId, Range, SymbolId};

        let behavior = KotlinBehavior::new();

        // Create a protected method in com.example.Parent
        let mut symbol = Symbol::new(
            SymbolId(1),
            "protectedMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 10,
                start_column: 5,
                end_line: 12,
                end_column: 6,
            },
        );
        symbol.module_path = Some("com.example".to_string().into());
        symbol.visibility = Visibility::Module; // Protected
        symbol.scope_context = Some(ScopeContext::ClassMember {
            class_name: Some("Parent".to_string().into()),
        });

        // Setup inheritance: Child extends Parent
        let mut resolver = super::super::KotlinInheritanceResolver::new();
        resolver.add_inheritance(
            "com.other.Child".to_string(),
            "com.example.Parent".to_string(),
            "extends",
        );

        // Test: Should be visible to subclass
        assert!(
            behavior.is_symbol_visible_from_context(
                &symbol,
                FileId(2),
                Some("com.other.Child"),
                &resolver
            ),
            "Protected symbol should be visible to subclass"
        );
    }

    #[test]
    fn test_protected_visibility_without_inheritance() {
        use crate::types::{FileId, Range, SymbolId};

        let behavior = KotlinBehavior::new();

        // Create a protected method in com.example.Parent
        let mut symbol = Symbol::new(
            SymbolId(1),
            "protectedMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 10,
                start_column: 5,
                end_line: 12,
                end_column: 6,
            },
        );
        symbol.module_path = Some("com.example".to_string().into());
        symbol.visibility = Visibility::Module; // Protected
        symbol.scope_context = Some(ScopeContext::ClassMember {
            class_name: Some("Parent".to_string().into()),
        });

        // No inheritance relationship
        let resolver = super::super::KotlinInheritanceResolver::new();

        // Test: Should be denied when no inheritance
        assert!(
            !behavior.is_symbol_visible_from_context(
                &symbol,
                FileId(2),
                Some("com.other.Unrelated"),
                &resolver
            ),
            "Protected symbol should NOT be visible to non-subclass"
        );
    }

    #[test]
    fn test_get_containing_class_kotlin() {
        use crate::types::{FileId, Range, SymbolId};

        let behavior = KotlinBehavior::new();

        // Method in com.example.MyClass
        let mut symbol = Symbol::new(
            SymbolId(1),
            "myMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 10,
                start_column: 5,
                end_line: 12,
                end_column: 6,
            },
        );
        symbol.module_path = Some("com.example".to_string().into());
        symbol.scope_context = Some(ScopeContext::ClassMember {
            class_name: Some("MyClass".to_string().into()),
        });

        // Test: Should return fully qualified class name
        assert_eq!(
            behavior.get_containing_class(&symbol),
            Some("com.example.MyClass".to_string()),
            "Should combine package and class name"
        );

        // Method in default package
        let mut symbol2 = Symbol::new(
            SymbolId(2),
            "method2",
            SymbolKind::Method,
            FileId(2),
            Range {
                start_line: 5,
                start_column: 5,
                end_line: 7,
                end_column: 6,
            },
        );
        symbol2.module_path = Some("".to_string().into()); // Empty package
        symbol2.scope_context = Some(ScopeContext::ClassMember {
            class_name: Some("SimpleClass".to_string().into()),
        });

        // Test: Default package should return simple name
        assert_eq!(
            behavior.get_containing_class(&symbol2),
            Some("SimpleClass".to_string()),
            "Should return simple name for default package"
        );
    }
}
