use codanna::parsing::{LanguageParser, cpp::CppParser};

#[test]
fn test_cpp_dot_call_captures_receiver() {
    let code = r#"
struct Obj { void method(); };
void caller() {
    Obj obj;
    obj.method();
}
"#;
    let mut parser = CppParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "method")
        .expect("obj.method call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("obj"));
    assert!(!call.is_static);
}

#[test]
fn test_cpp_arrow_call_captures_receiver() {
    let code = r#"
struct Obj { void method(); };
void caller(Obj* ptr) {
    ptr->method();
}
"#;
    let mut parser = CppParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "method")
        .expect("ptr->method call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("ptr"));
    assert!(!call.is_static);
}

#[test]
fn test_cpp_scope_call_is_static_true() {
    let code = r#"
class Class { public: static void method(); };
void caller() {
    Class::method();
}
"#;
    let mut parser = CppParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "method")
        .expect("Class::method call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("Class"));
    assert!(call.is_static);
}
