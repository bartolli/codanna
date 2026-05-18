use codanna::FileId;
use codanna::parsing::{LanguageParser, swift::SwiftParser};
use codanna::symbol::ScopeContext;
use codanna::types::SymbolCounter;

fn parse(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = SwiftParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

fn assert_class_member(symbols: &[codanna::Symbol], name: &str, parent: &str) {
    let sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == name)
        .unwrap_or_else(|| panic!("symbol {name} not extracted"));
    match &sym.scope_context {
        Some(ScopeContext::ClassMember { class_name }) => {
            assert_eq!(
                class_name.as_deref(),
                Some(parent),
                "symbol {name} class_name"
            );
        }
        other => panic!("symbol {name}: expected ClassMember{{{parent}}}, got {other:?}"),
    }
}

#[test]
fn test_property_in_class_carries_classmember() {
    let code = r#"
class Cache {
    var size: Int = 0
}
"#;
    assert_class_member(&parse(code), "size", "Cache");
}

#[test]
fn test_init_in_class_carries_classmember() {
    let code = r#"
class Cache {
    init() {}
}
"#;
    assert_class_member(&parse(code), "init", "Cache");
}

#[test]
fn test_typealias_in_class_carries_classmember() {
    let code = r#"
class Cache {
    typealias Key = String
}
"#;
    assert_class_member(&parse(code), "Key", "Cache");
}

#[test]
fn test_deinit_in_class_carries_classmember() {
    let code = r#"
class Cache {
    deinit {}
}
"#;
    assert_class_member(&parse(code), "deinit", "Cache");
}

#[test]
fn test_top_level_emits_module() {
    let code = r#"
class TopLevel {}
typealias Alias = Int
"#;
    let symbols = parse(code);
    for name in ["TopLevel", "Alias"] {
        let sym = symbols
            .iter()
            .find(|s| s.name.as_ref() == name)
            .unwrap_or_else(|| panic!("symbol {name} not extracted"));
        assert!(
            matches!(sym.scope_context, Some(ScopeContext::Module)),
            "symbol {name}: expected Module, got {:?}",
            sym.scope_context
        );
    }
}

#[test]
fn test_nested_class_carries_classmember() {
    let code = r#"
class Outer {
    class Inner {}
}
"#;
    assert_class_member(&parse(code), "Inner", "Outer");
}

#[test]
fn test_enum_case_carries_classmember() {
    let code = r#"
enum Color {
    case red
}
"#;
    assert_class_member(&parse(code), "red", "Color");
}

#[test]
fn test_subscript_in_class_carries_classmember() {
    let code = r#"
class Table {
    subscript(idx: Int) -> Int { return 0 }
}
"#;
    assert_class_member(&parse(code), "subscript", "Table");
}
