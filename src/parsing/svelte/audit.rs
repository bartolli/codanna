//! Svelte parser audit module
//!
//! Tracks which AST nodes the parser handles vs what's available in the grammar.
//!
//! Svelte symbol extraction is split: `<script>` bodies are delegated to the
//! JS/TS parsers, while template constructs such as `{#snippet}` are handled
//! directly against the tree-sitter-svelte grammar. This audit records the
//! Svelte-level nodes the parser acts on (script blocks, raw text, snippets).

use super::SvelteParser;
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

pub struct SvelteParserAudit {
    pub grammar_nodes: HashMap<String, u16>,
    pub implemented_nodes: HashSet<String>,
    pub extracted_symbol_kinds: HashSet<String>,
}

impl SvelteParserAudit {
    pub fn audit_file(file_path: &str) -> Result<Self, AuditError> {
        let code = std::fs::read_to_string(file_path)?;
        Self::audit_code(&code)
    }

    pub fn audit_code(code: &str) -> Result<Self, AuditError> {
        let mut parser = Parser::new();
        let language = tree_sitter_svelte_next::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| AuditError::LanguageSetup(e.to_string()))?;

        let tree = parser.parse(code, None).ok_or(AuditError::ParseFailure)?;

        let mut grammar_nodes = HashMap::new();
        discover_nodes(tree.root_node(), &mut grammar_nodes);

        let mut svelte_parser =
            SvelteParser::new().map_err(|e| AuditError::ParserCreation(e.to_string()))?;
        let file_id = FileId(1);
        let mut symbol_counter = crate::types::SymbolCounter::new();
        let symbols = svelte_parser.parse(code, file_id, &mut symbol_counter);

        let mut extracted_symbol_kinds = HashSet::new();
        for symbol in &symbols {
            extracted_symbol_kinds.insert(format!("{:?}", symbol.kind));
        }

        let implemented_nodes: HashSet<String> = svelte_parser
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

        report.push_str("# Svelte Parser Symbol Extraction Coverage Report\n\n");
        report.push_str(&format!("*Generated: {}*\n\n", format_utc_timestamp()));

        let key_nodes = vec![
            "document",
            "script_element",
            "style_element",
            "start_tag",
            "end_tag",
            "raw_text",
            "attribute",
            "attribute_name",
            "quoted_attribute_value",
            "element",
            "expression_tag",
            "render_tag",
            "snippet_statement",
            "snippet_start",
            "snippet_name",
            "if_statement",
            "each_statement",
            "await_statement",
            "key_statement",
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
            "\n> **Note:** Svelte delegates `<script>` bodies to the JS/TS parsers; \
             the nodes below are the Svelte-level constructs handled directly.\n\n",
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
                    "- `{node}`: Add example to comprehensive.svelte or verify node name\n"
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
    // Use iterative traversal with an explicit stack to avoid stack overflow on large ASTs
    let mut stack = vec![node];

    while let Some(current_node) = stack.pop() {
        registry.insert(current_node.kind().to_string(), current_node.kind_id());

        let mut cursor = current_node.walk();
        // Push children onto the stack for processing
        for child in current_node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_simple_svelte() {
        let code = r#"<script lang="ts">
    export function greet(name: string): string {
        return `Hello ${name}`;
    }
</script>

{#snippet card(item)}
    <div>{item}</div>
{/snippet}
"#;

        let audit = SvelteParserAudit::audit_code(code).unwrap();

        assert!(audit.grammar_nodes.contains_key("script_element"));
        assert!(audit.grammar_nodes.contains_key("snippet_statement"));

        // Script body delegates to TS: greet is a Function.
        assert!(audit.extracted_symbol_kinds.contains("Function"));

        // Svelte-level nodes the parser acts on are registered.
        assert!(audit.implemented_nodes.contains("script_element"));
        assert!(audit.implemented_nodes.contains("snippet_statement"));
    }

    #[test]
    fn test_template_node_names() {
        let code = r#"{#if ready}
    <p>ok</p>
{/if}
{#each items as item}
    {@render row(item)}
{/each}
"#;

        let audit = SvelteParserAudit::audit_code(code).unwrap();

        assert!(audit.grammar_nodes.contains_key("if_statement"));
        assert!(audit.grammar_nodes.contains_key("each_statement"));
        assert!(audit.grammar_nodes.contains_key("render_tag"));
    }
}
