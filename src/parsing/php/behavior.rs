//! PHP-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::storage::DocumentIndex;
use crate::{FileId, SymbolId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// PHP language behavior implementation
#[derive(Clone)]
pub struct PhpBehavior {
    language: Language,
    state: BehaviorState,
}

impl PhpBehavior {
    /// Create a new PHP behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_php::LANGUAGE_PHP.into(),
            state: BehaviorState::new(),
        }
    }
}

impl StatefulBehavior for PhpBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for PhpBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for PhpBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("php")
    }

    fn create_resolution_context(
        &self,
        file_id: FileId,
    ) -> Box<dyn crate::parsing::ResolutionScope> {
        Box::new(crate::parsing::php::PhpResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn crate::parsing::InheritanceResolver> {
        Box::new(crate::parsing::php::PhpInheritanceResolver::new())
    }
    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // PHP typically uses file paths as module paths, not including the symbol name
        // PHP parsers should set more specific paths for methods in the parser itself
        base_path.to_string()
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // PHP has explicit visibility modifiers
        if signature.contains("private ") {
            Visibility::Private
        } else if signature.contains("protected ") {
            Visibility::Module // Protected in PHP = Module visibility
        } else if signature.contains("public ") {
            Visibility::Public
        } else {
            // PHP defaults to public if no modifier specified
            Visibility::Public
        }
    }

    fn module_separator(&self) -> &'static str {
        "\\" // PHP namespace separator
    }

    fn supports_traits(&self) -> bool {
        true // PHP has traits
    }

    fn supports_inherent_methods(&self) -> bool {
        false // PHP methods are always in classes/traits
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            None
        } else {
            // PHP uses backslash for namespaces with leading backslash
            Some(format!("\\{}", components.join("\\")))
        }
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Get relative path from project root
        let relative_path = file_path.strip_prefix(project_root).ok()?;

        // Convert path to string
        let path_str = relative_path.to_str()?;

        // Remove common PHP source directories if present (PSR-4 style)
        let path_without_src = path_str
            .strip_prefix("src/")
            .or_else(|| path_str.strip_prefix("app/"))
            .or_else(|| path_str.strip_prefix("lib/"))
            .or_else(|| path_str.strip_prefix("classes/"))
            .unwrap_or(path_str);

        // Remove the .php extension (check .class.php first since it's longer)
        let path_without_ext = path_without_src
            .strip_suffix(".class.php")
            .or_else(|| path_without_src.strip_suffix(".php"))
            .or_else(|| path_without_src.strip_suffix(".inc"))
            .unwrap_or(path_without_src);

        // Skip special files that aren't typically namespaced
        if path_without_ext == "index"
            || path_without_ext == "config"
            || path_without_ext.starts_with(".")
        {
            return None;
        }

        // Convert path separators to PHP namespace separators
        // PHP uses backslash for namespaces
        let namespace_path = path_without_ext.replace('/', "\\");

        // Add leading backslash for fully qualified namespace
        if namespace_path.is_empty() {
            None
        } else {
            Some(format!("\\{namespace_path}"))
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

    // ========== Resolution API Required Methods ==========

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
            return true;
        }

        // 2. Normalize by removing leading backslash for comparison
        let import_normalized = import_path.trim_start_matches('\\');
        let symbol_normalized = symbol_module_path.trim_start_matches('\\');

        // Check if normalized versions match (handles Symfony\Component vs \Symfony\Component)
        if import_normalized == symbol_normalized {
            return true;
        }

        // 3. Handle relative namespace resolution when we have context
        if let Some(importing_ns) = importing_module {
            let importing_normalized = importing_ns.trim_start_matches('\\');

            // Check if it's a relative namespace (no leading \)
            if !import_path.starts_with('\\') {
                // For namespaced imports (contains \), try relative resolution
                if import_path.contains('\\') {
                    // Try as sibling namespace
                    // e.g., from App\Controllers, "Services\AuthService" -> "App\Services\AuthService"
                    if let Some(parent_ns) = importing_normalized.rsplit_once('\\') {
                        let candidate = format!("{}\\{}", parent_ns.0, import_normalized);
                        if candidate == symbol_normalized {
                            return true;
                        }
                    }

                    // Try as child of current namespace
                    let candidate = format!("{importing_normalized}\\{import_normalized}");
                    if candidate == symbol_normalized {
                        return true;
                    }
                } else {
                    // Single name import - only match in same namespace
                    // e.g., "User" should match "App\Models\User" only when in App\Models
                    if let Some((symbol_ns, symbol_name)) = symbol_normalized.rsplit_once('\\') {
                        if symbol_ns == importing_normalized && symbol_name == import_normalized {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    fn resolve_import_path_with_context(
        &self,
        import_path: &str,
        importing_module: Option<&str>,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Split the path using PHP's namespace separator
        let separator = self.module_separator();
        let segments: Vec<&str> = import_path.split(separator).collect();

        if segments.is_empty() {
            return None;
        }

        // The symbol name is the last segment
        let symbol_name = segments.last()?;

        // Find symbols with this name (using index for performance)
        let candidates = document_index
            .find_symbols_by_name(symbol_name, None)
            .ok()?;

        // Find the one with matching module path using PHP-specific rules
        for candidate in &candidates {
            if let Some(module_path) = &candidate.module_path {
                if self.import_matches_symbol(import_path, module_path.as_ref(), importing_module) {
                    return Some(candidate.id);
                }
            }
        }

        None
    }

    // PHP-specific: Handle PHP use statements and namespace imports
    fn resolve_import(
        &self,
        import: &crate::parsing::Import,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // PHP imports can be:
        // 1. Use statements: use App\Controllers\UserController;
        // 2. Aliased use: use App\Models\User as UserModel;
        // 3. Function/const imports: use function array_map;
        // 4. Grouped imports: use App\{Model, Controller};

        // Get the importing module path for context
        let importing_module = self.get_module_path_for_file(import.file_id);

        // Use enhanced resolution with module context
        // This will use our import_matches_symbol method for PHP-specific matching
        self.resolve_import_path_with_context(
            &import.path,
            importing_module.as_deref(),
            document_index,
        )
    }

    // PHP-specific: Check visibility based on access modifiers
    fn is_symbol_visible_from_file(&self, symbol: &crate::Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // PHP visibility is explicit:
        // - public: Visible everywhere
        // - protected: Visible in same class hierarchy
        // - private: Only visible in same class

        // For cross-file visibility, we only expose public symbols
        matches!(symbol.visibility, Visibility::Public)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_module_path() {
        let behavior = PhpBehavior::new();
        assert_eq!(
            behavior.format_module_path("App\\Controllers", "UserController"),
            "App\\Controllers"
        );
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = PhpBehavior::new();

        // Explicit visibility
        assert_eq!(
            behavior.parse_visibility("public function foo()"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("private function bar()"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("protected function baz()"),
            Visibility::Module
        );

        // Default visibility (public in PHP)
        assert_eq!(
            behavior.parse_visibility("function legacy()"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("static function helper()"),
            Visibility::Public
        );
    }

    #[test]
    fn test_module_separator() {
        let behavior = PhpBehavior::new();
        assert_eq!(behavior.module_separator(), "\\");
    }

    #[test]
    fn test_supports_features() {
        let behavior = PhpBehavior::new();
        assert!(behavior.supports_traits()); // PHP has traits
        assert!(!behavior.supports_inherent_methods());
    }

    #[test]
    fn test_validate_node_kinds() {
        let behavior = PhpBehavior::new();

        // Valid PHP node kinds
        assert!(behavior.validate_node_kind("function_definition"));
        assert!(behavior.validate_node_kind("class_declaration"));
        assert!(behavior.validate_node_kind("method_declaration"));

        // Invalid node kind
        assert!(!behavior.validate_node_kind("struct_item")); // Rust-specific
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = PhpBehavior::new();
        let root = Path::new("/project");

        // Test PSR-4 style namespace
        let class_path = Path::new("/project/src/App/Controllers/UserController.php");
        assert_eq!(
            behavior.module_path_from_file(class_path, root),
            Some("\\App\\Controllers\\UserController".to_string())
        );

        // Test without src directory
        let no_src_path = Path::new("/project/Models/User.php");
        assert_eq!(
            behavior.module_path_from_file(no_src_path, root),
            Some("\\Models\\User".to_string())
        );

        // Test nested namespace
        let nested_path = Path::new("/project/src/App/Http/Middleware/Auth.php");
        assert_eq!(
            behavior.module_path_from_file(nested_path, root),
            Some("\\App\\Http\\Middleware\\Auth".to_string())
        );

        // Test index.php (should return None)
        let index_path = Path::new("/project/index.php");
        assert_eq!(behavior.module_path_from_file(index_path, root), None);

        // Test config.php (should return None)
        let config_path = Path::new("/project/config.php");
        assert_eq!(behavior.module_path_from_file(config_path, root), None);

        // Test class.php extension
        let class_ext_path = Path::new("/project/src/MyClass.class.php");
        assert_eq!(
            behavior.module_path_from_file(class_ext_path, root),
            Some("\\MyClass".to_string())
        );

        // Test app directory
        let app_path = Path::new("/project/app/Services/PaymentService.php");
        assert_eq!(
            behavior.module_path_from_file(app_path, root),
            Some("\\Services\\PaymentService".to_string())
        );
    }
}
