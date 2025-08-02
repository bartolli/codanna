struct Data {
    value: i32,
}

impl Data {
    fn new() -> Self {
        Data { value: 42 }
    }
    
    fn process(&self) -> i32 {
        self.validate();  // Self call
        self.value * 2
    }
    
    fn validate(&self) {
        println!("Validating: {}", self.value);
    }
}

fn main() {
    let data = Data::new();     // Static call: Data::new
    let result = data.process(); // Instance call: data.process
    println!("Result: {}", result);
    
    // Method chain example
    let text = String::from("hello");
    let len = text.trim().len(); // Chain: text.trim().len()
}