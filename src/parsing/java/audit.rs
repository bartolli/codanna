//! Java parser audit module
//!
//! Provides ABI-15 coverage tracking for Java parser.

use super::JavaParser;
use crate::io::format::format_utc_timestamp;
use crate::parsing::{LanguageParser, NodeTracker};
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

pub struct JavaParserAudit {
    pub grammar_nodes: HashMap<String, u16>,
    pub implemented_nodes: HashSet<String>,
    pub extracted_symbol_kinds: HashSet<String>,
}

impl JavaParserAudit {
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        // Parse with tree-sitter to discover all nodes
        let mut parser = Parser::new();
        let language = tree_sitter_java::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        // Parse with our parser to see what symbols get extracted
        let mut lang_parser =
            JavaParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId(1);
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = lang_parser.parse(code, file_id, &mut symbol_counter);

        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        let implemented_nodes: HashSet<String> = lang_parser
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

    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str("# Java Parser Symbol Extraction Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        let key_nodes = vec![
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "annotation_type_declaration",
            "method_declaration",
            "constructor_declaration",
            "field_declaration",
            "package_declaration",
            "import_declaration",
            "modifiers",
            "formal_parameters",
            "type_parameters",
            "marker_annotation", // @Override, @Deprecated (simple annotations)
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
            "\n> **Note:** Key nodes are symbol-producing constructs (classes, functions, imports).\n\n",
        );

        // Coverage table
        report.push_str("## Coverage Table\n\n");
        report.push_str("| Node Type | ID | Status |\n");
        report.push_str("|-----------|-----|--------|\n");

        let mut gaps = Vec::new();
        let mut missing = Vec::new();

        for node_name in key_nodes {
            let status = if let Some(id) = self.grammar_nodes.get(node_name) {
                if self.implemented_nodes.contains(node_name) {
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
        report.push_str("- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify\n");

        // Recommendations
        report.push_str("\n## Recommended Actions\n\n");
        if !gaps.is_empty() {
            report.push_str("### Implementation Gaps\n");
            for gap in &gaps {
                report.push_str(&format!(
                    "- `{gap}`: add handling in `java/parser.rs` if symbol extraction is required.\n"
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
            report.push_str("All tracked nodes are currently implemented ✅\n");
        }

        report
    }
}

fn discover_nodes(node: Node, nodes: &mut HashMap<String, u16>) {
    nodes.insert(node.kind().to_string(), node.kind_id());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        discover_nodes(child, nodes);
    }
}
