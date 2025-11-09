use codanna::Range;
use codanna::parsing::{LanguageParser, kotlin::KotlinParser};
use codanna::types::SymbolCounter;

#[test]
fn test_reddit_challenge_parsing() {
    let code = r#"
fun <T> foo(x: T): T = x

fun Int.bar(): String {
    return "Int.bar()"
}

fun String.bar(): String {
    return "String.bar()"
}

fun testGenericFlow() {
    val result1 = foo(3).bar()
    val result2 = foo("abc").bar()
}
"#;

    let mut parser = KotlinParser::new().unwrap();

    let mut counter = SymbolCounter::new();
    let symbols = parser.parse(code, codanna::FileId(1), &mut counter);

    assert!(
        symbols.iter().any(|s| s.name.as_ref() == "Int.bar"),
        "Should register Int.bar extension",
    );
    assert!(
        symbols.iter().any(|s| s.name.as_ref() == "String.bar"),
        "Should register String.bar extension",
    );

    let method_calls = parser.find_method_calls(code);

    let bar_calls: Vec<_> = method_calls
        .iter()
        .filter(|mc| mc.method_name == "bar")
        .collect();
    assert_eq!(bar_calls.len(), 2, "Should find two bar() invocations");

    let receivers: Vec<_> = bar_calls
        .iter()
        .map(|call| call.receiver.clone().unwrap_or_default())
        .collect();
    assert!(
        receivers.contains(&"foo(3)".to_string()),
        "foo(3).bar() should have receiver foo(3)"
    );
    assert!(
        receivers.contains(&"foo(\"abc\")".to_string()),
        "foo(\"abc\").bar() should have receiver foo(\"abc\")"
    );

    let var_types = parser.find_variable_types(code);

    fn assert_type(var_types: &[(&str, &str, Range)], expr: &str, expected: &str) {
        let entry = var_types
            .iter()
            .find(|(value, _, _)| *value == expr)
            .unwrap_or_else(|| panic!("{expr} missing from variable types"));
        assert_eq!(
            entry.1, expected,
            "Expression {expr} should infer as {expected}"
        );
    }

    assert_type(&var_types, "3", "Int");
    assert_type(&var_types, "\"abc\"", "String");
    assert_type(&var_types, "foo(3)", "Int");
    assert_type(&var_types, "foo(\"abc\")", "String");
    assert_type(&var_types, "foo(3).bar()", "String");
    assert_type(&var_types, "foo(\"abc\").bar()", "String");
}
