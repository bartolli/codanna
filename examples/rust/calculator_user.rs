/// A module that uses Calculator to test relationship preservation on reindex
use crate::calculator::Calculator;

/// Process some numbers using the calculator
pub fn process_numbers() -> i32 {
    let mut calc = Calculator::new(10);
    calc.add(5);
    calc.times(2);
    calc.get_value()
}

/// Another function that uses the calculator
pub fn compute_total(values: &[i32]) -> i32 {
    let mut calc = Calculator::new(0);
    for v in values {
        calc.add(*v);
    }
    calc.get_value()
}

/// Compute difference between two values
pub fn compute_difference(a: i32, b: i32) -> i32 {
    let mut calc = Calculator::new(a);
    calc.subtract(b);
    calc.get_value()
}
