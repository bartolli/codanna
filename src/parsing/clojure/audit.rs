//! Clojure parser audit module
//!
//! Tracks which AST nodes the parser actually handles vs what's available in the grammar.
//! This helps identify gaps in our symbol extraction.

use super::ClojureParser;
use crate::io::format::format_utc_timestamp;
use crate::parsing::parser::LanguageParser;
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

pub struct ClojureParserAudit {
    /// Nodes found in the grammar/file
    pub grammar_nodes: HashMap<String, u16>,
    /// Nodes our parser actually processes (from tracking parse calls)
    pub implemented_nodes: HashSet<String>,
    /// Symbols actually extracted
    pub extracted_symbol_kinds: HashSet<String>,
}

impl ClojureParserAudit {
    /// Run audit on a Clojure source file
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    /// Run audit on Clojure source code
    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        // First, discover all nodes in the file using tree-sitter directly
        let mut parser = Parser::new();
        let language = tree_sitter_clojure_orchard::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        // Now parse with our actual parser to see what symbols get extracted
        let mut clojure_parser =
            ClojureParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId(1);
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = clojure_parser.parse(code, file_id, &mut symbol_counter);

        // Track which symbol kinds were produced
        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        // Get dynamically tracked nodes from the parser
        let implemented_nodes: HashSet<String> = clojure_parser
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

        report.push_str("# Clojure Parser Symbol Extraction Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        // Key nodes we care about for symbol extraction in Clojure
        let key_nodes = vec![
            "list_lit",   // (defn ...), (def ...), etc.
            "sym_lit",    // Symbol names
            "vec_lit",    // [params], [fields]
            "map_lit",    // {:keys [...]}
            "kwd_lit",    // :require, :as, etc.
            "str_lit",    // Docstrings
            "comment",    // Comments
            "meta_lit",   // ^:private, ^{...}
            "num_lit",    // Numbers
            "nil_lit",    // nil
            "bool_lit",   // true/false
            "set_lit",    // #{...}
            "anon_fn_lit", // #(...)
            "regex_lit",  // #"pattern"
            "read_cond_lit", // #?(:clj ... :cljs ...)
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
            if key_nodes.is_empty() {
                0
            } else {
                (key_implemented * 100) / key_nodes.len()
            }
        ));
        report.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            self.extracted_symbol_kinds.len()
        ));
        report.push_str(
            "\n> **Note:** Key nodes are symbol-producing constructs (lists containing def forms).\n\n",
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
                    "- `{node}`: Add example to comprehensive.clj or verify node name\n"
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
    fn test_audit_simple_clojure() {
        let code = r#"
(ns my.example
  (:require [clojure.string :as str]))

(def my-var 42)

(defn greet
  "Greets a person"
  [name]
  (str "Hello, " name "!"))

(defn- helper [x]
  (* x 2))

(defprotocol IAnimal
  (speak [this]))

(defrecord Dog [name breed])

(defmulti process :type)

(defmethod process :default [_]
  (println "Unknown"))
"#;

        let audit = ClojureParserAudit::audit_code(code).unwrap();

        // Should find these nodes in the code
        assert!(audit.grammar_nodes.contains_key("list_lit"));
        assert!(audit.grammar_nodes.contains_key("sym_lit"));
        assert!(audit.grammar_nodes.contains_key("vec_lit"));

        // Should extract various symbol kinds
        assert!(audit.extracted_symbol_kinds.contains("Function"));
        assert!(audit.extracted_symbol_kinds.contains("Variable"));
        assert!(audit.extracted_symbol_kinds.contains("Module"));
        assert!(audit.extracted_symbol_kinds.contains("Interface"));
        assert!(audit.extracted_symbol_kinds.contains("Struct"));
        assert!(audit.extracted_symbol_kinds.contains("Method"));
    }

    #[test]
    fn test_generate_report() {
        let code = r#"
(defn hello [] (println "Hello"))
"#;

        let audit = ClojureParserAudit::audit_code(code).unwrap();
        let report = audit.generate_report();

        assert!(report.contains("Clojure Parser"));
        assert!(report.contains("Coverage"));
    }
}
