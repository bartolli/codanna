package com.example.reddit

/**
 * Generic identity function with type parameter inference.
 *
 * This function preserves the type of its argument through generic parameter T.
 * When called with an Int, it returns Int. When called with String, returns String.
 * The inferred return type enables extension function resolution in call chains.
 *
 * @param x The value to return unchanged
 * @return The same value with preserved type information
 */
fun <T> foo(x: T): T = x

/**
 * Extension function on Int type.
 *
 * This method is only callable on Int instances or expressions that resolve to Int.
 * When foo(3).bar() is called, the return type of foo(3) is inferred as Int,
 * allowing this extension to be resolved.
 *
 * @return A string describing the Int value this was called on
 */
fun Int.bar(): String {
    return "Int.bar() called on $this"
}

/**
 * Extension function on String type.
 *
 * Parallel extension to Int.bar() but operates on String receivers. Identically-named
 * extensions on different receiver types are disambiguated by the receiver's inferred type.
 * The call foo("abc").bar() resolves here because foo("abc") returns String.
 *
 * @return A string describing the String value this was called on
 */
fun String.bar(): String {
    return "String.bar() called on '$this'"
}

/**
 * Demonstrates generic type flow with extension function resolution.
 *
 * This function shows how extension method calls are resolved when the receiver type
 * depends on generic parameter inference. The resolution process involves:
 *
 * 1. Extract generic parameters from function signatures
 * 2. Infer concrete types from call-site arguments
 * 3. Substitute type parameters in return types
 * 4. Resolve extension functions based on the inferred receiver type
 *
 * Call flow:
 * - foo(3) infers T=Int, returns Int, resolves to Int.bar()
 * - foo("abc") infers T=String, returns String, resolves to String.bar()
 */
fun testGenericFlow() {
    // foo(3) returns Int, so .bar() resolves to Int.bar()
    val result1 = foo(3).bar()

    // foo("abc") returns String, so .bar() resolves to String.bar()
    val result2 = foo("abc").bar()
}
