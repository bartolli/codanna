//! Svelte language behavior

use super::resolution::{SvelteInheritanceResolver, SvelteResolutionContext};
use crate::parsing::{InheritanceResolver, LanguageBehavior, ResolutionScope};
use crate::{FileId, Visibility};
use tree_sitter::Language;

pub struct SvelteBehavior;

impl SvelteBehavior {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SvelteBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for SvelteBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("svelte")
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        base_path.to_string()
    }

    fn module_separator(&self) -> &'static str {
        "."
    }

    fn source_roots(&self) -> &'static [&'static str] {
        &["src", "lib", "routes", "components", "pages"]
    }

    fn format_path_as_module(&self, components: &[&str]) -> Option<String> {
        if components.is_empty() {
            None
        } else {
            Some(components.join("."))
        }
    }

    fn get_language(&self) -> Language {
        tree_sitter_svelte_next::LANGUAGE.into()
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        if signature.contains("export ") {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(SvelteResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(SvelteInheritanceResolver::new())
    }
}
