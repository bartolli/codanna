//! Clojure language definition for the registry
//!
//! Provides the Clojure language implementation that self-registers
//! with the global registry.

use std::sync::Arc;

use super::{ClojureBehavior, ClojureParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexError, IndexResult, Settings};

/// Clojure language definition
pub struct ClojureLanguage;

impl ClojureLanguage {
    /// Language identifier constant
    pub const ID: LanguageId = LanguageId::new("clojure");
}

impl LanguageDefinition for ClojureLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "Clojure"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["clj", "cljc", "cljs", "edn"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = ClojureParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(ClojureBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // Clojure is enabled by default
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(true) // Clojure is enabled by default
    }
}

/// Register Clojure language with the global registry
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(ClojureLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{LanguageId, get_registry};

    #[test]
    fn test_clojure_definition() {
        let clojure = ClojureLanguage;

        assert_eq!(clojure.id(), LanguageId::new("clojure"));
        assert_eq!(clojure.name(), "Clojure");
        assert!(clojure.extensions().contains(&"clj"));
        assert!(clojure.extensions().contains(&"cljc"));
        assert!(clojure.extensions().contains(&"cljs"));
        assert!(clojure.extensions().contains(&"edn"));
    }

    #[test]
    fn test_clojure_enabled_by_default() {
        let clojure = ClojureLanguage;
        let settings = Settings::default();

        // Clojure is enabled by default
        assert!(clojure.default_enabled());
        assert!(clojure.is_enabled(&settings));
    }

    #[test]
    fn test_clojure_in_registry() {
        let registry = get_registry();
        let registry = registry.lock().unwrap();
        assert!(registry.is_available(LanguageId::new("clojure")));
    }
}
