use super::NixParser;
use crate::parsing::{LanguageParser, NodeTracker};
use crate::types::FileId;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Error, Debug)]
pub enum AuditError {
    #[error("IO error: {0}")]
    FileRead(#[from] std::io::Error),
    #[error("Language setup error: {0}")]
    LanguageSetup(String),
    #[error("Parse failure")]
    ParseFailure,
    #[error("Parser creation error: {0}")]
    ParserCreation(String),
}

pub struct NixParserAudit {
    pub grammar_nodes: HashMap<String, u16>,
    pub implemented_nodes: HashSet<String>,
    pub extracted_symbol_kinds: HashSet<String>,
}

impl NixParserAudit {
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        let mut parser = Parser::new();
        let language = tree_sitter_nix::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        let mut nix_parser =
            NixParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId::new(1).unwrap();
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = nix_parser.parse(code, file_id, &mut symbol_counter);

        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        let implemented_nodes: HashSet<String> = nix_parser
            .get_handled_nodes()
            .iter()
            .map(|n| n.name.clone())
            .collect();

        Ok(Self {
            grammar_nodes,
            implemented_nodes,
            extracted_symbol_kinds,
        })
    }

    pub fn generate_report(&self) -> String {
        let key_nodes = vec![
            "source_code",
            "binding",
            "attrset_expression",
            "rec_attrset_expression",
            "let_expression",
            "function_expression",
            "formals",
            "formal",
            "inherit",
            "inherit_from",
            "apply_expression",
            "select_expression",
            "attrpath",
            "identifier",
            "if_expression",
            "assert_expression",
            "with_expression",
            "comment",
        ];

        let key_implemented = key_nodes
            .iter()
            .filter(|n| self.implemented_nodes.contains(**n))
            .count();

        let mut report = String::new();
        report.push_str("# Nix Parser Symbol Extraction Coverage Report\n\n");
        report.push_str("## Summary\n");
        report.push_str(&format!(
            "- Key nodes: {}/{} ({:.0}%)\n",
            key_implemented,
            key_nodes.len(),
            (key_implemented as f64 / key_nodes.len() as f64) * 100.0
        ));
        report.push_str(&format!(
            "- Total grammar nodes: {}\n",
            self.grammar_nodes.len()
        ));
        report.push_str(&format!(
            "- Total implemented: {}\n",
            self.implemented_nodes.len()
        ));
        report.push_str(&format!(
            "- Symbol kinds extracted: {:?}\n\n",
            self.extracted_symbol_kinds
        ));

        report.push_str("## Key Nodes Coverage\n");
        for node in &key_nodes {
            let status = if self.implemented_nodes.contains(*node) {
                "✓"
            } else {
                "✗"
            };
            report.push_str(&format!("- [{status}] {node}\n"));
        }
        report
    }
}

pub fn discover_nodes(node: Node, registry: &mut HashMap<String, u16>) {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        registry.insert(current.kind().to_string(), current.kind_id());
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            stack.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_nix_comprehensive() {
        let code = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/nix/comprehensive.nix"
        ))
        .unwrap_or_else(|_| r#"{ x = 1; add = a: b: a + b; inherit x; }"#.to_string());

        let audit = NixParserAudit::audit_code(&code).unwrap();
        let report = audit.generate_report();
        println!("{report}");

        assert!(
            !audit.grammar_nodes.is_empty(),
            "Should have discovered grammar nodes"
        );
    }

    #[test]
    fn test_audit_simple_nix() {
        let code = r#"{ x = 1; add = a: b: a + b; }"#;
        let audit = NixParserAudit::audit_code(code).unwrap();
        assert!(audit.grammar_nodes.contains_key("binding"));
    }
}
