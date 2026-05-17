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

#[test]
fn test_cpp_call_caller_is_enclosing_function() {
    let code = r#"
struct Obj { void method(); };
void caller_fn() {
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
    assert_eq!(
        call.caller, "caller_fn",
        "caller should be the enclosing function name, not empty"
    );
}

#[test]
fn test_cpp_call_caller_inline_method_in_struct() {
    let code = r#"
struct Bag { void touch(); };
struct S {
    void inline_method() {
        Bag b;
        b.touch();
    }
};
"#;
    let mut parser = CppParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "touch")
        .expect("b.touch call should be extracted");
    assert_eq!(
        call.caller, "inline_method",
        "inline method body's caller should be the field_identifier name"
    );
}

#[test]
fn test_cpp_call_caller_method_impl_uses_unqualified_name() {
    let code = r#"
struct Helper { void log(); };
class Service {
public:
    void run();
};
void Service::run() {
    Helper h;
    h.log();
}
"#;
    let mut parser = CppParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "log")
        .expect("h.log call should be extracted");
    assert_eq!(
        call.caller, "run",
        "method-impl caller should be the unqualified name (matches symbol-extraction)"
    );
}
