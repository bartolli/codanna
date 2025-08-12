//! Language parser factory with configuration-based instantiation.
//!
//! Creates LanguageParser instances based on Language enum and Settings.
//! Validates language enablement and provides discovery of supported languages.

use super::{Language, LanguageParser, PhpParser, PythonParser, RustParser};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

/// Parser factory that creates LanguageParser instances based on configuration.
///
/// Validates language support and configuration before instantiation.
/// Currently supports: Rust (full), Python/JS/TS (placeholder).
#[derive(Debug)]
pub struct ParserFactory {
    settings: Arc<Settings>,
}

impl ParserFactory {
    /// Creates factory instance with shared configuration.
    ///
    /// Args: settings - Arc-wrapped Settings for thread-safe access across parsing tasks.
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }

    /// Creates parser instance for the specified language.
    ///
    /// Validates language is enabled in configuration before creation.
    /// Returns ConfigError if language disabled, General error if unimplemented.
    #[must_use = "Parser creation may fail and should be handled"]
    pub fn create_parser(&self, language: Language) -> IndexResult<Box<dyn LanguageParser>> {
        // Validate language enablement before expensive parser creation
        let lang_key = language.config_key();
        if let Some(config) = self.settings.languages.get(lang_key) {
            if !config.enabled {
                return Err(IndexError::ConfigError {
                    reason: format!(
                        "Language {} is disabled in configuration. Enable it in your settings to use.",
                        language.name()
                    ),
                });
            }
        }

        match language {
            Language::Rust => {
                let parser =
                    RustParser::with_debug(self.settings.debug).map_err(IndexError::General)?;
                Ok(Box::new(parser))
            }
            Language::Python => {
                let parser = PythonParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                Ok(Box::new(parser))
            }
            Language::JavaScript => {
                // TODO: Implement JavaScriptParser
                Err(IndexError::General(format!(
                    "{} parser not yet implemented. Currently only Rust is supported.",
                    language.name()
                )))
            }
            Language::TypeScript => {
                // TODO: Implement TypeScriptParser
                Err(IndexError::General(format!(
                    "{} parser not yet implemented. Currently only Rust is supported.",
                    language.name()
                )))
            }
            Language::Php => {
                let parser = PhpParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                Ok(Box::new(parser))
            }
        }
    }

    /// Checks if language is enabled in configuration.
    ///
    /// Returns false if language not found in settings (fail-safe default).
    pub fn is_language_enabled(&self, language: Language) -> bool {
        let lang_key = language.config_key();
        self.settings
            .languages
            .get(lang_key)
            .map(|config| config.enabled)
            .unwrap_or(false)
    }

    /// Returns list of all enabled languages from configuration.
    ///
    /// Filters all supported languages against settings.languages map.
    pub fn enabled_languages(&self) -> Vec<Language> {
        vec![
            Language::Rust,
            Language::Python,
            Language::JavaScript,
            Language::TypeScript,
            Language::Php,
        ]
        .into_iter()
        .filter(|&lang| self.is_language_enabled(lang))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_rust_parser() {
        let settings = Arc::new(Settings::default());
        let factory = ParserFactory::new(settings);

        let parser = factory.create_parser(Language::Rust);
        assert!(parser.is_ok());

        let parser = parser.unwrap();
        assert_eq!(parser.language(), Language::Rust);
    }

    #[test]
    fn test_disabled_language() {
        let mut settings = Settings::default();
        // Disable Rust
        if let Some(rust_config) = settings.languages.get_mut("rust") {
            rust_config.enabled = false;
        }

        let factory = ParserFactory::new(Arc::new(settings));
        let result = factory.create_parser(Language::Rust);

        assert!(result.is_err());
        if let Err(err) = result {
            assert!(
                matches!(err, IndexError::ConfigError { reason } if reason.contains("disabled"))
            );
        }
    }

    #[test]
    fn test_enabled_languages() {
        let settings = Arc::new(Settings::default());
        let factory = ParserFactory::new(settings);

        let enabled = factory.enabled_languages();
        // By default, only Rust is enabled
        assert_eq!(enabled, vec![Language::Rust]);
    }

    #[test]
    fn test_create_python_parser() {
        use crate::config::LanguageConfig;
        use std::collections::HashMap;

        let mut settings = Settings::default();
        let mut languages = HashMap::new();
        languages.insert(
            "python".to_string(),
            LanguageConfig {
                enabled: true,
                extensions: vec!["py".to_string()],
                parser_options: HashMap::new(),
            },
        );
        settings.languages = languages;

        let factory = ParserFactory::new(Arc::new(settings));
        let parser = factory.create_parser(Language::Python);

        assert!(parser.is_ok());
        let parser = parser.unwrap();
        assert_eq!(parser.language(), Language::Python);
    }
}
