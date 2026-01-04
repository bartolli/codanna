/// A simple calculator module for testing relationship preservation
pub struct Calculator {
    value: i32,
}

impl Calculator {
    /// Create a new calculator with initial value
    pub fn new(initial: i32) -> Self {
        Calculator { value: initial }
    }

    /// Add a number to the current value
    /// Returns the new total after addition
    pub fn plus(&mut self, x: i32) -> i32 {
        self.value += x;
        self.value
    }

    /// Multiply the current value by the given factor
    pub fn times(&mut self, x: i32) -> i32 {
        self.value *= x;
        self.value
    }

    /// Get the current value stored in the calculator
    pub fn get_value(&self) -> i32 {
        self.value
    }

    /// Subtract a number from the current value
    pub fn subtract(&mut self, x: i32) -> i32 {
        self.value -= x;
        self.value
    }

    /// Reset calculator to zero
    pub fn reset(&mut self) {
        self.value = 0;
    }
}
