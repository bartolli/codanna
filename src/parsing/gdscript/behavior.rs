//! GDScript-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::{InheritanceResolver, ResolutionScope};
use crate::types::compact_string;
use crate::{FileId, Symbol, SymbolKind, Visibility};
use std::path::Path;
use tree_sitter::Language;

/// Language behavior for Godot's GDScript
#[derive(Clone, Default)]
pub struct GdscriptBehavior;

impl GdscriptBehavior {
    /// Create a new behavior instance
    pub fn new() -> Self {
        Self
    }

    /// Extract the first identifier name from a signature string
    fn extract_identifier(signature: &str) -> Option<&str> {
        let trimmed = signature.trim();

        // Split on common separators after keywords
        trimmed
            .split([' ', '(', ':', '=', ',', '\t'])
            .filter(|token| !token.is_empty())
            .find(|token| {
                !matches!(
                    *token,
                    "func"
                        | "static"
                        | "remote"
                        | "master"
                        | "puppet"
                        | "remotesync"
                        | "mastersync"
                        | "puppetsync"
                        | "var"
                        | "const"
                        | "signal"
                        | "class"
                        | "class_name"
                        | "export"
                        | "onready"
                        | "tool"
                )
            })
    }
}

impl LanguageBehavior for GdscriptBehavior {
    fn configure_symbol(&self, symbol: &mut Symbol, module_path: Option<&str>) {
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }

        if let Some(signature) = &symbol.signature {
            symbol.visibility = self.parse_visibility(signature);
        }

        // Adjust module symbol naming to use the last path segment for readability
        if symbol.kind == SymbolKind::Module {
            if let Some(path) = module_path {
                if let Some(name) = path.rsplit('/').next() {
                    let name = name.trim_end_matches(".gd");
                    if !name.is_empty() {
                        symbol.name = compact_string(name);
                    }
                }
            }
        }
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(crate::parsing::gdscript::GdscriptResolutionContext::new(
            file_id,
        ))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(crate::parsing::gdscript::GdscriptInheritanceResolver::new())
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        base_path.to_string()
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        let identifier = Self::extract_identifier(signature).unwrap_or_default();

        // Godot treats leading underscores as script-private by convention.
        if identifier.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn module_separator(&self) -> &'static str {
        "/"
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        let relative = file_path.strip_prefix(project_root).ok()?;
        let mut path = relative.to_string_lossy().replace('\\', "/");

        if path.ends_with(".gd") {
            path.truncate(path.len() - 3);
        }

        let normalized = path.trim_start_matches('/');

        Some(format!("res://{normalized}"))
    }

    fn get_language(&self) -> Language {
        tree_sitter_gdscript::LANGUAGE.into()
    }
}
