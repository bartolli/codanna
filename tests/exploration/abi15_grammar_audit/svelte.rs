//! Svelte grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::svelte::audit::SvelteParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Svelte",
    file_extension: "svelte",
    grammar_json_path: "contributing/parsers/svelte/node-types.json",
    example_file_path: "examples/svelte/comprehensive.svelte",
    output_dir: "contributing/parsers/svelte",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "SCRIPT NODES",
            vec![
                "script_element",
                "style_element",
                "start_tag",
                "end_tag",
                "raw_text",
            ],
        ),
        (
            "ATTRIBUTE NODES",
            vec![
                "attribute",
                "attribute_name",
                "attribute_value",
                "quoted_attribute_value",
            ],
        ),
        (
            "ELEMENT NODES",
            vec!["element", "self_closing_tag", "tag_name", "text", "doctype"],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "expression",
                "expression_tag",
                "render_tag",
                "html_tag",
                "const_tag",
                "debug_tag",
                "svelte_raw_text",
            ],
        ),
        (
            "BLOCK NODES",
            vec![
                "if_statement",
                "each_statement",
                "await_statement",
                "key_statement",
                "block_start_tag",
                "block_end_tag",
                "else_block",
            ],
        ),
        (
            "SNIPPET NODES",
            vec![
                "snippet_statement",
                "snippet_start",
                "snippet_name",
                "snippet_end",
            ],
        ),
        ("COMMENT NODES", vec!["comment"]),
    ]
}

#[test]
fn comprehensive_svelte_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_svelte_next::LANGUAGE.into(),
        "<script>let x = 1;</script>\n<p>{x}</p>\n",
        &node_categories(),
        |path| {
            let audit = SvelteParserAudit::audit_file(path).map_err(|e| e.to_string())?;
            let report = audit.generate_report();
            Ok((
                AuditData::new(
                    audit.grammar_nodes,
                    audit.implemented_nodes,
                    audit.extracted_symbol_kinds,
                ),
                report,
            ))
        },
    );
}
