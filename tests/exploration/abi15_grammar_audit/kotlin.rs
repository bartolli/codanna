//! Kotlin grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use super::tree_sitter_kotlin;
use codanna::parsing::kotlin::audit::KotlinParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Kotlin",
    file_extension: "kt",
    grammar_json_path: "contributing/parsers/kotlin/node-types.json",
    example_file_path: "examples/kotlin/comprehensive.kt",
    output_dir: "contributing/parsers/kotlin",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "CLASS & OBJECT DECLARATIONS",
            vec![
                "class_declaration",
                "object_declaration",
                "interface",
                "enum_class",
                "data_class",
                "sealed_class",
                "companion_object",
            ],
        ),
        (
            "FUNCTION DECLARATIONS",
            vec![
                "function_declaration",
                "primary_constructor",
                "secondary_constructor",
                "anonymous_function",
                "lambda_literal",
            ],
        ),
        (
            "PROPERTY & VARIABLE DECLARATIONS",
            vec![
                "property_declaration",
                "variable_declaration",
                "class_parameter",
                "function_value_parameter",
            ],
        ),
        (
            "TYPE SYSTEM",
            vec![
                "type_alias",
                "type_reference",
                "nullable_type",
                "user_type",
                "function_type",
                "type_projection",
            ],
        ),
        (
            "MODIFIERS & VISIBILITY",
            vec![
                "modifiers",
                "visibility_modifier",
                "inheritance_modifier",
                "function_modifier",
                "property_modifier",
            ],
        ),
        (
            "EXPRESSIONS",
            vec![
                "call_expression",
                "string_literal",
                "integer_literal",
                "boolean_literal",
                "binary_expression",
                "prefix_expression",
            ],
        ),
    ]
}

#[test]
fn comprehensive_kotlin_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_kotlin::language(),
        "fun main() {}\n",
        &node_categories(),
        |path| {
            let audit = KotlinParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
