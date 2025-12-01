//! Swift visibility extraction tests
//!
//! Tests that visibility modifiers are correctly extracted from AST

use codanna::parsing::LanguageParser;
use codanna::parsing::swift::SwiftParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, Visibility};

fn parse_swift(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

#[test]
fn test_public_class() {
    let code = r#"
public class PublicClass {
    public func publicMethod() {}
}
"#;
    let symbols = parse_swift(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "PublicClass");
    assert!(class_sym.is_some(), "Should find PublicClass");
    assert_eq!(
        class_sym.unwrap().visibility,
        Visibility::Public,
        "PublicClass should be Public"
    );

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "publicMethod");
    assert!(method_sym.is_some(), "Should find publicMethod");
    assert_eq!(
        method_sym.unwrap().visibility,
        Visibility::Public,
        "publicMethod should be Public"
    );
}

#[test]
fn test_open_class() {
    let code = r#"
open class OpenClass {
    open func openMethod() {}
}
"#;
    let symbols = parse_swift(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "OpenClass");
    assert!(class_sym.is_some(), "Should find OpenClass");
    assert_eq!(
        class_sym.unwrap().visibility,
        Visibility::Public,
        "open maps to Public"
    );
}

#[test]
fn test_private_members() {
    let code = r#"
class MyClass {
    private var privateVar: Int = 0
    private func privateMethod() {}
}
"#;
    let symbols = parse_swift(code);

    let var_sym = symbols.iter().find(|s| s.name.as_ref() == "privateVar");
    assert!(var_sym.is_some(), "Should find privateVar");
    assert_eq!(
        var_sym.unwrap().visibility,
        Visibility::Private,
        "privateVar should be Private"
    );

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "privateMethod");
    assert!(method_sym.is_some(), "Should find privateMethod");
    assert_eq!(
        method_sym.unwrap().visibility,
        Visibility::Private,
        "privateMethod should be Private"
    );
}

#[test]
fn test_fileprivate_members() {
    let code = r#"
class MyClass {
    fileprivate var filePrivateVar: Int = 0
    fileprivate func filePrivateMethod() {}
}
"#;
    let symbols = parse_swift(code);

    let var_sym = symbols.iter().find(|s| s.name.as_ref() == "filePrivateVar");
    assert!(var_sym.is_some(), "Should find filePrivateVar");
    assert_eq!(
        var_sym.unwrap().visibility,
        Visibility::Private,
        "fileprivate maps to Private"
    );

    let method_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "filePrivateMethod");
    assert!(method_sym.is_some(), "Should find filePrivateMethod");
    assert_eq!(
        method_sym.unwrap().visibility,
        Visibility::Private,
        "fileprivate maps to Private"
    );
}

#[test]
fn test_internal_explicit() {
    let code = r#"
internal class InternalClass {
    internal func internalMethod() {}
}
"#;
    let symbols = parse_swift(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "InternalClass");
    assert!(class_sym.is_some(), "Should find InternalClass");
    assert_eq!(
        class_sym.unwrap().visibility,
        Visibility::Module,
        "internal maps to Module"
    );
}

#[test]
fn test_default_visibility_is_internal() {
    let code = r#"
class DefaultClass {
    var defaultVar: Int = 0
    func defaultMethod() {}
}
"#;
    let symbols = parse_swift(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "DefaultClass");
    assert!(class_sym.is_some(), "Should find DefaultClass");
    assert_eq!(
        class_sym.unwrap().visibility,
        Visibility::Module,
        "Default visibility is internal (Module)"
    );

    let var_sym = symbols.iter().find(|s| s.name.as_ref() == "defaultVar");
    assert!(var_sym.is_some(), "Should find defaultVar");
    assert_eq!(
        var_sym.unwrap().visibility,
        Visibility::Module,
        "Default var visibility is internal"
    );

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "defaultMethod");
    assert!(method_sym.is_some(), "Should find defaultMethod");
    assert_eq!(
        method_sym.unwrap().visibility,
        Visibility::Module,
        "Default method visibility is internal"
    );
}

#[test]
fn test_mixed_visibility() {
    let code = r#"
public class MixedClass {
    public var publicVar: Int = 0
    private var privateVar: Int = 0
    var internalVar: Int = 0
}
"#;
    let symbols = parse_swift(code);

    let public_var = symbols.iter().find(|s| s.name.as_ref() == "publicVar");
    let private_var = symbols.iter().find(|s| s.name.as_ref() == "privateVar");
    let internal_var = symbols.iter().find(|s| s.name.as_ref() == "internalVar");

    assert_eq!(public_var.unwrap().visibility, Visibility::Public);
    assert_eq!(private_var.unwrap().visibility, Visibility::Private);
    assert_eq!(internal_var.unwrap().visibility, Visibility::Module);
}

#[test]
fn test_protocol_visibility() {
    let code = r#"
public protocol PublicProtocol {
    func requiredMethod()
}

private protocol PrivateProtocol {
    func privateRequired()
}
"#;
    let symbols = parse_swift(code);

    let public_proto = symbols.iter().find(|s| s.name.as_ref() == "PublicProtocol");
    assert!(public_proto.is_some());
    assert_eq!(public_proto.unwrap().visibility, Visibility::Public);

    let private_proto = symbols
        .iter()
        .find(|s| s.name.as_ref() == "PrivateProtocol");
    assert!(private_proto.is_some());
    assert_eq!(private_proto.unwrap().visibility, Visibility::Private);
}

// Tests for resolution-time visibility checking
mod resolution_visibility {
    use codanna::parsing::LanguageBehavior;
    use codanna::parsing::swift::SwiftBehavior;
    use codanna::types::Range;
    use codanna::{FileId, Symbol, SymbolId, SymbolKind, Visibility};

    fn make_symbol(name: &str, visibility: Visibility, file_id: FileId) -> Symbol {
        let mut sym = Symbol::new(
            SymbolId(1),
            name,
            SymbolKind::Function,
            file_id,
            Range {
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 10,
            },
        );
        sym.visibility = visibility;
        sym
    }

    #[test]
    fn test_public_visible_from_other_file() {
        let behavior = SwiftBehavior::new();
        let symbol = make_symbol("publicFunc", Visibility::Public, FileId(1));

        // Public symbol visible from another file
        assert!(behavior.is_symbol_visible_from_file(&symbol, FileId(2)));
    }

    #[test]
    fn test_private_not_visible_from_other_file() {
        let behavior = SwiftBehavior::new();
        let symbol = make_symbol("privateFunc", Visibility::Private, FileId(1));

        // Private symbol not visible from another file
        assert!(!behavior.is_symbol_visible_from_file(&symbol, FileId(2)));
    }

    #[test]
    fn test_private_visible_from_same_file() {
        let behavior = SwiftBehavior::new();
        let symbol = make_symbol("privateFunc", Visibility::Private, FileId(1));

        // Private symbol visible from same file
        assert!(behavior.is_symbol_visible_from_file(&symbol, FileId(1)));
    }

    #[test]
    fn test_internal_visible_from_other_file() {
        let behavior = SwiftBehavior::new();
        let symbol = make_symbol("internalFunc", Visibility::Module, FileId(1));

        // Internal (Module) symbol visible from another file in same module
        assert!(behavior.is_symbol_visible_from_file(&symbol, FileId(2)));
    }
}
