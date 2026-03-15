//! C++ grammar audit.

use super::helpers::{
    AuditData, LanguageAuditConfig, run_comprehensive_analysis, run_tree_structure_analysis,
};
use codanna::parsing::cpp::audit::CppParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "C++",
    file_extension: "cpp",
    grammar_json_path: "contributing/parsers/cpp/node-types.json",
    example_file_path: "examples/cpp/comprehensive.cpp",
    output_dir: "contributing/parsers/cpp",
};

const FALLBACK_CODE: &str = "#include <iostream>\n\nint main() {\n    return 0;\n}\n";

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
            "NAMESPACE AND USING NODES",
            vec![
                "namespace_definition",
                "namespace_identifier",
                "using_declaration",
                "using_directive",
                "alias_declaration",
                "qualified_identifier",
                "scope_resolution",
                "nested_namespace_specifier",
            ],
        ),
        (
            "CLASS AND STRUCT NODES",
            vec![
                "class_specifier",
                "struct_specifier",
                "access_specifier",
                "field_declaration",
                "field_declaration_list",
                "field_identifier",
                "bitfield_clause",
                "base_class_clause",
                "virtual_specifier",
                "explicit_function_specifier",
            ],
        ),
        (
            "TEMPLATE-RELATED NODES",
            vec![
                "template_declaration",
                "template_instantiation",
                "template_type",
                "template_function",
                "template_method",
                "template_parameter_list",
                "type_parameter_declaration",
                "optional_parameter_declaration",
                "variadic_declaration",
                "template_template_parameter_declaration",
                "template_argument_list",
                "type_descriptor",
            ],
        ),
        (
            "FUNCTION-RELATED NODES",
            vec![
                "function_definition",
                "function_declarator",
                "function_type",
                "method_definition",
                "constructor_definition",
                "destructor_definition",
                "operator_overload",
                "parameter_declaration",
                "parameter_list",
                "variadic_parameter",
                "abstract_function_declarator",
                "call_expression",
                "argument_list",
                "trailing_return_type",
            ],
        ),
        (
            "INHERITANCE AND VIRTUAL NODES",
            vec![
                "virtual_function_specifier",
                "override_specifier",
                "final_specifier",
                "pure_virtual_function_definition",
                "virtual_specifier",
                "access_specifier",
            ],
        ),
        (
            "ENUM-RELATED NODES",
            vec![
                "enum_specifier",
                "scoped_enum_specifier",
                "enumerator",
                "enumerator_list",
            ],
        ),
        (
            "DECLARATION AND TYPEDEF NODES",
            vec![
                "declaration",
                "simple_declaration",
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
                "reference_declarator",
                "structured_binding_declarator",
            ],
        ),
        (
            "TYPE-RELATED NODES",
            vec![
                "primitive_type",
                "sized_type_specifier",
                "type_identifier",
                "pointer_type",
                "reference_type",
                "array_type",
                "auto",
                "decltype",
                "placeholder_type_specifier",
                "dependent_type",
                "qualified_type",
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
                "typeid_expression",
                "new_expression",
                "delete_expression",
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
                "lambda_expression",
                "lambda_capture_specifier",
                "parameter_pack_expansion",
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
                "for_range_loop",
                "do_statement",
                "break_statement",
                "continue_statement",
                "return_statement",
                "goto_statement",
                "try_statement",
                "catch_clause",
                "throw_statement",
            ],
        ),
    ]
}

fn ts_language() -> tree_sitter::Language {
    tree_sitter_cpp::LANGUAGE.into()
}

#[test]
fn comprehensive_cpp_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        ts_language(),
        FALLBACK_CODE,
        &node_categories(),
        |path| {
            let audit = CppParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
fn generate_cpp_tree_structure() {
    run_tree_structure_analysis(&CONFIG, ts_language(), FALLBACK_CODE);
}
