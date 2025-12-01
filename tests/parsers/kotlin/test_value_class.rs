//! Kotlin value class SymbolKind tests

use codanna::parsing::LanguageParser;
use codanna::parsing::kotlin::KotlinParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind};

fn parse_kotlin(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = KotlinParser::new().expect("Failed to create Kotlin parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

#[test]
fn test_value_class_symbol_kind() {
    // Kotlin value class should be extracted as Class, not Function
    let code = r#"value class UserId(val id: Int)"#;
    let symbols = parse_kotlin(code);

    let user_id = symbols.iter().find(|s| s.name.as_ref() == "UserId");
    if let Some(sym) = user_id {
        eprintln!(
            "UserId kind: {:?}, signature: {:?}",
            sym.kind, sym.signature
        );
    }
    assert!(user_id.is_some(), "Should find UserId symbol");
    assert_eq!(
        user_id.unwrap().kind,
        SymbolKind::Class,
        "value class should be SymbolKind::Class"
    );
}

#[test]
fn test_value_class_with_visibility() {
    // private value class should also be Class
    let code = r#"private value class Email(val address: String)"#;
    let symbols = parse_kotlin(code);

    let email = symbols.iter().find(|s| s.name.as_ref() == "Email");
    if let Some(sym) = email {
        eprintln!("Email kind: {:?}, signature: {:?}", sym.kind, sym.signature);
    }
    assert!(email.is_some(), "Should find Email symbol");
    assert_eq!(
        email.unwrap().kind,
        SymbolKind::Class,
        "private value class should be SymbolKind::Class"
    );
}
