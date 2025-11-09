use codanna::parsing::{LanguageParser, kotlin::KotlinParser};
use codanna::types::SymbolCounter;

#[test]
fn test_extension_function_resolution_with_literals() {
    let code = r#"
// Extension function on Int
fun Int.double(): Int {
    return this * 2
}

// Extension function on String
fun String.shout(): String {
    return this.uppercase()
}

fun testDirectCalls() {
    val x = 42.double()        // Should resolve to Int.double()
    val y = "hello".shout()    // Should resolve to String.shout()
}
"#;

    let mut parser = KotlinParser::new().unwrap();

    // Step 1: Parse and extract symbols
    let mut counter = SymbolCounter::new();
    let symbols = parser.parse(code, codanna::FileId(1), &mut counter);

    println!("\n=== SYMBOLS EXTRACTED ===");
    for symbol in &symbols {
        println!("  {} (kind: {:?})", symbol.name, symbol.kind);
        if let Some(sig) = &symbol.signature {
            println!("    Signature: {sig}");
        }
    }

    // Verify extension functions have receiver types in their names
    let int_double = symbols.iter().find(|s| s.name.as_ref() == "Int.double");
    assert!(
        int_double.is_some(),
        "Should find Int.double extension function"
    );

    let string_shout = symbols.iter().find(|s| s.name.as_ref() == "String.shout");
    assert!(
        string_shout.is_some(),
        "Should find String.shout extension function"
    );

    // Step 2: Extract method calls with receivers
    let method_calls = parser.find_method_calls(code);

    println!("\n=== METHOD CALLS EXTRACTED ===");
    for mc in &method_calls {
        println!(
            "  {} -> {} (receiver: {:?})",
            mc.caller, mc.method_name, mc.receiver
        );
    }

    let double_call = method_calls.iter().find(|mc| mc.method_name == "double");
    assert!(double_call.is_some(), "Should find double() call");
    assert_eq!(
        double_call.unwrap().receiver,
        Some("42".to_string()),
        "Receiver should be '42'"
    );

    let shout_call = method_calls.iter().find(|mc| mc.method_name == "shout");
    assert!(shout_call.is_some(), "Should find shout() call");
    assert_eq!(
        shout_call.unwrap().receiver,
        Some("\"hello\"".to_string()),
        "Receiver should be '\"hello\"'"
    );

    // Step 3: Extract variable types (literal type inference)
    let var_types = parser.find_variable_types(code);

    println!("\n=== VARIABLE TYPES INFERRED ===");
    for (var, typ, _) in &var_types {
        println!("  {var} -> {typ}");
    }

    let int_literal = var_types.iter().find(|(var, _, _)| *var == "42");
    assert!(int_literal.is_some(), "Should infer type for literal 42");
    assert_eq!(int_literal.unwrap().1, "Int", "42 should be typed as Int");

    let string_literal = var_types.iter().find(|(var, _, _)| *var == "\"hello\"");
    assert!(
        string_literal.is_some(),
        "Should infer type for literal \"hello\""
    );
    assert_eq!(
        string_literal.unwrap().1,
        "String",
        "\"hello\" should be typed as String"
    );

    println!("\n=== TEST SUMMARY ===");
    println!("✓ Extension functions extracted with receiver types");
    println!("✓ Method calls extracted with literal receivers");
    println!("✓ Literal types inferred correctly");
    println!("\nNext: Indexer should resolve:");
    println!("  - 42.double() -> Int.double");
    println!("  - \"hello\".shout() -> String.shout");
}
