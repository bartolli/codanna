use codanna::parsing::{LanguageParser, gdscript::GdscriptParser};

#[test]
fn test_gdscript_instance_call_captures_receiver() {
    let code = include_str!("../../fixtures/gdscript/player.gd");
    let mut parser = GdscriptParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "setup")
        .expect("enemy_scene.setup call should be extracted");
    assert_eq!(call.receiver.as_deref(), Some("enemy_scene"));
    assert!(!call.is_static);
}

#[test]
fn test_gdscript_static_call_uppercase_receiver_is_static_true() {
    let code = include_str!("../../fixtures/gdscript/player.gd");
    let mut parser = GdscriptParser::new().unwrap();
    let calls = parser.find_method_calls(code);
    let call = calls
        .iter()
        .find(|c| c.method_name == "instantiate" && c.receiver.as_deref() == Some("EnemyScene"))
        .expect("EnemyScene.instantiate call should be extracted");
    assert!(
        call.is_static,
        "uppercase-leading receiver should mark is_static=true"
    );
}
