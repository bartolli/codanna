//! Lua grammar audit.

use super::helpers::{AuditData, LanguageAuditConfig, run_comprehensive_analysis};
use codanna::parsing::lua::audit::LuaParserAudit;

const CONFIG: LanguageAuditConfig = LanguageAuditConfig {
    language_name: "Lua",
    file_extension: "lua",
    grammar_json_path: "contributing/parsers/lua/node-types.json",
    example_file_path: "examples/lua/comprehensive.lua",
    output_dir: "contributing/parsers/lua",
};

fn node_categories() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "FUNCTION NODES",
            vec![
                "function_declaration",
                "function_definition",
                "function_call",
                "parameters",
                "return_statement",
            ],
        ),
        (
            "VARIABLE NODES",
            vec![
                "variable_declaration",
                "assignment_statement",
                "variable_list",
                "expression_list",
                "identifier",
            ],
        ),
        (
            "TABLE NODES",
            vec![
                "table_constructor",
                "field",
                "dot_index_expression",
                "bracket_index_expression",
                "method_index_expression",
            ],
        ),
        (
            "CONTROL FLOW NODES",
            vec![
                "if_statement",
                "elseif_statement",
                "else_statement",
                "for_statement",
                "for_in_statement",
                "while_statement",
                "repeat_statement",
                "do_statement",
                "block",
            ],
        ),
        (
            "EXPRESSION NODES",
            vec![
                "binary_expression",
                "unary_expression",
                "parenthesized_expression",
                "string",
                "number",
                "true",
                "false",
                "nil",
            ],
        ),
        ("COMMENT NODES", vec!["comment"]),
    ]
}

#[test]
fn comprehensive_lua_analysis() {
    run_comprehensive_analysis(
        &CONFIG,
        tree_sitter_lua::LANGUAGE.into(),
        "-- Lua module\nlocal M = {}\nreturn M\n",
        &node_categories(),
        |path| {
            let audit = LuaParserAudit::audit_file(path).map_err(|e| e.to_string())?;
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
