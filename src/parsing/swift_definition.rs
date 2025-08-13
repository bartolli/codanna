//! Swift language definition for the registry
//!
//! Provides the Swift language implementation that self-registers
//! with the global registry. This module defines how Swift parsers
//! and behaviors are created based on settings.

use std::sync::Arc;

use super::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, SwiftBehavior, SwiftParser,
};
use crate::{IndexResult, Settings};

/// Swift language definition
pub struct SwiftLanguage;

impl SwiftLanguage {
    /// Language identifier constant
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

    fn create_parser(&self, settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = SwiftParser::with_debug(settings.debug).map_err(crate::IndexError::General)?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(SwiftBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // Swift is enabled by default for Apple development
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(true) // Swift is enabled by default
    }
}

/// Register Swift language with the global registry
///
/// This function is called from initialize_registry() to add
/// Swift support to the system.
pub(super) fn register(registry: &mut super::LanguageRegistry) {
    registry.register(Arc::new(SwiftLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swift_definition() {
        let swift = SwiftLanguage;

        assert_eq!(swift.id(), LanguageId::new("swift"));
        assert_eq!(swift.name(), "Swift");
        assert_eq!(swift.extensions(), &["swift"]);
    }

    #[test]
    fn test_swift_enabled_by_default() {
        let swift = SwiftLanguage;
        let settings = Settings::default();

        // Should be enabled by default even if not in settings
        assert!(swift.is_enabled(&settings));
    }
}