//! C# grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::csharp::audit::CSharpParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "C#",
    file_extension: "cs",
    grammar_json_path: "contributing/parsers/csharp/node-types.json",
    example_file_path: "examples/csharp/comprehensive.cs",
    output_dir: "contributing/parsers/csharp",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "NAMESPACE & USING NODES",
            vec![
                "using_directive",
                "namespace_declaration",
                "file_scoped_namespace_declaration",
                "qualified_name",
            ],
        ),
        (
            "TYPE DEFINITION NODES",
            vec![
                "class_declaration",
                "struct_declaration",
                "interface_declaration",
                "enum_declaration",
                "record_declaration",
                "delegate_declaration",
                "type_parameter_list",
                "type_parameter",
                "type_parameter_constraint",
            ],
        ),
        (
            "MEMBER NODES",
            vec![
                "method_declaration",
                "property_declaration",
                "field_declaration",
                "event_declaration",
                "indexer_declaration",
                "constructor_declaration",
                "destructor_declaration",
                "operator_declaration",
                "accessor_declaration",
            ],
        ),
        (
            "STATEMENT NODES",
            vec![
                "if_statement",
                "switch_statement",
                "switch_expression",
                "for_statement",
                "foreach_statement",
                "while_statement",
                "do_statement",
                "try_statement",
                "catch_clause",
                "finally_clause",
                "using_statement",
                "lock_statement",
                "return_statement",
                "throw_statement",
                "yield_statement",
                "break_statement",
                "continue_statement",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "invocation_expression",
                "member_access_expression",
                "assignment_expression",
                "binary_expression",
                "prefix_unary_expression",
                "postfix_unary_expression",
                "conditional_expression",
                "lambda_expression",
                "object_creation_expression",
                "array_creation_expression",
                "element_access_expression",
                "cast_expression",
                "as_expression",
                "is_expression",
                "await_expression",
                "query_expression",
                "interpolated_string_expression",
            ],
        ),
        (
            "ASYNC & LINQ NODES",
            vec![
                "query_expression",
                "from_clause",
                "select_clause",
                "where_clause",
                "order_by_clause",
                "group_clause",
                "join_clause",
                "await_expression",
            ],
        ),
        (
            "PATTERN NODES",
            vec![
                "switch_expression_arm",
                "when_clause",
                "declaration_pattern",
                "recursive_pattern",
                "var_pattern",
                "discard_pattern",
            ],
        ),
        (
            "TYPE NODES",
            vec![
                "predefined_type",
                "nullable_type",
                "array_type",
                "tuple_type",
                "pointer_type",
                "generic_name",
            ],
        ),
        (
            "LITERAL NODES",
            vec![
                "integer_literal",
                "real_literal",
                "string_literal",
                "verbatim_string_literal",
                "interpolated_string_text",
                "character_literal",
                "boolean_literal",
                "null_literal",
            ],
        ),
        ("COMMENT & DOCUMENTATION NODES", vec!["comment"]),
    ]
}

#[test]
fn comprehensive_csharp_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_c_sharp::LANGUAGE.into(),
        "class Program { static void Main() {} }",
        &node_categories(),
        |path| {
            let audit = CSharpParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
