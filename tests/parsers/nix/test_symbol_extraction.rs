use codanna::parsing::Language;
use codanna::parsing::LanguageParser;
use codanna::parsing::nix::NixParser;
use codanna::types::{FileId, SymbolCounter};
use codanna::SymbolKind;

fn parse(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = NixParser::new().expect("Failed to create NixParser");
    let mut counter = SymbolCounter::new();
    let file_id = FileId::new(1).unwrap();
    parser.parse(code, file_id, &mut counter)
}

// ── language identity ────────────────────────────────────────────────────────

#[test]
fn test_nix_language_identity() {
    let parser = NixParser::new().unwrap();
    assert_eq!(parser.language(), Language::Nix);
}

// ── basic attrset bindings ───────────────────────────────────────────────────

#[test]
fn test_attrset_simple_values() {
    let symbols = parse(r#"{ host = "localhost"; port = 8080; debug = false; }"#);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"host"), "expected host, got {names:?}");
    assert!(names.contains(&"port"), "expected port, got {names:?}");
    assert!(names.contains(&"debug"), "expected debug, got {names:?}");
}

#[test]
fn test_attrset_function_binding_kind() {
    let symbols = parse(r#"{ add = a: b: a + b; }"#);
    let add = symbols.iter().find(|s| s.name.as_ref() == "add").unwrap();
    assert_eq!(add.kind, SymbolKind::Function, "add should be Function");
}

#[test]
fn test_attrset_value_binding_kind() {
    let symbols = parse(r#"{ x = 42; }"#);
    let x = symbols.iter().find(|s| s.name.as_ref() == "x").unwrap();
    assert_eq!(x.kind, SymbolKind::Variable, "x should be Variable");
}

// ── lambda parameters ────────────────────────────────────────────────────────

#[test]
fn test_simple_lambda_param() {
    let symbols = parse(r#"{ f = x: x + 1; }"#);
    let params: Vec<&str> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Parameter)
        .map(|s| s.name.as_ref())
        .collect();
    assert!(params.contains(&"x"), "expected param x, got {params:?}");
}

#[test]
fn test_formals_params() {
    let symbols = parse(r#"{ f = { a, b ? 0, c ? 1 }: a + b + c; }"#);
    let params: Vec<&str> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Parameter)
        .map(|s| s.name.as_ref())
        .collect();
    assert!(params.contains(&"a"), "expected param a, got {params:?}");
    assert!(params.contains(&"b"), "expected param b, got {params:?}");
    assert!(params.contains(&"c"), "expected param c, got {params:?}");
}

#[test]
fn test_curried_lambda_params() {
    let symbols = parse(r#"{ add = a: b: a + b; }"#);
    let params: Vec<&str> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Parameter)
        .map(|s| s.name.as_ref())
        .collect();
    assert!(params.contains(&"a"), "expected param a, got {params:?}");
    assert!(params.contains(&"b"), "expected param b, got {params:?}");
}

// ── let expressions ──────────────────────────────────────────────────────────

#[test]
fn test_let_bindings() {
    let symbols = parse(r#"let x = 1; y = 2; in x + y"#);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"x"), "expected x, got {names:?}");
    assert!(names.contains(&"y"), "expected y, got {names:?}");
}

#[test]
fn test_let_function_binding() {
    let symbols = parse(r#"let double = x: x * 2; in double 5"#);
    let double = symbols.iter().find(|s| s.name.as_ref() == "double").unwrap();
    assert_eq!(double.kind, SymbolKind::Function);
}

// ── rec attrset ──────────────────────────────────────────────────────────────

#[test]
fn test_rec_attrset_bindings() {
    let symbols = parse(r#"rec { base = "/var"; data = "${base}/data"; }"#);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"base"), "expected base, got {names:?}");
    assert!(names.contains(&"data"), "expected data, got {names:?}");
}

// ── inherit ──────────────────────────────────────────────────────────────────

#[test]
fn test_inherit_emits_variables() {
    let symbols = parse(r#"{ inherit stdenv fetchurl; }"#);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"stdenv"), "expected stdenv, got {names:?}");
    assert!(names.contains(&"fetchurl"), "expected fetchurl, got {names:?}");
}

#[test]
fn test_inherit_from_emits_variables() {
    let symbols = parse(r#"{ inherit (pkgs) stdenv fetchurl; }"#);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"stdenv"), "expected stdenv, got {names:?}");
    assert!(names.contains(&"fetchurl"), "expected fetchurl, got {names:?}");
}

// ── nested attrsets ──────────────────────────────────────────────────────────

#[test]
fn test_nested_attrset_outer_binding() {
    let symbols = parse(r#"{ config = { host = "localhost"; port = 8080; }; }"#);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"config"), "expected config, got {names:?}");
    // inner bindings also visible
    assert!(names.contains(&"host"), "expected host, got {names:?}");
    assert!(names.contains(&"port"), "expected port, got {names:?}");
}

// ── fixture files ────────────────────────────────────────────────────────────

#[test]
fn test_basic_fixture() {
    let code = include_str!("../../fixtures/nix/basic.nix");
    let symbols = parse(code);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();

    assert!(names.contains(&"host"), "expected host");
    assert!(names.contains(&"add"), "expected add");
    assert!(names.contains(&"config"), "expected config");

    let add = symbols.iter().find(|s| s.name.as_ref() == "add").unwrap();
    assert_eq!(add.kind, SymbolKind::Function);
}

#[test]
fn test_functions_fixture() {
    let code = include_str!("../../fixtures/nix/functions.nix");
    let symbols = parse(code);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();

    assert!(names.contains(&"add"), "expected add");
    assert!(names.contains(&"mkService"), "expected mkService");
    assert!(names.contains(&"compose"), "expected compose");
}
