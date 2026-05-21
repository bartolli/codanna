use crate::parsing::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, LanguageRegistry,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

use super::{NixBehavior, NixParser};

pub struct NixLanguage;

impl LanguageDefinition for NixLanguage {
    fn id(&self) -> LanguageId {
        LanguageId::new("nix")
    }

    fn name(&self) -> &'static str {
        "Nix"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["nix"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = NixParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(NixBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        false
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(NixLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nix_language_id() {
        assert_eq!(NixLanguage.id(), LanguageId::new("nix"));
    }

    #[test]
    fn test_nix_language_name() {
        assert_eq!(NixLanguage.name(), "Nix");
    }

    #[test]
    fn test_nix_file_extensions() {
        assert_eq!(NixLanguage.extensions(), &["nix"]);
    }

    #[test]
    fn test_nix_disabled_by_default() {
        assert!(!NixLanguage.default_enabled());
    }

    #[test]
    fn test_nix_parser_creation() {
        let settings = Settings::default();
        let result = NixLanguage.create_parser(&settings);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().language(), crate::parsing::Language::Nix);
    }

    #[test]
    fn test_nix_language_registry_registration() {
        use crate::parsing::LanguageRegistry;
        let mut registry = LanguageRegistry::new();
        register(&mut registry);
        assert!(registry.get(LanguageId::new("nix")).is_some());
    }
}
