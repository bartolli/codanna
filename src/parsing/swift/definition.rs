//! Swift language definition for the registry
//!
//! Provides the language metadata and glue code used by the language registry
//! to instantiate parsers and behaviors for Swift.

use std::sync::Arc;

use super::{SwiftBehavior, SwiftParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexError, IndexResult, Settings};

/// Language definition for Swift
pub struct SwiftLanguage;

impl SwiftLanguage {
    /// Stable identifier used throughout the registry
    pub const ID: LanguageId = LanguageId::new("swift");
}

impl LanguageDefinition for SwiftLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "Swift"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["swift"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = SwiftParser::new().map_err(IndexError::General)?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(SwiftBehavior::new())
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

/// Register Swift language with the global registry
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(SwiftLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_metadata() {
        let lang = SwiftLanguage;

        assert_eq!(lang.id(), LanguageId::new("swift"));
        assert_eq!(lang.name(), "Swift");
        assert_eq!(lang.extensions(), &["swift"]);
    }

    #[test]
    fn test_default_enabled_flag() {
        let lang = SwiftLanguage;
        assert!(lang.default_enabled());

        let settings = Settings::default();
        assert_eq!(lang.is_enabled(&settings), lang.default_enabled());
    }

    #[test]
    fn test_parser_creation() {
        let lang = SwiftLanguage;
        let settings = Settings::default();
        let parser = lang.create_parser(&settings);
        assert!(parser.is_ok());
    }
}
