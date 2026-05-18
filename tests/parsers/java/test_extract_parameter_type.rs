use codanna::parsing::LanguageBehavior;
use codanna::parsing::java::JavaBehavior;

#[test]
fn test_java_extract_parameter_type_bare_type_identifier() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(Type name)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_primitive_integral() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(int count)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "count"),
        Some("int".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_modifier_final_stripped() {
    let behavior = JavaBehavior::new();
    let signature = "String bar(final Type name)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_generic_returns_base_not_inner() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(List<Type> items)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "items"),
        Some("List".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_scoped_returns_rightmost() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(com.example.Type node)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "node"),
        Some("Type".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_array_is_out_of_scope() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(Type[] arr)";
    assert_eq!(behavior.extract_parameter_type(signature, "arr"), None);
}

#[test]
fn test_java_extract_parameter_type_varargs_strips_spread() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(Type... varArgs)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "varArgs"),
        Some("Type".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_annotation_modifier() {
    let behavior = JavaBehavior::new();
    let signature = "Type plug(@NotNull Type name)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_full_modifiers_and_generics() {
    let behavior = JavaBehavior::new();
    let signature = "public static <T> void process(Type name)";
    assert_eq!(
        behavior.extract_parameter_type(signature, "name"),
        Some("Type".to_string())
    );
}

#[test]
fn test_java_extract_parameter_type_unknown_var_returns_none() {
    let behavior = JavaBehavior::new();
    let signature = "void foo(Type name, int other)";
    assert_eq!(behavior.extract_parameter_type(signature, "missing"), None);
}
