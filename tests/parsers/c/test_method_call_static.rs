use codanna::parsing::{LanguageParser, c::CParser};

#[test]
fn test_c_function_pointer_through_struct_field() {
    let code = r#"
struct V { void (*draw)(int); };
void caller(struct V* vtable, int ctx) {
    vtable->draw(ctx);
}
"#;
    let mut parser = CParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "draw")
        .expect("vtable->draw call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("vtable"));
    assert!(!call.is_static);
}

#[test]
fn test_c_calls_carry_enclosing_function() {
    let code = r#"
struct V { void (*draw)(int); };
void render(struct V* vtable, int ctx) {
    vtable->draw(ctx);
    helper(ctx);
}
"#;
    let mut parser = CParser::new().unwrap();
    let method_calls = parser.find_method_calls(code);
    let draw = method_calls
        .iter()
        .find(|c| c.method_name == "draw")
        .expect("vtable->draw extracted");
    assert_eq!(
        draw.caller, "render",
        "method-call record must carry the enclosing function"
    );
    let plain = parser.find_calls(code);
    let helper = plain
        .iter()
        .find(|(_, callee, _)| *callee == "helper")
        .expect("helper() extracted");
    assert_eq!(
        helper.0, "render",
        "plain-call record must carry the same caller identity (twin guard keys on it)"
    );
}
