//! JavaScript language definition and registration

use crate::parsing::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, LanguageRegistry,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

use super::{JavaScriptBehavior, JavaScriptParser};

/// JavaScript language definition
pub struct JavaScriptLanguage;

impl LanguageDefinition for JavaScriptLanguage {
    fn id(&self) -> LanguageId {
        LanguageId::new("javascript")
    }

    fn name(&self) -> &'static str {
        "JavaScript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["js", "jsx", "mjs", "cjs"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = JavaScriptParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(JavaScriptBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // Enable JavaScript by default
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("javascript")
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register JavaScript language with the registry
pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(JavaScriptLanguage));
}
