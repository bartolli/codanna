//! Nix grammar audit.

use super::helpers::{run_comprehensive_analysis, AuditData, LanguageAuditConfig};
use codanna::parsing::nix::audit::NixParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Nix",
    file_extension: "nix",
    grammar_json_path: "contributing/parsers/nix/node-types.json",
    example_file_path: "examples/nix/comprehensive.nix",
    output_dir: "contributing/parsers/nix",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("ROOT NODES", vec!["source_code"]),
        (
            "BINDING NODES",
            vec!["binding_set", "binding", "attrpath", "identifier"],
        ),
        (
            "ATTRSET NODES",
            vec!["attrset_expression", "rec_attrset_expression"],
        ),
        (
            "FUNCTION NODES",
            vec![
                "function_expression",
                "formals",
                "formal",
                "apply_expression",
            ],
        ),
        (
            "SCOPE NODES",
            vec![
                "let_expression",
                "with_expression",
                "inherit",
                "inherit_from",
                "inherited_attrs",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "if_expression",
                "assert_expression",
                "select_expression",
                "binary_expression",
                "parenthesized_expression",
                "list_expression",
                "variable_expression",
            ],
        ),
        (
            "LITERAL NODES",
            vec![
                "integer_expression",
                "string_expression",
                "indented_string_expression",
                "path_expression",
                "spath_expression",
                "interpolation",
            ],
        ),
        ("COMMENT NODES", vec!["comment"]),
    ]
}

#[test]
fn comprehensive_nix_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_nix::LANGUAGE.into(),
        "{ x = 1; f = a: a + 1; }\n",
        &node_categories(),
        |path| {
            let audit = NixParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
