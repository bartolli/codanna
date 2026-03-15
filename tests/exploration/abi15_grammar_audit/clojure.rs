//! Clojure grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::clojure::audit::ClojureParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Clojure",
    file_extension: "clj",
    grammar_json_path: "contributing/parsers/clojure/node-types.json",
    example_file_path: "examples/clojure/comprehensive.clj",
    output_dir: "contributing/parsers/clojure",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "DEFINITION NODES",
            vec!["list_lit", "sym_lit", "sym_name", "sym_ns"],
        ),
        (
            "LITERAL NODES",
            vec![
                "num_lit", "str_lit", "bool_lit", "nil_lit", "kwd_lit", "kwd_name", "char_lit",
            ],
        ),
        ("COLLECTION NODES", vec!["vec_lit", "map_lit", "set_lit"]),
        (
            "SPECIAL FORM NODES",
            vec![
                "meta_lit",
                "syn_quoting_lit",
                "unquoting_lit",
                "unquote_splicing_lit",
                "derefing_lit",
                "anon_fn_lit",
            ],
        ),
        ("STRUCTURAL NODES", vec!["source", "comment"]),
    ]
}

#[test]
fn comprehensive_clojure_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_clojure_orchard::LANGUAGE.into(),
        "(ns test.core)\n(defn main [])\n",
        &node_categories(),
        |path| {
            let audit = ClojureParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
