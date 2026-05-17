use codanna::parsing::LanguageBehavior;
use codanna::parsing::rust::RustBehavior;

#[test]
fn test_rust_extract_parameter_type_bare_type_identifier() {
    let behavior = RustBehavior::new();
    let signature = "fn foo(node: Node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_rust_extract_parameter_type_reference() {
    let behavior = RustBehavior::new();
    let signature = "fn foo(node: &Node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_rust_extract_parameter_type_lifetime_and_mut() {
    let behavior = RustBehavior::new();
    let signature = "fn foo<'a>(node: &'a mut Node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_rust_extract_parameter_type_generic_strips_type_args() {
    let behavior = RustBehavior::new();
    let signature = "fn foo(v: Vec<u32>)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "v"),
        Some("Vec".to_string())
    );
}

#[test]
fn test_rust_extract_parameter_type_scoped_path_strips_module() {
    let behavior = RustBehavior::new();
    let signature = "fn foo(node: &tree_sitter::Node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_rust_extract_parameter_type_unknown_var_returns_none() {
    let behavior = RustBehavior::new();
    let signature = "fn foo(node: Node, code: &str)";
    assert_eq!(behavior.extract_parameter_type(signature, "missing"), None);
}

#[test]
fn test_rust_extract_parameter_type_tuple_type_is_out_of_scope() {
    let behavior = RustBehavior::new();
    let signature = "fn foo(pair: (u32, String))";
    assert_eq!(behavior.extract_parameter_type(signature, "pair"), None);
}
