//! C-specific language behavior implementation

use crate::FileId;
use crate::Visibility;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::{LanguageBehavior, ResolutionScope};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// C language behavior implementation
#[derive(Clone)]
pub struct CBehavior {
    language: Language,
    state: BehaviorState,
}

impl CBehavior {
    /// Create a new C behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_c::LANGUAGE.into(),
            state: BehaviorState::new(),
        }
    }
}

impl StatefulBehavior for CBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for CBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for CBehavior {
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        format!("{base_path}::{symbol_name}")
    }

    fn parse_visibility(&self, _signature: &str) -> Visibility {
        // C doesn't have visibility modifiers like Rust
        // All symbols are effectively public in C
        Visibility::Public
    }

    fn module_separator(&self) -> &'static str {
        "::"
    }

    fn supports_traits(&self) -> bool {
        false
    }

    fn supports_inherent_methods(&self) -> bool {
        false
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Get relative path from project root
        let relative_path = file_path.strip_prefix(project_root).ok()?;

        // Remove the file extension
        let path_str = relative_path.to_str()?;
        let mut path_without_ext = path_str.to_string();
        
        // Remove extension
        for ext in &["c", "h"] {
            if let Some(stripped) = path_str.strip_suffix(&format!(".{}", ext)) {
                path_without_ext = stripped.to_string();
                break;
            }
        }

        // Convert path separators to module separators
        let module_path = path_without_ext.replace('/', "::");

        // Handle empty paths
        let module_path = if module_path.is_empty() {
            "root".to_string()
        } else {
            module_path
        };

        Some(module_path)
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(crate::parsing::resolution::GenericResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn crate::parsing::InheritanceResolver> {
        Box::new(crate::parsing::resolution::GenericInheritanceResolver::new())
    }

    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // Check scope_context first if available
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember => {
                    matches!(symbol.kind, SymbolKind::Method)
                        || matches!(symbol.visibility, crate::Visibility::Public)
                }
            }
        } else {
            // Fallback to symbol kind for backward compatibility
            matches!(
                symbol.kind,
                SymbolKind::Function
                    | SymbolKind::Struct
                    | SymbolKind::Enum
                    | SymbolKind::Constant
            )
        }
    }

    fn format_method_call(&self, receiver: &str, method: &str) -> String {
        // C uses function calls, not method calls
        format!("{method}({receiver})")
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // C doesn't have inheritance
        "uses"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;
        match language_specific {
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
            "references" => RelationKind::References,
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

    fn is_symbol_visible_from_file(&self, symbol: &crate::Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // Different file: check visibility based on C rules
        // In C, symbols are visible if they are declared in headers
        match symbol.visibility {
            Visibility::Public => true,
            Visibility::Crate => true,
            Visibility::Module => false,
            Visibility::Private => false,
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        // Simple exact match for C
        import_path == symbol_module_path
    }
}