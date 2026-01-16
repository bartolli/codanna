//! PHP-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::{FileId, Visibility};
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

    fn module_path_from_file(
        &self,
        file_path: &Path,
        project_root: &Path,
        extensions: &[&str],
    ) -> Option<String> {
        use crate::parsing::paths::strip_extension;
        use crate::project_resolver::persist::{ResolutionIndex, ResolutionPersistence};
        use std::cell::RefCell;
        use std::time::{Duration, Instant};

        // Thread-local cache for resolution rules (1-second TTL)
        thread_local! {
            static RULES_CACHE: RefCell<Option<(Instant, ResolutionIndex)>> = const { RefCell::new(None) };
        }

        // Try to resolve using PSR-4 rules from composer.json
        let cached_result = RULES_CACHE.with(|cache| {
            let mut cache_ref = cache.borrow_mut();

            // Reload if >1 second old
            let needs_reload = cache_ref
                .as_ref()
                .map(|(ts, _)| ts.elapsed() >= Duration::from_secs(1))
                .unwrap_or(true);

            if needs_reload {
                let persistence = ResolutionPersistence::new(Path::new(".codanna"));
                if let Ok(index) = persistence.load("php") {
                    *cache_ref = Some((Instant::now(), index));
                }
            }

            // Use rules to compute namespace
            if let Some((_, ref index)) = *cache_ref {
                if let Ok(canon_file) = file_path.canonicalize() {
                    if let Some(config_path) = index.get_config_for_file(&canon_file) {
                        if let Some(rules) = index.rules.get(config_path) {
                            // Sort paths by length (longest first) to match most specific path
                            // This ensures src/Illuminate/Macroable/ matches before src/Illuminate/
                            let mut sorted_paths: Vec<_> = rules.paths.iter().collect();
                            sorted_paths.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

                            // Try each source root to find the matching namespace prefix
                            for (source_root_str, namespace_prefixes) in sorted_paths {
                                let source_root = Path::new(source_root_str);
                                let canon_root = source_root
                                    .canonicalize()
                                    .unwrap_or_else(|_| source_root.to_path_buf());

                                if let Ok(relative) = canon_file.strip_prefix(&canon_root) {
                                    // Remove .php extension
                                    let relative_str = relative.to_string_lossy();
                                    let without_ext = relative_str
                                        .strip_suffix(".php")
                                        .or_else(|| relative_str.strip_suffix(".class.php"))
                                        .unwrap_or(&relative_str);

                                    // Convert path separators to namespace separators
                                    let namespace_suffix = without_ext.replace('/', "\\");

                                    // Get namespace prefix (first element in prefixes array)
                                    let namespace_prefix = namespace_prefixes
                                        .first()
                                        .map(|s| s.as_str())
                                        .unwrap_or("");

                                    // Combine prefix + suffix
                                    let prefix_trimmed = namespace_prefix.trim_end_matches('\\');
                                    if namespace_suffix.is_empty() {
                                        if prefix_trimmed.is_empty() {
                                            return None;
                                        }
                                        return Some(format!("\\{prefix_trimmed}"));
                                    } else if prefix_trimmed.is_empty() {
                                        return Some(format!("\\{namespace_suffix}"));
                                    } else {
                                        return Some(format!(
                                            "\\{prefix_trimmed}\\{namespace_suffix}"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            None
        });

        if cached_result.is_some() {
            return cached_result;
        }

        // Fallback: directory-based path resolution (original behavior)
        let relative_path = file_path.strip_prefix(project_root).ok()?;
        let path_str = relative_path.to_str()?;

        // Remove common PHP source directories if present (PSR-4 style)
        let path_without_src = path_str
            .strip_prefix("src/")
            .or_else(|| path_str.strip_prefix("app/"))
            .or_else(|| path_str.strip_prefix("lib/"))
            .or_else(|| path_str.strip_prefix("classes/"))
            .unwrap_or(path_str);

        // Remove extension using passed extensions from settings.toml
        let path_without_ext = strip_extension(path_without_src, extensions);

        // Skip special files that aren't typically namespaced
        if path_without_ext == "index"
            || path_without_ext == "config"
            || path_without_ext.starts_with(".")
        {
            return None;
        }

        // Convert path separators to PHP namespace separators
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
        let extensions = &["class.php", "php"];

        // Test PSR-4 style namespace
        let class_path = Path::new("/project/src/App/Controllers/UserController.php");
        assert_eq!(
            behavior.module_path_from_file(class_path, root, extensions),
            Some("\\App\\Controllers\\UserController".to_string())
        );

        // Test without src directory
        let no_src_path = Path::new("/project/Models/User.php");
        assert_eq!(
            behavior.module_path_from_file(no_src_path, root, extensions),
            Some("\\Models\\User".to_string())
        );

        // Test nested namespace
        let nested_path = Path::new("/project/src/App/Http/Middleware/Auth.php");
        assert_eq!(
            behavior.module_path_from_file(nested_path, root, extensions),
            Some("\\App\\Http\\Middleware\\Auth".to_string())
        );

        // Test index.php (should return None)
        let index_path = Path::new("/project/index.php");
        assert_eq!(
            behavior.module_path_from_file(index_path, root, extensions),
            None
        );

        // Test config.php (should return None)
        let config_path = Path::new("/project/config.php");
        assert_eq!(
            behavior.module_path_from_file(config_path, root, extensions),
            None
        );

        // Test class.php extension
        let class_ext_path = Path::new("/project/src/MyClass.class.php");
        assert_eq!(
            behavior.module_path_from_file(class_ext_path, root, extensions),
            Some("\\MyClass".to_string())
        );

        // Test app directory
        let app_path = Path::new("/project/app/Services/PaymentService.php");
        assert_eq!(
            behavior.module_path_from_file(app_path, root, extensions),
            Some("\\Services\\PaymentService".to_string())
        );
    }
}
