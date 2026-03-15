//! TypeScript grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::typescript::audit::TypeScriptParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "TypeScript",
    file_extension: "ts",
    grammar_json_path: "contributing/parsers/typescript/node-types.json",
    example_file_path: "examples/typescript/comprehensive.ts",
    output_dir: "contributing/parsers/typescript",
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
                "import_alias",
                "default_type",
            ],
        ),
        (
            "CLASS NODES",
            vec![
                "class_declaration",
                "class_body",
                "method_definition",
                "method_signature",
                "public_field_definition",
                "private_field_definition",
                "abstract_method_signature",
                "class_heritage",
                "extends_clause",
                "implements_clause",
                "decorator",
                "computed_property_name",
            ],
        ),
        (
            "INTERFACE & TYPE NODES",
            vec![
                "interface_declaration",
                "interface_body",
                "type_alias_declaration",
                "enum_declaration",
                "enum_body",
                "enum_assignment",
                "type_parameter",
                "type_parameters",
                "type_arguments",
                "type_annotation",
                "type_predicate",
                "type_predicate_annotation",
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
                "required_parameter",
                "optional_parameter",
                "rest_parameter",
                "async_function",
                "async_arrow_function",
            ],
        ),
        (
            "TYPE SYSTEM NODES",
            vec![
                "union_type",
                "intersection_type",
                "conditional_type",
                "generic_type",
                "type_query",
                "index_type_query",
                "lookup_type",
                "literal_type",
                "template_literal_type",
                "flow_maybe_type",
                "parenthesized_type",
                "predefined_type",
                "type_identifier",
            ],
        ),
        (
            "JSX NODES",
            vec![
                "jsx_element",
                "jsx_self_closing_element",
                "jsx_opening_element",
                "jsx_closing_element",
                "jsx_fragment",
                "jsx_expression",
                "jsx_attribute",
                "jsx_namespace_name",
                "jsx_text",
            ],
        ),
        (
            "MODULE NODES",
            vec![
                "module",
                "internal_module",
                "module_body",
                "ambient_declaration",
                "namespace_declaration",
                "namespace_body",
                "export_assignment",
                "export_default_declaration",
            ],
        ),
    ]
}

#[test]
fn comprehensive_typescript_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "function main() {}\n",
        &node_categories(),
        |path| {
            let audit = TypeScriptParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
