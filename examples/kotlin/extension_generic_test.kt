/**
 * Test file for extension function resolution with generic type inference
 * This is the exact challenge from Reddit user natandestroyer
 */

package com.example.extensiontest

// Generic function that returns the same type it receives
fun <T> foo(x: T): T = x

// Extension function on Int receiver
fun Int.bar(): String {
    return "Int.bar() called on $this"
}

// Extension function on String receiver
fun String.bar(): String {
    return "String.bar() called on '$this'"
}

/**
 * Test function demonstrating the challenge:
 * Can Codanna track that:
 * 1. foo(3) returns Int (generic type inference)
 * 2. .bar() on Int resolves to Int.bar() (not String.bar())
 * 3. foo("abc") returns String
 * 4. .bar() on String resolves to String.bar() (not Int.bar())
 */
fun testExtensionResolution() {
    // EXPECTED: Should call Int.bar() because foo(3) returns Int
    val result1 = foo(3).bar()

    // EXPECTED: Should call String.bar() because foo("abc") returns String
    val result2 = foo("abc").bar()

    println(result1)  // Prints: Int.bar() called on 3
    println(result2)  // Prints: String.bar() called on 'abc'
}

// Additional test cases for completeness
fun additionalTests() {
    // Direct extension calls (simpler case)
    val directInt = 42.bar()           // Calls Int.bar()
    val directString = "hello".bar()   // Calls String.bar()

    // Chained with variables
    val num: Int = foo(100)
    val numResult = num.bar()          // Calls Int.bar()

    val text: String = foo("world")
    val textResult = text.bar()        // Calls String.bar()

    // More complex: nested calls
    val nested = foo(foo(5)).bar()     // foo(5) → Int, foo(Int) → Int, .bar() → Int.bar()
}
