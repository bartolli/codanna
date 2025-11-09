use codanna::Range;
use codanna::parsing::{LanguageParser, kotlin::KotlinParser};

fn infer_types(code: &str) -> Vec<(&str, &str, Range)> {
    let mut parser = KotlinParser::new().unwrap();
    parser.find_variable_types(code)
}

fn assert_type<'a>(entries: &[(&'a str, &'a str, Range)], expr: &str, expected: &str) {
    let (found_expr, found_type, _) = entries
        .iter()
        .find(|(value, _, _)| *value == expr)
        .unwrap_or_else(|| panic!("{expr} missing from variable types"));
    assert_eq!(
        *found_type, expected,
        "Expression {found_expr} should infer as {expected}"
    );
}

#[test]
fn test_simple_generic_call_inference() {
    let code = r#"
fun <T> identity(x: T): T = x

fun demo() {
    val a = identity(3)
    val b = identity("abc")
}
"#;

    let var_types = infer_types(code);

    assert_type(&var_types, "identity(3)", "Int");
    assert_type(&var_types, "identity(\"abc\")", "String");
}

#[test]
fn test_multi_generic_call_inference() {
    let code = r#"
fun <T, R> select(first: T, second: R): R = second

fun demo() {
    val first = select(42, "right")
    val second = select("left", 100)
}
"#;

    let var_types = infer_types(code);

    assert_type(&var_types, "select(42, \"right\")", "String");
    assert_type(&var_types, "select(\"left\", 100)", "Int");
}

#[test]
fn test_extension_on_generic_result() {
    let code = r#"
fun <T> passthrough(x: T): T = x

fun Int.double(): Int = this * 2

fun callSite() {
    val value = passthrough(21).double()
}
"#;

    let var_types = infer_types(code);

    assert_type(&var_types, "passthrough(21)", "Int");
    assert_type(&var_types, "passthrough(21).double()", "Int");
}

#[test]
fn test_nested_generic_call_type_inference() {
    let code = r#"
fun <T> identity(x: T): T = x

fun bar(x: Int): Int {
    return x * 2
}

fun nested() {
    val value = identity(bar(3))
}
"#;

    let var_types = infer_types(code);

    assert_type(&var_types, "3", "Int");
    assert_type(&var_types, "bar(3)", "Int");
    assert_type(&var_types, "identity(bar(3))", "Int");
}

#[test]
fn test_complex_generic_substitution_list() {
    let code = r#"
fun <T> wrap(x: T): List<T> = listOf(x)

fun demo() {
    val data = wrap(42)
}
"#;

    let mut parser = KotlinParser::new().unwrap();
    let owned_types = parser
        .find_variable_types_with_substitution(code)
        .expect("Should infer owned types");

    // Helper for owned types
    fn assert_owned_type(types: &[(String, String, Range)], expr: &str, expected: &str) {
        let found = types.iter().find(|(e, _, _)| e == expr);
        assert!(found.is_some(), "Expression '{expr}' not found in types");
        let (_, ty, _) = found.unwrap();
        assert_eq!(
            ty, expected,
            "Expression '{expr}' should have type '{expected}' but got '{ty}'"
        );
    }

    assert_owned_type(&owned_types, "42", "Int");
    assert_owned_type(&owned_types, "wrap(42)", "List<Int>");
}

#[test]
fn debug_reddit_challenge_calls() {
    let code = r#"
fun <T> foo(x: T): T = x
fun Int.bar(): String = "test"
fun String.bar(): String = "test"

fun testGenericFlow() {
    val result1 = foo(3).bar()
    val result2 = foo("abc").bar()
}
"#;

    let mut parser = KotlinParser::new().unwrap();
    let calls = parser.find_calls(code);

    println!("\n[CALL-DEBUG] === FUNCTION CALLS FOUND ===");
    for (caller, callee, range) in &calls {
        println!(
            "[CALL-DEBUG]   {} -> {} at line {}",
            caller, callee, range.start_line
        );
    }
    println!("[CALL-DEBUG] Total: {} calls\n", calls.len());

    let method_calls = parser.find_method_calls(code);
    println!("[CALL-DEBUG] === METHOD CALLS FOUND ===");
    for mc in &method_calls {
        println!(
            "[CALL-DEBUG]   {} -> {} (receiver: {:?}) at line {}",
            mc.caller, mc.method_name, mc.receiver, mc.range.start_line
        );
    }
    println!("[CALL-DEBUG] Total: {} method calls\n", method_calls.len());
}
