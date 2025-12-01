//! Test nested type extraction in Swift

use codanna::parsing::LanguageParser;
use codanna::parsing::swift::SwiftParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind};
use std::path::Path;

fn parse_swift(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

#[test]
fn test_nested_struct_in_class() {
    let code = r#"
public class DataStreamRequest {
    public struct Stream: Sendable {
        public let value: Int
    }
}
"#;
    let symbols = parse_swift(code);

    eprintln!("Symbols found:");
    for sym in &symbols {
        eprintln!("  {} ({:?}) at {:?}", sym.name, sym.kind, sym.range);
    }

    // Should find both DataStreamRequest and Stream
    let class_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "DataStreamRequest");
    assert!(class_sym.is_some(), "DataStreamRequest class not found");
    assert_eq!(class_sym.unwrap().kind, SymbolKind::Class);

    let stream_sym = symbols.iter().find(|s| s.name.as_ref() == "Stream");
    assert!(stream_sym.is_some(), "Nested Stream struct not found");
    assert_eq!(stream_sym.unwrap().kind, SymbolKind::Struct);
}

#[test]
fn test_nested_enum_in_class() {
    let code = r#"
public class DataStreamRequest {
    public enum Event: Sendable {
        case stream
        case complete
    }
}
"#;
    let symbols = parse_swift(code);

    eprintln!("Symbols found:");
    for sym in &symbols {
        eprintln!("  {} ({:?}) at {:?}", sym.name, sym.kind, sym.range);
    }

    let event_sym = symbols.iter().find(|s| s.name.as_ref() == "Event");
    assert!(event_sym.is_some(), "Nested Event enum not found");
    assert_eq!(event_sym.unwrap().kind, SymbolKind::Enum);
}

#[test]
fn test_nested_types_with_unchecked_sendable() {
    // This tests the @unchecked Sendable case which triggers ERROR recovery
    let code = r#"
public final class DataStreamRequest: Request, @unchecked Sendable {
    public struct Stream: Sendable {
        public let value: Int
    }

    public enum Event: Sendable {
        case stream
        case complete
    }

    public struct Completion: Sendable {
        public let error: Error?
    }
}
"#;
    let symbols = parse_swift(code);

    eprintln!("Symbols found:");
    for sym in &symbols {
        eprintln!("  {} ({:?}) at {:?}", sym.name, sym.kind, sym.range);
    }

    // DataStreamRequest should be extracted (even with ERROR recovery)
    let class_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "DataStreamRequest");
    assert!(class_sym.is_some(), "DataStreamRequest class not found");

    // Nested types should also be extracted
    let stream_sym = symbols.iter().find(|s| s.name.as_ref() == "Stream");
    assert!(stream_sym.is_some(), "Nested Stream struct not found");

    let event_sym = symbols.iter().find(|s| s.name.as_ref() == "Event");
    assert!(event_sym.is_some(), "Nested Event enum not found");

    let completion_sym = symbols.iter().find(|s| s.name.as_ref() == "Completion");
    assert!(
        completion_sym.is_some(),
        "Nested Completion struct not found"
    );
}

#[test]
fn test_alamofire_datastreamrequest_nested_types() {
    let path = Path::new("test_monorepos/Alamofire/Source/Core/DataStreamRequest.swift");
    if !path.exists() {
        eprintln!("Skipping test - Alamofire not available");
        return;
    }

    let code = std::fs::read_to_string(path).expect("Failed to read file");
    let symbols = parse_swift(&code);

    eprintln!("Symbols found in DataStreamRequest.swift:");
    for sym in &symbols {
        if matches!(
            sym.kind,
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum
        ) {
            eprintln!("  {} ({:?}) at {:?}", sym.name, sym.kind, sym.range);
        }
    }

    // DataStreamRequest should be extracted
    let class_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "DataStreamRequest");
    assert!(class_sym.is_some(), "DataStreamRequest class not found");

    // Nested Stream struct at line 34
    let stream_sym = symbols.iter().find(|s| s.name.as_ref() == "Stream");
    assert!(stream_sym.is_some(), "Nested Stream struct not found");

    // Nested Event enum at line 48
    let event_sym = symbols.iter().find(|s| s.name.as_ref() == "Event");
    assert!(event_sym.is_some(), "Nested Event enum not found");

    // Nested Completion struct at line 58
    let completion_sym = symbols.iter().find(|s| s.name.as_ref() == "Completion");
    assert!(
        completion_sym.is_some(),
        "Nested Completion struct not found"
    );

    // Nested CancellationToken struct at line 70
    let token_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "CancellationToken");
    assert!(
        token_sym.is_some(),
        "Nested CancellationToken struct not found"
    );
}
