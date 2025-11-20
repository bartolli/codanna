//! Language registry for dynamic language discovery and management
//!
//! This module provides a registry system that:
//! - Auto-discovers all available language parsers at startup
//! - Integrates with settings.toml for enable/disable control
//! - Provides zero-cost lookups (returning references)
//! - Enables adding new languages without modifying core code
//!
//! # Architecture
//!
//! The registry separates "available" from "enabled":
//! - Available: All languages compiled into the binary
//! - Enabled: Languages activated in settings.toml
//!
//! This allows users to control which languages are active
//! without recompilation, while still maintaining zero-cost
//! abstractions and type safety.

use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use super::{LanguageBehavior, LanguageParser};
use crate::{IndexResult, Settings};

/// Type alias for parser and behavior pair to reduce complexity
pub type ParserBehaviorPair = (Box<dyn LanguageParser>, Box<dyn LanguageBehavior>);

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Type-safe language identifier
///
/// Uses &'static str for zero-cost comparisons and storage.
/// The string must be a compile-time constant (language key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LanguageId(&'static str);

impl LanguageId {
    /// Create a new LanguageId from a static string
    ///
    /// # Safety
    /// The string MUST be a compile-time constant that lives for 'static
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// Get the string identifier
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for LanguageId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Convert to a static string by matching known languages
        // This is necessary because LanguageId requires &'static str
        let static_str = match s.as_str() {
            "rust" => "rust",
            "python" => "python",
            "javascript" => "javascript",
            "typescript" => "typescript",
            "php" => "php",
            "go" => "go",
            "csharp" => "csharp",
            "kotlin" => "kotlin",
            // For unknown languages, we leak the string to get 'static lifetime
            // This is safe because language identifiers are typically created once
            // at startup and live for the entire program
            _ => Box::leak(s.into_boxed_str()),
        };

        Ok(LanguageId(static_str))
    }
}

/// Registry errors with actionable suggestions
#[derive(Error, Debug)]
pub enum RegistryError {
    #[error(
        "Language '{0}' not found in registry\nSuggestion: Check available languages with 'codanna list-languages' or ensure the language module is compiled in"
    )]
    LanguageNotFound(LanguageId),

    #[error(
        "Language '{0}' is available but disabled\nSuggestion: Enable it in .codanna/settings.toml by setting languages.{0}.enabled = true"
    )]
    LanguageDisabled(LanguageId),

    #[error(
        "No language found for extension '.{0}'\nSuggestion: Check if the file type is supported or add a language mapping in settings.toml"
    )]
    ExtensionNotMapped(String),

    #[error(
        "Failed to create parser for language '{language}': {reason}\nSuggestion: Check the language configuration in settings.toml"
    )]
    ParserCreationFailed {
        language: LanguageId,
        reason: String,
    },
}

/// Trait for language modules to implement
///
/// Each language provides a static definition that the registry
/// uses for discovery and instantiation. This trait follows
/// zero-cost principles by returning borrowed static data.
pub trait LanguageDefinition: Send + Sync {
    /// Unique identifier for this language (e.g., "rust", "python")
    /// Must match the key used in settings.toml
    fn id(&self) -> LanguageId;

    /// Human-readable name (e.g., "Rust", "Python")
    fn name(&self) -> &'static str;

    /// File extensions this language handles (e.g., ["rs"] for Rust)
    /// Extensions should NOT include the dot prefix
    fn extensions(&self) -> &'static [&'static str];

    /// Create a parser instance for this language
    /// Takes borrowed Settings to access language-specific configuration
    fn create_parser(&self, settings: &Settings) -> IndexResult<Box<dyn LanguageParser>>;

    /// Create a behavior instance for this language
    /// Behaviors are lightweight and don't need configuration
    fn create_behavior(&self) -> Box<dyn LanguageBehavior>;

    /// Default enabled state for configuration generation
    /// This is used when generating initial configuration files
    fn default_enabled(&self) -> bool {
        false // Most languages disabled by default
    }

    /// Check if this language is enabled in settings
    /// Default implementation checks `settings.languages\[id\].enabled`
    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(false)
    }
}

/// Language registry that manages available and enabled languages
///
/// The registry maintains two views:
/// - All available languages (compiled in)
/// - Currently enabled languages (from settings)
///
/// This separation allows runtime control via settings.toml
pub struct LanguageRegistry {
    /// All available language definitions
    definitions: HashMap<LanguageId, Arc<dyn LanguageDefinition>>,

    /// Extension to language mapping for quick lookup
    /// Built from all available languages, not just enabled ones
    extension_map: HashMap<&'static str, LanguageId>,
}

impl LanguageRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            extension_map: HashMap::new(),
        }
    }

    /// Register a language definition
    ///
    /// This is called during initialization to register all
    /// available languages. Whether they're enabled is determined
    /// by settings.toml at runtime.
    pub fn register(&mut self, definition: Arc<dyn LanguageDefinition>) {
        let id = definition.id();

        // Register the definition
        self.definitions.insert(id, definition.clone());

        // Update extension mappings
        for ext in definition.extensions() {
            self.extension_map.insert(ext, id);
        }
    }

    /// Get a language definition by ID
    ///
    /// Returns None if the language is not available (not compiled in).
    /// Use is_enabled() to check if it's active in settings.
    #[must_use]
    pub fn get(&self, id: LanguageId) -> Option<&dyn LanguageDefinition> {
        self.definitions.get(&id).map(|def| def.as_ref())
    }

    /// Get a language by file extension
    ///
    /// Returns the language definition if a mapping exists.
    /// The language may still be disabled in settings.
    #[must_use]
    pub fn get_by_extension(&self, extension: &str) -> Option<&dyn LanguageDefinition> {
        // Remove leading dot if present
        let ext = extension.strip_prefix('.').unwrap_or(extension);

        self.extension_map.get(ext).and_then(|id| self.get(*id))
    }

    /// Convert a string to LanguageId by looking up registered languages
    ///
    /// This is useful when reading language identifiers from storage
    /// where they are stored as strings. Returns None if the language
    /// is not registered.
    #[must_use]
    pub fn find_language_id(&self, name: &str) -> Option<LanguageId> {
        // Iterate through all registered languages to find a match
        for def in self.definitions.values() {
            if def.id().as_str() == name {
                return Some(def.id());
            }
        }
        None
    }

    /// Iterate over all available languages
    ///
    /// This includes disabled languages. Filter by is_enabled()
    /// to get only active languages.
    pub fn iter_all(&self) -> impl Iterator<Item = &dyn LanguageDefinition> {
        self.definitions.values().map(|def| def.as_ref())
    }

    /// Iterate over enabled languages only
    ///
    /// Filters available languages by checking settings.toml
    pub fn iter_enabled<'a>(
        &'a self,
        settings: &'a Settings,
    ) -> impl Iterator<Item = &'a dyn LanguageDefinition> {
        self.iter_all().filter(move |def| def.is_enabled(settings))
    }

    /// Get all supported extensions from enabled languages
    ///
    /// Returns extensions only from languages enabled in settings
    pub fn enabled_extensions<'a>(
        &'a self,
        settings: &'a Settings,
    ) -> impl Iterator<Item = &'static str> + 'a {
        self.iter_enabled(settings)
            .flat_map(|def| def.extensions().iter().copied())
    }

    /// Check if a language is available (compiled in)
    #[must_use]
    pub fn is_available(&self, id: LanguageId) -> bool {
        self.definitions.contains_key(&id)
    }

    /// Check if a language is enabled in settings
    ///
    /// Returns false if language is not available or disabled
    #[must_use]
    pub fn is_enabled(&self, id: LanguageId, settings: &Settings) -> bool {
        self.get(id)
            .map(|def| def.is_enabled(settings))
            .unwrap_or(false)
    }

    /// Create a parser for a language
    ///
    /// Checks both availability and settings before creation.
    /// Returns appropriate error with suggestions.
    pub fn create_parser(
        &self,
        id: LanguageId,
        settings: &Settings,
    ) -> Result<Box<dyn LanguageParser>, RegistryError> {
        match self.get(id) {
            None => Err(RegistryError::LanguageNotFound(id)),
            Some(def) => {
                if !def.is_enabled(settings) {
                    return Err(RegistryError::LanguageDisabled(id));
                }

                def.create_parser(settings)
                    .map_err(|e| RegistryError::ParserCreationFailed {
                        language: id,
                        reason: e.to_string(),
                    })
            }
        }
    }

    /// Create a parser and behavior pair
    ///
    /// Convenience method for getting both parser and behavior
    pub fn create_parser_with_behavior(
        &self,
        id: LanguageId,
        settings: &Settings,
    ) -> Result<ParserBehaviorPair, RegistryError> {
        match self.get(id) {
            None => Err(RegistryError::LanguageNotFound(id)),
            Some(def) => {
                if !def.is_enabled(settings) {
                    return Err(RegistryError::LanguageDisabled(id));
                }

                let parser = def.create_parser(settings).map_err(|e| {
                    RegistryError::ParserCreationFailed {
                        language: id,
                        reason: e.to_string(),
                    }
                })?;

                let behavior = def.create_behavior();

                Ok((parser, behavior))
            }
        }
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

use std::sync::LazyLock;

/// Global registry instance
///
/// Uses LazyLock for lazy initialization. Languages register
/// themselves during first access.
static REGISTRY: LazyLock<std::sync::Mutex<LanguageRegistry>> = LazyLock::new(|| {
    let mut registry = LanguageRegistry::new();

    // Languages will register themselves here
    // This happens during first access
    initialize_registry(&mut registry);

    std::sync::Mutex::new(registry)
});

/// Initialize the registry with all available languages
///
/// This is called once during first registry access.
/// Each language module adds itself to the registry here.
fn initialize_registry(registry: &mut LanguageRegistry) {
    // Register all available languages
    // Each language module provides a register function
    super::rust::register(registry);
    super::python::register(registry);
    super::php::register(registry);
    super::typescript::register(registry);
    super::go::register(registry);
    super::c::register(registry);
    super::cpp::register(registry);
    super::csharp::register(registry);
    super::gdscript::register(registry);
    super::java::register(registry);
    super::kotlin::register(registry);

    // Future languages will be added here:
    // super::javascript_definition::register(registry);
}

/// Get the global registry
///
/// Provides access to the singleton registry instance
pub fn get_registry() -> &'static std::sync::Mutex<LanguageRegistry> {
    &REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock language for testing
    struct MockLanguage {
        id: LanguageId,
        enabled: bool,
    }

    impl LanguageDefinition for MockLanguage {
        fn id(&self) -> LanguageId {
            self.id
        }

        fn name(&self) -> &'static str {
            "Mock Language"
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["mock", "test"]
        }

        fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
            unimplemented!("Mock parser creation")
        }

        fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
            unimplemented!("Mock behavior creation")
        }

        fn is_enabled(&self, _settings: &Settings) -> bool {
            self.enabled
        }
    }

    #[test]
    fn test_language_id() {
        let id1 = LanguageId::new("rust");
        let id2 = LanguageId::new("rust");
        let id3 = LanguageId::new("python");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.as_str(), "rust");
        assert_eq!(format!("{id1}"), "rust");
    }

    #[test]
    fn test_registry_registration() {
        let mut registry = LanguageRegistry::new();
        let mock = Arc::new(MockLanguage {
            id: LanguageId::new("mock"),
            enabled: true,
        });

        registry.register(mock);

        assert!(registry.is_available(LanguageId::new("mock")));
        assert!(!registry.is_available(LanguageId::new("unknown")));

        // Check extension mapping
        assert!(registry.get_by_extension("mock").is_some());
        assert!(registry.get_by_extension("test").is_some());
        assert!(registry.get_by_extension(".mock").is_some()); // With dot
        assert!(registry.get_by_extension("unknown").is_none());
    }

    #[test]
    fn test_registry_iteration() {
        let mut registry = LanguageRegistry::new();

        registry.register(Arc::new(MockLanguage {
            id: LanguageId::new("enabled"),
            enabled: true,
        }));

        registry.register(Arc::new(MockLanguage {
            id: LanguageId::new("disabled"),
            enabled: false,
        }));

        // All languages
        assert_eq!(registry.iter_all().count(), 2);

        // Only enabled (with mock settings)
        let settings = Settings::default();
        let enabled: Vec<_> = registry.iter_enabled(&settings).collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id(), LanguageId::new("enabled"));
    }

    #[test]
    fn test_global_registry_initialization() {
        // Access the global registry
        let registry = get_registry();
        let registry = registry.lock().unwrap();

        // Should have all three languages registered
        assert!(registry.is_available(LanguageId::new("rust")));
        assert!(registry.is_available(LanguageId::new("python")));
        assert!(registry.is_available(LanguageId::new("php")));
        assert!(registry.is_available(LanguageId::new("go")));

        // Check extension mappings
        assert!(registry.get_by_extension("rs").is_some());
        assert!(registry.get_by_extension("py").is_some());
        assert!(registry.get_by_extension("php").is_some());
        assert!(registry.get_by_extension("go").is_some());

        // Check language names
        let rust = registry.get(LanguageId::new("rust")).unwrap();
        assert_eq!(rust.name(), "Rust");

        let python = registry.get(LanguageId::new("python")).unwrap();
        assert_eq!(python.name(), "Python");

        let php = registry.get(LanguageId::new("php")).unwrap();
        assert_eq!(php.name(), "PHP");

        let go = registry.get(LanguageId::new("go")).unwrap();
        assert_eq!(go.name(), "Go");
    }
}
