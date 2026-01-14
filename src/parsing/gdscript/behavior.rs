//! GDScript-specific language behavior implementation

use crate::parsing::LanguageBehavior;
use crate::parsing::ResolutionScope;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::paths::strip_extension;
use crate::parsing::{Import, InheritanceResolver};
use crate::types::compact_string;
use crate::{FileId, Symbol, SymbolKind, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

/// Language behavior for Godot's GDScript
#[derive(Clone)]
pub struct GdscriptBehavior {
    language: Language,
    state: BehaviorState,
}

impl GdscriptBehavior {
    /// Create a new behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_gdscript::LANGUAGE.into(),
            state: BehaviorState::new(),
        }
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

    /// Resolve GDScript relative paths (./file.gd, ../dir/file.gd)
    /// Works on normalized module paths (res://...), not filesystem paths
    fn resolve_gdscript_relative_import(&self, import_path: &str, from_module: &str) -> String {
        // Count leading ../ segments
        let mut up_levels = 0;
        let mut remaining = import_path;

        // Handle ./file.gd (same directory)
        if let Some(rest) = remaining.strip_prefix("./") {
            remaining = rest;
        }

        // Handle ../file.gd (parent directories)
        while let Some(rest) = remaining.strip_prefix("../") {
            up_levels += 1;
            remaining = rest;
        }

        // Split the current module path (res://scripts/player -> ["scripts", "player"])
        let normalized_from = from_module.strip_prefix("res://").unwrap_or(from_module);
        let mut parts: Vec<_> = normalized_from
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // Remove the filename itself (we're in the directory)
        if !parts.is_empty() {
            parts.pop();
        }

        // Go up the specified number of levels
        for _ in 0..up_levels {
            if !parts.is_empty() {
                parts.pop();
            }
        }

        // Add the remaining path
        if !remaining.is_empty() {
            let remaining = remaining.strip_suffix(".gd").unwrap_or(remaining);
            for part in remaining.split('/') {
                if !part.is_empty() {
                    parts.push(part);
                }
            }
        }

        format!("res://{}", parts.join("/"))
    }
}

impl StatefulBehavior for GdscriptBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl Default for GdscriptBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for GdscriptBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("gdscript")
    }

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

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            Some("res://".to_string())
        } else {
            Some(format!("res://{}", components.join("/")))
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

        let normalized = path_without_ext.trim_start_matches('/');

        Some(format!("res://{normalized}"))
    }

    fn get_language(&self) -> Language {
        self.language.clone()
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

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // 1. Exact match first
        if import_path == symbol_module_path {
            return true;
        }

        // 2. Handle relative imports (./file.gd, ../dir/file.gd)
        if let Some(importing_mod) = importing_module {
            if import_path.starts_with("./") || import_path.starts_with("../") {
                let resolved = self.resolve_gdscript_relative_import(import_path, importing_mod);
                // Compare with normalized symbol path
                let norm_symbol = symbol_module_path
                    .strip_prefix("res://")
                    .unwrap_or(symbol_module_path)
                    .strip_suffix(".gd")
                    .unwrap_or(
                        symbol_module_path
                            .strip_prefix("res://")
                            .unwrap_or(symbol_module_path),
                    );

                let norm_resolved = resolved.strip_prefix("res://").unwrap_or(&resolved);

                if norm_resolved == norm_symbol {
                    return true;
                }
            }
        }

        // 3. Normalize both paths (remove res:// prefix, remove .gd extension)
        let norm_import = import_path
            .strip_prefix("res://")
            .unwrap_or(import_path)
            .strip_suffix(".gd")
            .unwrap_or(import_path.strip_prefix("res://").unwrap_or(import_path));

        let norm_symbol = symbol_module_path
            .strip_prefix("res://")
            .unwrap_or(symbol_module_path)
            .strip_suffix(".gd")
            .unwrap_or(
                symbol_module_path
                    .strip_prefix("res://")
                    .unwrap_or(symbol_module_path),
            );

        // 4. Compare normalized paths
        norm_import == norm_symbol
    }

    fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool {
        use crate::symbol::ScopeContext;

        // GDScript resolves classes, functions, signals, variables
        let resolvable_kind = matches!(
            symbol.kind,
            SymbolKind::Function
                | SymbolKind::Class
                | SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Method
                | SymbolKind::Field // For signals
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

    fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // GDScript uses underscore prefix for private (like Python)
        let name = symbol.name.as_ref();

        // Private symbols not visible outside file
        if name.starts_with('_') {
            return false;
        }

        // Public symbols are visible
        true
    }
}
