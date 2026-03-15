//! Java grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::java::audit::JavaParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Java",
    file_extension: "java",
    grammar_json_path: "contributing/parsers/java/node-types.json",
    example_file_path: "examples/java/comprehensive.java",
    output_dir: "contributing/parsers/java",
};

#[test]
fn comprehensive_java_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_java::LANGUAGE.into(),
        "class Main {}\n",
        &[],
        |path| {
            let audit = JavaParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
