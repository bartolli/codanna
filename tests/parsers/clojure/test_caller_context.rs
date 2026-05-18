use codanna::parsing::{LanguageParser, clojure::ClojureParser};

fn calls_for(code: &str) -> Vec<(String, String)> {
    let mut parser = ClojureParser::new().unwrap();
    parser
        .find_calls(code)
        .into_iter()
        .map(|(caller, callee, _)| (caller.to_string(), callee.to_string()))
        .collect()
}

#[test]
fn test_defn_caller_is_enclosing_function() {
    let code = r#"
(defn foo [] (bar))
"#;
    let calls = calls_for(code);
    let bar = calls
        .iter()
        .find(|(_, callee)| callee == "bar")
        .expect("bar call not captured");
    assert_eq!(bar.0, "foo", "caller of bar should be foo, not <module>");
}

#[test]
fn test_defmethod_caller_is_multimethod_name() {
    let code = r#"
(defmethod area :circle [s] (radius s))
"#;
    let calls = calls_for(code);
    let radius = calls
        .iter()
        .find(|(_, callee)| callee == "radius")
        .expect("radius call not captured inside defmethod body");
    assert_eq!(
        radius.0, "area",
        "caller should be the multimethod name, not <module>"
    );
}

#[test]
fn test_defn_private_caller_is_function_name() {
    let code = r#"
(defn- private-fn [] (helper))
"#;
    let calls = calls_for(code);
    let helper = calls
        .iter()
        .find(|(_, callee)| callee == "helper")
        .expect("helper call not captured");
    assert_eq!(helper.0, "private-fn");
}

#[test]
fn test_top_level_call_caller_is_module_sentinel() {
    let code = r#"
(println "x")
"#;
    let calls = calls_for(code);
    let println = calls
        .iter()
        .find(|(_, callee)| callee == "println")
        .expect("println call not captured");
    assert_eq!(
        println.0, "<module>",
        "top-level call should retain the <module> sentinel"
    );
}
