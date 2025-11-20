//! Java language definition and registration

use crate::parsing::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, LanguageRegistry,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

use super::{JavaBehavior, JavaParser};

/// Java language definition
pub struct JavaLanguage;

impl LanguageDefinition for JavaLanguage {
    fn id(&self) -> LanguageId {
        LanguageId::new("java")
    }

    fn name(&self) -> &'static str {
        "Java"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = JavaParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(JavaBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("java")
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register Java language with the registry
pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(JavaLanguage));
}
