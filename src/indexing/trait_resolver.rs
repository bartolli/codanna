use std::collections::HashMap;
use crate::types::FileId;

/// Tracks trait implementations to help resolve method calls
#[derive(Debug, Default)]
pub struct TraitResolver {
    /// Maps type names to traits they implement
    /// Key: "TypeName", Value: Vec<("TraitName", file_id)>
    type_to_traits: HashMap<String, Vec<(String, FileId)>>,
    
    /// Maps trait names to their methods
    /// Key: "TraitName", Value: Vec<"method_name">
    trait_methods: HashMap<String, Vec<String>>,
    
    /// Maps (type, method) pairs to the trait that defines the method
    /// Key: ("TypeName", "method_name"), Value: "TraitName"
    type_method_to_trait: HashMap<(String, String), String>,
    
    /// Tracks inherent methods on types (methods defined in impl blocks without traits)
    /// Key: "TypeName", Value: Vec<"method_name">
    inherent_methods: HashMap<String, Vec<String>>,
}

impl TraitResolver {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Register that a type implements a trait
    pub fn add_trait_impl(&mut self, type_name: String, trait_name: String, file_id: FileId) {
        self.type_to_traits
            .entry(type_name)
            .or_default()
            .push((trait_name, file_id));
    }
    
    /// Register methods that a trait defines
    pub fn add_trait_methods(&mut self, trait_name: String, methods: Vec<String>) {
        self.trait_methods.insert(trait_name, methods);
    }
    
    /// Register that a specific method on a type comes from a trait
    pub fn add_type_method(&mut self, type_name: String, method_name: String, trait_name: String) {
        self.type_method_to_trait
            .insert((type_name, method_name), trait_name);
    }
    
    /// Add inherent methods for a type
    pub fn add_inherent_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.inherent_methods
            .entry(type_name)
            .or_default()
            .extend(methods);
    }
    
    /// Check if a method is an inherent method on a type
    pub fn is_inherent_method(&self, type_name: &str, method_name: &str) -> bool {
        self.inherent_methods
            .get(type_name)
            .map(|methods| methods.contains(&method_name.to_string()))
            .unwrap_or(false)
    }
    
    /// Given a type and method name, find which trait it comes from
    /// Returns None if it's an inherent method or not found
    pub fn resolve_method_trait(&self, type_name: &str, method_name: &str) -> Option<&str> {
        // Skip if this is an inherent method (Rust prefers inherent methods)
        if self.is_inherent_method(type_name, method_name) {
            return None;
        }
        
        // First check direct mapping
        if let Some(trait_name) = self.type_method_to_trait.get(&(type_name.to_string(), method_name.to_string())) {
            return Some(trait_name);
        }
        
        // Then check if type implements any traits that have this method
        if let Some(traits) = self.type_to_traits.get(type_name) {
            let mut matching_traits = Vec::new();
            
            for (trait_name, _) in traits {
                if let Some(methods) = self.trait_methods.get(trait_name) {
                    if methods.contains(&method_name.to_string()) {
                        matching_traits.push(trait_name.as_str());
                    }
                }
            }
            
            // If multiple traits define the same method, return the first one
            // In real Rust this would be an error requiring disambiguation
            if !matching_traits.is_empty() {
                if matching_traits.len() > 1 {
                    eprintln!("WARNING: Ambiguous method '{}' on type '{}' - found in traits: {:?}", 
                              method_name, type_name, matching_traits);
                }
                return Some(matching_traits[0]);
            }
        }
        
        None
    }
    
    
    /// Get all traits implemented by a type
    pub fn get_implemented_traits(&self, type_name: &str) -> Vec<&str> {
        self.type_to_traits
            .get(type_name)
            .map(|traits| traits.iter().map(|(name, _)| name.as_str()).collect())
            .unwrap_or_default()
    }
    
    /// Get methods defined by a trait
    pub fn get_trait_methods(&self, trait_name: &str) -> Option<Vec<String>> {
        self.trait_methods.get(trait_name).cloned()
    }
    
    /// Clear all trait data
    pub fn clear(&mut self) {
        self.type_to_traits.clear();
        self.trait_methods.clear();
        self.type_method_to_trait.clear();
        self.inherent_methods.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_trait_resolution() {
        let mut resolver = TraitResolver::new();
        
        // Add trait with methods
        resolver.add_trait_methods("Display".to_string(), vec!["fmt".to_string()]);
        
        // Add implementation
        resolver.add_trait_impl("MyStruct".to_string(), "Display".to_string(), FileId(1));
        
        // Should resolve method to trait
        assert_eq!(
            resolver.resolve_method_trait("MyStruct", "fmt"),
            Some("Display")
        );
        
        // Non-existent method should return None
        assert_eq!(
            resolver.resolve_method_trait("MyStruct", "unknown"),
            None
        );
    }
}