use codanna::parsing::LanguageBehavior;
use codanna::parsing::typescript::TypeScriptBehavior;

#[test]
fn test_typescript_self_receiver_aliases_is_this() {
    let behavior = TypeScriptBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["this"]);
}
