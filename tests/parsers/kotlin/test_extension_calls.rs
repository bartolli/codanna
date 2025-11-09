use codanna::parsing::{LanguageParser, kotlin::KotlinParser};
use codanna::types::SymbolCounter;

#[test]
fn test_extension_function_signatures() {
    let code = r#"
fun Int.bar(): String {
    return "Int.bar()"
}

fun String.bar(): String {
    return "String.bar()"
}
"#;

    let mut parser = KotlinParser::new().unwrap();
    let mut counter = SymbolCounter::new();
    let symbols = parser.parse(code, codanna::FileId(1), &mut counter);

    println!("\nFound {} symbols:", symbols.len());
    for symbol in &symbols {
        println!("  {} - Signature: {:?}", symbol.name, symbol.signature);
    }

    // Find bar functions (qualified with receiver type)
    let bar_funcs: Vec<_> = symbols.iter().filter(|s| s.name.contains(".bar")).collect();

    assert_eq!(bar_funcs.len(), 2, "Should find 2 bar extension functions");

    // Check if signatures contain receiver types
    for func in &bar_funcs {
        let sig = func.signature.as_ref().expect("Should have signature");
        println!("bar signature: {sig}");
        // One should have Int., the other String.
        assert!(
            sig.contains("Int.") || sig.contains("String."),
            "Signature should contain receiver type: {sig}"
        );
    }
}

#[test]
fn test_extension_function_call_tracking() {
    let code = r#"
fun <T> foo(x: T): T = x

fun Int.bar(): String {
    return "Int.bar() called on $this"
}

fun String.bar(): String {
    return "String.bar() called on '$this'"
}

fun testExtensionResolution() {
    val result1 = foo(3).bar()
    val result2 = foo("abc").bar()
}
"#;

    let mut parser = KotlinParser::new().unwrap();
    let method_calls = parser.find_method_calls(code);

    println!("\nFound {} method calls:", method_calls.len());
    for call in &method_calls {
        println!(
            "  {} -> {} (receiver: {:?}, static: {})",
            call.caller, call.method_name, call.receiver, call.is_static
        );
    }

    // We should find at least the .bar() calls
    let bar_calls: Vec<_> = method_calls
        .iter()
        .filter(|c| c.method_name == "bar")
        .collect();

    println!("\nFound {} bar() calls:", bar_calls.len());
    for call in &bar_calls {
        println!("  Receiver: {:?} -> bar()", call.receiver);
    }

    assert!(!bar_calls.is_empty(), "Should find bar() method calls");
}

#[test]
fn test_literal_type_inference() {
    let code = r#"
fun testLiterals() {
    val x = 42.double()
    val y = "hello".shout()
    val z = true.toString()
}
"#;

    let mut parser = KotlinParser::new().unwrap();
    let var_types = parser.find_variable_types(code);

    println!("\nFound {} variable types:", var_types.len());
    for (var_name, type_name, _range) in &var_types {
        println!("  {var_name} -> {type_name}");
    }

    // Should find literal types
    let int_literal = var_types.iter().find(|(var, _, _)| *var == "42");
    assert!(int_literal.is_some(), "Should find integer literal 42");
    assert_eq!(int_literal.unwrap().1, "Int", "42 should map to Int");

    let string_literal = var_types.iter().find(|(var, _, _)| *var == "\"hello\"");
    assert!(string_literal.is_some(), "Should find string literal");
    assert_eq!(
        string_literal.unwrap().1,
        "String",
        "\"hello\" should map to String"
    );

    let bool_literal = var_types.iter().find(|(var, _, _)| *var == "true");
    assert!(bool_literal.is_some(), "Should find boolean literal");
    assert_eq!(
        bool_literal.unwrap().1,
        "Boolean",
        "true should map to Boolean"
    );
}
