//! Lua-specific language behavior implementation

use crate::Visibility;
use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::resolution::{InheritanceResolver, ResolutionScope};
use crate::types::FileId;
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::{LuaInheritanceResolver, LuaResolutionContext};

/// Lua language behavior implementation
#[derive(Clone)]
pub struct LuaBehavior {
    state: BehaviorState,
}

impl LuaBehavior {
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }
}

impl Default for LuaBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for LuaBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for LuaBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("lua")
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_lua::LANGUAGE.into()
    }

    fn module_separator(&self) -> &'static str {
        "."
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            Some(".".to_string())
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
        use crate::parsing::paths::strip_extension;

        let relative_path = if file_path.is_absolute() {
            // For absolute paths, must be within project_root
            file_path.strip_prefix(project_root).ok()?
        } else {
            // For relative paths, use as-is
            file_path
        };

        let path = relative_path.to_str()?;
        let path_clean = path.trim_start_matches("./");
        let module_path = strip_extension(path_clean, extensions);

        // Convert path separators to dots (Lua module convention)
        let module_path = module_path.replace(['/', '\\'], ".");

        if module_path.is_empty() {
            Some(".".to_string())
        } else {
            Some(module_path)
        }
    }

    /// Parse visibility from Lua symbol
    ///
    /// Lua visibility is determined by:
    /// - `local` keyword -> Private
    /// - Underscore prefix convention -> Private
    /// - Otherwise -> Public
    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Check for local keyword
        if signature.starts_with("local ") {
            return Visibility::Private;
        }

        // Extract the actual symbol name and check underscore prefix
        let name = if signature.starts_with("function ") {
            // For "function M.process()" or "function Foo:bar()", extract the last identifier
            let after_function = signature.trim_start_matches("function ");
            // First, get everything before the parameters
            let before_params = after_function.split('(').next().unwrap_or("");
            // Then get the last part after . or :
            before_params
                .split(['.', ':'])
                .next_back()
                .unwrap_or("")
                .trim()
        } else {
            // For assignments like "M.field = value", get the last identifier
            let before_equals = signature.split('=').next().unwrap_or("");
            before_equals
                .split(['.', ' ']).rfind(|s| !s.is_empty())
                .unwrap_or("")
                .trim()
        };

        if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn supports_traits(&self) -> bool {
        false
    }

    fn supports_inherent_methods(&self) -> bool {
        true
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(LuaResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(LuaInheritanceResolver::new())
    }

    fn inheritance_relation_name(&self) -> &'static str {
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

    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.register_file_with_state(path, file_id, module_path);
    }

    fn add_import(&self, import: crate::parsing::Import) {
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<crate::parsing::Import> {
        self.get_imports_from_state(file_id)
    }

    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        let module_level_symbol = matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Class | SymbolKind::Constant | SymbolKind::Variable
        );

        if module_level_symbol {
            return true;
        }

        if matches!(symbol.kind, SymbolKind::Method) {
            return true;
        }

        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember { .. } => {
                    matches!(symbol.visibility, Visibility::Public)
                }
            }
        } else {
            false
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        if let Some(path) = module_path {
            symbol.module_path = Some(path.to_string().into());
        }

        if let Some(ref sig) = symbol.signature {
            symbol.visibility = self.parse_visibility(sig);
        }

        if symbol.module_path.is_none() {
            symbol.module_path = Some(".".to_string().into());
        }
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        // Direct match
        if import_path == symbol_module_path {
            return true;
        }

        // Convert require path to module path format
        // require("foo.bar") should match module path "foo.bar"
        let normalized_import = import_path.replace(['/', '\\'], ".");
        if normalized_import == symbol_module_path {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Visibility;
    use std::path::Path;

    #[test]
    fn test_module_separator() {
        let behavior = LuaBehavior::new();
        assert_eq!(behavior.module_separator(), ".");
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = LuaBehavior::new();
        let project_root = Path::new("/home/user/project");
        let extensions = &["lua"];

        let file_path = Path::new("/home/user/project/lib/utils.lua");
        assert_eq!(
            behavior.module_path_from_file(file_path, project_root, extensions),
            Some("lib.utils".to_string())
        );

        let file_path = Path::new("/home/user/project/main.lua");
        assert_eq!(
            behavior.module_path_from_file(file_path, project_root, extensions),
            Some("main".to_string())
        );
    }

    #[test]
    fn test_parse_visibility() {
        let behavior = LuaBehavior::new();

        assert_eq!(
            behavior.parse_visibility("function publicFunc()"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("local function privateFunc()"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("function _internal()"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("local counter = 0"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("GLOBAL_CONST = 100"),
            Visibility::Public
        );
        // Module function patterns
        assert_eq!(
            behavior.parse_visibility("function M.process()"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("function M._internal()"),
            Visibility::Private
        );
        assert_eq!(
            behavior.parse_visibility("function MyClass:method()"),
            Visibility::Public
        );
        assert_eq!(
            behavior.parse_visibility("function MyClass:_privateMethod()"),
            Visibility::Private
        );
        // Field assignments
        assert_eq!(behavior.parse_visibility("M.VERSION"), Visibility::Public);
        assert_eq!(behavior.parse_visibility("M._config"), Visibility::Private);
    }

    #[test]
    fn test_supports_traits() {
        let behavior = LuaBehavior::new();
        assert!(!behavior.supports_traits());
    }

    #[test]
    fn test_supports_inherent_methods() {
        let behavior = LuaBehavior::new();
        assert!(behavior.supports_inherent_methods());
    }

    #[test]
    fn test_import_matches_symbol() {
        let behavior = LuaBehavior::new();

        assert!(behavior.import_matches_symbol("mymodule.utils", "mymodule.utils", None));
        assert!(behavior.import_matches_symbol("mymodule/utils", "mymodule.utils", None));
        assert!(!behavior.import_matches_symbol("mymodule.utils", "other.module", None));
    }
}
