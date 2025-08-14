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
        format!("{base_path}.{symbol_name}")
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Check for combined modifiers first, in order of decreasing permissiveness
        if signature.contains("protected internal ") || signature.contains("internal protected ") {
            // C# allows both 'protected internal' and 'internal protected' as valid syntax (though 'protected internal' is the conventional ordering).
            // Both mean accessible from the same assembly or any derived type.
            // This is most similar to 'crate' in Rust, but more permissive than 'protected' alone
            Visibility::Crate
        } else if signature.contains("private protected ") {
            // C# 'private protected' means accessible from the same assembly and only by derived types
            // This is more restrictive than 'protected internal'
            Visibility::Module
        } else if signature.contains("public ") {
            Visibility::Public
        } else if signature.contains("internal ") {
            // Only 'internal', not part of a combined modifier
            Visibility::Crate
        } else if signature.contains("protected ") {
            // Only 'protected', not part of a combined modifier
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
        true // C# supports interface-like behavior via interfaces (analogous to traits)
    }

    fn get_language(&self) -> Language {
        self.language.clone()
    }
}