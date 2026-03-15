//! Rust grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::rust::audit::RustParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Rust",
    file_extension: "rs",
    grammar_json_path: "contributing/parsers/rust/node-types.json",
    example_file_path: "examples/rust/comprehensive.rs",
    output_dir: "contributing/parsers/rust",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "MODULE & USE NODES",
            vec![
                "mod_item",
                "use_declaration",
                "use_clause",
                "use_list",
                "use_as_clause",
                "use_wildcard",
                "scoped_use_list",
                "extern_crate",
            ],
        ),
        (
            "STRUCT & ENUM NODES",
            vec![
                "struct_item",
                "enum_item",
                "enum_variant",
                "enum_variant_list",
                "field_declaration",
                "field_declaration_list",
                "ordered_field_declaration_list",
                "struct_expression",
                "struct_pattern",
                "tuple_struct_pattern",
            ],
        ),
        (
            "TRAIT & IMPL NODES",
            vec![
                "trait_item",
                "impl_item",
                "associated_type",
                "trait_bounds",
                "where_clause",
                "where_predicate",
                "higher_ranked_trait_bound",
                "removed_trait_bound",
                "trait_type",
                "abstract_type",
            ],
        ),
        (
            "FUNCTION NODES",
            vec![
                "function_item",
                "function_signature_item",
                "parameters",
                "parameter",
                "self_parameter",
                "variadic_parameter",
                "optional_type_parameter",
                "closure_expression",
                "closure_parameters",
                "async_block",
            ],
        ),
        (
            "TYPE NODES",
            vec![
                "type_alias",
                "type_item",
                "generic_type",
                "generic_type_with_turbofish",
                "function_type",
                "tuple_type",
                "array_type",
                "pointer_type",
                "reference_type",
                "empty_type",
                "dynamic_type",
                "bounded_type",
            ],
        ),
        (
            "PATTERN NODES",
            vec![
                "tuple_pattern",
                "slice_pattern",
                "tuple_struct_pattern",
                "struct_pattern",
                "remaining_field_pattern",
                "mut_pattern",
                "range_pattern",
                "ref_pattern",
                "captured_pattern",
                "reference_pattern",
                "or_pattern",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "macro_invocation",
                "macro_definition",
                "macro_rule",
                "token_tree",
                "match_expression",
                "match_arm",
                "match_pattern",
                "if_expression",
                "while_expression",
                "loop_expression",
                "for_expression",
                "const_item",
                "static_item",
                "attribute_item",
                "inner_attribute_item",
            ],
        ),
    ]
}

#[test]
fn comprehensive_rust_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_rust::LANGUAGE.into(),
        "fn main() {}\n",
        &node_categories(),
        |path| {
            let audit = RustParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
