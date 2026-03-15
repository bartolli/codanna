//! C grammar audit.

use super::helpers::{
    AuditData, LanguageAuditConfig, run_comprehensive_analysis, run_tree_structure_analysis,
};
use codanna::parsing::c::audit::CParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "C",
    file_extension: "c",
    grammar_json_path: "contributing/parsers/c/node-types.json",
    example_file_path: "examples/c/comprehensive.c",
    output_dir: "contributing/parsers/c",
};

const FALLBACK_CODE: &str = "#include <stdio.h>\n\nint main(void) {\n    return 0;\n}\n";

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "PREPROCESSOR AND INCLUDE NODES",
            vec![
                "translation_unit",
                "preproc_include",
                "preproc_define",
                "preproc_function_def",
                "preproc_call",
                "preproc_def",
                "preproc_if",
                "preproc_ifdef",
                "preproc_ifndef",
                "preproc_else",
                "preproc_elif",
                "preproc_endif",
                "system_lib_string",
                "string_literal",
                "identifier",
            ],
        ),
        (
            "FUNCTION-RELATED NODES",
            vec![
                "function_definition",
                "function_declarator",
                "function_type",
                "parameter_declaration",
                "parameter_list",
                "variadic_parameter",
                "abstract_function_declarator",
                "call_expression",
                "argument_list",
            ],
        ),
        (
            "STRUCT AND UNION NODES",
            vec![
                "struct_specifier",
                "union_specifier",
                "field_declaration",
                "field_declaration_list",
                "field_identifier",
                "bitfield_clause",
                "field_designator",
                "init_declarator",
            ],
        ),
        (
            "ENUM-RELATED NODES",
            vec!["enum_specifier", "enumerator", "enumerator_list"],
        ),
        (
            "DECLARATION AND TYPEDEF NODES",
            vec![
                "declaration",
                "typedef_declaration",
                "type_definition",
                "declarator",
                "init_declarator",
                "storage_class_specifier",
                "type_qualifier",
                "pointer_declarator",
                "array_declarator",
                "abstract_pointer_declarator",
                "abstract_array_declarator",
                "type_descriptor",
            ],
        ),
        (
            "TYPE-RELATED NODES",
            vec![
                "primitive_type",
                "sized_type_specifier",
                "type_identifier",
                "pointer_type",
                "array_type",
                "struct_type",
                "union_type",
                "enum_type",
            ],
        ),
        (
            "STATEMENT NODES",
            vec![
                "compound_statement",
                "expression_statement",
                "labeled_statement",
                "if_statement",
                "switch_statement",
                "case_statement",
                "while_statement",
                "for_statement",
                "do_statement",
                "break_statement",
                "continue_statement",
                "return_statement",
                "goto_statement",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "assignment_expression",
                "update_expression",
                "cast_expression",
                "sizeof_expression",
                "alignof_expression",
                "offsetof_expression",
                "generic_expression",
                "subscript_expression",
                "field_expression",
                "comma_expression",
                "conditional_expression",
                "binary_expression",
                "unary_expression",
                "postfix_expression",
                "parenthesized_expression",
                "initializer_list",
                "initializer_pair",
            ],
        ),
    ]
}

fn ts_language() -> tree_sitter::Language {
    tree_sitter_c::LANGUAGE.into()
}

#[test]
fn comprehensive_c_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        ts_language(),
        FALLBACK_CODE,
        &node_categories(),
        |path| {
            let audit = CParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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

#[test]
fn generate_c_tree_structure() {
    run_tree_structure_analysis(&CONFIG, ts_language(), FALLBACK_CODE);
}
