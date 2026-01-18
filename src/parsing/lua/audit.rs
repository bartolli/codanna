//! Lua parser audit module
//!
//! Tracks which AST nodes the parser handles vs what's available in the grammar.

use super::LuaParser;
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

pub struct LuaParserAudit {
    pub grammar_nodes: HashMap<String, u16>,
    pub implemented_nodes: HashSet<String>,
    pub extracted_symbol_kinds: HashSet<String>,
}

impl LuaParserAudit {
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        let mut parser = Parser::new();
        let language = tree_sitter_lua::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        let mut lua_parser =
            LuaParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId(1);
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = lua_parser.parse(code, file_id, &mut symbol_counter);

        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        let implemented_nodes: HashSet<String> = lua_parser
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

        report.push_str("# Lua Parser Symbol Extraction Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        let key_nodes = vec![
            "chunk",
            "function_declaration",
            "function_definition",
            "variable_declaration",
            "assignment_statement",
            "table_constructor",
            "field",
            "function_call",
            "method_index_expression",
            "dot_index_expression",
            "bracket_index_expression",
            "for_statement",
            "for_generic_clause",
            "for_numeric_clause",
            "while_statement",
            "repeat_statement",
            "if_statement",
            "do_statement",
            "block",
            "return_statement",
            "comment",
        ];

        let key_implemented = key_nodes
            .iter()
            .filter(|n| self.implemented_nodes.contains(**n))
            .count();

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
            "\n> **Note:** Key nodes are symbol-producing constructs (functions, tables, imports).\n\n",
        );

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

        report.push_str("\n## Legend\n\n");
        report
            .push_str("- ✅ **implemented**: Node type is recognized and handled by the parser\n");
        report.push_str("- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)\n");
        report.push_str("- ❌ **not found**: Node type not present in the example file (may need better examples)\n");

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
                    "- `{node}`: Add example to comprehensive.lua or verify node name\n"
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
    fn test_audit_simple_lua() {
        let code = r#"
-- A simple Lua module
local M = {}

function M.hello(name)
    print("Hello, " .. name)
end

local function helper()
    return 42
end

return M
"#;

        let audit = LuaParserAudit::audit_code(code).unwrap();

        assert!(audit.grammar_nodes.contains_key("function_declaration"));
        assert!(audit.grammar_nodes.contains_key("variable_declaration"));

        assert!(audit.extracted_symbol_kinds.contains("Function"));
    }

    #[test]
    fn test_control_flow_node_names() {
        let code = r#"
for i, v in ipairs(t) do print(v) end
for i = 1, 10 do print(i) end
while x > 0 do x = x - 1 end
repeat x = x + 1 until x > 10
do local scoped = true end
"#;

        let audit = LuaParserAudit::audit_code(code).unwrap();

        assert!(audit.grammar_nodes.contains_key("for_statement"));
        assert!(audit.grammar_nodes.contains_key("for_generic_clause"));
        assert!(audit.grammar_nodes.contains_key("for_numeric_clause"));
        assert!(audit.grammar_nodes.contains_key("while_statement"));
        assert!(audit.grammar_nodes.contains_key("repeat_statement"));
        assert!(audit.grammar_nodes.contains_key("do_statement"));
    }
}
