// Example Rust code demonstrating various code relationships
// This file showcases all the relationship types our code intelligence system can detect

use std::fmt;

// Trait definition (will be detected as defining methods)
trait Operation {
    fn execute(&self, a: f64, b: f64) -> f64;
    fn name(&self) -> &str;
}

// Struct using another type (Uses relationship)
struct Calculator {
    history: Vec<String>,
    precision: u32,
}

// Another struct
struct Addition;
struct Multiplication;
struct Division {
    safe_mode: bool,
}

// Implementations (Implements relationship)
impl Operation for Addition {
    fn execute(&self, a: f64, b: f64) -> f64 {
        a + b
    }
    
    fn name(&self) -> &str {
        "addition"
    }
}

impl Operation for Multiplication {
    fn execute(&self, a: f64, b: f64) -> f64 {
        a * b
    }
    
    fn name(&self) -> &str {
        "multiplication"
    }
}

impl Operation for Division {
    fn execute(&self, a: f64, b: f64) -> f64 {
        if b != 0.0 {
            a / b
        } else {
            f64::NAN
        }
    }
    
    fn name(&self) -> &str {
        "division"
    }
}

// Methods defined for Calculator (Defines relationship)
impl Calculator {
    fn new() -> Self {
        Self {
            history: Vec::new(),
            precision: 2,
        }
    }
    
    fn set_precision(&mut self, precision: u32) {
        self.precision = precision;
    }
    
    // Function that uses types and calls other functions
    fn calculate(&mut self, op: &dyn Operation, a: f64, b: f64) -> f64 {
        let result = op.execute(a, b);
        let formatted = self.format_result(result);
        self.add_to_history(&formatted);
        result
    }
    
    // Helper function (will be called by calculate)
    fn format_result(&self, value: f64) -> String {
        format!("{:.prec$}", value, prec = self.precision as usize)
    }
    
    // Another helper function
    fn add_to_history(&mut self, entry: &str) {
        self.history.push(entry.to_string());
    }
    
    fn print_history(&self) {
        for entry in &self.history {
            println!("{}", entry);
        }
    }
}

// Free functions demonstrating call relationships
fn create_calculator() -> Calculator {
    Calculator::new()
}

fn perform_calculation(calc: &mut Calculator, a: f64, b: f64) -> f64 {
    let add = Addition;
    calc.calculate(&add, a, b)
}

fn run_demo() {
    let mut calc = create_calculator();
    calc.set_precision(4);
    
    let result = perform_calculation(&mut calc, 10.0, 20.0);
    println!("Result: {}", result);
    
    calc.print_history();
}

fn main() {
    run_demo();
}

// Additional complex relationships
mod analysis {
    use super::*;
    
    pub struct Report {
        calculator: Calculator,
        operations_count: usize,
    }
    
    impl Report {
        pub fn new() -> Self {
            Self {
                calculator: Calculator::new(),
                operations_count: 0,
            }
        }
        
        pub fn analyze(&mut self, values: &[f64]) -> f64 {
            if values.is_empty() {
                return 0.0;
            }
            
            let sum = self.sum_values(values);
            let avg = sum / values.len() as f64;
            
            self.operations_count += 1;
            avg
        }
        
        fn sum_values(&mut self, values: &[f64]) -> f64 {
            let mut sum = 0.0;
            for &val in values {
                sum = self.calculator.calculate(&Addition, sum, val);
            }
            sum
        }
    }
}