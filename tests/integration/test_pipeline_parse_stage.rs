//! Integration tests for the parallel pipeline parse stage.
//!
//! Tests the parse stage in isolation (no Tantivy, no indexing).

use codanna::indexing::file_info::calculate_hash;
use codanna::indexing::pipeline::{FileContent, init_parser_cache, parse_file};
use codanna::{Settings, SymbolKind};
use std::sync::Arc;

/// Parse Rust code and verify specific symbols are extracted.
#[test]
fn test_parse_rust_extracts_symbols() {
    let settings = Arc::new(Settings::default());
    init_parser_cache(settings.clone());

    let content = r#"
pub fn greet(name: &str) -> String {
    format!("Hello, {name}")
}

pub struct Person {
    name: String,
    age: u32,
}

impl Person {
    pub fn new(name: String) -> Self {
        Self { name, age: 0 }
    }
}

trait Greeter {
    fn greet(&self);
}
"#;

    let file_content = FileContent::new(
        "test.rs".into(),
        content.to_string(),
        calculate_hash(content),
    );

    let parsed = parse_file(file_content, &settings).expect("Parse should succeed");

    // Print what we found for verification
    println!("Symbols found:");
    for sym in &parsed.raw_symbols {
        println!(
            "  - {} ({:?}) at lines {}-{}",
            sym.name, sym.kind, sym.range.start_line, sym.range.end_line
        );
    }

    // Verify specific symbols exist with correct kinds
    let symbols: Vec<_> = parsed
        .raw_symbols
        .iter()
        .map(|s| (s.name.as_ref(), s.kind))
        .collect();

    assert!(
        symbols.contains(&("greet", SymbolKind::Function)),
        "Expected function 'greet', found: {symbols:?}"
    );
    assert!(
        symbols.contains(&("Person", SymbolKind::Struct)),
        "Expected struct 'Person', found: {symbols:?}"
    );
    assert!(
        symbols.contains(&("new", SymbolKind::Method)),
        "Expected method 'new', found: {symbols:?}"
    );
    assert!(
        symbols.contains(&("Greeter", SymbolKind::Trait)),
        "Expected trait 'Greeter', found: {symbols:?}"
    );
}

/// Parse Rust code and verify relationships are extracted with correct structure.
#[test]
fn test_parse_rust_extracts_relationships() {
    let settings = Arc::new(Settings::default());
    init_parser_cache(settings.clone());

    let content = r#"
fn caller() {
    callee();
    helper();
}

fn callee() {}
fn helper() {}

struct Foo;

impl Foo {
    fn method(&self) {
        self.internal();
    }

    fn internal(&self) {}
}
"#;

    let file_content = FileContent::new(
        "test_rels.rs".into(),
        content.to_string(),
        calculate_hash(content),
    );

    let parsed = parse_file(file_content, &settings).expect("Parse should succeed");

    // Print relationships for verification
    println!("Relationships found:");
    for rel in &parsed.raw_relationships {
        println!(
            "  - {} --{:?}--> {} (at line {})",
            rel.from_name, rel.kind, rel.to_name, rel.from_range.start_line
        );
    }

    // Verify specific relationships exist
    let has_caller_to_callee = parsed
        .raw_relationships
        .iter()
        .any(|r| r.from_name.as_ref() == "caller" && r.to_name.as_ref() == "callee");
    let has_caller_to_helper = parsed
        .raw_relationships
        .iter()
        .any(|r| r.from_name.as_ref() == "caller" && r.to_name.as_ref() == "helper");
    let has_method_to_internal = parsed
        .raw_relationships
        .iter()
        .any(|r| r.from_name.as_ref() == "method" && r.to_name.as_ref() == "internal");

    assert!(
        has_caller_to_callee,
        "Expected caller -> callee relationship"
    );
    assert!(
        has_caller_to_helper,
        "Expected caller -> helper relationship"
    );
    assert!(
        has_method_to_internal,
        "Expected method -> internal relationship"
    );

    // Verify from_range is populated (critical for disambiguation)
    for rel in &parsed.raw_relationships {
        assert!(
            rel.from_range.start_line > 0,
            "Relationship from_range should have valid line number"
        );
    }
}

/// Parse Rust code and verify imports are extracted.
#[test]
fn test_parse_rust_extracts_imports() {
    let settings = Arc::new(Settings::default());
    init_parser_cache(settings.clone());

    let content = r#"
use std::collections::HashMap;
use crate::module::SomeType;
use super::other::Thing as Alias;

fn main() {}
"#;

    let file_content = FileContent::new(
        "test_imports.rs".into(),
        content.to_string(),
        calculate_hash(content),
    );

    let parsed = parse_file(file_content, &settings).expect("Parse should succeed");

    // Print imports for verification
    println!("Imports found:");
    for imp in &parsed.raw_imports {
        println!(
            "  - {} (alias: {:?}, glob: {})",
            imp.path, imp.alias, imp.is_glob
        );
    }

    // Verify specific imports
    let paths: Vec<_> = parsed.raw_imports.iter().map(|i| i.path.as_str()).collect();

    assert!(
        paths.iter().any(|p| p.contains("HashMap")),
        "Expected HashMap import, found: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.contains("SomeType")),
        "Expected SomeType import, found: {paths:?}"
    );

    // Verify alias is captured
    let alias_import = parsed.raw_imports.iter().find(|i| i.path.contains("Thing"));
    assert!(alias_import.is_some(), "Expected Thing import");
    assert_eq!(
        alias_import.unwrap().alias.as_deref(),
        Some("Alias"),
        "Expected alias 'Alias' for Thing import"
    );
}

/// Verify unsupported file types return UnsupportedFileType error.
#[test]
fn test_parse_unsupported_file_type() {
    let settings = Arc::new(Settings::default());
    init_parser_cache(settings.clone());

    let file_content = FileContent::new(
        "test.xyz".into(),
        "content".to_string(),
        "abc123def456".to_string(),
    );

    let result = parse_file(file_content, &settings);

    assert!(result.is_err(), "Expected error for .xyz file");
    let err = result.unwrap_err();
    let err_msg = format!("{err}");
    assert!(
        err_msg.contains("Unsupported") || err_msg.contains("unsupported"),
        "Expected UnsupportedFileType error, got: {err_msg}"
    );
}

/// Verify RawSymbol has NO id field (design requirement).
#[test]
fn test_raw_symbol_has_no_id_field() {
    // This test documents the design: RawSymbol intentionally lacks an ID
    // IDs are assigned in the COLLECT stage, not during parsing
    use codanna::indexing::pipeline::RawSymbol;
    use codanna::types::Range;

    let sym = RawSymbol::new("test", SymbolKind::Function, Range::new(1, 0, 1, 10));

    // If RawSymbol had an `id` field, this code would need to set it
    // The fact that we can create it with just name/kind/range proves the design
    assert_eq!(sym.name.as_ref(), "test");
    assert_eq!(sym.kind, SymbolKind::Function);

    // Verify range is captured (needed for COLLECT stage disambiguation)
    assert_eq!(sym.range.start_line, 1);
    assert_eq!(sym.range.end_line, 1);
}
