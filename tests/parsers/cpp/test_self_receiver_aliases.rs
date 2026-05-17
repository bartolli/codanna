use codanna::parsing::LanguageBehavior;
use codanna::parsing::cpp::CppBehavior;

#[test]
fn test_cpp_self_receiver_aliases_is_this() {
    let behavior = CppBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["this"]);
}
