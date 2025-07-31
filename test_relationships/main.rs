mod module_a;
mod module_b;

use module_a::ConfigA;
use module_b::ConfigB;

fn main() {
    // Use both configs
    let config_a = ConfigA::new("main".to_string());
    let result_a = config_a.process();
    
    let config_b = ConfigB::new(100);
    let result_b = config_b.process();
    
    // Call module-specific helpers
    let help_a = module_a::helper();
    let help_b = module_b::helper();
}