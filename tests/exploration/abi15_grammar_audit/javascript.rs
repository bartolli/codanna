//! JavaScript grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::javascript::audit::JavaScriptParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "JavaScript",
    file_extension: "js",
    grammar_json_path: "contributing/parsers/javascript/node-types.json",
    example_file_path: "examples/javascript/comprehensive.js",
    output_dir: "contributing/parsers/javascript",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "IMPORT/EXPORT NODES",
            vec![
                "import_statement",
                "import_clause",
                "named_imports",
                "namespace_import",
                "export_statement",
                "export_clause",
                "export_specifier",
                "import_specifier",
            ],
        ),
        (
            "CLASS NODES",
            vec![
                "class_declaration",
                "class_body",
                "class_heritage",
                "method_definition",
                "field_definition",
                "static_block",
                "computed_property_name",
            ],
        ),
        (
            "FUNCTION NODES",
            vec![
                "function_declaration",
                "function_expression",
                "arrow_function",
                "generator_function",
                "generator_function_declaration",
                "formal_parameters",
                "rest_pattern",
            ],
        ),
        (
            "VARIABLE NODES",
            vec![
                "variable_declaration",
                "lexical_declaration",
                "variable_declarator",
                "assignment_expression",
                "assignment_pattern",
                "object_pattern",
                "array_pattern",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "call_expression",
                "new_expression",
                "member_expression",
                "binary_expression",
                "unary_expression",
                "update_expression",
                "ternary_expression",
                "await_expression",
                "yield_expression",
                "spread_element",
                "parenthesized_expression",
            ],
        ),
        (
            "JSX NODES",
            vec![
                "jsx_element",
                "jsx_self_closing_element",
                "jsx_opening_element",
                "jsx_closing_element",
                "jsx_expression",
                "jsx_attribute",
                "jsx_text",
            ],
        ),
        (
            "CONTROL FLOW NODES",
            vec![
                "if_statement",
                "for_statement",
                "for_in_statement",
                "while_statement",
                "do_statement",
                "switch_statement",
                "try_statement",
                "catch_clause",
                "return_statement",
                "throw_statement",
            ],
        ),
        (
            "LITERAL NODES",
            vec![
                "string",
                "string_fragment",
                "template_string",
                "template_substitution",
                "number",
                "true",
                "false",
                "null",
                "regex",
                "object",
                "array",
            ],
        ),
    ]
}

#[test]
fn comprehensive_javascript_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_javascript::LANGUAGE.into(),
        "function main() {}\n",
        &node_categories(),
        |path| {
            let audit = JavaScriptParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
