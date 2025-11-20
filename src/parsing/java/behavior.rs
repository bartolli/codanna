//! Java language behavior implementation
//!
//! Provides language-specific behaviors for Java.
//!
//! TODO: Implement methods after exploring actual Java AST with tree-sitter.

use crate::parsing::{
    Import, InheritanceResolver, LanguageBehavior, ResolutionScope,
    behavior_state::{BehaviorState, StatefulBehavior},
};
use crate::symbol::ScopeContext;
use crate::{FileId, Symbol, SymbolKind, Visibility};
use std::path::{Path, PathBuf};

/// Behavior handler for Java language
#[derive(Clone)]
pub struct JavaBehavior {
    state: BehaviorState,
}

impl JavaBehavior {
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }
}

impl Default for JavaBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for JavaBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for JavaBehavior {
    /// Format module path for Java packages using dot notation
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        if base_path.is_empty() {
            symbol_name.to_string()
        } else if symbol_name == "<file>" {
            base_path.to_string()
        } else {
            format!("{base_path}.{symbol_name}")
        }
    }

    /// Parse visibility from signature
    ///
    /// Java visibility levels:
    /// - public: accessible everywhere
    /// - protected: accessible in same package + subclasses
    /// - package-private: accessible in same package only (default when no modifier)
    /// - private: accessible only within class
    fn parse_visibility(&self, signature: &str) -> Visibility {
        let trimmed = signature.trim();

        if trimmed.contains("private") {
            Visibility::Private
        } else if trimmed.contains("protected") {
            Visibility::Module // Protected in Java
        } else if trimmed.contains("public") {
            Visibility::Public
        } else {
            Visibility::Crate // Package-private (Java default when no modifier)
        }
    }

    /// Get module separator for Java (dot notation)
    fn module_separator(&self) -> &'static str {
        "." // Java uses dot separation for packages
    }

    /// Java supports interfaces (similar to traits)
    fn supports_traits(&self) -> bool {
        true
    }

    /// Java defines methods within class bodies (not as inherent impls)
    fn supports_inherent_methods(&self) -> bool {
        false
    }

    /// Get tree-sitter language
    fn get_language(&self) -> tree_sitter::Language {
        tree_sitter_java::LANGUAGE.into()
    }

    /// Validate node kind (tree-sitter ABI compatibility)
    fn validate_node_kind(&self, _node_kind: &str) -> bool {
        true
    }

    /// Get ABI version for Java grammar
    fn get_abi_version(&self) -> usize {
        15 // Java grammar uses ABI-15
    }

    /// Normalize caller name for better matching
    /// TODO: Implement if needed for Java-specific normalization
    fn normalize_caller_name(&self, name: &str, _file_id: crate::FileId) -> String {
        name.to_string()
    }

    /// Configure symbol with module path and other metadata
    ///
    /// This method is called for every symbol during parsing to set:
    /// - module_path: The package path (e.g., "com.example")
    /// - visibility: Parsed from the signature (public, protected, private, package-private)
    fn configure_symbol(&self, symbol: &mut Symbol, module_path: Option<&str>) {
        // Set module path (package)
        if let Some(path) = module_path {
            symbol.module_path = Some(path.to_string().into());
        }

        // Visibility is already set by parser's determine_visibility() during symbol extraction
        // Do not overwrite it here, as the signature may not contain modifier keywords
    }

    /// Check if import matches symbol (trait implementation)
    ///
    /// Java imports include the symbol name: `import com.example.Person;`
    /// Must split into module (`com.example`) and symbol (`Person`) for matching.
    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // Strip "static " prefix if present
        let import_path = import_path.strip_prefix("static ").unwrap_or(import_path);

        // Handle wildcard imports: import com.example.*
        if let Some(base) = import_path.strip_suffix(".*") {
            // Wildcard matches symbols in that exact package (not nested)
            return symbol_module_path == base;
        }

        // For single-type imports: import com.example.Person
        // Split into package and class name
        if let Some((import_package, _import_class)) = import_path.rsplit_once('.') {
            // Match if packages are the same
            if import_package == symbol_module_path {
                return true;
            }
        }

        // Same package: symbols in same package don't need imports
        if let Some(current_pkg) = importing_module {
            if current_pkg == symbol_module_path {
                return true;
            }
        }

        false
    }

    /// Convert file path to Java package path
    ///
    /// Uses cached resolution rules to map file paths to Java packages.
    /// Requires that JavaProvider.rebuild_cache() has been called with project settings.
    fn module_path_from_file(&self, file_path: &Path, _project_root: &Path) -> Option<String> {
        use crate::project_resolver::persist::ResolutionPersistence;
        use std::cell::RefCell;
        use std::time::{Duration, Instant};

        // Thread-local cache with 1-second TTL (per TypeScript pattern)
        thread_local! {
            static RULES_CACHE: RefCell<Option<(Instant, crate::project_resolver::persist::ResolutionIndex)>> = const { RefCell::new(None) };
        }

        RULES_CACHE.with(|cache| {
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
                if let Ok(index) = persistence.load("java") {
                    *cache_ref = Some((Instant::now(), index));
                } else {
                    return None;
                }
            }

            // Get rules for this file
            if let Some((_, ref index)) = *cache_ref {
                // Canonicalize file path for matching
                let canon_file = file_path.canonicalize().ok()?;

                // Find config that applies to this file
                let config_path = index.get_config_for_file(&canon_file)?;
                let rules = index.rules.get(config_path)?;

                // Extract package from file path using source roots
                for root_path in rules.paths.keys() {
                    let root = std::path::Path::new(root_path);

                    // Canonicalize root path if it exists (runtime resolution)
                    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

                    if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                        // Convert path to package: com/example/Foo.java → com.example
                        let package_path = relative
                            .parent()? // Remove Foo.java
                            .to_string_lossy()
                            .replace(['/', '\\'], ".");

                        return Some(package_path);
                    }
                }
            }

            None
        })
    }

    /// Get module path for a file (delegates to BehaviorState)
    ///
    /// This override is CRITICAL for same-package symbol resolution.
    /// Without it, the default implementation returns None and same-package
    /// symbols won't be added to the resolution context.
    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    /// Register file with behavior state (trait override)
    ///
    /// This override is CRITICAL - without it, module paths won't be stored
    /// and same-package resolution won't work.
    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.state.register_file(path, file_id, module_path);
    }
}

impl JavaBehavior {
    // =========================================================================
    // ADDITIONAL BEHAVIOR METHODS (matching Kotlin's API)
    // =========================================================================

    /// Create resolution context for this file
    pub fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(super::JavaResolutionContext::new(file_id))
    }

    /// Create inheritance resolver
    pub fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(super::JavaInheritanceResolver::new())
    }

    /// Register file in behavior state
    pub fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.state.register_file(path, file_id, module_path);
    }

    /// Add import to behavior state
    pub fn add_import(&self, import: Import) {
        self.state.add_import(import);
    }

    /// Get all imports for a file
    pub fn get_imports_for_file(&self, file_id: FileId) -> Vec<Import> {
        self.state.get_imports_for_file(file_id)
    }

    /// Get module path for a file
    pub fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    /// Register expression types for a file
    /// TODO: Implement if Java needs type inference tracking
    pub fn register_expression_types(&self, _file_id: FileId, _entries: &[(String, String)]) {
        // TODO: Implement if needed for Java type inference
    }

    /// Initialize resolution context with imports and file state
    ///
    /// This method:
    /// 1. Retrieves imports for this file from BehaviorState
    /// 2. Populates the resolution context with those imports
    /// 3. Adds package-level symbols to enable package-private access
    pub fn initialize_resolution_context(
        &self,
        context: &mut dyn ResolutionScope,
        file_id: FileId,
    ) {
        // Downcast to JavaResolutionContext to access Java-specific methods
        if context
            .as_any_mut()
            .downcast_mut::<super::JavaResolutionContext>()
            .is_some()
        {
            // Get imports for this file from BehaviorState
            let imports = self.state.get_imports_for_file(file_id);

            // Populate imports into the context
            context.populate_imports(&imports);

            // Note: Package-level symbols will be added by the indexer
            // when it processes all files in the same package
        }
    }

    /// Check if import matches symbol (legacy - not used by resolution system)
    #[allow(dead_code)]
    fn import_matches_symbol_legacy(
        &self,
        import_path: &str,
        symbol: &Symbol,
        current_package: Option<&str>,
    ) -> bool {
        // Get symbol's module path
        let symbol_module_path = match symbol.module_path {
            Some(ref path) => path.as_ref(),
            None => return false,
        };

        // Strip "static " prefix if present (static imports encoded in path)
        let import_path = import_path.strip_prefix("static ").unwrap_or(import_path);

        // Build full symbol path: package.ClassName
        // For nested classes: package.OuterClass.InnerClass
        let symbol_full_path = if symbol_module_path.is_empty() {
            symbol.name.as_ref()
        } else {
            // Symbol name might already be qualified, check if module_path ends with class name
            if symbol_module_path.ends_with(symbol.name.as_ref()) {
                symbol_module_path
            } else {
                // Need to construct: module_path.name
                return self.check_import_match(
                    import_path,
                    symbol_module_path,
                    &symbol.name,
                    current_package,
                );
            }
        };

        // Exact match: import com.example.MyClass
        if import_path == symbol_full_path {
            return true;
        }

        // Wildcard imports: import com.example.*
        if let Some(base) = import_path.strip_suffix(".*") {
            // Check if symbol is in this package
            if let Some(stripped) = symbol_full_path.strip_prefix(base) {
                if let Some(remainder) = stripped.strip_prefix('.') {
                    // Only match direct children (no more dots)
                    return !remainder.contains('.');
                }
            }
        }

        // Same package: no import needed if in same package
        if let Some(current_pkg) = current_package {
            if let Some((symbol_pkg, _)) = symbol_full_path.rsplit_once('.') {
                if current_pkg == symbol_pkg {
                    return true;
                }
            }
        }

        false
    }

    /// Helper to check import match when constructing qualified name
    fn check_import_match(
        &self,
        import_path: &str,
        module_path: &str,
        symbol_name: &str,
        current_package: Option<&str>,
    ) -> bool {
        // Construct full path: module_path.symbol_name
        let full_path = format!("{module_path}.{symbol_name}");

        // Exact match
        if import_path == full_path {
            return true;
        }

        // Wildcard match
        if let Some(base) = import_path.strip_suffix(".*") {
            if module_path == base {
                return true;
            }
        }

        // Same package
        if let Some(current_pkg) = current_package {
            if module_path == current_pkg {
                return true;
            }
        }

        false
    }

    /// Check if symbol is resolvable (has enough information)
    ///
    /// Filters which symbols can participate in resolution based on:
    /// 1. Symbol kind (classes, methods, fields, etc. are resolvable)
    /// 2. Scope context (excludes local variables and parameters)
    pub fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool {
        use crate::symbol::ScopeContext;

        // Java resolves classes, methods, fields, interfaces, enums, etc.
        let resolvable_kind = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Method
                | SymbolKind::Field
                | SymbolKind::Enum
                | SymbolKind::Constant
        );

        if !resolvable_kind {
            return false;
        }

        // Check scope context - exclude local variables and parameters
        if let Some(ref scope_context) = symbol.scope_context {
            matches!(
                scope_context,
                ScopeContext::Module
                    | ScopeContext::Global
                    | ScopeContext::ClassMember { .. }
                    | ScopeContext::Package
            )
        } else {
            true // No scope context = resolvable
        }
    }

    /// Check if symbol is visible from another file
    ///
    /// Implements Java visibility rules:
    /// - Public: visible everywhere
    /// - Protected: visible in same package + subclasses
    /// - Package-private (default): visible in same package only
    /// - Private: not visible outside class/file
    ///
    /// Check if symbol is visible from another file (backward compatible)
    ///
    /// Delegates to is_symbol_visible_from_context() without inheritance context.
    /// For full inheritance-based protected checks, use is_symbol_visible_from_context().
    pub fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Delegate to enhanced method without inheritance context
        // This makes protected symbols permissive for cross-package access
        let resolver = super::JavaInheritanceResolver::new();
        self.is_symbol_visible_from_context(symbol, from_file, None, &resolver)
    }

    /// Check if a symbol and a file are in the same package
    ///
    /// Compares the symbol's module_path with the package of the given file.
    /// Returns true if both are in the same Java package.
    fn is_same_package(&self, symbol: &Symbol, file_id: FileId) -> bool {
        // Get the package of the file asking for access
        let from_package = self.state.get_module_path(file_id);

        // Get the package of the symbol
        let symbol_package = symbol.module_path.as_deref();

        // Compare packages
        match (symbol_package, from_package.as_deref()) {
            (Some(sym_pkg), Some(from_pkg)) => sym_pkg == from_pkg,
            (None, None) => true, // Both in default package
            _ => false,           // One has package, other doesn't
        }
    }

    /// Get the fully qualified class name containing this symbol
    ///
    /// Extracts the class name from ClassMember scope context and combines
    /// with module_path (package) to form fully qualified name.
    ///
    /// Returns:
    /// - For methods/fields: "com.example.MyClass" or "com.example.Outer.Inner"
    /// - For default package: "MyClass"
    /// - For non-members: None
    fn get_containing_class(&self, symbol: &Symbol) -> Option<String> {
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
    /// - Same package access
    /// - Inheritance-based protected access (requires inheritance resolver)
    ///
    /// Parameters:
    /// - `symbol`: The symbol being accessed
    /// - `from_file`: The file attempting access
    /// - `accessing_class`: Optional fully qualified class name doing the access
    /// - `inheritance`: Inheritance resolver for subclass checks
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
            Visibility::Private => false, // Private symbols are class-scoped only
            Visibility::Crate => {
                // Package-private (Java default) - same package only
                self.is_same_package(symbol, from_file)
            }
            Visibility::Module => {
                // Protected - accessible in same package + subclasses

                // Same package: always grants access
                if self.is_same_package(symbol, from_file) {
                    return true;
                }

                // Cross-package: check inheritance if context available
                if let Some(accessing) = accessing_class {
                    if let Some(containing) = self.get_containing_class(symbol) {
                        return inheritance.is_subtype(accessing, &containing);
                    }
                }

                // No context for inheritance check: be permissive
                // The compiler will catch actual violations
                true
            }
            Visibility::Public => true, // Public symbols visible everywhere
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, Range, SymbolId};

    #[test]
    fn test_protected_visibility_same_package() {
        let behavior = JavaBehavior::new();

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

        // Register file in same package
        behavior.register_file(
            "com/example/Child.java".into(),
            FileId(2),
            "com.example".to_string(),
        );

        // Test: Same package access should succeed
        let resolver = super::super::JavaInheritanceResolver::new();
        assert!(
            behavior.is_symbol_visible_from_context(
                &symbol,
                FileId(2),
                Some("com.example.Child"),
                &resolver
            ),
            "Protected symbol should be visible in same package"
        );
    }

    #[test]
    fn test_protected_visibility_cross_package_with_inheritance() {
        let behavior = JavaBehavior::new();

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

        // Register file in different package
        behavior.register_file(
            "com/other/Child.java".into(),
            FileId(2),
            "com.other".to_string(),
        );

        // Setup inheritance: Child extends Parent
        let mut resolver = super::super::JavaInheritanceResolver::new();
        resolver.add_inheritance(
            "com.other.Child".to_string(),
            "com.example.Parent".to_string(),
            "extends",
        );

        // Test: Cross-package access should succeed for subclass
        assert!(
            behavior.is_symbol_visible_from_context(
                &symbol,
                FileId(2),
                Some("com.other.Child"),
                &resolver
            ),
            "Protected symbol should be visible to subclass in different package"
        );
    }

    #[test]
    fn test_protected_visibility_cross_package_without_inheritance() {
        let behavior = JavaBehavior::new();

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

        // Register file in different package
        behavior.register_file(
            "com/other/Unrelated.java".into(),
            FileId(2),
            "com.other".to_string(),
        );

        // No inheritance relationship
        let resolver = super::super::JavaInheritanceResolver::new();

        // Test: Cross-package access should FAIL when no inheritance exists
        // We have full context - both classes known, so we can check inheritance
        // Since Unrelated does NOT extend Parent, access should be denied
        assert!(
            !behavior.is_symbol_visible_from_context(
                &symbol,
                FileId(2),
                Some("com.other.Unrelated"),
                &resolver
            ),
            "Protected symbol should NOT be visible to non-subclass in different package"
        );
    }

    #[test]
    fn test_get_containing_class() {
        let behavior = JavaBehavior::new();

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

    #[test]
    fn test_module_path_from_file_uses_provider() {
        use crate::project_resolver::provider::ProjectResolutionProvider;
        use std::fs;
        use tempfile::TempDir;

        // TDD: Given a Java file and cached project configuration
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        let pom_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
</project>"#;

        fs::write(&pom_path, pom_content).unwrap();

        // Create settings and build provider cache
        let settings_content = format!(
            r#"
[languages.java]
enabled = true
config_files = ["{}"]
"#,
            pom_path.display()
        );

        let settings: crate::config::Settings = toml::from_str(&settings_content).unwrap();

        // Save original directory to restore later
        let original_dir = std::env::current_dir().unwrap();

        // Build cache (simulate indexer startup)
        std::env::set_current_dir(&temp_dir).unwrap();
        fs::create_dir_all(temp_dir.path().join(crate::init::local_dir_name())).unwrap();

        let provider = crate::project_resolver::providers::JavaProvider::new();
        provider.rebuild_cache(&settings).unwrap();

        // Create Java file
        let java_file = temp_dir.path().join("src/main/java/org/example/App.java");
        fs::create_dir_all(java_file.parent().unwrap()).unwrap();
        fs::write(&java_file, "package org.example; public class App {}").unwrap();

        // When calling module_path_from_file() (must run while cwd is still temp_dir)
        let behavior = JavaBehavior::new();
        let module_path = behavior.module_path_from_file(&java_file, temp_dir.path());

        // Then it should return the package from JavaProvider
        assert_eq!(
            module_path,
            Some("org.example".to_string()),
            "Should use JavaProvider to determine package path"
        );

        // Restore original directory
        std::env::set_current_dir(&original_dir).unwrap();
    }

    #[test]
    fn test_configure_symbol_sets_visibility() {
        let behavior = JavaBehavior::new();

        // Test public visibility
        let mut symbol = Symbol::new(
            SymbolId(1),
            "publicMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 10,
                start_column: 5,
                end_line: 12,
                end_column: 6,
            },
        );
        symbol.signature = Some("public void publicMethod()".to_string().into());
        symbol.visibility = Visibility::Public; // Set by parser's determine_visibility()
        behavior.configure_symbol(&mut symbol, Some("com.example"));

        assert_eq!(symbol.visibility, Visibility::Public);
        assert_eq!(symbol.module_path, Some("com.example".to_string().into()));

        // Test protected visibility
        let mut symbol2 = Symbol::new(
            SymbolId(2),
            "protectedMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 15,
                start_column: 5,
                end_line: 17,
                end_column: 6,
            },
        );
        symbol2.signature = Some("protected void protectedMethod()".to_string().into());
        symbol2.visibility = Visibility::Module; // Set by parser's determine_visibility()
        behavior.configure_symbol(&mut symbol2, Some("com.example"));

        assert_eq!(symbol2.visibility, Visibility::Module);

        // Test private visibility
        let mut symbol3 = Symbol::new(
            SymbolId(3),
            "privateMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 20,
                start_column: 5,
                end_line: 22,
                end_column: 6,
            },
        );
        symbol3.signature = Some("private void privateMethod()".to_string().into());
        symbol3.visibility = Visibility::Private; // Set by parser's determine_visibility()
        behavior.configure_symbol(&mut symbol3, Some("com.example"));

        assert_eq!(symbol3.visibility, Visibility::Private);

        // Test package-private (no modifier)
        let mut symbol4 = Symbol::new(
            SymbolId(4),
            "packageMethod",
            SymbolKind::Method,
            FileId(1),
            Range {
                start_line: 25,
                start_column: 5,
                end_line: 27,
                end_column: 6,
            },
        );
        symbol4.signature = Some("void packageMethod()".to_string().into());
        symbol4.visibility = Visibility::Crate; // Set by parser's determine_visibility()
        behavior.configure_symbol(&mut symbol4, Some("com.example"));

        assert_eq!(symbol4.visibility, Visibility::Crate); // Package-private
    }

    #[test]
    fn test_import_matches_symbol_trait_impl() {
        let behavior = JavaBehavior::new();

        // Test case from Spring PetClinic:
        // Import: org.springframework.samples.petclinic.model.Person
        // Symbol module_path: org.springframework.samples.petclinic.model
        assert!(
            behavior.import_matches_symbol(
                "org.springframework.samples.petclinic.model.Person",
                "org.springframework.samples.petclinic.model",
                Some("org.springframework.samples.petclinic.owner")
            ),
            "Single-type import should match symbol in imported package"
        );

        // Wildcard import: org.springframework.samples.petclinic.model.*
        assert!(
            behavior.import_matches_symbol(
                "org.springframework.samples.petclinic.model.*",
                "org.springframework.samples.petclinic.model",
                Some("org.springframework.samples.petclinic.owner")
            ),
            "Wildcard import should match symbols in that package"
        );

        // Same package - no import needed
        assert!(
            behavior.import_matches_symbol(
                "org.springframework.samples.petclinic.owner.Owner",
                "org.springframework.samples.petclinic.owner",
                Some("org.springframework.samples.petclinic.owner")
            ),
            "Symbols in same package should match"
        );

        // Should not match different package
        assert!(
            !behavior.import_matches_symbol("com.example.Foo", "com.other.Bar", None),
            "Different packages should not match"
        );
    }

    #[test]
    #[ignore = "Run with: cargo test test_module_path_from_file_with_real_cache -- --ignored"]
    fn test_module_path_from_file_with_real_cache() {
        use std::path::PathBuf;

        // This test uses the actual cache file generated by the indexer
        // Cache should exist at: .codanna-test/index/resolvers/java_resolution.json

        let behavior = JavaBehavior::new();

        // Test with Spring PetClinic Owner.java
        let owner_path = PathBuf::from(
            "test_monorepos/spring-petclinic/src/main/java/org/springframework/samples/petclinic/owner/Owner.java",
        );

        let module_path = behavior.module_path_from_file(&owner_path, Path::new("."));

        // Should return package path, not file path
        assert_eq!(
            module_path,
            Some("org.springframework.samples.petclinic.owner".to_string()),
            "Should extract package path from file path using cache"
        );
    }
}
