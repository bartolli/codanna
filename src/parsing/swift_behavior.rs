//! Swift language behavior implementation
//!
//! Provides Swift-specific patterns and language behaviors for enhanced analysis.

use super::language_behavior::LanguageBehavior;
use crate::{Visibility};

/// Swift language behavior implementation
#[derive(Debug)]
pub struct SwiftBehavior;

impl SwiftBehavior {
    /// Create a new SwiftBehavior instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for SwiftBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for SwiftBehavior {
    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        // Swift modules use dot notation like: ModuleName.Type.Method
        if base_path.is_empty() {
            symbol_name.to_string()
        } else {
            format!("{}.{}", base_path, symbol_name)
        }
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Parse Swift access modifiers
        if signature.contains("private") {
            Visibility::Private
        } else if signature.contains("fileprivate") {
            Visibility::Module // fileprivate is module-scoped in Swift
        } else if signature.contains("internal") {
            Visibility::Module // internal is the default, module-scoped
        } else if signature.contains("public") {
            Visibility::Public
        } else if signature.contains("open") {
            Visibility::Public // open is public + inheritable
        } else {
            Visibility::Module // Swift default is internal (module-scoped)
        }
    }

    fn module_separator(&self) -> &'static str {
        "." // Swift uses dots for module separation
    }

    fn supports_traits(&self) -> bool {
        true // Swift has protocols
    }

    fn supports_inherent_methods(&self) -> bool {
        true // Swift supports methods on types
    }

    fn get_language(&self) -> tree_sitter::Language {
        tree_sitter_swift::LANGUAGE.into()
    }
}