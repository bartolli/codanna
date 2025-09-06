//! Language parser factory with configuration-based instantiation.
//!
//! Creates LanguageParser instances based on Language enum and Settings.
//! Validates language enablement and provides discovery of supported languages.

use super::{
    GoBehavior, GoParser, Language, LanguageBehavior, LanguageId, LanguageParser, PhpBehavior,
    PhpParser, PythonBehavior, PythonParser, RustBehavior, RustParser, TypeScriptBehavior,
    TypeScriptParser, get_registry, CParser, CBehavior, CppParser, CppBehavior,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

/// A parser paired with its language-specific behavior
pub struct ParserWithBehavior {
    pub parser: Box<dyn LanguageParser>,
    pub behavior: Box<dyn LanguageBehavior>,
}

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

    // ========== Registry-based methods (new approach) ==========

    /// Creates parser using the registry system
    ///
    /// This method uses the global language registry instead of hardcoded match statements.
    /// It will eventually replace create_parser() once migration is complete.
    #[must_use = "Parser creation may fail and should be handled"]
    pub fn create_parser_from_registry(
        &self,
        language_id: LanguageId,
    ) -> IndexResult<Box<dyn LanguageParser>> {
        let registry = get_registry();
        let registry = registry
            .lock()
            .map_err(|e| IndexError::General(format!("Failed to acquire registry lock: {e}")))?;

        registry
            .create_parser(language_id, &self.settings)
            .map_err(|e| IndexError::General(e.to_string()))
    }

    /// Creates parser with behavior using the registry system
    ///
    /// This method uses the global language registry instead of hardcoded match statements.
    /// It will eventually replace create_parser_with_behavior() once migration is complete.
    pub fn create_parser_with_behavior_from_registry(
        &self,
        language_id: LanguageId,
    ) -> IndexResult<ParserWithBehavior> {
        let registry = get_registry();
        let registry = registry
            .lock()
            .map_err(|e| IndexError::General(format!("Failed to acquire registry lock: {e}")))?;

        let (parser, behavior) = registry
            .create_parser_with_behavior(language_id, &self.settings)
            .map_err(|e| IndexError::General(e.to_string()))?;

        Ok(ParserWithBehavior { parser, behavior })
    }

    /// Checks if a language is enabled using the registry
    pub fn is_language_enabled_in_registry(&self, language_id: LanguageId) -> bool {
        let registry = get_registry();
        if let Ok(registry) = registry.lock() {
            registry.is_enabled(language_id, &self.settings)
        } else {
            false
        }
    }

    /// Get language by file extension using the registry
    pub fn get_language_for_extension(&self, extension: &str) -> Option<LanguageId> {
        let registry = get_registry();
        if let Ok(registry) = registry.lock() {
            registry
                .get_by_extension(extension)
                .filter(|def| def.is_enabled(&self.settings))
                .map(|def| def.id())
        } else {
            None
        }
    }

    // ========== Legacy methods (will be removed after migration) ==========

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
            Language::Go => {
                let parser = GoParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                Ok(Box::new(parser))
            }
            Language::C => {
                let parser = CParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                Ok(Box::new(parser))
            }
            Language::Cpp => {
                let parser = CppParser::new().map_err(|e| IndexError::General(e.to_string()))?;
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

    /// Creates a parser with its corresponding language behavior.
    ///
    /// This method pairs each parser with its language-specific behavior,
    /// enabling the removal of hardcoded language logic from indexing.
    pub fn create_parser_with_behavior(
        &self,
        language: Language,
    ) -> IndexResult<ParserWithBehavior> {
        // Validate language enablement
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

        // Create parser and behavior pair
        let result = match language {
            Language::Rust => {
                let parser =
                    RustParser::with_debug(self.settings.debug).map_err(IndexError::General)?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(RustBehavior::new()),
                }
            }
            Language::Python => {
                let parser = PythonParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(PythonBehavior::new()),
                }
            }
            Language::Php => {
                let parser = PhpParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(PhpBehavior::new()),
                }
            }
            Language::TypeScript => {
                let parser =
                    TypeScriptParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(TypeScriptBehavior::new()),
                }
            }
            Language::JavaScript => {
                return Err(IndexError::General(format!(
                    "{} parser not yet implemented.",
                    language.name()
                )));
            }
            Language::Go => {
                let parser = GoParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(GoBehavior::new()),
                }
            }
            Language::C => {
                let parser = CParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(CBehavior::new()),
                }
            }
            Language::Cpp => {
                let parser = CppParser::new().map_err(|e| IndexError::General(e.to_string()))?;
                ParserWithBehavior {
                    parser: Box::new(parser),
                    behavior: Box::new(CppBehavior::new()),
                }
            }
        };

        Ok(result)
    }

    /// Create just the behavior for a language from registry
    pub fn create_behavior_from_registry(
        &self,
        language_id: LanguageId,
    ) -> Box<dyn LanguageBehavior> {
        let registry = get_registry();
        let registry = registry.lock().unwrap();

        if let Some(definition) = registry.get(language_id) {
            definition.create_behavior()
        } else {
            // Fallback to a default behavior if language not found
            // This shouldn't happen in practice
            Box::new(RustBehavior::new())
        }
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
    fn test_registry_based_parser_creation() {
        let mut settings = Settings::default();
        // Enable all languages for testing
        for (_, config) in settings.languages.iter_mut() {
            config.enabled = true;
        }
        let settings = Arc::new(settings);
        let factory = ParserFactory::new(settings.clone());

        // Test creating parser from registry
        let rust_id = LanguageId::new("rust");
        let parser = factory.create_parser_from_registry(rust_id);
        assert!(parser.is_ok(), "Should create Rust parser from registry");

        // Test creating parser with behavior from registry
        let python_id = LanguageId::new("python");
        let parser_with_behavior = factory.create_parser_with_behavior_from_registry(python_id);

        // Test creating parser with behavior for Go
        let go_id = LanguageId::new("go");

        if let Err(e) = &parser_with_behavior {
            eprintln!("Failed to create Python parser with behavior: {e}");
        }
        assert!(
            parser_with_behavior.is_ok(),
            "Should create Python parser with behavior from registry"
        );

        // Test checking if language is enabled
        assert!(factory.is_language_enabled_in_registry(rust_id));
        assert!(factory.is_language_enabled_in_registry(python_id));

        // Test getting language by extension
        assert_eq!(factory.get_language_for_extension("rs"), Some(rust_id));
        assert_eq!(factory.get_language_for_extension("py"), Some(python_id));
        assert_eq!(
            factory.get_language_for_extension("php"),
            Some(LanguageId::new("php"))
        );
        assert_eq!(factory.get_language_for_extension("go"), Some(go_id));
        assert_eq!(factory.get_language_for_extension("unknown"), None);
    }

    #[test]
    fn test_language_to_language_id_conversion() {
        // Test the transitional conversion method
        assert_eq!(Language::Rust.to_language_id(), LanguageId::new("rust"));
        assert_eq!(Language::Python.to_language_id(), LanguageId::new("python"));
        assert_eq!(Language::Php.to_language_id(), LanguageId::new("php"));
        assert_eq!(
            Language::JavaScript.to_language_id(),
            LanguageId::new("javascript")
        );
        assert_eq!(
            Language::TypeScript.to_language_id(),
            LanguageId::new("typescript")
        );
    }

    #[test]
    fn test_registry_legacy_parity() {
        let settings = Arc::new(Settings::default());
        let factory = ParserFactory::new(settings.clone());

        // Verify that both methods produce the same results for Rust
        let language = Language::Rust;
        let language_id = language.to_language_id();

        // Both should report enabled
        assert_eq!(
            factory.is_language_enabled(language),
            factory.is_language_enabled_in_registry(language_id),
            "Registry and legacy should agree on enabled status"
        );

        // Both should successfully create parsers
        let legacy_result = factory.create_parser(language);
        let registry_result = factory.create_parser_from_registry(language_id);

        assert!(legacy_result.is_ok());
        assert!(registry_result.is_ok());
    }

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
    fn test_create_parser_with_behavior() {
        use crate::config::LanguageConfig;
        use std::collections::HashMap;

        // Create settings with all languages enabled
        let mut settings = Settings::default();
        let mut languages = HashMap::new();

        // Enable Rust (already enabled by default, but be explicit)
        languages.insert(
            "rust".to_string(),
            LanguageConfig {
                enabled: true,
                extensions: vec!["rs".to_string()],
                parser_options: HashMap::new(),
            },
        );

        // Enable Python
        languages.insert(
            "python".to_string(),
            LanguageConfig {
                enabled: true,
                extensions: vec!["py".to_string()],
                parser_options: HashMap::new(),
            },
        );

        // Enable PHP
        languages.insert(
            "php".to_string(),
            LanguageConfig {
                enabled: true,
                extensions: vec!["php".to_string()],
                parser_options: HashMap::new(),
            },
        );

        settings.languages = languages;
        let factory = ParserFactory::new(Arc::new(settings));

        // Test Rust
        let result = factory.create_parser_with_behavior(Language::Rust);
        assert!(result.is_ok());
        let rust_pair = result.unwrap();
        assert_eq!(rust_pair.parser.language(), Language::Rust);
        assert_eq!(rust_pair.behavior.module_separator(), "::");

        // Test Python
        let result = factory.create_parser_with_behavior(Language::Python);
        assert!(result.is_ok());
        let python_pair = result.unwrap();
        assert_eq!(python_pair.parser.language(), Language::Python);
        assert_eq!(python_pair.behavior.module_separator(), ".");

        // Test PHP
        let result = factory.create_parser_with_behavior(Language::Php);
        assert!(result.is_ok());
        let php_pair = result.unwrap();
        assert_eq!(php_pair.parser.language(), Language::Php);
        assert_eq!(php_pair.behavior.module_separator(), "\\");
    }

    #[test]
    fn test_create_parser_with_behavior_disabled_language() {
        let mut settings = Settings::default();
        // Disable Rust
        if let Some(rust_config) = settings.languages.get_mut("rust") {
            rust_config.enabled = false;
        }

        let factory = ParserFactory::new(Arc::new(settings));
        let result = factory.create_parser_with_behavior(Language::Rust);

        assert!(result.is_err());
        if let Err(err) = result {
            assert!(
                matches!(err, IndexError::ConfigError { reason } if reason.contains("disabled"))
            );
        }
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

        // Dynamically check that we have enabled languages from the registry
        // This test won't break when we add new languages
        assert!(
            !enabled.is_empty(),
            "Should have at least one enabled language"
        );

        // Verify core languages are still enabled (these should always be present)
        assert!(
            enabled.contains(&Language::Rust),
            "Rust should be enabled by default"
        );
        assert!(
            enabled.contains(&Language::Python),
            "Python should be enabled by default"
        );

        // Just verify we can get the count - don't hardcode it
        println!("Currently {} languages enabled by default", enabled.len());
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
