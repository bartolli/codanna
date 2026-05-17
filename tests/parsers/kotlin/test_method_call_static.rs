use codanna::FileId;
use codanna::parsing::{LanguageParser, kotlin::KotlinParser};
use codanna::symbol::ScopeContext;
use codanna::types::SymbolCounter;

#[test]
fn test_kotlin_static_call_uppercase_receiver_is_static_true() {
    let code = r#"
fun build(): String {
    return Logger.create()
}
"#;
    let mut parser = KotlinParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "create")
        .expect("Logger.create call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("Logger"));
    assert!(call.is_static);
}

#[test]
fn test_kotlin_method_symbol_carries_classmember_scope() {
    let code = r#"
class Foo {
    fun process(x: Int): Int {
        return x + 1
    }
}
"#;
    let mut parser = KotlinParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    let symbols = parser.parse(code, FileId(1), &mut counter);
    let process = symbols
        .iter()
        .find(|s| s.name.as_ref() == "process")
        .expect("process method symbol should be extracted");
    match &process.scope_context {
        Some(ScopeContext::ClassMember { class_name }) => {
            assert_eq!(
                class_name.as_deref(),
                Some("Foo"),
                "method's containing-type name should be Foo"
            );
        }
        other => panic!(
            "expected ScopeContext::ClassMember{{ class_name: Some(\"Foo\") }}, got {other:?}"
        ),
    }
}
