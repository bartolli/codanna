//! Parser context for tracking scope during AST traversal
//!
//! This module provides scope tracking utilities that all language parsers
//! can use to communicate proper scope information to resolvers.

use crate::symbol::ScopeContext;
use crate::types::SymbolKind;

/// Scope types that parsers track during AST traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    /// Global scope (project-wide)
    Global,
    /// Module/file scope
    Module,
    /// Function or method scope
    Function {
        /// For JS/TS: whether this function's declarations are hoisted
        hoisting: bool,
    },
    /// Class, struct, or similar type scope
    Class,
    /// Block scope (if/for/while/etc)
    Block,
    /// Package or namespace scope
    Package,
    /// Namespace scope (for languages that support it)
    Namespace,
}

impl ScopeType {
    /// Create a non-hoisting function scope (default for most languages)
    pub fn function() -> Self {
        ScopeType::Function { hoisting: false }
    }

    /// Create a hoisting function scope (for JavaScript/TypeScript)
    pub fn hoisting_function() -> Self {
        ScopeType::Function { hoisting: true }
    }
}

/// Parser context for tracking current scope during parsing
#[derive(Debug, Clone)]
pub struct ParserContext {
    /// Stack of current scopes (innermost last)
    scope_stack: Vec<ScopeType>,
    /// Current class name (if inside a class)
    current_class: Option<String>,
    /// Current function name (if inside a function)
    current_function: Option<String>,
}

impl Default for ParserContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserContext {
    /// Create a new parser context starting at module scope
    pub fn new() -> Self {
        Self {
            scope_stack: vec![ScopeType::Module],
            current_class: None,
            current_function: None,
        }
    }

    /// Enter a new scope
    pub fn enter_scope(&mut self, scope_type: ScopeType) {
        // Update tracking based on scope type
        match scope_type {
            ScopeType::Class => {
                // Class name should be set separately via set_current_class
            }
            ScopeType::Function { .. } => {
                // Function name should be set separately via set_current_function
            }
            _ => {}
        }
        self.scope_stack.push(scope_type);
    }

    /// Exit the current scope
    pub fn exit_scope(&mut self) {
        if self.scope_stack.len() > 1 {
            // Never pop the module scope
            let exited = self.scope_stack.pop();

            // Clear context when exiting certain scopes
            if let Some(scope) = exited {
                match scope {
                    ScopeType::Class => self.current_class = None,
                    ScopeType::Function { .. } => self.current_function = None,
                    _ => {}
                }
            }
        }
    }

    /// Get the current scope context for symbol creation
    pub fn current_scope_context(&self) -> ScopeContext {
        // Find the most relevant scope
        for scope in self.scope_stack.iter().rev() {
            match scope {
                ScopeType::Function { hoisting } => {
                    // Determine parent info
                    let (parent_name, parent_kind) = if let Some(func_name) = &self.current_function
                    {
                        (Some(func_name.clone().into()), Some(SymbolKind::Function))
                    } else if let Some(class_name) = &self.current_class {
                        (Some(class_name.clone().into()), Some(SymbolKind::Class))
                    } else {
                        (None, None)
                    };

                    return ScopeContext::Local {
                        hoisted: *hoisting,
                        parent_name,
                        parent_kind,
                    };
                }
                ScopeType::Block => {
                    // Block scope is still local
                    // Determine parent info
                    let (parent_name, parent_kind) = if let Some(func_name) = &self.current_function
                    {
                        (Some(func_name.clone().into()), Some(SymbolKind::Function))
                    } else if let Some(class_name) = &self.current_class {
                        (Some(class_name.clone().into()), Some(SymbolKind::Class))
                    } else {
                        (None, None)
                    };

                    return ScopeContext::Local {
                        hoisted: false,
                        parent_name,
                        parent_kind,
                    };
                }
                ScopeType::Class => {
                    return ScopeContext::ClassMember {
                        class_name: None, // Parsers can populate this if they track class names
                    };
                }
                ScopeType::Package | ScopeType::Namespace => {
                    return ScopeContext::Package;
                }
                ScopeType::Global => {
                    return ScopeContext::Global;
                }
                ScopeType::Module => {
                    // Keep looking for more specific scope
                    continue;
                }
            }
        }

        // Default to module scope
        ScopeContext::Module
    }

    /// Check if currently inside a class
    pub fn is_in_class(&self) -> bool {
        self.scope_stack
            .iter()
            .any(|s| matches!(s, ScopeType::Class))
    }

    /// Check if currently inside a function
    pub fn is_in_function(&self) -> bool {
        self.scope_stack
            .iter()
            .any(|s| matches!(s, ScopeType::Function { .. }))
    }

    /// Check if at module level (not inside class or function)
    pub fn is_module_level(&self) -> bool {
        !self.is_in_class() && !self.is_in_function()
    }

    /// Set the current class name
    pub fn set_current_class(&mut self, name: Option<String>) {
        self.current_class = name;
    }

    /// Set the current function name
    pub fn set_current_function(&mut self, name: Option<String>) {
        self.current_function = name;
    }

    /// Get the current class name
    pub fn current_class(&self) -> Option<&str> {
        self.current_class.as_deref()
    }

    /// Get the current function name
    pub fn current_function(&self) -> Option<&str> {
        self.current_function.as_deref()
    }

    /// Create a scope context for a parameter
    pub fn parameter_scope_context() -> ScopeContext {
        ScopeContext::Parameter
    }

    /// Create a scope context for a global symbol
    pub fn global_scope_context() -> ScopeContext {
        ScopeContext::Global
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_context() {
        let ctx = ParserContext::new();
        assert_eq!(ctx.current_scope_context(), ScopeContext::Module);
        assert!(ctx.is_module_level());
        assert!(!ctx.is_in_class());
        assert!(!ctx.is_in_function());
    }

    #[test]
    fn test_class_scope() {
        let mut ctx = ParserContext::new();
        ctx.enter_scope(ScopeType::Class);
        ctx.set_current_class(Some("MyClass".to_string()));

        assert!(matches!(
            ctx.current_scope_context(),
            ScopeContext::ClassMember { .. }
        ));
        assert!(ctx.is_in_class());
        assert!(!ctx.is_module_level());
        assert_eq!(ctx.current_class(), Some("MyClass"));

        ctx.exit_scope();
        assert_eq!(ctx.current_scope_context(), ScopeContext::Module);
        assert!(ctx.is_module_level());
        assert_eq!(ctx.current_class(), None);
    }

    #[test]
    fn test_function_scope() {
        let mut ctx = ParserContext::new();
        ctx.enter_scope(ScopeType::Function { hoisting: false });
        ctx.set_current_function(Some("my_func".to_string()));

        assert_eq!(
            ctx.current_scope_context(),
            ScopeContext::Local {
                hoisted: false,
                parent_name: Some("my_func".to_string().into()),
                parent_kind: Some(SymbolKind::Function),
            }
        );
        assert!(ctx.is_in_function());
        assert!(!ctx.is_module_level());

        ctx.exit_scope();
        assert_eq!(ctx.current_scope_context(), ScopeContext::Module);
    }

    #[test]
    fn test_nested_scopes() {
        let mut ctx = ParserContext::new();

        // Enter class
        ctx.enter_scope(ScopeType::Class);
        assert!(matches!(
            ctx.current_scope_context(),
            ScopeContext::ClassMember { .. }
        ));

        // Enter method within class
        ctx.enter_scope(ScopeType::Function { hoisting: false });
        // Since we didn't set current_function, parent info will be None
        assert_eq!(
            ctx.current_scope_context(),
            ScopeContext::Local {
                hoisted: false,
                parent_name: None,
                parent_kind: None,
            }
        );
        assert!(ctx.is_in_class());
        assert!(ctx.is_in_function());

        // Exit method
        ctx.exit_scope();
        assert!(matches!(
            ctx.current_scope_context(),
            ScopeContext::ClassMember { .. }
        ));

        // Exit class
        ctx.exit_scope();
        assert_eq!(ctx.current_scope_context(), ScopeContext::Module);
    }

    #[test]
    fn test_hoisted_function() {
        let mut ctx = ParserContext::new();
        ctx.enter_scope(ScopeType::Function { hoisting: true });

        assert_eq!(
            ctx.current_scope_context(),
            ScopeContext::Local {
                hoisted: true,
                parent_name: None,
                parent_kind: None,
            }
        );
    }
}
