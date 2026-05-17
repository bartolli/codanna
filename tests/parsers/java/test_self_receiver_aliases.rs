use codanna::parsing::LanguageBehavior;
use codanna::parsing::java::JavaBehavior;

#[test]
fn test_java_self_receiver_aliases_is_this() {
    let behavior = JavaBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["this"]);
}
