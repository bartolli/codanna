use codanna::parsing::LanguageBehavior;
use codanna::parsing::kotlin::KotlinBehavior;

#[test]
fn test_kotlin_self_receiver_aliases_is_this() {
    let behavior = KotlinBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["this"]);
}
