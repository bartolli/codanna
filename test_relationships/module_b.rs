/// Module B with its own types and methods
pub struct ConfigB {
    data: i32,
}

impl ConfigB {
    /// Creates a new ConfigB instance - same method name as ConfigA
    pub fn new(data: i32) -> Self {
        Self { data }
    }
    
    /// Process the config - same method name as ConfigA
    pub fn process(&self) -> i32 {
        self.data * 2
    }
}

/// Helper function in module B - same name as in module A
pub fn helper() -> i32 {
    42
}

/// Function that uses ConfigB
pub fn use_config_b() {
    let config = ConfigB::new(10);
    let result = config.process();
    let help = helper();
}