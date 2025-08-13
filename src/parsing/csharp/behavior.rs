//! C# language-specific behavior implementation

use super::LanguageBehavior;

/// C# language-specific behavior implementation
pub struct CSharpBehavior;

impl CSharpBehavior {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageBehavior for CSharpBehavior {
    fn module_separator(&self) -> &str {
        "."
    }

    fn is_extension_method(&self, _name: &str) -> bool {
        // C# extension methods are identified by having `this` as first parameter
        // This would need proper parsing context to determine accurately
        false
    }

    fn is_inherent_method(&self, _name: &str) -> bool {
        // In C#, all methods defined directly in a class are inherent
        // Would need more context to determine accurately
        true
    }

    fn supports_extension_methods(&self) -> bool {
        true
    }

    fn supports_traits(&self) -> bool {
        // C# has interfaces, not traits
        false
    }

    fn supports_interfaces(&self) -> bool {
        true
    }

    fn supports_namespaces(&self) -> bool {
        true
    }
}