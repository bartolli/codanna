//! Lua language definition and registration
//!
//! This module defines the Lua language support for Codanna, providing
//! tree-sitter-based parsing and symbol extraction for Lua codebases.
//!
//! ## AST Node Types and Symbol Mappings
//!
//! The Lua parser uses tree-sitter-lua and handles the following
//! primary node types and their corresponding symbol classifications:
//!
//! ### Function Declarations
//! - **Global functions** (`function_declaration`) -> `SymbolKind::Function`
//! - **Local functions** (`local_function_declaration`) -> `SymbolKind::Function` (Private)
//! - **Methods** (colon syntax) -> `SymbolKind::Method`
//!
//! ### Variable Declarations
//! - **Local variables** (`variable_declaration`) -> `SymbolKind::Variable` (Private)
//! - **Global assignments** (`assignment_statement`) -> `SymbolKind::Variable`
//!
//! ### Table Constructs
//! - **Table constructors** (`table_constructor`) -> `SymbolKind::Class` (when pattern detected)
//! - **Table fields** (`field`) -> `SymbolKind::Field`
//!
//! ## Lua-Specific Language Features
//!
//! The Lua parser handles unique Lua constructs including:
//! - Module patterns (returning tables from files)
//! - Metatable-based OOP patterns
//! - Visibility via `local` keyword and underscore convention
//! - require() for module imports

use crate::parsing::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, LanguageRegistry,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

use super::{LuaBehavior, LuaParser};

/// Lua language definition
///
/// Provides factory methods for creating Lua parsers and behaviors,
/// and defines language metadata like file extensions and identification.
pub struct LuaLanguage;

impl LanguageDefinition for LuaLanguage {
    fn id(&self) -> LanguageId {
        LanguageId::new("lua")
    }

    fn name(&self) -> &'static str {
        "Lua"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["lua"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = LuaParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(LuaBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register Lua language with the registry
pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(LuaLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lua_language_id() {
        let lua_lang = LuaLanguage;
        assert_eq!(lua_lang.id(), LanguageId::new("lua"));
    }

    #[test]
    fn test_lua_language_name() {
        let lua_lang = LuaLanguage;
        assert_eq!(lua_lang.name(), "Lua");
    }

    #[test]
    fn test_lua_file_extensions() {
        let lua_lang = LuaLanguage;
        assert_eq!(lua_lang.extensions(), &["lua"]);
    }

    #[test]
    fn test_lua_enabled_by_default() {
        let lua_lang = LuaLanguage;
        assert!(lua_lang.default_enabled());
    }

    #[test]
    fn test_lua_enabled_with_default_settings() {
        let lua_lang = LuaLanguage;
        let settings = Settings::default();
        assert!(lua_lang.is_enabled(&settings));
    }

    #[test]
    fn test_lua_parser_creation() {
        let lua_lang = LuaLanguage;
        let settings = Settings::default();

        let parser_result = lua_lang.create_parser(&settings);
        assert!(parser_result.is_ok(), "Lua parser creation should succeed");

        let parser = parser_result.unwrap();
        assert_eq!(parser.language(), crate::parsing::Language::Lua);
    }

    #[test]
    fn test_lua_behavior_creation() {
        let lua_lang = LuaLanguage;
        let behavior = lua_lang.create_behavior();

        assert_eq!(behavior.module_separator(), ".");
        assert!(behavior.supports_inherent_methods());
        assert!(!behavior.supports_traits());
    }

    #[test]
    fn test_lua_language_registry_registration() {
        use crate::parsing::LanguageRegistry;

        let mut registry = LanguageRegistry::new();
        register(&mut registry);

        let lua_id = LanguageId::new("lua");
        assert!(registry.get(lua_id).is_some());
    }

    #[test]
    fn test_lua_file_extension_recognition() {
        use crate::parsing::LanguageRegistry;

        let mut registry = LanguageRegistry::new();
        register(&mut registry);

        let detected = registry.get_by_extension("lua");
        assert!(detected.is_some());
        assert_eq!(detected.unwrap().id(), LanguageId::new("lua"));
    }
}
