//! Parser factory for creating language-specific parsers
//! 
//! This module provides a factory for creating parsers based on
//! language detection and configuration settings.

use std::sync::Arc;
use crate::{Settings, IndexError, IndexResult};
use super::{Language, LanguageParser, RustParser};

/// Factory for creating language parsers based on configuration
#[derive(Debug)]
pub struct ParserFactory {
    settings: Arc<Settings>,
}

impl ParserFactory {
    /// Create a new parser factory with the given settings
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }
    
    /// Create a parser for the specified language
    #[must_use = "Parser creation may fail and should be handled"]
    pub fn create_parser(&self, language: Language) -> IndexResult<Box<dyn LanguageParser>> {
        // Check if language is enabled in settings
        let lang_key = language.config_key();
        if let Some(config) = self.settings.languages.get(lang_key) {
            if !config.enabled {
                return Err(IndexError::ConfigError {
                    reason: format!("Language {} is disabled in configuration. Enable it in your settings to use.", language.name()),
                });
            }
        }
        
        match language {
            Language::Rust => {
                let parser = RustParser::new()
                    .map_err(|e| IndexError::General(e))?;
                Ok(Box::new(parser))
            }
            Language::Python => {
                // TODO: Implement PythonParser
                Err(IndexError::General(format!(
                    "{} parser not yet implemented. Currently only Rust is supported.", 
                    language.name()
                )))
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
        }
    }
    
    /// Check if a language is enabled
    pub fn is_language_enabled(&self, language: Language) -> bool {
        let lang_key = language.config_key();
        self.settings.languages
            .get(lang_key)
            .map(|config| config.enabled)
            .unwrap_or(false)
    }
    
    /// Get all enabled languages
    pub fn enabled_languages(&self) -> Vec<Language> {
        vec![Language::Rust, Language::Python, Language::JavaScript, Language::TypeScript]
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
            assert!(matches!(err, IndexError::ConfigError { reason } if reason.contains("disabled")));
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
}