use codanna::parsing::LanguageBehavior;
use codanna::parsing::javascript::JavaScriptBehavior;

#[test]
fn test_javascript_self_receiver_aliases_is_this() {
    let behavior = JavaScriptBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["this"]);
}
