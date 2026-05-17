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
