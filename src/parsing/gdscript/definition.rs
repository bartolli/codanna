//! GDScript language definition for the registry
//!
//! Provides the language metadata and glue code used by the language registry
//! to instantiate parsers and behaviors for Godot's GDScript.

use std::sync::Arc;

use super::{GdscriptBehavior, GdscriptParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexError, IndexResult, Settings};

/// Language definition for GDScript
pub struct GdscriptLanguage;

impl GdscriptLanguage {
    /// Stable identifier used throughout the registry
    pub const ID: LanguageId = LanguageId::new("gdscript");
}

impl LanguageDefinition for GdscriptLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "GDScript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gd"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = GdscriptParser::new().map_err(IndexError::General)?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(GdscriptBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        false // Experimental support; opt-in by default
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register GDScript language with the global registry
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(GdscriptLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_metadata() {
        let lang = GdscriptLanguage;

        assert_eq!(lang.id(), LanguageId::new("gdscript"));
        assert_eq!(lang.name(), "GDScript");
        assert_eq!(lang.extensions(), &["gd"]);
    }

    #[test]
    fn test_default_enabled_flag() {
        let lang = GdscriptLanguage;
        assert!(!lang.default_enabled());

        let settings = Settings::default();
        assert_eq!(lang.is_enabled(&settings), lang.default_enabled());
    }

    #[test]
    fn test_parser_creation() {
        let lang = GdscriptLanguage;
        let settings = Settings::default();
        let parser = lang.create_parser(&settings);
        assert!(parser.is_ok());
    }
}
