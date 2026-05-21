use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::resolution::{InheritanceResolver, ResolutionScope};
use crate::parsing::LanguageBehavior;
use crate::types::FileId;
use crate::Visibility;
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::{NixInheritanceResolver, NixResolutionContext};

#[derive(Clone)]
pub struct NixBehavior {
    state: BehaviorState,
}

impl NixBehavior {
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }
}

impl Default for NixBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for NixBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for NixBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("nix")
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_nix::LANGUAGE.into()
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
            file_path.strip_prefix(project_root).ok()?
        } else {
            file_path
        };

        let path = relative_path.to_str()?;
        let path_clean = path.trim_start_matches("./");
        let module_path = strip_extension(path_clean, extensions);
        let module_path = module_path.replace(['/', '\\'], ".");

        if module_path.is_empty() {
            Some(".".to_string())
        } else {
            Some(module_path)
        }
    }

    fn parse_visibility(&self, _signature: &str) -> Visibility {
        // Nix has no visibility keywords — callers set Public/Private based on context.
        Visibility::Public
    }

    fn supports_traits(&self) -> bool {
        false
    }

    fn supports_inherent_methods(&self) -> bool {
        false
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(NixResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(NixInheritanceResolver::new())
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
        use crate::symbol::ScopeContext;
        use crate::SymbolKind;

        let module_level = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Constant
                | SymbolKind::Variable
                | SymbolKind::Field
        );
        if module_level {
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
        if import_path == symbol_module_path {
            return true;
        }
        let normalized = import_path.replace(['/', '\\'], ".");
        normalized == symbol_module_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_module_separator() {
        assert_eq!(NixBehavior::new().module_separator(), ".");
    }

    #[test]
    fn test_supports_traits() {
        assert!(!NixBehavior::new().supports_traits());
    }

    #[test]
    fn test_module_path_from_file() {
        let behavior = NixBehavior::new();
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let file = root.join("pkgs/hello.nix");
        assert_eq!(
            behavior.module_path_from_file(&file, root, &["nix"]),
            Some("pkgs.hello".to_string())
        );

        let file = root.join("default.nix");
        assert_eq!(
            behavior.module_path_from_file(&file, root, &["nix"]),
            Some("default".to_string())
        );
    }
}
