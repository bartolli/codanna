//! Common utilities for ABI-15 exploration tests
//!
//! Minimal shared functionality for language-specific ABI-15 tests.
//! Each language test can import this module for common helpers.

use tree_sitter::{Language, Node, Parser};

/// Create a parser for the given language
pub fn create_parser(language: Language) -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("Failed to set language");
    parser
}

/// Parse code and return the tree
pub fn parse_code(parser: &mut Parser, code: &str) -> tree_sitter::Tree {
    parser.parse(code, None).expect("Failed to parse code")
}

/// Print a node and its children in a tree format for exploration
///
/// This is a debugging utility that's intentionally kept for development.
/// It's only used when DEBUG_TREE environment variable is set.
#[allow(dead_code)]
pub fn print_node_tree(node: Node, code: &str, indent: usize) {
    let node_text = &code[node.byte_range()];
    let truncated = if node_text.len() > 60 {
        format!("{}...", node_text[..57].replace('\n', " "))
    } else {
        node_text.replace('\n', " ")
    };

    println!(
        "{:indent$}[{}] '{}'",
        "",
        node.kind(),
        truncated,
        indent = indent
    );

    let mut cursor = node.walk();
    for (i, child) in node.children(&mut cursor).enumerate() {
        if let Some(field_name) = node.field_name_for_child(i as u32) {
            println!(
                "{:indent$}  └─ field: '{}'",
                "",
                field_name,
                indent = indent + 2
            );
        }
        print_node_tree(child, code, indent + 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_utilities() {
        // Quick smoke test that utilities compile and work
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = create_parser(language);
        let tree = parse_code(&mut parser, "fn main() {}");
        assert_eq!(tree.root_node().kind(), "source_file");
    }
}
