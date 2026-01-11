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
    Go,
    C,
    Cpp,
    CSharp,
    Gdscript,
    Java,
    Kotlin,
    Swift,
    Clojure,
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
            Language::Go => super::LanguageId::new("go"),
            Language::C => super::LanguageId::new("c"),
            Language::Cpp => super::LanguageId::new("cpp"),
            Language::CSharp => super::LanguageId::new("csharp"),
            Language::Gdscript => super::LanguageId::new("gdscript"),
            Language::Java => super::LanguageId::new("java"),
            Language::Kotlin => super::LanguageId::new("kotlin"),
            Language::Swift => super::LanguageId::new("swift"),
            Language::Clojure => super::LanguageId::new("clojure"),
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
            "go" => Some(Language::Go),
            "c" => Some(Language::C),
            "cpp" => Some(Language::Cpp),
            "csharp" => Some(Language::CSharp),
            "gdscript" => Some(Language::Gdscript),
            "java" => Some(Language::Java),
            "kotlin" => Some(Language::Kotlin),
            "swift" => Some(Language::Swift),
            "clojure" => Some(Language::Clojure),
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
            "go" | "go.mod" | "go.sum" => Some(Language::Go),
            "c" | "h" => Some(Language::C),
            "cpp" | "hpp" | "cc" | "cxx" | "hxx" => Some(Language::Cpp),
            "cs" | "csx" => Some(Language::CSharp),
            "gd" => Some(Language::Gdscript),
            "java" => Some(Language::Java),
            "kt" | "kts" => Some(Language::Kotlin),
            "swift" => Some(Language::Swift),
            "clj" | "cljc" | "cljs" | "edn" => Some(Language::Clojure),
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
            Language::Go => &["go", "go.mod", "go.sum"],
            Language::C => &["c", "h"],
            Language::Cpp => &["cpp", "hpp", "cc", "cxx", "hxx"],
            Language::CSharp => &["cs", "csx"],
            Language::Gdscript => &["gd"],
            Language::Java => &["java"],
            Language::Kotlin => &["kt", "kts"],
            Language::Swift => &["swift"],
            Language::Clojure => &["clj", "cljc", "cljs", "edn"],
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
            Language::Go => "go",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::CSharp => "csharp",
            Language::Gdscript => "gdscript",
            Language::Java => "java",
            Language::Kotlin => "kotlin",
            Language::Swift => "swift",
            Language::Clojure => "clojure",
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
            Language::Go => "Go",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::CSharp => "C#",
            Language::Gdscript => "GDScript",
            Language::Java => "Java",
            Language::Kotlin => "Kotlin",
            Language::Swift => "Swift",
            Language::Clojure => "Clojure",
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
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("go.mod"), Some(Language::Go));
        assert_eq!(Language::from_extension("go.sum"), Some(Language::Go));
        assert_eq!(Language::from_extension("txt"), None);
        assert_eq!(Language::from_extension("gd"), Some(Language::Gdscript));
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
            Language::from_path(Path::new("main.go")),
            Some(Language::Go)
        );
        assert_eq!(Language::from_path(Path::new("main.c")), Some(Language::C));
        assert_eq!(
            Language::from_path(Path::new("header.h")),
            Some(Language::C)
        );
        assert_eq!(
            Language::from_path(Path::new("main.cpp")),
            Some(Language::Cpp)
        );
        assert_eq!(
            Language::from_path(Path::new("header.hpp")),
            Some(Language::Cpp)
        );
        assert_eq!(
            Language::from_path(Path::new("player.gd")),
            Some(Language::Gdscript)
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
        assert!(Language::Go.extensions().contains(&"go"));
        assert!(Language::Go.extensions().contains(&"go.mod"));
        assert!(Language::Go.extensions().contains(&"go.sum"));
        assert!(Language::Gdscript.extensions().contains(&"gd"));
    }
}
