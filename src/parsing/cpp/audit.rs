//! C++ parser audit module
//!
//! Tracks which AST nodes the parser actually handles vs what's available in the grammar.
//! This helps identify gaps in our symbol extraction.

use super::CppParser;
use crate::io::format::format_utc_timestamp;
use crate::parsing::NodeTracker;
use crate::types::FileId;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tree_sitter::Parser;

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

pub struct CppParserAudit {
    /// Nodes found in the grammar/file
    pub grammar_nodes: HashMap<String, u16>,
    /// Nodes our parser actually processes (from tracking parse calls)
    pub implemented_nodes: HashSet<String>,
    /// Symbols actually extracted
    pub extracted_symbol_kinds: HashSet<String>,
}

impl CppParserAudit {
    /// Run audit on a C++ source file
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    /// Run audit on C++ source code  
    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        // First, discover all nodes in the file using tree-sitter directly
        let mut parser = Parser::new();
        let language = tree_sitter_cpp::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;
        let mut grammar_nodes = HashMap::new();

        // Walk the tree to collect all node types
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        // Now run our parser to see what we actually extract
        let mut cpp_parser =
            CppParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();
        let symbols = cpp_parser.parse(code, file_id, &mut symbol_counter);

        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        // Get dynamically tracked nodes from the parser (zero maintenance!)
        let implemented_nodes: HashSet<String> = cpp_parser
            .get_handled_nodes()
            .iter()
            .map(|handled_node| handled_node.name.clone())
            .collect();

        Ok(CppParserAudit {
            grammar_nodes,
            implemented_nodes,
            extracted_symbol_kinds,
        })
    }

    /// Get coverage percentage (nodes implemented vs total)
    pub fn coverage_percentage(&self) -> f64 {
        if self.grammar_nodes.is_empty() {
            return 0.0;
        }

        let total = self.grammar_nodes.len();
        let implemented = self.implemented_nodes.len();
        (implemented as f64 / total as f64) * 100.0
    }

    /// Generate coverage report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str("# C++ Parser Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        // Key nodes we care about for symbol extraction
        // NOTE: These are ACTUAL tree-sitter node names only.
        // Constructors/destructors are function_definition nodes with special children:
        // - Constructors: have field_initializer_list child
        // - Destructors: have destructor_name declarator child
        // - Operators: have operator_name declarator child
        let key_nodes = vec![
            // Core structure
            "translation_unit",
            // Symbol-level declarations
            "function_definition",
            "class_specifier",
            "struct_specifier",
            "union_specifier",
            "enum_specifier",
            "namespace_definition",
            // Templates
            "template_declaration",
            "template_function",
            "template_type",
            // Declarations and specifiers
            "function_declarator",
            "init_declarator",
            "parameter_declaration",
            "field_declaration",
            "type_definition",   // typedef keyword
            "alias_declaration", // using Type = OtherType;
            // Access and inheritance
            "access_specifier",
            "base_class_clause",
            // Special member identifiers (within function_definition)
            "destructor_name",
            "operator_name",
            "field_initializer_list", // Indicates constructor
            // Modern C++ features
            "lambda_expression",
            "using_declaration", // using std::vector;
            // Call expressions and member access
            "call_expression",      // Function/method calls
            "field_expression",     // Member access (obj->member, obj.member)
            "qualified_identifier", // Qualified names (namespace::function, Class::method)
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
            "\n> **Note:** Key nodes are symbol-producing constructs (classes, functions, templates).\n\n",
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
                    "- `{node}`: Add example to comprehensive.cpp or verify node name\n"
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

fn discover_nodes(node: tree_sitter::Node, registry: &mut HashMap<String, u16>) {
    registry.insert(node.kind().to_string(), node.kind_id());

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        discover_nodes(child, registry);
    }
}
