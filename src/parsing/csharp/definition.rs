//! C# language definition for the registry
//!
//! Provides the C# language implementation that self-registers
//! with the global registry. This module defines how C# parsers
//! and behaviors are created based on settings.

use std::sync::Arc;

use super::{CSharpBehavior, CSharpParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexResult, Settings};

/// C# language definition
pub struct CSharpLanguage;

impl CSharpLanguage {
    /// Language identifier constant
    pub const ID: LanguageId = LanguageId::new("csharp");
}

impl LanguageDefinition for CSharpLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "C#"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["cs", "csx"]
    }

    fn create_parser(&self, settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = CSharpParser::with_debug(settings.debug)
            .map_err(|e| crate::IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(CSharpBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // C# is enabled by default
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register C# language with the global registry
///
/// This function is called from initialize_registry() to add
/// C# support to the system.
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(CSharpLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csharp_definition() {
        let csharp = CSharpLanguage;

        assert_eq!(csharp.id(), LanguageId::new("csharp"));
        assert_eq!(csharp.name(), "C#");
        assert_eq!(csharp.extensions(), &["cs", "csx"]);
    }

    #[test]
    fn test_csharp_enabled_by_default() {
        let csharp = CSharpLanguage;
        let settings = Settings::default();

        // Should be enabled by default
        assert!(csharp.is_enabled(&settings));
    }
}