use codanna::SymbolKind;
use codanna::parsing::LanguageParser;
use codanna::parsing::clojure::ClojureParser;
use codanna::types::{FileId, SymbolCounter};

fn load_basic_fixture() -> &'static str {
    include_str!("../../fixtures/clojure/basic.clj")
}

#[test]
fn test_clojure_parses_without_error() {
    let code = load_basic_fixture();
    let mut parser = ClojureParser::new().expect("Failed to create Clojure parser");
    let file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    let symbols = parser.parse(code, file_id, &mut counter);
    assert!(
        !symbols.is_empty(),
        "Should extract symbols from Clojure code"
    );

    println!("Extracted {} symbols:", symbols.len());
    for sym in &symbols {
        println!("  {:?} {} ({:?})", sym.kind, sym.name, sym.visibility);
    }
}

#[test]
fn test_clojure_extracts_namespace() {
    let code = load_basic_fixture();
    let mut parser = ClojureParser::new().expect("Failed to create Clojure parser");
    let file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    let symbols = parser.parse(code, file_id, &mut counter);

    let ns = symbols.iter().find(|s| s.kind == SymbolKind::Module);
    assert!(ns.is_some(), "Should extract namespace as Module symbol");
    assert_eq!(ns.unwrap().name.as_ref(), "test.basic");
}

#[test]
fn test_clojure_extracts_functions() {
    let code = load_basic_fixture();
    let mut parser = ClojureParser::new().expect("Failed to create Clojure parser");
    let file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    let symbols = parser.parse(code, file_id, &mut counter);
    let functions: Vec<_> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Function)
        .collect();

    println!("Functions found:");
    for f in &functions {
        println!("  {} ({:?})", f.name, f.visibility);
    }

    assert!(
        functions.iter().any(|f| f.name.as_ref() == "process"),
        "Should find 'process' function"
    );
}

#[test]
fn test_clojure_extracts_protocol() {
    let code = load_basic_fixture();
    let mut parser = ClojureParser::new().expect("Failed to create Clojure parser");
    let file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    let symbols = parser.parse(code, file_id, &mut counter);

    let protocol = symbols.iter().find(|s| s.kind == SymbolKind::Interface);
    assert!(protocol.is_some(), "Should extract protocol as Interface");
    assert_eq!(protocol.unwrap().name.as_ref(), "Validator");
}

#[test]
fn test_clojure_extracts_record() {
    let code = load_basic_fixture();
    let mut parser = ClojureParser::new().expect("Failed to create Clojure parser");
    let file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    let symbols = parser.parse(code, file_id, &mut counter);

    let record = symbols.iter().find(|s| s.kind == SymbolKind::Struct);
    assert!(record.is_some(), "Should extract record as Struct");
    assert_eq!(record.unwrap().name.as_ref(), "StringValidator");
}

#[test]
fn test_clojure_extracts_doc_comments() {
    let code = load_basic_fixture();
    let mut parser = ClojureParser::new().expect("Failed to create Clojure parser");
    let file_id = FileId::new(1).unwrap();
    let mut counter = SymbolCounter::new();

    let symbols = parser.parse(code, file_id, &mut counter);

    let process_fn = symbols.iter().find(|s| s.name.as_ref() == "process");
    assert!(process_fn.is_some(), "Should find process function");

    let doc = process_fn.unwrap().doc_comment.as_deref();
    assert!(
        doc.is_some(),
        "process function should have doc_comment for semantic search"
    );
    assert!(
        doc.unwrap().contains("Process input data"),
        "Doc comment should contain the docstring text"
    );
}
