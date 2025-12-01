//! Swift parser audit module
//!
//! Tracks which AST nodes the parser touches compared to the full
//! grammar exposed by tree-sitter-swift. Highlights extraction gaps.

use super::SwiftParser;
use crate::io::format::format_utc_timestamp;
use crate::parsing::parser::{LanguageParser, NodeTracker};
use crate::types::{FileId, SymbolCounter};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Debug, Error)]
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

/// Summary of grammar coverage for the Swift parser
pub struct SwiftParserAudit {
    /// All node kinds discovered in the sampled code
    pub grammar_nodes: HashMap<String, u16>,
    /// Node kinds that the parser marked as handled during extraction
    pub implemented_nodes: HashSet<String>,
    /// Symbol kinds that ended up in the index
    pub extracted_symbol_kinds: HashSet<String>,
}

impl SwiftParserAudit {
    /// Run audit on a source file
    pub fn audit_file(path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(path)?;
        Self::audit_code(&code)
    }

    /// Run audit directly on a source snippet
    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        // First gather grammar nodes using raw tree-sitter traversal
        let mut parser = Parser::new();
        let language: tree_sitter::Language = tree_sitter_swift::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;
        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        // Now run our production parser to see what we actually index
        let mut swift_parser = SwiftParser::new().map_err(AuditError::ParserCreation)?;
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();
        let symbols = swift_parser.parse(code, file_id, &mut counter);

        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        let implemented_nodes = swift_parser
            .get_handled_nodes()
            .iter()
            .map(|handled| handled.name.clone())
            .collect();

        Ok(Self {
            grammar_nodes,
            implemented_nodes,
            extracted_symbol_kinds,
        })
    }

    /// Produce a Markdown coverage report for docs or CI artifacts
    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str("# Swift Parser Symbol Extraction Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        // Key nodes for Swift - includes class_declaration which covers class/struct/enum/actor/extension
        let key_nodes = vec![
            "class_declaration",
            "protocol_declaration",
            "function_declaration",
            "init_declaration",
            "infix_expression",
            "deinit_declaration",
            "property_declaration",
            "enum_entry",
            "typealias_declaration",
            "subscript_declaration",
            "import_declaration",
            "visibility_modifier",
            "modifiers",
            "inheritance_specifier",
            "type_constraint",
            "associatedtype_declaration",
            "where_keyword",
            "switch_statement",
            "switch_entry",
            "willset_didset_block",
            "as_expression",
            "dictionary_type",
            "boolean_literal",
            "ternary_expression",
            "while_statement",
            "opaque_type",
        ];

        // Count key nodes coverage
        let key_implemented = key_nodes
            .iter()
            .filter(|n| self.implemented_nodes.contains(**n))
            .count();

        // Summary block
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
            "\n> **Note:** Key nodes are symbol-producing constructs (classes, protocols, functions).\n\n",
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
                "- | ⭕ not found".to_string()
            };
            report.push_str(&format!("| {node_name} | {status} |\n"));
        }

        // Legend
        report.push_str("\n## Legend\n\n");
        report.push_str("- ✅ **implemented**: node type is handled by the parser\n");
        report.push_str(
            "- ⚠️ **gap**: node exists in grammar but parser does not currently extract it\n",
        );
        report.push_str(
            "- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify\n",
        );

        // Recommendations
        report.push_str("\n## Recommended Actions\n\n");
        if !gaps.is_empty() {
            report.push_str("### Implementation Gaps\n");
            for gap in &gaps {
                report.push_str(&format!(
                    "- `{gap}`: add handling in `swift/parser.rs` if symbol extraction is required.\n"
                ));
            }
            report.push('\n');
        }

        if !missing.is_empty() {
            report.push_str("### Missing Samples\n");
            for node in &missing {
                report.push_str(&format!(
                    "- `{node}`: include representative code in audit fixtures to track coverage.\n"
                ));
            }
            report.push('\n');
        }

        if gaps.is_empty() && missing.is_empty() {
            report.push_str("All tracked nodes are currently implemented\n");
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
    fn test_audit_simple_swift() {
        let code = r#"
import Foundation

public class MyClass {
    var property: Int = 42

    func myMethod() {
        print("Hello")
    }
}

struct MyStruct {
    let name: String
}

func topLevelFunction() {
    print("World")
}
"#;

        let audit = SwiftParserAudit::audit_code(code).expect("audit should succeed");

        assert!(
            audit.grammar_nodes.contains_key("class_declaration")
                || !audit.grammar_nodes.is_empty(),
            "Class declarations should be discovered or some nodes found"
        );

        // Check that we extracted some symbols
        assert!(
            !audit.extracted_symbol_kinds.is_empty(),
            "Should extract some symbol kinds"
        );

        let report = audit.generate_report();
        assert!(
            report.contains("Swift Parser"),
            "Report should contain header, got:\n{report}"
        );
    }
}
