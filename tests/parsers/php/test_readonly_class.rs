//! PHP readonly class SymbolKind tests

use codanna::parsing::php::PhpParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind};

fn parse_php(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = PhpParser::new().expect("Failed to create PHP parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

#[test]
fn test_readonly_class_symbol_kind() {
    let code = r#"<?php
readonly class ImmutableUser {
    public function __construct(
        public string $name
    ) {}
}
"#;
    let symbols = parse_php(code);

    let user = symbols.iter().find(|s| s.name.as_ref() == "ImmutableUser");
    if let Some(sym) = user {
        eprintln!(
            "ImmutableUser kind: {:?}, signature: {:?}",
            sym.kind, sym.signature
        );
    }
    assert!(user.is_some(), "Should find ImmutableUser symbol");
    assert_eq!(
        user.unwrap().kind,
        SymbolKind::Class,
        "readonly class should be SymbolKind::Class"
    );
}
