use codanna::FileId;
use codanna::parsing::{LanguageParser, swift::SwiftParser};
use codanna::symbol::ScopeContext;
use codanna::types::SymbolCounter;

#[test]
fn test_swift_instance_call_captures_receiver() {
    let code = r#"
func run(cache: Cache, key: String) {
    cache.fetch(key)
}
"#;
    let mut parser = SwiftParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "fetch")
        .expect("cache.fetch call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("cache"));
    assert!(!call.is_static);
}

#[test]
fn test_swift_static_call_uppercase_receiver_is_static_true() {
    let code = r#"
func stamp() {
    let _ = Date.now()
}
"#;
    let mut parser = SwiftParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "now")
        .expect("Date.now call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("Date"));
    assert!(call.is_static);
}

#[test]
fn test_swift_method_symbol_carries_classmember_scope() {
    let code = r#"
class Cache {
    func fetch(key: String) -> String {
        return ""
    }
}
"#;
    let mut parser = SwiftParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    let symbols = parser.parse(code, FileId(1), &mut counter);
    let fetch = symbols
        .iter()
        .find(|s| s.name.as_ref() == "fetch")
        .expect("fetch method symbol should be extracted");
    match &fetch.scope_context {
        Some(ScopeContext::ClassMember { class_name }) => {
            assert_eq!(class_name.as_deref(), Some("Cache"));
        }
        other => panic!(
            "expected ScopeContext::ClassMember{{ class_name: Some(\"Cache\") }}, got {other:?}"
        ),
    }
}
