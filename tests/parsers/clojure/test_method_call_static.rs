use codanna::parsing::{LanguageParser, MethodCall, clojure::ClojureParser};

fn method_calls(code: &str) -> Vec<MethodCall> {
    let mut parser = ClojureParser::new().unwrap();
    parser.find_method_calls(code)
}

#[test]
fn test_instance_interop_emits_method_call() {
    let code = r#"
(defn f [s] (.toLowerCase s))
"#;
    let calls = method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "toLowerCase")
        .expect("toLowerCase MethodCall not emitted");
    assert_eq!(call.caller, "f");
    assert_eq!(call.receiver.as_deref(), Some("s"));
    assert!(!call.is_static);
}

#[test]
fn test_top_level_instance_interop_caller_is_module_sentinel() {
    let code = r#"
(.size items)
"#;
    let calls = method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "size")
        .expect("size MethodCall not emitted");
    assert_eq!(call.caller, "<module>");
    assert_eq!(call.receiver.as_deref(), Some("items"));
}

#[test]
fn test_static_interop_emits_method_call() {
    let code = r#"
(defn f [] (Integer/parseInt "42"))
"#;
    let calls = method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "parseInt")
        .expect("parseInt MethodCall not emitted");
    assert_eq!(call.caller, "f");
    assert_eq!(call.receiver.as_deref(), Some("Integer"));
    assert!(call.is_static);
}

#[test]
fn test_top_level_static_interop_caller_is_module_sentinel() {
    let code = r#"
(Math/pow 2 3)
"#;
    let calls = method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "pow")
        .expect("pow MethodCall not emitted");
    assert_eq!(call.caller, "<module>");
    assert_eq!(call.receiver.as_deref(), Some("Math"));
    assert!(call.is_static);
}

#[test]
fn test_static_interop_excluded_from_find_calls_tuples() {
    let code = r#"
(defn f [] (Integer/parseInt "42"))
"#;
    let mut parser = ClojureParser::new().unwrap();
    let tuples = parser.find_calls(code);
    assert!(
        tuples.iter().all(|(_, called, _)| !called.contains('/')),
        "static interop must not also appear as a find_calls tuple (dedupe-gate guard)"
    );
}

#[test]
fn test_non_symbolic_receiver_skipped() {
    let code = r#"
(defn f [] (.method 42))
"#;
    let calls = method_calls(code);
    assert!(
        calls.iter().all(|c| c.method_name != "method"),
        "non-symbolic receiver must not produce a MethodCall in slice 2"
    );
}

#[test]
fn test_standard_call_stays_in_find_calls_only() {
    let code = r#"
(defn f [] (foo 1))
"#;
    let mut parser = ClojureParser::new().unwrap();

    let method = parser.find_method_calls(code);
    assert!(
        method.iter().all(|c| c.method_name != "foo"),
        "standard `(foo x)` must not appear as MethodCall"
    );

    let tuples = parser.find_calls(code);
    let foo = tuples
        .iter()
        .find(|(_, called, _)| *called == "foo")
        .expect("standard `(foo x)` must remain in find_calls tuples");
    assert_eq!(foo.0, "f");
}
