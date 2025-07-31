use crate::Range;

/// Represents a method call with receiver information
#[derive(Debug, Clone)]
pub struct MethodCall {
    /// The function/method making the call
    pub caller: String,
    /// The method being called
    pub method_name: String,
    /// The receiver expression (e.g., "self", "vec", "String")
    pub receiver: Option<String>,
    /// Whether this is a static method call (e.g., String::new)
    pub is_static: bool,
    /// Location of the call
    pub range: Range,
}

impl MethodCall {
    pub fn new(caller: String, method_name: String, range: Range) -> Self {
        Self {
            caller,
            method_name,
            receiver: None,
            is_static: false,
            range,
        }
    }
    
    pub fn with_receiver(mut self, receiver: String) -> Self {
        self.receiver = Some(receiver);
        self
    }
    
    pub fn static_method(mut self) -> Self {
        self.is_static = true;
        self
    }
    
    /// Convert to the simplified format used by existing code
    pub fn to_simple_call(&self) -> (String, String, Range) {
        let target = if let Some(ref receiver) = self.receiver {
            if receiver == "self" {
                format!("self.{}", self.method_name)
            } else if self.is_static {
                format!("{}::{}", receiver, self.method_name)
            } else {
                self.method_name.clone()
            }
        } else {
            self.method_name.clone()
        };
        
        (self.caller.clone(), target, self.range)
    }
}