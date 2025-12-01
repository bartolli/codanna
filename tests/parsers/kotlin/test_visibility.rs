//! Kotlin visibility extraction tests
//!
//! Tests that visibility modifiers are correctly extracted using node-based detection

use codanna::parsing::LanguageParser;
use codanna::parsing::kotlin::KotlinParser;
use codanna::types::SymbolCounter;
use codanna::{FileId, Visibility};

fn parse_kotlin(code: &str) -> Vec<codanna::Symbol> {
    let mut parser = KotlinParser::new().expect("Failed to create Kotlin parser");
    let mut counter = SymbolCounter::new();
    parser.parse(code, FileId(1), &mut counter)
}

#[test]
fn test_public_class() {
    let code = r#"
public class PublicClass {
    public fun publicMethod() {}
}
"#;
    let symbols = parse_kotlin(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "PublicClass");
    assert!(class_sym.is_some(), "Should find PublicClass");
    assert_eq!(class_sym.unwrap().visibility, Visibility::Public);

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "publicMethod");
    assert!(method_sym.is_some(), "Should find publicMethod");
    assert_eq!(method_sym.unwrap().visibility, Visibility::Public);
}

#[test]
fn test_private_members() {
    let code = r#"
class MyClass {
    private val privateVal: Int = 0
    private fun privateMethod() {}
}
"#;
    let symbols = parse_kotlin(code);

    let val_sym = symbols.iter().find(|s| s.name.as_ref() == "privateVal");
    assert!(val_sym.is_some(), "Should find privateVal");
    assert_eq!(val_sym.unwrap().visibility, Visibility::Private);

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "privateMethod");
    assert!(method_sym.is_some(), "Should find privateMethod");
    assert_eq!(method_sym.unwrap().visibility, Visibility::Private);
}

#[test]
fn test_protected_members() {
    let code = r#"
open class BaseClass {
    protected fun protectedMethod() {}
    protected val protectedVal: Int = 0
}
"#;
    let symbols = parse_kotlin(code);

    let method_sym = symbols
        .iter()
        .find(|s| s.name.as_ref() == "protectedMethod");
    assert!(method_sym.is_some(), "Should find protectedMethod");
    // Protected maps to Module in our visibility model
    assert_eq!(method_sym.unwrap().visibility, Visibility::Module);
}

#[test]
fn test_internal_members() {
    let code = r#"
internal class InternalClass {
    internal fun internalMethod() {}
}
"#;
    let symbols = parse_kotlin(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "InternalClass");
    assert!(class_sym.is_some(), "Should find InternalClass");
    // Internal maps to Crate in our visibility model
    assert_eq!(class_sym.unwrap().visibility, Visibility::Crate);

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "internalMethod");
    assert!(method_sym.is_some(), "Should find internalMethod");
    assert_eq!(method_sym.unwrap().visibility, Visibility::Crate);
}

#[test]
fn test_default_visibility_is_public() {
    // Kotlin default visibility is public
    let code = r#"
class DefaultClass {
    val defaultVal: Int = 0
    fun defaultMethod() {}
}
"#;
    let symbols = parse_kotlin(code);

    let class_sym = symbols.iter().find(|s| s.name.as_ref() == "DefaultClass");
    assert!(class_sym.is_some(), "Should find DefaultClass");
    assert_eq!(class_sym.unwrap().visibility, Visibility::Public);

    let val_sym = symbols.iter().find(|s| s.name.as_ref() == "defaultVal");
    assert!(val_sym.is_some(), "Should find defaultVal");
    assert_eq!(val_sym.unwrap().visibility, Visibility::Public);

    let method_sym = symbols.iter().find(|s| s.name.as_ref() == "defaultMethod");
    assert!(method_sym.is_some(), "Should find defaultMethod");
    assert_eq!(method_sym.unwrap().visibility, Visibility::Public);
}

#[test]
fn test_mixed_visibility() {
    let code = r#"
class MixedClass {
    public val publicVal: Int = 0
    private val privateVal: Int = 0
    protected val protectedVal: Int = 0
    internal val internalVal: Int = 0
    val defaultVal: Int = 0
}
"#;
    let symbols = parse_kotlin(code);

    let public_val = symbols.iter().find(|s| s.name.as_ref() == "publicVal");
    let private_val = symbols.iter().find(|s| s.name.as_ref() == "privateVal");
    let protected_val = symbols.iter().find(|s| s.name.as_ref() == "protectedVal");
    let internal_val = symbols.iter().find(|s| s.name.as_ref() == "internalVal");
    let default_val = symbols.iter().find(|s| s.name.as_ref() == "defaultVal");

    assert_eq!(public_val.unwrap().visibility, Visibility::Public);
    assert_eq!(private_val.unwrap().visibility, Visibility::Private);
    assert_eq!(protected_val.unwrap().visibility, Visibility::Module);
    assert_eq!(internal_val.unwrap().visibility, Visibility::Crate);
    assert_eq!(default_val.unwrap().visibility, Visibility::Public);
}

#[test]
fn test_interface_visibility() {
    let code = r#"
public interface PublicInterface {
    fun interfaceMethod()
}

private interface PrivateInterface {
    fun privateInterfaceMethod()
}
"#;
    let symbols = parse_kotlin(code);

    let public_iface = symbols
        .iter()
        .find(|s| s.name.as_ref() == "PublicInterface");
    assert!(public_iface.is_some());
    assert_eq!(public_iface.unwrap().visibility, Visibility::Public);

    let private_iface = symbols
        .iter()
        .find(|s| s.name.as_ref() == "PrivateInterface");
    assert!(private_iface.is_some());
    assert_eq!(private_iface.unwrap().visibility, Visibility::Private);
}

#[test]
fn test_object_visibility() {
    let code = r#"
private object PrivateSingleton {
    fun doSomething() {}
}

object DefaultSingleton {
    fun doSomething() {}
}
"#;
    let symbols = parse_kotlin(code);

    let private_obj = symbols
        .iter()
        .find(|s| s.name.as_ref() == "PrivateSingleton");
    assert!(private_obj.is_some());
    assert_eq!(private_obj.unwrap().visibility, Visibility::Private);

    let default_obj = symbols
        .iter()
        .find(|s| s.name.as_ref() == "DefaultSingleton");
    assert!(default_obj.is_some());
    assert_eq!(default_obj.unwrap().visibility, Visibility::Public);
}
