// Test fixture with various type definitions and trait implementations

pub trait Operation {
    fn execute(&self, value: i32) -> i32;
    fn name(&self) -> &str;
}

pub struct Addition {
    amount: i32,
}

impl Addition {
    pub fn new(amount: i32) -> Self {
        Self { amount }
    }
}

impl Operation for Addition {
    fn execute(&self, value: i32) -> i32 {
        value + self.amount
    }
    
    fn name(&self) -> &str {
        "addition"
    }
}

pub struct Multiplication {
    factor: i32,
}

impl Multiplication {
    pub fn new(factor: i32) -> Self {
        Self { factor }
    }
}

impl Operation for Multiplication {
    fn execute(&self, value: i32) -> i32 {
        value * self.factor
    }
    
    fn name(&self) -> &str {
        "multiplication"
    }
}

pub enum Calculator {
    Add(Addition),
    Multiply(Multiplication),
}

impl Calculator {
    pub fn apply(&self, value: i32) -> i32 {
        match self {
            Calculator::Add(op) => op.execute(value),
            Calculator::Multiply(op) => op.execute(value),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error: {}", self.message)
    }
}

impl std::error::Error for Error {}