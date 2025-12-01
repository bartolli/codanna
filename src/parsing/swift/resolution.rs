//! Swift-specific resolution context and inheritance resolver
//!
//! Provides scoping and inheritance tracking tailored for Swift's language features,
//! including protocols, extensions, and class inheritance.

use crate::parsing::resolution::{ImportBinding, InheritanceResolver, ResolutionScope};
use crate::parsing::{ScopeLevel, ScopeType};
use crate::{FileId, SymbolId};
use std::collections::{HashMap, HashSet};

/// Resolution context implementing Swift scoping rules
///
/// Swift resolution order:
/// Local -> Type Members -> Extension Members -> Protocol -> Module -> Imported
pub struct SwiftResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,
    /// Stack of local scopes (functions/closures)
    local_scopes: Vec<HashMap<String, SymbolId>>,
    /// Stack of type member scopes (class/struct/enum)
    type_scopes: Vec<HashMap<String, SymbolId>>,
    /// Extension-added members (type_name -> method_name -> symbol_id)
    extension_scope: HashMap<String, HashMap<String, SymbolId>>,
    /// Protocol requirements
    protocol_scope: HashMap<String, SymbolId>,
    /// File (module-level) scope
    module_scope: HashMap<String, SymbolId>,
    /// Imported symbols
    import_scope: HashMap<String, SymbolId>,
    /// Active scope stack for contextual decisions
    scope_stack: Vec<ScopeType>,
    /// Registered import bindings
    import_bindings: HashMap<String, ImportBinding>,
    /// Current type name (for extension resolution)
    current_type: Option<String>,
}

impl SwiftResolutionContext {
    /// Create a new resolution context for a file
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scopes: Vec::new(),
            type_scopes: Vec::new(),
            extension_scope: HashMap::new(),
            protocol_scope: HashMap::new(),
            module_scope: HashMap::new(),
            import_scope: HashMap::new(),
            scope_stack: vec![ScopeType::Global],
            import_bindings: HashMap::new(),
            current_type: None,
        }
    }

    /// Set the current type context (for extension method resolution)
    pub fn set_current_type(&mut self, type_name: Option<String>) {
        self.current_type = type_name;
    }

    fn current_local_scope_mut(&mut self) -> &mut HashMap<String, SymbolId> {
        if self.local_scopes.is_empty() {
            self.local_scopes.push(HashMap::new());
        }
        self.local_scopes.last_mut().unwrap()
    }

    fn current_type_scope_mut(&mut self) -> Option<&mut HashMap<String, SymbolId>> {
        self.type_scopes.last_mut()
    }

    fn resolve_in_locals(&self, name: &str) -> Option<SymbolId> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    fn resolve_in_types(&self, name: &str) -> Option<SymbolId> {
        for scope in self.type_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    fn resolve_in_extensions(&self, name: &str) -> Option<SymbolId> {
        // Check if we have a current type context
        if let Some(ref type_name) = self.current_type {
            if let Some(ext_methods) = self.extension_scope.get(type_name) {
                if let Some(&id) = ext_methods.get(name) {
                    return Some(id);
                }
            }
        }

        // Also check all extensions (for static resolution)
        for ext_methods in self.extension_scope.values() {
            if let Some(&id) = ext_methods.get(name) {
                return Some(id);
            }
        }
        None
    }

    /// Add a symbol to the extension scope for a specific type
    pub fn add_extension_symbol(&mut self, type_name: String, name: String, symbol_id: SymbolId) {
        self.extension_scope
            .entry(type_name)
            .or_default()
            .insert(name, symbol_id);
    }
}

impl ResolutionScope for SwiftResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                self.current_local_scope_mut().insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                // If we're inside a type, add to type scope; otherwise module scope
                if matches!(self.scope_stack.last(), Some(ScopeType::Class)) {
                    if let Some(scope) = self.current_type_scope_mut() {
                        scope.insert(name.clone(), symbol_id);
                    }
                }
                self.module_scope.entry(name).or_insert(symbol_id);
            }
            ScopeLevel::Package => {
                self.import_scope.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                self.module_scope.insert(name, symbol_id);
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Swift resolution order:
        // 1. Local scopes (innermost first)
        if let Some(id) = self.resolve_in_locals(name) {
            return Some(id);
        }

        // 2. Type members (from innermost type outward)
        if let Some(id) = self.resolve_in_types(name) {
            return Some(id);
        }

        // 3. Extension members
        if let Some(id) = self.resolve_in_extensions(name) {
            return Some(id);
        }

        // 4. Protocol requirements
        if let Some(&id) = self.protocol_scope.get(name) {
            return Some(id);
        }

        // 5. Module-level definitions
        if let Some(&id) = self.module_scope.get(name) {
            return Some(id);
        }

        // 6. Imported symbols
        if let Some(&id) = self.import_scope.get(name) {
            return Some(id);
        }

        // Handle qualified names like "Type.member"
        if let Some((head, tail)) = name.split_once('.') {
            if self.resolve(head).is_some() {
                // Try to resolve the member in type scope
                if let Some(id) = self.resolve_in_types(tail) {
                    return Some(id);
                }
            }
        }

        None
    }

    fn clear_local_scope(&mut self) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.clear();
        }
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        match scope_type {
            ScopeType::Function { .. } | ScopeType::Block => {
                self.local_scopes.push(HashMap::new());
            }
            ScopeType::Class => {
                self.type_scopes.push(HashMap::new());
            }
            _ => {}
        }
        self.scope_stack.push(scope_type);
    }

    fn exit_scope(&mut self) {
        if let Some(scope) = self.scope_stack.pop() {
            match scope {
                ScopeType::Function { .. } | ScopeType::Block => {
                    self.local_scopes.pop();
                }
                ScopeType::Class => {
                    self.type_scopes.pop();
                }
                _ => {}
            }
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut results = Vec::new();

        // Local scope
        if let Some(local) = self.local_scopes.last() {
            for (name, &id) in local {
                results.push((name.clone(), id, ScopeLevel::Local));
            }
        }

        // Type scope
        if let Some(type_scope) = self.type_scopes.last() {
            for (name, &id) in type_scope {
                results.push((name.clone(), id, ScopeLevel::Module));
            }
        }

        // Module scope
        for (name, &id) in &self.module_scope {
            results.push((name.clone(), id, ScopeLevel::Module));
        }

        // Import scope
        for (name, &id) in &self.import_scope {
            results.push((name.clone(), id, ScopeLevel::Package));
        }

        results
    }

    fn resolve_relationship(
        &self,
        _from_name: &str,
        to_name: &str,
        _kind: crate::RelationKind,
        _from_file: FileId,
    ) -> Option<SymbolId> {
        self.resolve(to_name)
    }

    fn populate_imports(&mut self, _imports: &[crate::parsing::Import]) {
        // Swift imports are module-level, handled by behavior
    }

    fn register_import_binding(&mut self, binding: ImportBinding) {
        if let Some(symbol_id) = binding.resolved_symbol {
            self.import_scope
                .insert(binding.exposed_name.clone(), symbol_id);
        }
        self.import_bindings
            .insert(binding.exposed_name.clone(), binding);
    }

    fn import_binding(&self, name: &str) -> Option<ImportBinding> {
        self.import_bindings.get(name).cloned()
    }
}

/// Inheritance resolver for Swift's class and protocol hierarchy
///
/// Supports:
/// - Single class inheritance
/// - Multiple protocol conformance
/// - Protocol extensions (default implementations)
/// - Type extensions
#[derive(Default)]
pub struct SwiftInheritanceResolver {
    /// child -> parent class (single inheritance)
    class_inheritance: HashMap<String, String>,
    /// type -> protocols it conforms to
    protocol_conformance: HashMap<String, Vec<String>>,
    /// protocol -> default methods from protocol extensions
    protocol_extensions: HashMap<String, HashSet<String>>,
    /// type -> methods added via extensions
    type_extensions: HashMap<String, HashSet<String>>,
    /// type -> methods defined directly on that type
    type_methods: HashMap<String, HashSet<String>>,
}

impl SwiftInheritanceResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a protocol extension method
    pub fn add_protocol_extension_method(&mut self, protocol: String, method: String) {
        self.protocol_extensions
            .entry(protocol)
            .or_default()
            .insert(method);
    }

    /// Add a type extension method
    pub fn add_type_extension_method(&mut self, type_name: String, method: String) {
        self.type_extensions
            .entry(type_name)
            .or_default()
            .insert(method);
    }

    fn resolve_method_recursive(
        &self,
        ty: &str,
        method: &str,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        if !visited.insert(ty.to_string()) {
            return None; // Cycle detection
        }

        // 1. Check methods defined directly on this type
        if self
            .type_methods
            .get(ty)
            .is_some_and(|methods| methods.contains(method))
        {
            return Some(ty.to_string());
        }

        // 2. Check type extension methods
        if self
            .type_extensions
            .get(ty)
            .is_some_and(|methods| methods.contains(method))
        {
            return Some(format!("{ty} (extension)"));
        }

        // 3. Check parent class
        if let Some(parent) = self.class_inheritance.get(ty) {
            if let Some(found) = self.resolve_method_recursive(parent, method, visited) {
                return Some(found);
            }
        }

        // 4. Check protocol conformance and protocol extensions
        if let Some(protocols) = self.protocol_conformance.get(ty) {
            for protocol in protocols {
                // Check if protocol extension provides default implementation
                if self
                    .protocol_extensions
                    .get(protocol)
                    .is_some_and(|methods| methods.contains(method))
                {
                    return Some(format!("{protocol} (extension)"));
                }
            }
        }

        None
    }

    fn collect_chain(&self, ty: &str, visited: &mut HashSet<String>, out: &mut Vec<String>) {
        if !visited.insert(ty.to_string()) {
            return; // Cycle detection
        }

        // Add parent class
        if let Some(parent) = self.class_inheritance.get(ty) {
            out.push(parent.clone());
            self.collect_chain(parent, visited, out);
        }

        // Add conformed protocols
        if let Some(protocols) = self.protocol_conformance.get(ty) {
            for protocol in protocols {
                if visited.insert(protocol.clone()) {
                    out.push(protocol.clone());
                }
            }
        }
    }

    fn gather_methods(&self, ty: &str, visited: &mut HashSet<String>, out: &mut HashSet<String>) {
        if !visited.insert(ty.to_string()) {
            return; // Cycle detection
        }

        // Add methods from this type
        if let Some(methods) = self.type_methods.get(ty) {
            out.extend(methods.iter().cloned());
        }

        // Add extension methods
        if let Some(methods) = self.type_extensions.get(ty) {
            out.extend(methods.iter().cloned());
        }

        // Recursively gather from parent class
        if let Some(parent) = self.class_inheritance.get(ty) {
            self.gather_methods(parent, visited, out);
        }

        // Gather from protocol extensions
        if let Some(protocols) = self.protocol_conformance.get(ty) {
            for protocol in protocols {
                if let Some(methods) = self.protocol_extensions.get(protocol) {
                    out.extend(methods.iter().cloned());
                }
            }
        }
    }

    fn is_subtype_recursive(
        &self,
        child: &str,
        parent: &str,
        visited: &mut HashSet<String>,
    ) -> bool {
        if !visited.insert(child.to_string()) {
            return false; // Cycle detection
        }

        // Check class inheritance
        if let Some(p) = self.class_inheritance.get(child) {
            if p == parent {
                return true;
            }
            if self.is_subtype_recursive(p, parent, visited) {
                return true;
            }
        }

        // Check protocol conformance
        if let Some(protocols) = self.protocol_conformance.get(child) {
            for p in protocols {
                if p == parent {
                    return true;
                }
            }
        }

        false
    }
}

impl InheritanceResolver for SwiftInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        match kind {
            "extends" | "class" => {
                self.class_inheritance.insert(child, parent);
            }
            "implements" | "protocol" | "conforms" => {
                self.protocol_conformance
                    .entry(child)
                    .or_default()
                    .push(parent);
            }
            _ => {
                // Default to protocol conformance
                self.protocol_conformance
                    .entry(child)
                    .or_default()
                    .push(parent);
            }
        }
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods
            .entry(type_name)
            .or_default()
            .extend(methods);
    }

    fn resolve_method(&self, type_name: &str, method_name: &str) -> Option<String> {
        let mut visited = HashSet::new();
        self.resolve_method_recursive(type_name, method_name, &mut visited)
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut chain = Vec::new();
        self.collect_chain(type_name, &mut visited, &mut chain);
        chain
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut methods = HashSet::new();
        self.gather_methods(type_name, &mut visited, &mut methods);
        methods.into_iter().collect()
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        if child == parent {
            return true;
        }

        let mut visited = HashSet::new();
        self.is_subtype_recursive(child, parent, &mut visited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolution_context() {
        let mut ctx = SwiftResolutionContext::new(FileId(1));

        // Add a module-level symbol
        ctx.add_symbol("topLevel".to_string(), SymbolId(1), ScopeLevel::Module);

        // Resolve it
        assert_eq!(ctx.resolve("topLevel"), Some(SymbolId(1)));
    }

    #[test]
    fn test_scope_nesting() {
        let mut ctx = SwiftResolutionContext::new(FileId(1));

        // Add module-level symbol
        ctx.add_symbol("outer".to_string(), SymbolId(1), ScopeLevel::Module);

        // Enter function scope
        ctx.enter_scope(ScopeType::Function { hoisting: false });
        ctx.add_symbol("inner".to_string(), SymbolId(2), ScopeLevel::Local);

        // Both should be resolvable
        assert_eq!(ctx.resolve("outer"), Some(SymbolId(1)));
        assert_eq!(ctx.resolve("inner"), Some(SymbolId(2)));

        // Exit function scope
        ctx.exit_scope();

        // Only outer should be resolvable
        assert_eq!(ctx.resolve("outer"), Some(SymbolId(1)));
        assert_eq!(ctx.resolve("inner"), None);
    }

    #[test]
    fn test_inheritance_resolver() {
        let mut resolver = SwiftInheritanceResolver::new();

        // Set up: Dog extends Animal, Animal conforms to Named
        resolver.add_inheritance("Dog".to_string(), "Animal".to_string(), "extends");
        resolver.add_inheritance("Animal".to_string(), "Named".to_string(), "conforms");

        // Add methods
        resolver.add_type_methods("Animal".to_string(), vec!["makeSound".to_string()]);
        resolver.add_type_methods("Dog".to_string(), vec!["fetch".to_string()]);

        // Test inheritance chain
        let chain = resolver.get_inheritance_chain("Dog");
        assert!(chain.contains(&"Animal".to_string()));

        // Test subtype
        assert!(resolver.is_subtype("Dog", "Animal"));
        assert!(!resolver.is_subtype("Animal", "Dog"));

        // Test method resolution
        assert_eq!(
            resolver.resolve_method("Dog", "fetch"),
            Some("Dog".to_string())
        );
        assert_eq!(
            resolver.resolve_method("Dog", "makeSound"),
            Some("Animal".to_string())
        );
    }

    #[test]
    fn test_protocol_extensions() {
        let mut resolver = SwiftInheritanceResolver::new();

        // Drawable protocol with default implementation
        resolver.add_protocol_extension_method("Drawable".to_string(), "draw".to_string());

        // Rectangle conforms to Drawable
        resolver.add_inheritance("Rectangle".to_string(), "Drawable".to_string(), "conforms");

        // Rectangle should get draw from protocol extension
        let result = resolver.resolve_method("Rectangle", "draw");
        assert!(result.is_some());
        assert!(result.unwrap().contains("Drawable"));
    }
}
