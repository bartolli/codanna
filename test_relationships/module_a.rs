/// Module A with its own types and methods
pub struct ConfigA {
    value: String,
}

impl ConfigA {
    /// Creates a new ConfigA instance
    pub fn new(value: String) -> Self {
        Self { value }
    }
    
    /// Process the config
    pub fn process(&self) -> String {
        format!("A: {}", self.value)
    }
}

/// Helper function in module A
pub fn helper() -> String {
    "Helper from module A".to_string()
}

/// Function that uses ConfigA
pub fn use_config_a() {
    let config = ConfigA::new("test".to_string());
    let result = config.process();
    let help = helper();
}