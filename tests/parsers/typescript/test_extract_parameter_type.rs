use codanna::parsing::LanguageBehavior;
use codanna::parsing::typescript::TypeScriptBehavior;

#[test]
fn test_typescript_extract_parameter_type_method_bare_type() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(name: Type): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_function_declaration_shape() {
    let behavior = TypeScriptBehavior::new();
    let signature = "function foo(name: Type): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_union_with_null_strips() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(name: Type | null): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_union_with_undefined_strips() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(name: Type | undefined): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_generic_returns_base_not_inner() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(items: Array<Type>): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "items"),
        Some("Array".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_nested_type_returns_rightmost() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(node: ts.Node): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Node".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_predefined_type() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(count: number): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "count"),
        Some("number".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_optional_parameter() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(name?: Type): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_unknown_var_returns_none() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(name: Type, other: number): void";
    assert_eq!(behavior.extract_parameter_type(signature, "missing"), None);
}

#[test]
fn test_typescript_extract_parameter_type_object_type_is_out_of_scope() {
    let behavior = TypeScriptBehavior::new();
    let signature = "m(opts: { a: number }): void";
    assert_eq!(behavior.extract_parameter_type(signature, "opts"), None);
}

#[test]
fn test_typescript_extract_parameter_type_async_method_with_generic() {
    let behavior = TypeScriptBehavior::new();
    let signature = "async fetch<T>(url: string): Promise<T>";
    assert_eq!(
        behavior.extract_parameter_type(signature, "url"),
        Some("string".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_static_method() {
    let behavior = TypeScriptBehavior::new();
    let signature = "static create(name: Type): Foo";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_typescript_extract_parameter_type_public_modifier() {
    let behavior = TypeScriptBehavior::new();
    let signature = "public m(name: Type): void";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}
