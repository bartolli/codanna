/**
 * Simple extension function test without generics
 */
package com.example.simple

// Extension function on Int
fun Int.double(): Int {
    return this * 2
}

// Extension function on String
fun String.shout(): String {
    return this.uppercase()
}

fun testDirectCalls() {
    // Direct literal calls - receiver type is obvious
    val x = 42.double()        // Should resolve to Int.double()
    val y = "hello".shout()    // Should resolve to String.shout()

    // Variable calls - receiver type from variable
    val num: Int = 10
    val result1 = num.double()  // Should resolve to Int.double()

    val text: String = "world"
    val result2 = text.shout()  // Should resolve to String.shout()
}
