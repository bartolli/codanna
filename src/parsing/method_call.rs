//! Method call representation and resolution
//!
//! This module provides:
//! - `MethodCall` - Structured representation of method calls with receiver information
//! - `MethodCallResolver` - Per-file storage for method calls and variable types
//!
//! # Architecture
//!
//! ```text
//! Parser
//!   ├─ find_variable_types() ──┐
//!   └─ find_method_calls() ────┼─→ MethodCallResolver (per file)
//!                              │      - variable_types: HashMap<String, String>
//!                              │      - method_calls: Vec<MethodCall>
//!                              │
//!                              ↓
//!                       LanguageBehavior::resolve_method_call()
//! ```
//!
//! # Separation of Concerns
//!
//! - `MethodCallResolver` - owns DATA (variable types, method calls)
//! - `LanguageBehavior` - owns LOGIC (language-specific resolution)
//! - Indexer - orchestrates WHEN to call what

use crate::Range;
use std::collections::HashMap;

/// Represents a method call with rich receiver information
///
/// This struct captures the full context of a method call, including:
/// - The calling context (which function contains this call)
/// - The method being called
/// - The receiver (if any) and whether it's a static call
/// - The source location
///
/// # Examples
///
/// ```rust
/// use codanna::parsing::MethodCall;
/// use codanna::Range;
///
/// let range = Range::new(1, 0, 1, 10);
///
/// // Instance method: vec.push(item)
/// let call = MethodCall::new("process_items", "push", range)
///     .with_receiver("vec");
///
/// // Static method: String::new()
/// let call = MethodCall::new("create_string", "new", range)
///     .with_receiver("String")
///     .static_method();
///
/// // Self method: self.validate()
/// let call = MethodCall::new("save", "validate", range)
///     .with_receiver("self");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct MethodCall {
    /// The function/method making the call
    ///
    /// This is the name of the function where this method call appears.
    /// For example, in a function `fn process()` that calls `vec.push()`,
    /// this would be `"process"`.
    pub caller: String,

    /// The method being called
    ///
    /// Just the method name without any qualification.
    /// For `String::new()`, this would be `"new"`.
    pub method_name: String,

    /// The receiver expression (e.g., "self", "vec", "String")
    ///
    /// - `None` for plain function calls
    /// - `Some("self")` for self method calls
    /// - `Some("vec")` for instance method calls
    /// - `Some("String")` for static method calls (when `is_static` is true)
    pub receiver: Option<String>,

    /// Whether this is a static method call (e.g., String::new)
    ///
    /// Used to distinguish between:
    /// - `string.len()` (instance method, is_static = false)
    /// - `String::new()` (static method, is_static = true)
    pub is_static: bool,

    /// Location of the call in the source file
    pub range: Range,
}

impl MethodCall {
    /// Creates a new method call with minimal information
    ///
    /// Use the builder methods to add receiver and type information.
    ///
    /// # Arguments
    ///
    /// * `caller` - The function containing this method call
    /// * `method_name` - The name of the method being called
    /// * `range` - The source location of the call
    pub fn new(caller: &str, method_name: &str, range: Range) -> Self {
        Self {
            caller: caller.to_string(),
            method_name: method_name.to_string(),
            receiver: None,
            is_static: false,
            range,
        }
    }

    /// Sets the receiver for this method call
    ///
    /// # Arguments
    ///
    /// * `receiver` - The receiver expression (e.g., "self", "vec", "String")
    pub fn with_receiver(mut self, receiver: &str) -> Self {
        self.receiver = Some(receiver.to_string());
        self
    }

    /// Marks this as a static method call
    ///
    /// Should be used when the receiver is a type name rather than an instance.
    pub fn static_method(mut self) -> Self {
        self.is_static = true;
        self
    }

    /// Checks if this is a self method call
    #[inline]
    pub fn is_self_call(&self) -> bool {
        self.receiver.as_deref() == Some("self")
    }

    /// Checks if this is a plain function call (no receiver)
    #[inline]
    pub fn is_function_call(&self) -> bool {
        self.receiver.is_none()
    }

    /// Gets the fully qualified method name for display
    ///
    /// # Returns
    ///
    /// - `"Type::method"` for static calls
    /// - `"receiver.method"` for instance calls
    /// - `"method"` for plain function calls
    #[must_use = "The formatted name should be used"]
    pub fn qualified_name(&self) -> String {
        match (&self.receiver, self.is_static) {
            (Some(receiver), true) => format!("{receiver}::{}", self.method_name),
            (Some(receiver), false) => format!("{receiver}.{}", self.method_name),
            (None, _) => self.method_name.clone(),
        }
    }
}

/// Owns method call resolution data for a single file
///
/// Collects variable types and method calls during parsing,
/// provides them for resolution in Pass 2.
///
/// # Example
///
/// ```rust
/// use codanna::parsing::{MethodCall, MethodCallResolver};
/// use codanna::Range;
///
/// let mut resolver = MethodCallResolver::new();
///
/// // Register variable types from let bindings
/// resolver.register_variable_type("calc", "Calculator");
///
/// // Add method calls from parsing
/// let call = MethodCall::new("process", "add", Range::new(5, 4, 5, 14))
///     .with_receiver("calc");
/// resolver.add_method_call(call);
///
/// // Later, during resolution:
/// if let Some(mc) = resolver.find_method_call("process", "add") {
///     let receiver_type = resolver.variable_types().get("calc");
///     // Use receiver_type for resolution...
/// }
/// ```
#[derive(Debug, Default)]
pub struct MethodCallResolver {
    /// Variable name to type mappings (e.g., "calc" -> "Calculator")
    variable_types: HashMap<String, String>,
    /// Method calls collected from parsing
    method_calls: Vec<MethodCall>,
}

impl MethodCallResolver {
    /// Creates a new empty resolver
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a variable's type from let binding, parameter, etc.
    ///
    /// # Arguments
    ///
    /// * `var` - The variable name (normalized)
    /// * `type_name` - The type name
    pub fn register_variable_type(&mut self, var: &str, type_name: &str) {
        self.variable_types
            .insert(var.to_string(), type_name.to_string());
    }

    /// Add a method call from parsing
    ///
    /// Returns `true` if added, `false` if duplicate (same caller, method, line, column)
    pub fn add_method_call(&mut self, call: MethodCall) -> bool {
        // Deduplication: check if already exists by (caller, method, line, col)
        let exists = self.method_calls.iter().any(|mc| {
            mc.caller == call.caller
                && mc.method_name == call.method_name
                && mc.range.start_line == call.range.start_line
                && mc.range.start_column == call.range.start_column
        });
        if exists {
            return false;
        }
        self.method_calls.push(call);
        true
    }

    /// Get the variable types map for resolution
    pub fn variable_types(&self) -> &HashMap<String, String> {
        &self.variable_types
    }

    /// Find a method call by caller and method name
    pub fn find_method_call(&self, caller: &str, method_name: &str) -> Option<&MethodCall> {
        self.method_calls
            .iter()
            .find(|mc| mc.caller == caller && mc.method_name == method_name)
    }

    /// Get all method calls
    pub fn method_calls(&self) -> &[MethodCall] {
        &self.method_calls
    }

    /// Check if a method call exists at a specific position
    ///
    /// Used to determine if a function_call is already tracked as a method_call.
    pub fn has_method_call_at(
        &self,
        caller: &str,
        method_name: &str,
        line: u32,
        column: u16,
    ) -> bool {
        self.method_calls.iter().any(|mc| {
            mc.caller == caller
                && mc.method_name == method_name
                && mc.range.start_line == line
                && mc.range.start_column == column
        })
    }

    /// Check if resolver has any data
    pub fn is_empty(&self) -> bool {
        self.method_calls.is_empty() && self.variable_types.is_empty()
    }

    /// Clear all data (for reuse)
    pub fn clear(&mut self) {
        self.variable_types.clear();
        self.method_calls.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_range() -> Range {
        Range::new(10, 5, 10, 20)
    }

    // === MethodCall tests ===

    #[test]
    fn test_basic_construction() {
        let call = MethodCall::new("main", "process", test_range());
        assert_eq!(call.caller, "main");
        assert_eq!(call.method_name, "process");
        assert_eq!(call.receiver, None);
        assert!(!call.is_static);
    }

    #[test]
    fn test_builder_pattern() {
        let call = MethodCall::new("handler", "clone", test_range()).with_receiver("data");

        assert_eq!(call.receiver, Some("data".to_string()));
        assert!(!call.is_static);
    }

    #[test]
    fn test_static_method() {
        let call = MethodCall::new("main", "new", test_range())
            .with_receiver("HashMap")
            .static_method();

        assert_eq!(call.receiver, Some("HashMap".to_string()));
        assert!(call.is_static);
    }

    #[test]
    fn test_helper_methods() {
        let self_call = MethodCall::new("foo", "bar", test_range()).with_receiver("self");
        assert!(self_call.is_self_call());
        assert!(!self_call.is_function_call());

        let func_call = MethodCall::new("main", "println", test_range());
        assert!(!func_call.is_self_call());
        assert!(func_call.is_function_call());
    }

    #[test]
    fn test_qualified_name() {
        // Static method
        let static_call = MethodCall::new("main", "new", test_range())
            .with_receiver("Vec")
            .static_method();
        assert_eq!(static_call.qualified_name(), "Vec::new");

        // Instance method
        let instance_call = MethodCall::new("process", "push", test_range()).with_receiver("items");
        assert_eq!(instance_call.qualified_name(), "items.push");

        // Function call
        let func_call = MethodCall::new("main", "println", test_range());
        assert_eq!(func_call.qualified_name(), "println");
    }

    #[test]
    fn test_equality() {
        let call1 = MethodCall::new("main", "test", test_range()).with_receiver("obj");
        let call2 = MethodCall::new("main", "test", test_range()).with_receiver("obj");
        let call3 = MethodCall::new("main", "test", test_range()).with_receiver("other");

        assert_eq!(call1, call2);
        assert_ne!(call1, call3);
    }

    // === MethodCallResolver tests ===

    #[test]
    fn test_resolver_new() {
        let resolver = MethodCallResolver::new();
        assert!(resolver.is_empty());
        assert!(resolver.variable_types().is_empty());
        assert!(resolver.method_calls().is_empty());
    }

    #[test]
    fn test_resolver_variable_types() {
        let mut resolver = MethodCallResolver::new();

        resolver.register_variable_type("calc", "Calculator");
        resolver.register_variable_type("items", "Vec<Item>");

        assert_eq!(
            resolver.variable_types().get("calc"),
            Some(&"Calculator".to_string())
        );
        assert_eq!(
            resolver.variable_types().get("items"),
            Some(&"Vec<Item>".to_string())
        );
        assert_eq!(resolver.variable_types().get("unknown"), None);
    }

    #[test]
    fn test_resolver_add_method_call() {
        let mut resolver = MethodCallResolver::new();

        let call1 =
            MethodCall::new("process", "add", Range::new(5, 4, 5, 14)).with_receiver("calc");
        let call2 =
            MethodCall::new("process", "multiply", Range::new(6, 4, 6, 18)).with_receiver("calc");

        assert!(resolver.add_method_call(call1));
        assert!(resolver.add_method_call(call2));

        assert_eq!(resolver.method_calls().len(), 2);
    }

    #[test]
    fn test_resolver_deduplication() {
        let mut resolver = MethodCallResolver::new();

        let call1 =
            MethodCall::new("process", "add", Range::new(5, 4, 5, 14)).with_receiver("calc");
        let call2 =
            MethodCall::new("process", "add", Range::new(5, 4, 5, 14)).with_receiver("calc");
        // Different call with same position
        let call3 =
            MethodCall::new("process", "add", Range::new(5, 4, 5, 14)).with_receiver("other");

        assert!(resolver.add_method_call(call1)); // First add succeeds
        assert!(!resolver.add_method_call(call2)); // Exact duplicate rejected
        assert!(!resolver.add_method_call(call3)); // Same position rejected

        assert_eq!(resolver.method_calls().len(), 1);
    }

    #[test]
    fn test_resolver_find_method_call() {
        let mut resolver = MethodCallResolver::new();

        let call = MethodCall::new("process", "add", test_range()).with_receiver("calc");
        resolver.add_method_call(call);

        assert!(resolver.find_method_call("process", "add").is_some());
        assert!(resolver.find_method_call("process", "subtract").is_none());
        assert!(resolver.find_method_call("other", "add").is_none());
    }

    #[test]
    fn test_resolver_clear() {
        let mut resolver = MethodCallResolver::new();

        resolver.register_variable_type("calc", "Calculator");
        resolver.add_method_call(MethodCall::new("process", "add", test_range()));

        assert!(!resolver.is_empty());

        resolver.clear();

        assert!(resolver.is_empty());
        assert!(resolver.variable_types().is_empty());
        assert!(resolver.method_calls().is_empty());
    }

    #[test]
    fn test_resolver_integration() {
        // Simulates the full flow: parsing → storage → lookup
        let mut resolver = MethodCallResolver::new();

        // 1. During parsing, register variable types
        resolver.register_variable_type("calc", "Calculator");
        resolver.register_variable_type("items", "Vec<i32>");

        // 2. During parsing, add method calls
        let call1 = MethodCall::new("process_numbers", "add", Range::new(7, 4, 7, 14))
            .with_receiver("calc");
        let call2 = MethodCall::new("process_numbers", "multiply", Range::new(8, 4, 8, 18))
            .with_receiver("calc");
        let call3 = MethodCall::new("compute_total", "add", Range::new(16, 8, 16, 18))
            .with_receiver("calc");

        resolver.add_method_call(call1);
        resolver.add_method_call(call2);
        resolver.add_method_call(call3);

        // 3. During resolution, lookup
        let mc = resolver.find_method_call("process_numbers", "add").unwrap();
        assert_eq!(mc.receiver.as_deref(), Some("calc"));

        let receiver_type = resolver.variable_types().get(mc.receiver.as_ref().unwrap());
        assert_eq!(receiver_type, Some(&"Calculator".to_string()));
    }
}
