// Simple test fixture with basic Rust constructs

fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn multiply(x: f64, y: f64) -> f64 {
    x * y
}

struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    
    fn distance(&self, other: &Point) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

pub struct Rectangle {
    top_left: Point,
    bottom_right: Point,
}

impl Rectangle {
    pub fn width(&self) -> f64 {
        (self.bottom_right.x - self.top_left.x).abs()
    }
    
    pub fn height(&self) -> f64 {
        (self.bottom_right.y - self.top_left.y).abs()
    }
    
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }
}