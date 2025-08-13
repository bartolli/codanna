//! C#-specific language behavior implementation

use crate::Visibility;
use crate::parsing::language_behavior::LanguageBehavior;
use tree_sitter::Language;

/// C# language behavior implementation
#[derive(Clone)]
pub struct CSharpBehavior {
    language: Language,
}

impl CSharpBehavior {
    /// Create a new C# behavior instance
    pub fn new() -> Self {
        Self {
            language: tree_sitter_c_sharp::LANGUAGE.into(),
        }
    }
}

impl Default for CSharpBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for CSharpBehavior {
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        format!("{}.{}", base_path, symbol_name)
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        if signature.contains("public ") {
            Visibility::Public
        } else if signature.contains("internal ") {
            Visibility::Crate
        } else if signature.contains("protected ") {
            Visibility::Module
        } else if signature.contains("private ") {
            Visibility::Private
        } else {
            // Default to private if no modifier specified
            Visibility::Private
        }
    }

    fn module_separator(&self) -> &'static str {
        "."
    }

    fn supports_traits(&self) -> bool {
        false // C# uses interfaces, not traits
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }
}