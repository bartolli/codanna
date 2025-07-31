//! Language parser trait
//! 
//! This module defines the common interface that all language parsers
//! must implement to work with the indexing system.

use crate::{Symbol, FileId, Range};
use tree_sitter::Node;
use std::any::Any;

/// Common interface for all language parsers
pub trait LanguageParser: Send + Sync {
    /// Parse source code and extract symbols
    fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut u32) -> Vec<Symbol>;
    
    /// Enable downcasting to concrete parser types
    fn as_any(&self) -> &dyn Any;
    
    /// Extract documentation comment for a node
    /// 
    /// Each language has its own documentation conventions:
    /// - Rust: `///` and `/** */`
    /// - Python: Docstrings (first string literal)
    /// - JavaScript/TypeScript: JSDoc `/** */`
    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String>;
    
    /// Find function/method calls in the code
    /// 
    /// Returns tuples of (caller_name, callee_name, range)
    fn find_calls(&mut self, code: &str) -> Vec<(String, String, Range)>;
    
    /// Find trait/interface implementations
    /// 
    /// Returns tuples of (type_name, trait_name, range)
    fn find_implementations(&mut self, code: &str) -> Vec<(String, String, Range)>;
    
    /// Find type usage (in fields, parameters, returns)
    /// 
    /// Returns tuples of (context_name, used_type, range)
    fn find_uses(&mut self, code: &str) -> Vec<(String, String, Range)>;
    
    /// Find method definitions (in traits/interfaces or types)
    /// 
    /// Returns tuples of (definer_name, method_name, range)
    fn find_defines(&mut self, code: &str) -> Vec<(String, String, Range)>;
    
    /// Find import statements in the code
    /// 
    /// Returns Import structs with path, alias, and glob information
    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<crate::indexing::Import>;
    
    /// Get the language this parser handles
    fn language(&self) -> crate::parsing::Language;
    
    /// Extract variable bindings with their types
    /// Returns tuples of (variable_name, type_name, range)
    fn find_variable_types(&mut self, _code: &str) -> Vec<(String, String, Range)> {
        // Default implementation returns empty - languages can override
        Vec::new()
    }
    
    /// Enable mutable downcasting to concrete parser types
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Trait for creating language parsers
pub trait ParserFactory: Send + Sync {
    /// Create a new parser instance
    fn create(&self) -> Result<Box<dyn LanguageParser>, String>;
}