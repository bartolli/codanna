//! PHP parser audit module
//!
//! Tracks which AST nodes the parser actually handles vs what's available in the grammar.
//! This helps identify gaps in our symbol extraction.

use super::PhpParser;
use crate::io::format::format_utc_timestamp;
use crate::parsing::NodeTracker;
use crate::types::FileId;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Error, Debug)]
pub enum AuditError {
    #[error("Failed to read file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to set language: {0}")]
    LanguageSetup(String),

    #[error("Failed to parse code")]
    ParseFailure,

    #[error("Failed to create parser: {0}")]
    ParserCreation(String),
}

pub struct PhpParserAudit {
    /// Nodes found in the grammar/file
    pub grammar_nodes: HashMap<String, u16>,
    /// Nodes our parser actually processes (from tracking parse calls)
    pub implemented_nodes: HashSet<String>,
    /// Symbols actually extracted
    pub extracted_symbol_kinds: HashSet<String>,
}

impl PhpParserAudit {
    /// Run audit on a PHP source file
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    /// Run audit on PHP source code  
    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        // First, discover all nodes in the file using tree-sitter directly
        let mut parser = Parser::new();
        let language = tree_sitter_php::LANGUAGE_PHP.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        // Now parse with our actual parser to see what symbols get extracted
        let mut php_parser =
            PhpParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId(1);
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = php_parser.parse(code, file_id, &mut symbol_counter);

        // Track which symbol kinds were produced
        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        // Get dynamically tracked nodes from the parser (zero maintenance!)
        let implemented_nodes: HashSet<String> = php_parser
            .get_handled_nodes()
            .iter()
            .map(|handled_node| handled_node.name.clone())
            .collect();

        Ok(Self {
            grammar_nodes,
            implemented_nodes,
            extracted_symbol_kinds,
        })
    }

    /// Generate coverage report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str("# PHP Parser Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        // Key nodes we care about for symbol extraction
        let key_nodes = vec![
            // Symbol-producing nodes
            "namespace_definition",
            "namespace_use_declaration",
            "class_declaration",
            "interface_declaration",
            "trait_declaration",
            "enum_declaration",
            "method_declaration",
            "function_definition",
            "property_declaration",
            "const_declaration", // class and global constants
            "simple_parameter",
            "property_promotion_parameter",
            "variadic_parameter",
            "anonymous_function",
            "arrow_function",
            "attribute_list",
            "attribute_group",
            "attribute",
            // Call-related nodes (for relationship tracking)
            "function_call_expression",
            "member_call_expression",
            "scoped_call_expression",
        ];

        // Count key nodes coverage
        let key_implemented = key_nodes
            .iter()
            .filter(|n| self.implemented_nodes.contains(**n))
            .count();

        // Summary
        report.push_str("## Summary\n");
        report.push_str(&format!(
            "- Key nodes: {}/{} ({}%)\n",
            key_implemented,
            key_nodes.len(),
            (key_implemented * 100) / key_nodes.len()
        ));
        report.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            self.extracted_symbol_kinds.len()
        ));
        report.push_str(
            "\n> **Note:** Key nodes are symbol-producing constructs (classes, functions, traits).\n\n",
        );

        // Coverage table
        report.push_str("## Coverage Table\n\n");
        report.push_str("| Node Type | ID | Status |\n");
        report.push_str("|-----------|-----|--------|\n");

        let mut gaps = Vec::new();
        let mut missing = Vec::new();

        for node_name in &key_nodes {
            let status = if let Some(id) = self.grammar_nodes.get(*node_name) {
                if self.implemented_nodes.contains(*node_name) {
                    format!("{id} | ✅ implemented")
                } else {
                    gaps.push(node_name);
                    format!("{id} | ⚠️ gap")
                }
            } else {
                missing.push(node_name);
                "- | ❌ not found".to_string()
            };
            report.push_str(&format!("| {node_name} | {status} |\n"));
        }

        // Add legend
        report.push_str("\n## Legend\n\n");
        report
            .push_str("- ✅ **implemented**: Node type is recognized and handled by the parser\n");
        report.push_str("- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)\n");
        report.push_str("- ❌ **not found**: Node type not present in the example file (may need better examples)\n");

        // Add recommendations
        report.push_str("\n## Recommended Actions\n\n");

        if !gaps.is_empty() {
            report.push_str("### Priority 1: Implementation Gaps\n");
            report.push_str("These nodes exist in your code but aren't being captured:\n\n");
            for gap in &gaps {
                report.push_str(&format!("- `{gap}`: Add parsing logic in parser.rs\n"));
            }
            report.push('\n');
        }

        if !missing.is_empty() {
            report.push_str("### Priority 2: Missing Examples\n");
            report.push_str("These nodes aren't in the comprehensive example. Consider:\n\n");
            for node in &missing {
                report.push_str(&format!(
                    "- `{node}`: Add example to comprehensive.php or verify node name\n"
                ));
            }
            report.push('\n');
        }

        if gaps.is_empty() && missing.is_empty() {
            report.push_str("✨ **Excellent coverage!** All key nodes are implemented.\n");
        }

        report
    }
}

fn discover_nodes(node: Node, registry: &mut HashMap<String, u16>) {
    registry.insert(node.kind().to_string(), node.kind_id());

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        discover_nodes(child, registry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_simple_php() {
        let code = r#"<?php
namespace App;

class Example {
    public function test() {
        echo "Hello";
    }
}
"#;

        let audit = PhpParserAudit::audit_code(code).unwrap();

        // Should find these nodes in the code
        assert!(audit.grammar_nodes.contains_key("namespace_definition"));
        assert!(audit.grammar_nodes.contains_key("class_declaration"));
        assert!(audit.grammar_nodes.contains_key("method_declaration"));

        // Should extract Class and Method symbols
        assert!(audit.extracted_symbol_kinds.contains("Class"));
        assert!(audit.extracted_symbol_kinds.contains("Method"));
    }
}
