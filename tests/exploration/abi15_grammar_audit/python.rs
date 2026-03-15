//! Python grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::python::audit::PythonParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Python",
    file_extension: "py",
    grammar_json_path: "contributing/parsers/python/node-types.json",
    example_file_path: "examples/python/comprehensive.py",
    output_dir: "contributing/parsers/python",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "IMPORT NODES",
            vec![
                "import_statement",
                "import_from_statement",
                "aliased_import",
                "dotted_name",
                "relative_import",
                "wildcard_import",
            ],
        ),
        (
            "CLASS & FUNCTION NODES",
            vec![
                "class_definition",
                "function_definition",
                "decorated_definition",
                "lambda",
                "parameters",
                "default_parameter",
                "typed_parameter",
                "typed_default_parameter",
                "list_splat_parameter",
                "dictionary_splat_parameter",
            ],
        ),
        (
            "ASYNC NODES",
            vec![
                "async_function_definition",
                "async_for_statement",
                "async_with_statement",
                "await",
                "async_for_in_clause",
                "async_comprehension",
            ],
        ),
        (
            "STATEMENT NODES",
            vec![
                "if_statement",
                "elif_clause",
                "else_clause",
                "while_statement",
                "for_statement",
                "try_statement",
                "except_clause",
                "finally_clause",
                "with_statement",
                "match_statement",
                "case_clause",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "assignment",
                "augmented_assignment",
                "annotated_assignment",
                "binary_operator",
                "unary_operator",
                "comparison_operator",
                "conditional_expression",
                "named_expression",
                "as_pattern",
            ],
        ),
        (
            "COMPREHENSION NODES",
            vec![
                "list_comprehension",
                "dictionary_comprehension",
                "set_comprehension",
                "generator_expression",
                "for_in_clause",
                "if_clause",
            ],
        ),
        (
            "TYPE NODES",
            vec![
                "type",
                "type_alias_statement",
                "type_parameter",
                "type_comment",
                "generic_type",
                "union_type",
                "constrained_type",
                "member_type",
            ],
        ),
    ]
}

#[test]
fn comprehensive_python_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_python::LANGUAGE.into(),
        "def main():\n    pass\n",
        &node_categories(),
        |path| {
            let audit = PythonParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
