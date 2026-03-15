//! GDScript grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::gdscript::audit::GdscriptParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "GDScript",
    file_extension: "gd",
    grammar_json_path: "contributing/parsers/gdscript/node-types.json",
    example_file_path: "examples/gdscript/comprehensive.gd",
    output_dir: "contributing/parsers/gdscript",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "SCRIPT DECLARATIONS",
            vec![
                "class_name_statement",
                "class_definition",
                "extends_statement",
                "enum_definition",
            ],
        ),
        (
            "SIGNALS & VARIABLES",
            vec![
                "signal_statement",
                "variable_statement",
                "const_statement",
                "export_variable_statement",
            ],
        ),
        (
            "FUNCTIONS",
            vec![
                "constructor_definition",
                "function_definition",
                "parameters",
                "block",
            ],
        ),
        (
            "CONTROL FLOW",
            vec![
                "if_statement",
                "while_statement",
                "for_statement",
                "match_statement",
                "pattern_section",
            ],
        ),
        (
            "EXPRESSIONS",
            vec!["assignment", "call", "binary_operator", "unary_operator"],
        ),
    ]
}

#[test]
fn comprehensive_gdscript_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_gdscript::LANGUAGE.into(),
        "class_name Temp\nextends Node\n",
        &node_categories(),
        |path| {
            let audit = GdscriptParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
