//! Rust language definition for the registry
//!
//! Provides the Rust language implementation that self-registers
//! with the global registry. This module defines how Rust parsers
//! and behaviors are created based on settings.

use std::sync::Arc;

use super::{RustBehavior, RustParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexResult, Settings};

/// Rust language definition
pub struct RustLanguage;

impl RustLanguage {
    /// Language identifier constant
    pub const ID: LanguageId = LanguageId::new("rust");
}

impl LanguageDefinition for RustLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "Rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = RustParser::new().map_err(crate::IndexError::General)?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(RustBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // Rust is enabled by default as the primary language
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(true) // Rust is enabled by default
    }
}

/// Register Rust language with the global registry
///
/// This function is called from initialize_registry() to add
/// Rust support to the system.
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(RustLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_definition() {
        let rust = RustLanguage;

        assert_eq!(rust.id(), LanguageId::new("rust"));
        assert_eq!(rust.name(), "Rust");
        assert_eq!(rust.extensions(), &["rs"]);
    }

    #[test]
    fn test_rust_enabled_by_default() {
        let rust = RustLanguage;
        let settings = Settings::default();

        // Should be enabled by default even if not in settings
        assert!(rust.is_enabled(&settings));
    }
}
