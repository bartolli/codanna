//! C language definition for the registry
//!
//! Provides the C language implementation that self-registers
//! with the global registry. This module defines how C parsers
//! and behaviors are created based on settings.

use std::sync::Arc;

use super::{CBehavior, CParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexResult, Settings};

/// C language definition
pub struct CLanguage;

impl CLanguage {
    /// Language identifier constant
    pub const ID: LanguageId = LanguageId::new("c");
}

impl LanguageDefinition for CLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "C"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["c", "h"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = CParser::new().map_err(crate::IndexError::General)?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(CBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // C is enabled by default
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(true) // C is enabled by default
    }
}

/// Register C language with the global registry
///
/// This function is called from initialize_registry() to add
/// C support to the system.
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(CLanguage));
}