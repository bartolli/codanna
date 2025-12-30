/// Main application that uses the calculator through process_numbers
use crate::calculator_user::{process_numbers, compute_total};

/// Entry point that calls process_numbers (which calls Calculator::new)
pub fn run_app() {
    let result = process_numbers();
    println!("Result: {}", result);
}

/// Another entry that uses compute_total
pub fn batch_process() {
    let values = vec![1, 2, 3, 4, 5];
    let total = compute_total(&values);
    println!("Total: {}", total);
}
