//! Svelte language definition and registration

use crate::parsing::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, LanguageRegistry,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

use super::{SvelteBehavior, SvelteParser};

pub struct SvelteLanguage;

impl LanguageDefinition for SvelteLanguage {
    fn id(&self) -> LanguageId {
        LanguageId::new("svelte")
    }

    fn name(&self) -> &'static str {
        "Svelte"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["svelte"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = SvelteParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(SvelteBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("svelte")
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(SvelteLanguage));
}
