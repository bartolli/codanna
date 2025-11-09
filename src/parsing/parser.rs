//! Language parser trait
//!
//! This module defines the common interface that all language parsers
//! must implement to work with the indexing system.

use crate::parsing::method_call::MethodCall;
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol};
use std::any::Any;
use std::collections::HashSet;
use tree_sitter::Node;

/// Common interface for all language parsers
pub trait LanguageParser: Send + Sync {
    /// Parse source code and extract symbols
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol>;

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
    /// Zero-cost: Returns string slices into the source code
    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)>;

    /// Find method calls with rich receiver information
    ///
    /// Default implementation converts from find_calls() for backward compatibility.
    /// Parsers can override this method to provide enhanced receiver tracking.
    ///
    /// # Returns
    ///
    /// A vector of MethodCall structs with structured receiver information
    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        self.find_calls(code)
            .into_iter()
            .map(|(caller, target, range)| MethodCall::from_legacy_format(caller, target, range))
            .collect()
    }

    /// Find trait/interface implementations
    ///
    /// Returns tuples of (type_name, trait_name, range)
    /// Zero-cost: Returns string slices into the source code
    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)>;

    /// Find inheritance relationships (extends for classes/interfaces)
    ///
    /// Returns tuples of (derived_type, base_type, range)
    /// Zero-cost: Returns string slices into the source code
    fn find_extends<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Default implementation returns empty for languages without inheritance
        Vec::new()
    }

    /// Find type usage (in fields, parameters, returns)
    ///
    /// Returns tuples of (context_name, used_type, range)
    /// Zero-cost: Returns string slices into the source code
    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)>;

    /// Find method definitions (in traits/interfaces or types)
    ///
    /// Returns tuples of (definer_name, method_name, range)
    /// Zero-cost: Returns string slices into the source code
    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)>;

    /// Find import statements in the code
    ///
    /// Returns Import structs with path, alias, and glob information
    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<crate::parsing::Import>;

    /// Get the language this parser handles
    fn language(&self) -> crate::parsing::Language;

    /// Extract variable bindings with their types
    /// Returns tuples of (variable_name, type_name, range)
    /// Zero-cost: Returns string slices into the source code
    fn find_variable_types<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Default implementation returns empty - languages can override
        Vec::new()
    }

    /// Optional: Extract variable types with complex generic substitution
    ///
    /// Returns owned strings to support type substitution like `List<T>` → `List<Int>`.
    /// Languages with generics can override this for richer type inference.
    /// Default returns None, causing indexer to fall back to zero-copy version.
    fn find_variable_types_with_substitution(
        &mut self,
        _code: &str,
    ) -> Option<Vec<(String, String, Range)>> {
        None
    }

    /// Find inherent methods (methods defined directly on types)
    /// Returns tuples of (type_name, method_name, range)
    ///
    /// This is for methods defined directly on types (not through traits/interfaces).
    /// Default implementation returns empty - languages can override.
    ///
    /// Note: Returns owned strings to support complex type names that need construction
    /// (e.g., Rust's `Option<String>`, `Vec<T>`, etc.)
    fn find_inherent_methods(&mut self, _code: &str) -> Vec<(String, String, Range)> {
        Vec::new()
    }
}

/// Trait for creating language parsers
pub trait ParserFactory: Send + Sync {
    /// Create a new parser instance
    fn create(&self) -> Result<Box<dyn LanguageParser>, String>;
}

/// Information about a handled AST node
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandledNode {
    pub name: String,
    pub id: u16,
}

/// Extension trait for tracking which AST node types a parser handles
///
/// This enables dynamic audit reporting by automatically tracking which
/// node types receive explicit handling during parsing. Eliminates the
/// need for manually maintaining static lists of implemented nodes.
pub trait NodeTracker {
    /// Get the set of node types this parser has encountered and handled
    fn get_handled_nodes(&self) -> &HashSet<HandledNode>;

    /// Register that we've handled a specific node type with its tree-sitter ID
    fn register_handled_node(&mut self, node_kind: &str, node_id: u16);
}

/// Default implementation of NodeTracker using a HashSet
///
/// Parsers can include this struct and delegate to it for zero-cost node tracking.
#[derive(Debug, Default)]
pub struct NodeTrackingState {
    handled_nodes: HashSet<HandledNode>,
}

impl NodeTrackingState {
    /// Create a new empty tracking state
    pub fn new() -> Self {
        Self {
            handled_nodes: HashSet::new(),
        }
    }
}

impl NodeTracker for NodeTrackingState {
    fn get_handled_nodes(&self) -> &HashSet<HandledNode> {
        &self.handled_nodes
    }

    #[inline]
    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        // Create node info - only allocates if we haven't seen this exact node before
        let node_info = HandledNode {
            name: node_kind.to_string(),
            id: node_id,
        };

        // HashSet::insert is efficient - only stores if not already present
        self.handled_nodes.insert(node_info);
    }
}

/// Safely truncate a UTF-8 string at a character boundary.
/// Returns a slice up to the last valid character boundary before max_bytes.
/// Zero-cost: returns a slice, no allocations.
#[inline]
pub fn safe_truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    // Find the last valid UTF-8 boundary before or at max_bytes
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }

    &s[..boundary]
}

/// Maximum recursion depth for AST traversal to prevent stack overflow
///
/// This limit protects against deeply nested structures (e.g., large array initializers,
/// deeply nested function calls). When the limit is reached, traversal stops and a
/// warning is logged if debug mode is enabled.
///
/// Value chosen based on:
/// - Default Rust stack size: 2MB
/// - Average stack frame size: ~4KB per recursive call
/// - Safety margin: 500 levels uses ~2MB, well within limits
pub const MAX_AST_DEPTH: usize = 500;

/// Check if recursion depth exceeds safe limits
///
/// This function provides centralized depth checking to prevent stack overflow
/// when processing deeply nested AST structures. All language parsers should
/// call this at the start of their recursive extract_symbols_from_node method.
///
/// # Arguments
///
/// * `depth` - Current recursion depth
/// * `node` - The tree-sitter node being processed (for error reporting)
///
/// # Returns
///
/// `true` if depth is safe to continue, `false` if limit exceeded
///
/// # Example
///
/// ```rust,ignore
/// fn extract_symbols_from_node(
///     &mut self,
///     node: Node,
///     code: &str,
///     file_id: FileId,
///     symbols: &mut Vec<Symbol>,
///     counter: &mut SymbolCounter,
///     depth: usize,
/// ) {
///     // Guard against stack overflow at the start
///     if !check_recursion_depth(depth, node) {
///         return;
///     }
///
///     // ... process node ...
///
///     // Recursive calls pass depth + 1
///     for child in node.children(&mut node.walk()) {
///         self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
///     }
/// }
/// ```
#[inline]
pub fn check_recursion_depth(depth: usize, node: Node) -> bool {
    if depth > MAX_AST_DEPTH {
        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "WARNING: Maximum AST depth ({}) exceeded at line {}:{}. Skipping subtree to prevent stack overflow.",
                MAX_AST_DEPTH,
                node.start_position().row + 1,
                node.start_position().column + 1
            );
        }
        return false;
    }
    true
}

/// Safely extract a substring window from source code, respecting UTF-8 boundaries.
///
/// This function creates a window of up to `window_size` bytes before the `end_byte` position,
/// ensuring we never slice in the middle of a UTF-8 character.
///
/// # Arguments
/// * `code` - The source code string
/// * `end_byte` - The ending byte position (exclusive)
/// * `window_size` - Maximum size of the window in bytes
///
/// # Returns
/// A slice of the code that:
/// - Ends at `end_byte`
/// - Starts at most `window_size` bytes before `end_byte`
/// - Respects UTF-8 character boundaries
///
/// # Example
/// ```ignore
/// let code = "export class 🔍 Scanner";
/// let window = safe_substring_window(code, 20, 10);
/// // Returns a safe slice without cutting the emoji
/// ```
pub fn safe_substring_window(code: &str, end_byte: usize, window_size: usize) -> &str {
    // Clamp end_byte to string length
    let end = end_byte.min(code.len());

    // Calculate the desired start position
    let start_raw = end.saturating_sub(window_size);

    // Find the nearest valid UTF-8 character boundary
    let start = if start_raw > 0 && !code.is_char_boundary(start_raw) {
        // Search forward for a valid boundary (up to 3 bytes for UTF-8)
        (start_raw..=start_raw.saturating_add(3).min(end))
            .find(|&i| code.is_char_boundary(i))
            .unwrap_or(end) // If no boundary found, return empty string
    } else {
        start_raw
    };

    &code[start..end]
}

/// Creates a truncated preview with ellipsis for display purposes.
/// Used for signatures and previews in parsers.
#[inline]
pub fn truncate_for_display(s: &str, max_bytes: usize) -> String {
    let truncated = safe_truncate_str(s, max_bytes);
    if truncated.len() < s.len() {
        format!("{truncated}...")
    } else {
        truncated.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_truncate_with_emoji_panic() {
        // This test reproduces issue #29 - emoji at bytes 8-12
        let text = "Status: 🔍 Active";
        eprintln!("Input text: '{}' (len: {} bytes)", text, text.len());
        eprintln!("Attempting to truncate at byte 10...");

        // This would panic with &text[..10] as it cuts the emoji in half
        let result = safe_truncate_str(text, 10);
        eprintln!("Result: '{}' (len: {} bytes)", result, result.len());

        assert_eq!(result, "Status: "); // Should stop before the 4-byte emoji
        assert!(result.len() <= 10);
        eprintln!("✅ Safe truncation avoided panic at emoji boundary!");
    }

    #[test]
    fn test_safe_truncate_exact_boundary() {
        let text = "Hello, World!";
        let result = safe_truncate_str(text, 7);
        assert_eq!(result, "Hello, ");
    }

    #[test]
    fn test_safe_truncate_multi_byte_chars() {
        // Test with 2-byte char (é is 2 bytes in UTF-8)
        let text = "Café is nice";
        eprintln!("\n2-byte char test:");
        eprintln!("  Input: '{}' (len: {} bytes)", text, text.len());
        eprintln!("  'é' starts at byte 3, is 2 bytes long");
        let result = safe_truncate_str(text, 4);
        eprintln!("  Truncate at 4: '{}' (len: {})", result, result.len());
        assert_eq!(result, "Caf"); // Should not include partial é

        // Test with 3-byte char (├ is 3 bytes in UTF-8)
        let text = "Tree├──branch";
        eprintln!("\n3-byte char test:");
        eprintln!("  Input: '{}' (len: {} bytes)", text, text.len());
        eprintln!("  '├' starts at byte 4, is 3 bytes long");
        let result = safe_truncate_str(text, 5);
        eprintln!("  Truncate at 5: '{}' (len: {})", result, result.len());
        assert_eq!(result, "Tree"); // Should not include partial ├
        eprintln!("✅ Multi-byte character boundaries handled correctly!");
    }

    #[test]
    fn test_truncate_for_display() {
        let text = "This is a very long string that needs truncation";
        let result = truncate_for_display(text, 10);
        assert_eq!(result, "This is a ...");

        let short_text = "Short";
        let result = truncate_for_display(short_text, 10);
        assert_eq!(result, "Short");
    }

    #[test]
    fn test_issue_29_exact_case() {
        // Exact case from issue #29
        let text = r#"[
            f"🔍 System Status: {health.status.title()} {health.status_emoji}",
            f"├── Active Processes: {health.process_count}/{self.config.critical_threshold} ""#;

        eprintln!("\n🐛 Issue #29 - Exact reproduction case:");
        eprintln!("Input text length: {} bytes", text.len());
        eprintln!("Text contains emojis: 🔍 at byte ~15, ├ at byte ~95");

        // Should not panic when truncating at byte 100
        eprintln!("\nAttempting truncation at byte 100...");
        let result = safe_truncate_str(text, 100);
        eprintln!("Truncated to {} bytes without panic!", result.len());
        eprintln!(
            "Result ends with: '{}'",
            &result[result.len().saturating_sub(20)..]
        );
        assert!(result.len() <= 100);
        assert!(text.starts_with(result));

        // Test display truncation
        let display = truncate_for_display(text, 100);
        eprintln!(
            "\nDisplay truncation result: {} bytes (includes '...' if truncated)",
            display.len()
        );
        assert!(display.len() <= 103); // 100 + "..."

        eprintln!("✅ Issue #29 fixed - no panic on emoji boundaries!");
    }
}
