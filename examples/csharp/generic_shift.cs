// C# generic shift conflict - exact pattern from research report
class Test {
    void Method() {
        var A = 10;
        var X = 2;
        var B = 1;
        var R = A < X >> B;  // right shift, not generic
    }
}
