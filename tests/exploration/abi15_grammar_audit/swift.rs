//! Swift grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::swift::audit::SwiftParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Swift",
    file_extension: "swift",
    grammar_json_path: "contributing/parsers/swift/node-types.json",
    example_file_path: "examples/swift/comprehensive.swift",
    output_dir: "contributing/parsers/swift",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "Type Declarations",
            vec![
                "class_declaration",
                "protocol_declaration",
                "struct_declaration",
                "enum_declaration",
                "actor_declaration",
                "extension_declaration",
            ],
        ),
        (
            "Member Declarations",
            vec![
                "function_declaration",
                "init_declaration",
                "deinit_declaration",
                "property_declaration",
                "subscript_declaration",
                "typealias_declaration",
            ],
        ),
        ("Enum Members", vec!["enum_entry"]),
        ("Imports", vec!["import_declaration"]),
        ("Modifiers", vec!["modifiers", "visibility_modifier"]),
        (
            "Inheritance",
            vec!["inheritance_specifier", "type_constraint"],
        ),
    ]
}

#[test]
#[ignore]
fn comprehensive_swift_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_swift::LANGUAGE.into(),
        "func main() {}\n",
        &node_categories(),
        |path| {
            let audit = SwiftParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
