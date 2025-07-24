//! Parser factory for creating language-specific parsers
//! 
//! This module provides a factory for creating parsers based on
//! language detection and configuration settings.

use std::sync::Arc;
use crate::Settings;
use super::{Language, LanguageParser, RustParser};

/// Factory for creating language parsers based on configuration
pub struct ParserFactory {
    settings: Arc<Settings>,
}

impl ParserFactory {
    /// Create a new parser factory with the given settings
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }
    
    /// Create a parser for the specified language
    pub fn create_parser(&self, language: Language) -> Result<Box<dyn LanguageParser>, String> {
        // Check if language is enabled in settings
        let lang_key = language.config_key();
        if let Some(config) = self.settings.languages.get(lang_key) {
            if !config.enabled {
                return Err(format!("Language {} is disabled in configuration", language.name()));
            }
        }
        
        match language {
            Language::Rust => {
                let parser = RustParser::new()?;
                Ok(Box::new(parser))
            }
            Language::Python => {
                // TODO: Implement PythonParser
                Err("Python parser not yet implemented".to_string())
            }
            Language::JavaScript => {
                // TODO: Implement JavaScriptParser
                Err("JavaScript parser not yet implemented".to_string())
            }
            Language::TypeScript => {
                // TODO: Implement TypeScriptParser
                Err("TypeScript parser not yet implemented".to_string())
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
        if let Err(err_msg) = result {
            assert!(err_msg.contains("disabled"));
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