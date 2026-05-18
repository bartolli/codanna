use codanna::parsing::LanguageBehavior;
use codanna::parsing::go::GoBehavior;

#[test]
fn test_go_extract_parameter_type_bare_type_identifier() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(name Type, other int)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_go_extract_parameter_type_pointer_strips_to_base() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(name *Type)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_go_extract_parameter_type_method_with_receiver() {
    let behavior = GoBehavior::new();
    let signature = "func (r *Receiver) Bar(name Type) error";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_go_extract_parameter_type_qualified_type_returns_rightmost() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(node tree_sitter.Node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_go_extract_parameter_type_slice_is_out_of_scope() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(items []Type)";
    assert_eq!(behavior.extract_parameter_type(signature, "items"), None);
}

#[test]
fn test_go_extract_parameter_type_map_is_out_of_scope() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(m map[string]Type)";
    assert_eq!(behavior.extract_parameter_type(signature, "m"), None);
}

#[test]
fn test_go_extract_parameter_type_channel_is_out_of_scope() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(ch <-chan Type)";
    assert_eq!(behavior.extract_parameter_type(signature, "ch"), None);
}

#[test]
fn test_go_extract_parameter_type_unknown_var_returns_none() {
    let behavior = GoBehavior::new();
    let signature = "func Foo(name Type, other int)";
    assert_eq!(behavior.extract_parameter_type(signature, "missing"), None);
}
