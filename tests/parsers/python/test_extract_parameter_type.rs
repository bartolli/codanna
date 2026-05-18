use codanna::parsing::LanguageBehavior;
use codanna::parsing::python::PythonBehavior;

#[test]
fn test_python_extract_parameter_type_bare_identifier() {
    let behavior = PythonBehavior::new();
    let signature = "(name: Type)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_optional_strips_to_inner() {
    let behavior = PythonBehavior::new();
    let signature = "(name: Optional[Type])";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_pep604_none_union_strips() {
    let behavior = PythonBehavior::new();
    let signature = "(name: Type | None)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_list_returns_base_not_inner() {
    let behavior = PythonBehavior::new();
    let signature = "(items: List[Type])";
    assert_eq!(
        behavior.extract_parameter_type(signature, "items"),
        Some("List".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_attribute_returns_rightmost() {
    let behavior = PythonBehavior::new();
    let signature = "(node: tree_sitter.Node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_skips_self_positional() {
    let behavior = PythonBehavior::new();
    let signature = "(self, name: Type)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_typed_default_parameter() {
    let behavior = PythonBehavior::new();
    let signature = "(self, name: Type = DEFAULT)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_async_prefix_stripped() {
    let behavior = PythonBehavior::new();
    let signature = "async (name: Type) -> int";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_python_extract_parameter_type_unknown_var_returns_none() {
    let behavior = PythonBehavior::new();
    let signature = "(name: Type, other: int)";
    assert_eq!(behavior.extract_parameter_type(signature, "missing"), None);
}

#[test]
fn test_python_extract_parameter_type_tuple_is_out_of_scope() {
    let behavior = PythonBehavior::new();
    let signature = "(pair: Tuple[int, str])";
    assert_eq!(
        behavior.extract_parameter_type(signature, "pair"),
        Some("Tuple".to_string())
    );
}
