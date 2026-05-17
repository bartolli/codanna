use codanna::parsing::LanguageBehavior;
use codanna::parsing::python::PythonBehavior;

#[test]
fn test_python_self_receiver_aliases_includes_self_and_cls() {
    let behavior = PythonBehavior::new();
    assert_eq!(behavior.self_receiver_aliases(), &["self", "cls"]);
}
