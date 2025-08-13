//! Language detection and enumeration
//!
//! This module provides language detection from file extensions
//! and language-specific configuration.

use serde::{Deserialize, Serialize};

/// Supported programming languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Php,
    CSharp,
}

impl Language {
    /// Convert to LanguageId for registry usage
    ///
    /// This is a transitional method that will be removed when
    /// we fully migrate to the registry system.
    pub fn to_language_id(&self) -> super::LanguageId {
        // We need to use static strings for LanguageId
        match self {
            Language::Rust => super::LanguageId::new("rust"),
            Language::Python => super::LanguageId::new("python"),
            Language::JavaScript => super::LanguageId::new("javascript"),
            Language::TypeScript => super::LanguageId::new("typescript"),
            Language::Php => super::LanguageId::new("php"),
            Language::CSharp => super::LanguageId::new("csharp"),
        }
    }

    /// Create Language from LanguageId (for backward compatibility)
    ///
    /// Returns None if the LanguageId doesn't correspond to a known Language variant.
    /// This is a transitional method for migration.
    pub fn from_language_id(id: super::LanguageId) -> Option<Self> {
        match id.as_str() {
            "rust" => Some(Language::Rust),
            "python" => Some(Language::Python),
            "javascript" => Some(Language::JavaScript),
            "typescript" => Some(Language::TypeScript),
            "php" => Some(Language::Php),
            _ => None,
        }
    }

    /// Detect language from file extension
    ///
    /// This now uses the registry internally for consistency.
    /// Will be deprecated once all code migrates to registry.
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext_lower = ext.to_lowercase();

        // Try the registry first for registered languages
        let registry = super::get_registry();
        if let Ok(registry) = registry.lock() {
            if let Some(def) = registry.get_by_extension(&ext_lower) {
                return Self::from_language_id(def.id());
            }
        }

        // Fallback to hardcoded for languages not yet in registry
        // (JavaScript and TypeScript don't have definitions yet)
        match ext_lower.as_str() {
            "rs" => Some(Language::Rust),
            "py" | "pyi" => Some(Language::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
            "php" | "php3" | "php4" | "php5" | "php7" | "php8" | "phps" | "phtml" => {
                Some(Language::Php)
            }
            "cs" | "csx" => Some(Language::CSharp),
            _ => None,
        }
    }

    /// Detect language from file path
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    /// Get default file extensions for this language
    pub fn extensions(&self) -> &[&str] {
        match self {
            Language::Rust => &["rs"],
            Language::Python => &["py", "pyi"],
            Language::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Language::TypeScript => &["ts", "tsx", "mts", "cts"],
            Language::Php => &[
                "php", "php3", "php4", "php5", "php7", "php8", "phps", "phtml",
            ],
            Language::CSharp => &["cs", "csx"],
        }
    }

    /// Get the configuration key for this language
    pub fn config_key(&self) -> &str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Php => "php",
            Language::CSharp => "csharp",
        }
    }

    /// Get human-readable name
    pub fn name(&self) -> &str {
        match self {
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Php => "PHP",
            Language::CSharp => "C#",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_language_from_extension() {
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("RS"), Some(Language::Rust));
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("pyi"), Some(Language::Python));
        assert_eq!(Language::from_extension("js"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("jsx"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("php"), Some(Language::Php));
        assert_eq!(Language::from_extension("PHP"), Some(Language::Php));
        assert_eq!(Language::from_extension("php5"), Some(Language::Php));
        assert_eq!(Language::from_extension("phtml"), Some(Language::Php));
        assert_eq!(Language::from_extension("cs"), Some(Language::CSharp));
        assert_eq!(Language::from_extension("CS"), Some(Language::CSharp));
        assert_eq!(Language::from_extension("csx"), Some(Language::CSharp));
        assert_eq!(Language::from_extension("txt"), None);
    }

    #[test]
    fn test_language_from_path() {
        assert_eq!(
            Language::from_path(Path::new("main.rs")),
            Some(Language::Rust)
        );
        assert_eq!(
            Language::from_path(Path::new("src/lib.rs")),
            Some(Language::Rust)
        );
        assert_eq!(
            Language::from_path(Path::new("script.py")),
            Some(Language::Python)
        );
        assert_eq!(
            Language::from_path(Path::new("app.js")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            Language::from_path(Path::new("types.d.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_path(Path::new("index.php")),
            Some(Language::Php)
        );
        assert_eq!(
            Language::from_path(Path::new("src/class.php5")),
            Some(Language::Php)
        );
        assert_eq!(
            Language::from_path(Path::new("Program.cs")),
            Some(Language::CSharp)
        );
        assert_eq!(
            Language::from_path(Path::new("src/Main.cs")),
            Some(Language::CSharp)
        );
        assert_eq!(Language::from_path(Path::new("README.md")), None);
    }

    #[test]
    fn test_extensions() {
        assert!(Language::Rust.extensions().contains(&"rs"));
        assert!(Language::Python.extensions().contains(&"py"));
        assert!(Language::JavaScript.extensions().contains(&"js"));
        assert!(Language::TypeScript.extensions().contains(&"ts"));
        assert!(Language::Php.extensions().contains(&"php"));
        assert!(Language::Php.extensions().contains(&"php5"));
        assert!(Language::Php.extensions().contains(&"phtml"));
        assert!(Language::CSharp.extensions().contains(&"cs"));
        assert!(Language::CSharp.extensions().contains(&"csx"));
    }
}
