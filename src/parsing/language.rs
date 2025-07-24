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
}

impl Language {
    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Language::Rust),
            "py" | "pyi" => Some(Language::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "ts" | "tsx" | "mts" | "cts" => Some(Language::TypeScript),
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
        }
    }
    
    /// Get the configuration key for this language
    pub fn config_key(&self) -> &str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
        }
    }
    
    /// Get human-readable name
    pub fn name(&self) -> &str {
        match self {
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
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
        assert_eq!(Language::from_extension("txt"), None);
    }
    
    #[test]
    fn test_language_from_path() {
        assert_eq!(Language::from_path(Path::new("main.rs")), Some(Language::Rust));
        assert_eq!(Language::from_path(Path::new("src/lib.rs")), Some(Language::Rust));
        assert_eq!(Language::from_path(Path::new("script.py")), Some(Language::Python));
        assert_eq!(Language::from_path(Path::new("app.js")), Some(Language::JavaScript));
        assert_eq!(Language::from_path(Path::new("types.d.ts")), Some(Language::TypeScript));
        assert_eq!(Language::from_path(Path::new("README.md")), None);
    }
    
    #[test]
    fn test_extensions() {
        assert!(Language::Rust.extensions().contains(&"rs"));
        assert!(Language::Python.extensions().contains(&"py"));
        assert!(Language::JavaScript.extensions().contains(&"js"));
        assert!(Language::TypeScript.extensions().contains(&"ts"));
    }
}