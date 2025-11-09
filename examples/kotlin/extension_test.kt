package com.example.test

// Extension function on Int
fun Int.double(): Int {
    return this * 2
}

// Extension function on String
fun String.shout(): String {
    return this.uppercase()
}

fun testLiterals() {
    val x = 42.double()        // Should call Int.double
    val y = "hello".shout()    // Should call String.shout
}
