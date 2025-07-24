// Test fixture demonstrating function calls and relationships

fn helper(x: i32) -> i32 {
    x * 2
}

fn process_single(value: i32) -> i32 {
    let doubled = helper(value);
    doubled + 1
}

fn process_batch(items: Vec<i32>) -> Vec<i32> {
    items.into_iter()
        .map(|x| process_single(x))
        .collect()
}

fn validate(x: i32) -> bool {
    x > 0 && x < 100
}

fn safe_process(value: i32) -> Option<i32> {
    if validate(value) {
        Some(process_single(value))
    } else {
        None
    }
}

fn main() {
    let data = vec![1, 2, 3, 4, 5];
    let results = process_batch(data);
    
    for value in results {
        if let Some(processed) = safe_process(value) {
            println!("Processed: {}", processed);
        }
    }
}

// Recursive example
fn factorial(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

// Mutual recursion
fn is_even(n: u32) -> bool {
    if n == 0 {
        true
    } else {
        is_odd(n - 1)
    }
}

fn is_odd(n: u32) -> bool {
    if n == 0 {
        false
    } else {
        is_even(n - 1)
    }
}