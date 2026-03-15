//! PHP grammar audit.

use super::helpers::{
    AuditData, LanguageAuditConfig, run_comprehensive_analysis, run_tree_structure_analysis,
};
use codanna::parsing::php::audit::PhpParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "PHP",
    file_extension: "php",
    grammar_json_path: "contributing/parsers/php/node-types.json",
    example_file_path: "examples/php/comprehensive.php",
    output_dir: "contributing/parsers/php",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "NAMESPACE & IMPORT NODES",
            vec![
                "namespace_definition",
                "namespace_use_declaration",
                "namespace_use_clause",
                "namespace_use_group",
                "namespace_name",
                "namespace_aliasing_clause",
            ],
        ),
        (
            "CLASS & TRAIT NODES",
            vec![
                "class_declaration",
                "interface_declaration",
                "trait_declaration",
                "enum_declaration",
                "abstract_modifier",
                "final_modifier",
                "readonly_modifier",
                "base_clause",
                "class_interface_clause",
                "trait_use_clause",
                "trait_alias",
                "trait_precedence",
            ],
        ),
        (
            "METHOD & FUNCTION NODES",
            vec![
                "method_declaration",
                "function_definition",
                "arrow_function",
                "anonymous_function",
                "anonymous_function_creation_expression",
                "anonymous_function_use_clause",
                "formal_parameters",
                "simple_parameter",
                "property_promotion_parameter",
                "variadic_parameter",
                "reference_parameter",
                "typed_parameter",
            ],
        ),
        (
            "PROPERTY & CONSTANT NODES",
            vec![
                "property_declaration",
                "property_element",
                "const_declaration",
                "const_element",
                "class_constant_access_expression",
                "visibility_modifier",
                "static_modifier",
                "var_modifier",
            ],
        ),
        (
            "ATTRIBUTE NODES",
            vec![
                "attribute_list",
                "attribute_group",
                "attribute",
                "attribute_arguments",
                "named_argument",
            ],
        ),
        (
            "TYPE NODES",
            vec![
                "union_type",
                "intersection_type",
                "nullable_type",
                "primitive_type",
                "named_type",
                "optional_type",
                "bottom_type",
                "void_type",
                "mixed_type",
                "never_type",
            ],
        ),
    ]
}

fn ts_language() -> tree_sitter::Language {
    tree_sitter_php::LANGUAGE_PHP.into()
}

#[test]
fn comprehensive_php_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        ts_language(),
        "<?php\nclass Example {}\n",
        &node_categories(),
        |path| {
            let audit = PhpParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
fn generate_php_tree_structure() {
    run_tree_structure_analysis(&CONFIG, ts_language(), "<?php\nclass Example {}\n");
}
