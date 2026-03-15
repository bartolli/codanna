//! Go grammar audit.

use super::helpers::{
    AuditData, LanguageAuditConfig, run_comprehensive_analysis, run_tree_structure_analysis,
};
use codanna::parsing::go::audit::GoParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Go",
    file_extension: "go",
    grammar_json_path: "contributing/parsers/go/node-types.json",
    example_file_path: "examples/go/comprehensive.go",
    output_dir: "contributing/parsers/go",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "PACKAGE AND IMPORT NODES",
            vec![
                "package_clause",
                "package_identifier",
                "import_declaration",
                "import_spec",
                "import_spec_list",
                "interpreted_string_literal",
                "dot",
                "blank_identifier",
                "import_alias",
            ],
        ),
        (
            "STRUCT-RELATED NODES",
            vec![
                "type_declaration",
                "type_spec",
                "struct_type",
                "field_declaration",
                "field_declaration_list",
                "type_identifier",
                "field_identifier",
                "tag",
                "struct_field",
                "embedded_field",
            ],
        ),
        (
            "INTERFACE-RELATED NODES",
            vec![
                "interface_type",
                "method_elem",
                "method_spec",
                "method_spec_list",
                "type_elem",
                "type_constraint",
                "type_set",
                "embedded_interface",
            ],
        ),
        (
            "FUNCTION-RELATED NODES",
            vec![
                "function_declaration",
                "func_literal",
                "function_type",
                "method_declaration",
                "receiver",
                "parameter_declaration",
                "parameter_list",
                "result",
                "variadic_parameter_declaration",
                "type_parameter_declaration",
                "type_parameter_list",
            ],
        ),
        (
            "VARIABLE/CONSTANT NODES",
            vec![
                "var_declaration",
                "var_spec",
                "const_declaration",
                "const_spec",
                "short_var_declaration",
                "assignment_statement",
                "inc_statement",
                "dec_statement",
                "expression_list",
                "identifier_list",
            ],
        ),
        (
            "TYPE-RELATED NODES",
            vec![
                "type_alias",
                "pointer_type",
                "array_type",
                "slice_type",
                "map_type",
                "channel_type",
                "generic_type",
                "type_instantiation",
                "type_arguments",
                "type_parameter",
                "qualified_type",
            ],
        ),
    ]
}

fn ts_language() -> tree_sitter::Language {
    tree_sitter_go::LANGUAGE.into()
}

#[test]
fn comprehensive_go_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        ts_language(),
        "package main\n\nfunc main() {}\n",
        &node_categories(),
        |path| {
            let audit = GoParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
fn generate_go_tree_structure() {
    run_tree_structure_analysis(&CONFIG, ts_language(), "package main\n\nfunc main() {}\n");
}
