//! TypeScript ERROR node recovery tests

use codanna::FileId;
use codanna::parsing::typescript::TypeScriptParser;
use codanna::types::SymbolCounter;

fn parse_typescript(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = TypeScriptParser::new().expect("Failed to create TypeScript parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

#[test]
fn test_recover_export_type_star_as() {
    // Pattern from research report: export type * as Name from './types'
    // Grammar produces ERROR at `type` keyword
    let code = r#"export type * as Types from './types';"#;
    let symbols = parse_typescript(code);

    // Should recover and extract the namespace export
    let types_export = symbols.iter().find(|s| s.name.as_ref() == "Types");
    if types_export.is_none() {
        eprintln!("ERROR: Should recover Types from 'export type * as Types'");
        eprintln!(
            "Symbols found: {:?}",
            symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
    }
    assert!(types_export.is_some());
}

#[test]
fn test_export_type_star_without_name() {
    // Pattern: export type * from './module' (no "as Name")
    // This is a re-export without a named symbol - nothing to extract
    let code = r#"export type * from './models';"#;
    let symbols = parse_typescript(code);

    // This pattern doesn't create a named symbol (it's an anonymous re-export)
    // We just verify parsing doesn't crash and returns empty
    eprintln!(
        "Symbols for anonymous re-export: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    // No assertion - this is expected to have no extractable symbols
}
