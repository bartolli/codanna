//! Method call representation for enhanced type-aware resolution
//! 
//! This module provides a structured representation of method calls that captures
//! receiver information, enabling more accurate cross-reference resolution than
//! the current string-based approach.
//! 
//! # Design Goals
//! 
//! - **Type Safety**: Structured data instead of string patterns
//! - **Zero-Cost Abstractions**: Accept borrowed types in APIs
//! - **Memory Efficiency**: Minimal overhead compared to tuple representation
//! - **Future Extensibility**: Foundation for type-aware resolution
//! 
//! # Current Integration Status
//! 
//! This struct is not yet integrated into the indexing pipeline. The current
//! system uses `(String, String, Range)` tuples with patterns like:
//! - `"self.method"` for self calls
//! - `"receiver@method"` for instance calls
//! - `"method"` for plain function calls
//! 
//! # Future Enhancements
//! 
//! - Add `receiver_type: Option<String>` for resolved type information
//! - Support method chains with `intermediate_types: Vec<String>`
//! - Generic type parameter tracking

use crate::Range;

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
    /// 
    /// # Examples
    /// 
    /// ```rust
    /// let call = MethodCall::new("main", "clone", range)
    ///     .with_receiver("data");
    /// ```
    pub fn with_receiver(mut self, receiver: &str) -> Self {
        self.receiver = Some(receiver.to_string());
        self
    }
    
    /// Marks this as a static method call
    /// 
    /// Should be used when the receiver is a type name rather than an instance.
    /// 
    /// # Examples
    /// 
    /// ```rust
    /// let call = MethodCall::new("main", "new", range)
    ///     .with_receiver("Vec")
    ///     .static_method();
    /// ```
    pub fn static_method(mut self) -> Self {
        self.is_static = true;
        self
    }
    
    /// Convert to the simplified format used by existing code
    /// 
    /// This provides backward compatibility with the current indexing system
    /// that expects `(caller, target, range)` tuples.
    /// 
    /// # Returns
    /// 
    /// A tuple of `(caller, target, range)` where target is:
    /// - `"self.method"` for self calls
    /// - `"Type::method"` for static calls
    /// - `"method"` for plain function calls or instance calls
    /// 
    /// # Note
    /// 
    /// The current format loses receiver information for instance calls.
    /// This is a limitation we aim to address with full MethodCall integration.
    #[must_use = "The converted tuple should be used"]
    pub fn to_simple_call(&self) -> (String, String, Range) {
        let target = if let Some(receiver) = &self.receiver {
            if receiver == "self" {
                // Preserve self calls with special format
                format!("self.{}", self.method_name)
            } else if self.is_static {
                // Static method calls use :: separator
                format!("{}::{}", receiver, self.method_name)
            } else {
                // Instance method calls lose receiver info in current format
                // TODO: This is where we lose type information!
                self.method_name.clone()
            }
        } else {
            // Plain function call
            self.method_name.clone()
        };
        
        (self.caller.clone(), target, self.range)
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
            (Some(receiver), true) => format!("{}::{}", receiver, self.method_name),
            (Some(receiver), false) => format!("{}.{}", receiver, self.method_name),
            (None, _) => self.method_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn test_range() -> Range {
        Range::new(10, 5, 10, 20)
    }
    
    // Integration test demonstrating how MethodCall would work with real parser output
    #[test]
    fn test_integration_with_parser_output() {
        let debug = true; // Enable debug output for tests
        
        // Simulating what the parser currently returns for method calls
        let parser_output = vec![
            ("process_data", "self.validate", Range::new(5, 10, 5, 25)),
            ("main", "HashMap::new", Range::new(10, 15, 10, 30)),
            ("handler", "clone", Range::new(15, 20, 15, 28)),  // Lost receiver info!
            ("save", "file@write", Range::new(20, 10, 20, 22)), // Current workaround
        ];
        
        if debug {
            eprintln!("\nDEBUG: Current parser output (legacy format):");
            for (caller, target, _) in &parser_output {
                eprintln!("  {} -> {}", caller, target);
            }
        }
        
        // Converting current output to MethodCall structures
        let method_calls: Vec<MethodCall> = parser_output.iter().map(|(caller, target, range)| {
            let result = if target.starts_with("self.") {
                // Self method call
                let method = &target[5..];
                if debug {
                    eprintln!("DEBUG: Parsing self call '{}' -> method: '{}'", target, method);
                }
                MethodCall::new(caller, method, *range)
                    .with_receiver("self")
            } else if target.contains("::") {
                // Static method call
                let parts: Vec<&str> = target.split("::").collect();
                if parts.len() == 2 {
                    if debug {
                        eprintln!("DEBUG: Parsing static call '{}' -> type: '{}', method: '{}'", 
                                 target, parts[0], parts[1]);
                    }
                    MethodCall::new(caller, parts[1], *range)
                        .with_receiver(parts[0])
                        .static_method()
                } else {
                    MethodCall::new(caller, target, *range)
                }
            } else if target.contains('@') {
                // Instance method with receiver hint
                let parts: Vec<&str> = target.split('@').collect();
                if parts.len() == 2 {
                    if debug {
                        eprintln!("DEBUG: Parsing receiver hint '{}' -> receiver: '{}', method: '{}'", 
                                 target, parts[0], parts[1]);
                    }
                    MethodCall::new(caller, parts[1], *range)
                        .with_receiver(parts[0])
                } else {
                    MethodCall::new(caller, target, *range)
                }
            } else {
                // Plain function call or instance method (receiver lost)
                if debug {
                    eprintln!("DEBUG: Parsing plain call '{}' -> NO RECEIVER INFO!", target);
                }
                MethodCall::new(caller, target, *range)
            };
            
            if debug {
                eprintln!("  -> MethodCall {{ caller: '{}', method: '{}', receiver: {:?}, is_static: {} }}",
                         result.caller, result.method_name, result.receiver, result.is_static);
            }
            
            result
        }).collect();
        
        // Verify conversions
        assert_eq!(method_calls.len(), 4);
        
        // Self call preserved
        assert_eq!(method_calls[0].qualified_name(), "self.validate");
        assert!(method_calls[0].is_self_call());
        
        // Static call preserved
        assert_eq!(method_calls[1].qualified_name(), "HashMap::new");
        assert!(method_calls[1].is_static);
        
        // Instance method - receiver lost in current format
        assert_eq!(method_calls[2].qualified_name(), "clone");
        assert!(method_calls[2].receiver.is_none()); // This is the problem!
        
        // Workaround pattern parsed
        assert_eq!(method_calls[3].qualified_name(), "file.write");
        assert_eq!(method_calls[3].receiver, Some("file".to_string()));
    }
    
    // Test demonstrating enhanced parser output with MethodCall
    #[test]
    fn test_enhanced_parser_with_method_call() {
        let debug = true; // Enable debug output
        
        // What the enhanced parser would return
        let enhanced_calls = vec![
            MethodCall::new("process_data", "validate", Range::new(5, 10, 5, 25))
                .with_receiver("self"),
            MethodCall::new("main", "new", Range::new(10, 15, 10, 30))
                .with_receiver("HashMap")
                .static_method(),
            MethodCall::new("handler", "clone", Range::new(15, 20, 15, 28))
                .with_receiver("data"), // Receiver preserved!
            MethodCall::new("save", "write", Range::new(20, 10, 20, 22))
                .with_receiver("file"),
        ];
        
        if debug {
            eprintln!("\nDEBUG: Enhanced parser output (MethodCall format):");
            for call in &enhanced_calls {
                eprintln!("  MethodCall {{");
                eprintln!("    caller: '{}'", call.caller);
                eprintln!("    method: '{}'", call.method_name);
                eprintln!("    receiver: {:?}", call.receiver);
                eprintln!("    is_static: {}", call.is_static);
                eprintln!("    qualified: '{}'", call.qualified_name());
                eprintln!("  }}");
            }
        }
        
        // All calls have complete information
        for call in &enhanced_calls {
            // Can always determine the full calling context
            match (&call.receiver, call.is_static) {
                (Some(_receiver), true) => {
                    // Static method: Type::method
                    assert!(call.qualified_name().contains("::"));
                }
                (Some(_receiver), false) => {
                    // Instance method: receiver.method
                    assert!(call.qualified_name().contains("."));
                }
                (None, _) => {
                    // Plain function call
                    assert!(!call.qualified_name().contains("."));
                    assert!(!call.qualified_name().contains("::"));
                }
            }
        }
        
        // Converting back for compatibility
        let legacy_format: Vec<(String, String, Range)> = enhanced_calls.iter()
            .map(|call| call.to_simple_call())
            .collect();
        
        if debug {
            eprintln!("\nDEBUG: Converting back to legacy format:");
            for (i, (caller, target, _)) in legacy_format.iter().enumerate() {
                let original = &enhanced_calls[i];
                eprintln!("  {} -> {} (was: {:?})", caller, target, original.qualified_name());
                if original.receiver.is_some() && !target.contains('.') && !target.contains("::") {
                    eprintln!("    WARNING: Lost receiver information for '{}'!", original.receiver.as_ref().unwrap());
                }
            }
        }
        
        // Verify backward compatibility
        assert_eq!(legacy_format[0].1, "self.validate");
        assert_eq!(legacy_format[1].1, "HashMap::new");
        assert_eq!(legacy_format[2].1, "clone"); // Still loses info in legacy format
        assert_eq!(legacy_format[3].1, "write"); // Still loses info in legacy format
    }
    
    // Priority Test Case 1: Basic method calls
    #[test]
    fn test_priority_1_basic_method_calls() {
        let debug = true;
        
        // Simulating enhanced parser output for basic method calls
        let basic_calls = vec![
            // self.method()
            MethodCall::new("save", "validate", Range::new(10, 8, 10, 23))
                .with_receiver("self"),
            
            // instance.method()
            MethodCall::new("process", "push", Range::new(15, 12, 15, 28))
                .with_receiver("items"),
            
            // Type::static_method()
            MethodCall::new("main", "from_str", Range::new(20, 16, 20, 40))
                .with_receiver("IpAddr")
                .static_method(),
        ];
        
        if debug {
            eprintln!("\nDEBUG: Priority 1 - Basic method calls:");
            for call in &basic_calls {
                eprintln!("  {} => {}", 
                    call.qualified_name(),
                    match (&call.receiver, call.is_static) {
                        (Some(_), true) => "Static method call",
                        (Some(r), false) if r == "self" => "Self method call",
                        (Some(_), false) => "Instance method call",
                        (None, _) => "Function call",
                    }
                );
            }
        }
        
        // Verify each type is correctly identified
        assert!(basic_calls[0].is_self_call());
        assert!(!basic_calls[1].is_self_call() && basic_calls[1].receiver.is_some());
        assert!(basic_calls[2].is_static);
        
        // Show how current system loses information
        let legacy_conversions: Vec<_> = basic_calls.iter()
            .map(|c| c.to_simple_call())
            .collect();
            
        if debug {
            eprintln!("\nDEBUG: Legacy format comparison:");
            for (i, (_, target, _)) in legacy_conversions.iter().enumerate() {
                let original = &basic_calls[i];
                eprintln!("  {} -> {} {}",
                    original.qualified_name(),
                    target,
                    if original.receiver.is_some() && !target.contains('.') && !target.contains("::") {
                        "(LOST RECEIVER!)"
                    } else {
                        "(preserved)"
                    }
                );
            }
        }
    }
    
    // Priority Test Case 2: Method chains
    #[test]
    fn test_priority_2_method_chains() {
        let debug = true;
        
        // Method chains require tracking intermediate types
        // This demonstrates the foundation for chain support
        let chain_components = vec![
            // foo().bar().baz() - broken into components
            ("main", "foo", None, Range::new(10, 12, 10, 17)),
            ("main", "bar", Some("foo_result"), Range::new(10, 18, 10, 23)),
            ("main", "baz", Some("bar_result"), Range::new(10, 24, 10, 29)),
            
            // self.field.method()
            ("process", "field", Some("self"), Range::new(15, 8, 15, 18)),
            ("process", "method", Some("field"), Range::new(15, 19, 15, 27)),
        ];
        
        let chain_calls: Vec<MethodCall> = chain_components.iter()
            .map(|(caller, method, receiver, range)| {
                let mut call = MethodCall::new(caller, method, *range);
                if let Some(recv) = receiver {
                    call = call.with_receiver(recv);
                }
                call
            })
            .collect();
        
        if debug {
            eprintln!("\nDEBUG: Priority 2 - Method chains:");
            eprintln!("  Chain: foo().bar().baz()");
            for i in 0..3 {
                eprintln!("    Step {}: {} (receiver: {:?})",
                    i + 1,
                    chain_calls[i].method_name,
                    chain_calls[i].receiver
                );
            }
            
            eprintln!("\n  Chain: self.field.method()");
            for i in 3..5 {
                eprintln!("    Step {}: {} (receiver: {:?})",
                    i - 2,
                    chain_calls[i].method_name,
                    chain_calls[i].receiver
                );
            }
        }
        
        // Verify chain relationships
        assert_eq!(chain_calls[0].receiver, None); // foo() has no receiver
        assert_eq!(chain_calls[1].receiver, Some("foo_result".to_string()));
        assert_eq!(chain_calls[2].receiver, Some("bar_result".to_string()));
        
        assert_eq!(chain_calls[3].receiver, Some("self".to_string()));
        assert_eq!(chain_calls[4].receiver, Some("field".to_string()));
    }
    
    // Priority Test Case 3: Type inference scenarios
    #[test]
    fn test_priority_3_type_inference() {
        let debug = true;
        
        // Scenarios where type information helps resolution
        struct TypedCall {
            call: MethodCall,
            inferred_type: Option<&'static str>, // Would come from type analysis
            source: &'static str,
        }
        
        let typed_scenarios = vec![
            // let x = MyType::new()
            TypedCall {
                call: MethodCall::new("main", "new", Range::new(10, 20, 10, 35))
                    .with_receiver("HashMap")
                    .static_method(),
                inferred_type: Some("HashMap<String, i32>"),
                source: "Constructor",
            },
            
            // let x: MyType = Default::default()
            TypedCall {
                call: MethodCall::new("init", "default", Range::new(15, 25, 15, 42))
                    .with_receiver("Default")
                    .static_method(),
                inferred_type: Some("MyStruct"), // From type annotation
                source: "Type annotation",
            },
            
            // let x = vec.clone()
            TypedCall {
                call: MethodCall::new("process", "clone", Range::new(20, 15, 20, 27))
                    .with_receiver("items"),
                inferred_type: Some("Vec<Item>"), // From variable type
                source: "Method return",
            },
        ];
        
        if debug {
            eprintln!("\nDEBUG: Priority 3 - Type inference scenarios:");
            for scenario in &typed_scenarios {
                eprintln!("  {} call: {}",
                    scenario.source,
                    scenario.call.qualified_name()
                );
                if let Some(typ) = scenario.inferred_type {
                    eprintln!("    -> Inferred type: {}", typ);
                    eprintln!("    -> Enables accurate resolution of '{}'", 
                        scenario.call.method_name);
                }
            }
            
            eprintln!("\nDEBUG: Why type inference matters:");
            eprintln!("  - 'clone' on Vec<T> vs HashMap<K,V> resolves to different methods");
            eprintln!("  - 'default' needs type context to know which impl to use");
            eprintln!("  - Method chains need intermediate types for correct resolution");
        }
        
        // Verify the calls are structured correctly for type resolution
        assert!(typed_scenarios[0].call.is_static);
        assert!(typed_scenarios[1].call.is_static);
        assert!(!typed_scenarios[2].call.is_static);
        
        // In current system, the last case loses receiver info
        let (_, target, _) = typed_scenarios[2].call.to_simple_call();
        assert_eq!(target, "clone"); // Lost "items" receiver!
    }
    
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
        let call = MethodCall::new("handler", "clone", test_range())
            .with_receiver("data");
        
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
    fn test_to_simple_call_self() {
        let call = MethodCall::new("save", "validate", test_range())
            .with_receiver("self");
        
        let (caller, target, _) = call.to_simple_call();
        assert_eq!(caller, "save");
        assert_eq!(target, "self.validate");
    }
    
    #[test]
    fn test_to_simple_call_static() {
        let call = MethodCall::new("main", "from_str", test_range())
            .with_receiver("IpAddr")
            .static_method();
        
        let (caller, target, _) = call.to_simple_call();
        assert_eq!(caller, "main");
        assert_eq!(target, "IpAddr::from_str");
    }
    
    #[test]
    fn test_to_simple_call_instance() {
        let call = MethodCall::new("process", "len", test_range())
            .with_receiver("vec");
        
        let (caller, target, _) = call.to_simple_call();
        assert_eq!(caller, "process");
        // Note: receiver information is lost in current format
        assert_eq!(target, "len");
    }
    
    #[test]
    fn test_helper_methods() {
        let self_call = MethodCall::new("foo", "bar", test_range())
            .with_receiver("self");
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
        let instance_call = MethodCall::new("process", "push", test_range())
            .with_receiver("items");
        assert_eq!(instance_call.qualified_name(), "items.push");
        
        // Function call
        let func_call = MethodCall::new("main", "println", test_range());
        assert_eq!(func_call.qualified_name(), "println");
    }
    
    #[test]
    fn test_equality() {
        let call1 = MethodCall::new("main", "test", test_range())
            .with_receiver("obj");
        let call2 = MethodCall::new("main", "test", test_range())
            .with_receiver("obj");
        let call3 = MethodCall::new("main", "test", test_range())
            .with_receiver("other");
        
        assert_eq!(call1, call2);
        assert_ne!(call1, call3);
    }
}