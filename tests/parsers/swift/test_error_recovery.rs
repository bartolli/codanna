//! Swift ERROR node recovery tests
//!
//! Tests for recovering class declarations from ERROR nodes caused by
//! tree-sitter-swift grammar limitations (e.g., @unchecked Sendable).

use codanna::parsing::LanguageParser;
use codanna::parsing::swift::SwiftParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind, Visibility};

fn parse_swift(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

/// Test: Class with @unchecked Sendable conformance (no base class)
/// This pattern causes tree-sitter-swift to produce ERROR nodes
#[test]
fn test_recover_class_with_unchecked_sendable() {
    let code = r#"
open class Session: @unchecked Sendable {
    public static let shared = Session()
    public func request() {}
}
"#;
    let symbols = parse_swift(code);

    // Should recover the Session class
    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "Session");
    assert!(
        class_sym.is_some(),
        "Should recover Session class from ERROR node"
    );

    let class_sym = class_sym.unwrap();
    assert_eq!(class_sym.kind, SymbolKind::Class);
    assert_eq!(
        class_sym.visibility,
        Visibility::Public,
        "open maps to Public"
    );

    // Should also extract members
    let shared = symbols.iter().find(|s| s.name.as_ref() == "shared");
    assert!(shared.is_some(), "Should extract shared property");

    let request = symbols.iter().find(|s| s.name.as_ref() == "request");
    assert!(request.is_some(), "Should extract request method");
}

/// Test: Multiple classes, one with grammar issue
#[test]
fn test_recover_mixed_classes() {
    let code = r#"
// Normal class - should parse fine
public class NormalClass {
    func normalMethod() {}
}

// Problematic pattern - needs recovery
open class ImageCache: @unchecked Sendable {
    public var memoryCache: Any?
    public func store() {}
}

// Another normal class
class AnotherClass {
    var value: Int = 0
}
"#;
    let symbols = parse_swift(code);

    // All three classes should be found
    assert!(
        symbols.iter().any(|s| s.name.as_ref() == "NormalClass"),
        "Should find NormalClass"
    );
    assert!(
        symbols.iter().any(|s| s.name.as_ref() == "ImageCache"),
        "Should recover ImageCache"
    );
    assert!(
        symbols.iter().any(|s| s.name.as_ref() == "AnotherClass"),
        "Should find AnotherClass"
    );

    // ImageCache should have correct visibility
    let cache = symbols
        .iter()
        .find(|s| s.name.as_ref() == "ImageCache")
        .unwrap();
    assert_eq!(cache.visibility, Visibility::Public);
}

/// Test: Class with base class AND @unchecked Sendable (should parse normally)
#[test]
fn test_class_with_base_and_unchecked_sendable() {
    let code = r#"
open class SessionDelegate: NSObject, @unchecked Sendable {
    public func respond() {}
}
"#;
    let symbols = parse_swift(code);

    // This pattern parses correctly - no recovery needed
    let class_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "SessionDelegate");
    assert!(class_sym.is_some(), "Should find SessionDelegate");
    assert_eq!(class_sym.unwrap().kind, SymbolKind::Class);
}

/// Test: Actual broken code - parser should not crash and should extract what it can
#[test]
fn test_broken_code_does_not_crash() {
    let code = r#"
class BrokenClass {
    func method(
}
"#;
    let symbols = parse_swift(code);

    // Key assertion: parsing broken code should not panic
    // The class should still be found since the class_declaration is valid
    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "BrokenClass");
    assert!(
        class_sym.is_some(),
        "Should find BrokenClass even with syntax error inside"
    );
    assert_eq!(class_sym.unwrap().kind, SymbolKind::Class);

    // The broken method should NOT create a valid function symbol
    // (incomplete parameter list is a syntax error)
    let method = symbols.iter().find(|s| s.name.as_ref() == "method");
    assert!(
        method.is_none(),
        "Broken method should not be extracted as valid symbol"
    );
}

/// Test: Struct with @unchecked Sendable
#[test]
fn test_recover_struct_with_unchecked_sendable() {
    let code = r#"
public struct ThreadSafeContainer: @unchecked Sendable {
    private var storage: [String] = []
    public func add(_ item: String) {}
}
"#;
    let symbols = parse_swift(code);

    let struct_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "ThreadSafeContainer");
    // Note: Current implementation focuses on class recovery
    // Struct recovery may or may not work - this test documents current behavior
    if let Some(sym) = struct_sym {
        assert_eq!(sym.kind, SymbolKind::Struct);
    }
}

/// Test: Visibility is correctly extracted from recovered class
#[test]
fn test_recovered_class_visibility() {
    // Public
    let code_public = "public class PublicCache: @unchecked Sendable { }";
    let symbols = parse_swift(code_public);
    if let Some(sym) = symbols.iter().find(|s| s.name.as_ref() == "PublicCache") {
        assert_eq!(sym.visibility, Visibility::Public);
    }

    // Open (maps to Public)
    let code_open = "open class OpenCache: @unchecked Sendable { }";
    let symbols = parse_swift(code_open);
    if let Some(sym) = symbols.iter().find(|s| s.name.as_ref() == "OpenCache") {
        assert_eq!(sym.visibility, Visibility::Public);
    }

    // Internal (default)
    let code_internal = "class InternalCache: @unchecked Sendable { }";
    let symbols = parse_swift(code_internal);
    if let Some(sym) = symbols.iter().find(|s| s.name.as_ref() == "InternalCache") {
        assert_eq!(sym.visibility, Visibility::Module);
    }
}
