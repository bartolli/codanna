//! PHP-specific resolution and inheritance implementation
//!
//! This module implements PHP's namespace resolution and inheritance model:
//! - Namespace resolution with PSR-4 autoloading conventions
//! - Class inheritance with single inheritance
//! - Interface implementation (multiple interfaces)
//! - Trait usage with precedence rules

use crate::parsing::resolution::ImportBinding;
use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::{FileId, SymbolId};
use std::collections::HashMap;

/// Type alias for use statement: (alias, full_path)
type UseStatement = (Option<String>, String);

/// Type alias for namespace use statements
type NamespaceUses = HashMap<String, UseStatement>;

/// PHP-specific resolution context implementing namespace scoping
///
/// PHP has a hierarchical namespace system:
/// 1. Current namespace (where we are)
/// 2. Use statements (imported symbols with aliases)
/// 3. Global namespace (with leading \)
pub struct PhpResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,

    /// Current namespace (e.g., "App\\Controllers")
    current_namespace: Option<String>,

    /// Local variables in current scope (function/method)
    local_scope: HashMap<String, SymbolId>,

    /// Class-level properties and methods
    class_scope: HashMap<String, SymbolId>,

    /// Namespace-level symbols (classes, functions, constants)
    namespace_scope: HashMap<String, SymbolId>,

    /// Global symbols (with leading \)
    global_scope: HashMap<String, SymbolId>,

    /// Use statements (alias -> full namespace path)
    use_statements: NamespaceUses,

    /// Track nested scopes
    scope_stack: Vec<ScopeType>,

    /// Current class for method resolution
    current_class: Option<String>,

    /// Binding info for imports keyed by visible name
    import_bindings: HashMap<String, ImportBinding>,
}

impl PhpResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            current_namespace: None,
            local_scope: HashMap::new(),
            class_scope: HashMap::new(),
            namespace_scope: HashMap::new(),
            global_scope: HashMap::new(),
            use_statements: HashMap::new(),
            scope_stack: Vec::new(),
            current_class: None,
            import_bindings: HashMap::new(),
        }
    }

    /// Set the current namespace
    pub fn set_namespace(&mut self, namespace: String) {
        self.current_namespace = Some(namespace);
    }

    /// Add a use statement (with optional alias)
    pub fn add_use_statement(&mut self, alias: Option<String>, full_path: String) {
        let key = alias.clone().unwrap_or_else(|| {
            // Extract the last part of the namespace as the default alias
            full_path
                .rsplit('\\')
                .next()
                .unwrap_or(&full_path)
                .to_string()
        });
        self.use_statements.insert(key, (alias, full_path));
    }

    /// Resolve a name considering use statements and current namespace
    fn resolve_name(&self, name: &str) -> Option<String> {
        // If it starts with \, it's already fully qualified
        if name.starts_with('\\') {
            return Some(name.to_string());
        }

        // Check if it's in use statements
        if let Some((_, full_path)) = self.use_statements.get(name) {
            return Some(full_path.clone());
        }

        // Check if the first part is in use statements (for qualified names)
        if let Some(pos) = name.find('\\') {
            let first_part = &name[..pos];
            if let Some((_, full_path)) = self.use_statements.get(first_part) {
                let rest = &name[pos + 1..];
                return Some(format!("{full_path}\\{rest}"));
            }
        }

        // Otherwise, it's relative to current namespace
        if let Some(ref ns) = self.current_namespace {
            Some(format!("{ns}\\{name}"))
        } else {
            Some(name.to_string())
        }
    }
}

impl ResolutionScope for PhpResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                self.local_scope.insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                // In PHP, Module level maps to class scope
                self.class_scope.insert(name, symbol_id);
            }
            ScopeLevel::Package => {
                // In PHP, Package level maps to namespace scope
                self.namespace_scope.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                self.global_scope.insert(name, symbol_id);
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // PHP resolution order: local → class → namespace → global

        // 1. Check local scope (variables, parameters)
        if let Some(&id) = self.local_scope.get(name) {
            return Some(id);
        }

        // 2. Check class scope (properties, methods)
        if let Some(&id) = self.class_scope.get(name) {
            return Some(id);
        }

        // 3. Try to resolve the name with namespace/use statements
        if let Some(full_name) = self.resolve_name(name) {
            // Check namespace scope with resolved name
            if let Some(&id) = self.namespace_scope.get(&full_name) {
                return Some(id);
            }

            // Check global scope
            if let Some(&id) = self.global_scope.get(&full_name) {
                return Some(id);
            }
        }

        // 4. Check raw name in namespace scope
        if let Some(&id) = self.namespace_scope.get(name) {
            return Some(id);
        }

        // 5. Check global scope
        if let Some(&id) = self.global_scope.get(name) {
            return Some(id);
        }

        // 6. Check if it's a qualified name (contains ::)
        if name.contains("::") {
            // CRITICAL FIX: First try to resolve the full qualified path directly
            // This handles cases where we have the full namespace path stored (e.g., "App\\Services\\Auth::login")
            // Check in all scopes for the full qualified name
            if let Some(&id) = self.namespace_scope.get(name) {
                return Some(id);
            }
            if let Some(&id) = self.global_scope.get(name) {
                return Some(id);
            }

            // If full path not found, try to resolve as a 2-part path
            let parts: Vec<&str> = name.split("::").collect();
            if parts.len() == 2 {
                let class_or_namespace = parts[0];
                let method_or_const = parts[1];

                // Check if class exists in our codebase
                if self.resolve(class_or_namespace).is_some() {
                    // Class exists, resolve the method/constant
                    return self.resolve(method_or_const);
                }
                // External library (like PDO::query) - return None
                return None;
            }
        }

        None
    }

    fn clear_local_scope(&mut self) {
        self.local_scope.clear();
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);
        match scope_type {
            ScopeType::Class => {
                // Entering a class, clear class scope for new members
                self.class_scope.clear();
            }
            ScopeType::Function { .. } => {
                // Entering a function/method, clear locals
                // PHP doesn't hoist, so we ignore the hoisting parameter
                self.clear_local_scope();
            }
            _ => {}
        }
    }

    fn exit_scope(&mut self) {
        if let Some(scope_type) = self.scope_stack.pop() {
            match scope_type {
                ScopeType::Function { .. } => {
                    // Exiting function/method, clear locals
                    self.clear_local_scope();
                }
                ScopeType::Class => {
                    // Exiting class
                    self.current_class = None;
                }
                _ => {}
            }
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut symbols = Vec::new();

        for (name, &id) in &self.local_scope {
            symbols.push((name.clone(), id, ScopeLevel::Local));
        }
        for (name, &id) in &self.class_scope {
            symbols.push((name.clone(), id, ScopeLevel::Module));
        }
        for (name, &id) in &self.namespace_scope {
            symbols.push((name.clone(), id, ScopeLevel::Package));
        }
        for (name, &id) in &self.global_scope {
            symbols.push((name.clone(), id, ScopeLevel::Global));
        }

        symbols
    }

    fn populate_imports(&mut self, imports: &[crate::parsing::Import]) {
        // Convert Import records into use statements
        for import in imports {
            self.add_use_statement(import.alias.clone(), import.path.clone());
        }
    }

    fn register_import_binding(&mut self, binding: ImportBinding) {
        self.import_bindings
            .insert(binding.exposed_name.clone(), binding);
    }

    fn import_binding(&self, name: &str) -> Option<ImportBinding> {
        self.import_bindings.get(name).cloned()
    }
}

/// PHP inheritance resolver
///
/// Handles:
/// - Single class inheritance (extends)
/// - Multiple interface implementation (implements)
/// - Trait usage (use)
#[derive(Clone)]
pub struct PhpInheritanceResolver {
    /// Maps class names to their parent class
    class_extends: HashMap<String, String>,

    /// Maps class names to interfaces they implement
    class_implements: HashMap<String, Vec<String>>,

    /// Maps class names to traits they use
    class_uses_traits: HashMap<String, Vec<String>>,

    /// Maps interface names to interfaces they extend
    interface_extends: HashMap<String, Vec<String>>,

    /// Maps classes/interfaces/traits to their methods
    type_methods: HashMap<String, Vec<String>>,

    /// Maps traits to their methods
    trait_methods: HashMap<String, Vec<String>>,
}

impl Default for PhpInheritanceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl PhpInheritanceResolver {
    pub fn new() -> Self {
        Self {
            class_extends: HashMap::new(),
            class_implements: HashMap::new(),
            class_uses_traits: HashMap::new(),
            interface_extends: HashMap::new(),
            type_methods: HashMap::new(),
            trait_methods: HashMap::new(),
        }
    }

    /// Add a class with its parent
    pub fn add_class_extends(&mut self, class: String, parent: String) {
        self.class_extends.insert(class, parent);
    }

    /// Add interfaces that a class implements
    pub fn add_class_implements(&mut self, class: String, interfaces: Vec<String>) {
        self.class_implements.insert(class, interfaces);
    }

    /// Add traits that a class uses
    pub fn add_class_uses(&mut self, class: String, traits: Vec<String>) {
        self.class_uses_traits.insert(class, traits);
    }

    /// Add interfaces that an interface extends
    pub fn add_interface_extends(&mut self, interface: String, parents: Vec<String>) {
        self.interface_extends.insert(interface, parents);
    }

    /// Add methods to a trait
    pub fn add_trait_methods(&mut self, trait_name: String, methods: Vec<String>) {
        self.trait_methods.insert(trait_name, methods);
    }
}

impl InheritanceResolver for PhpInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        match kind {
            "extends" => {
                self.class_extends.insert(child, parent);
            }
            "implements" => {
                self.class_implements.entry(child).or_default().push(parent);
            }
            "uses" => {
                // Trait usage
                self.class_uses_traits
                    .entry(child)
                    .or_default()
                    .push(parent);
            }
            _ => {}
        }
    }

    fn parent_of(&self, child: &str) -> Option<String> {
        self.class_extends.get(child).cloned()
    }

    fn resolve_method(&self, type_name: &str, method_name: &str) -> Option<String> {
        // PHP method resolution order:
        // 1. Own methods
        // 2. Trait methods (in use order, later traits override earlier)
        // 3. Parent class methods
        // 4. Interface methods (though these are usually abstract)

        // 1. Check own methods
        if let Some(methods) = self.type_methods.get(type_name) {
            if methods.iter().any(|m| m == method_name) {
                return Some(type_name.to_string());
            }
        }

        // 2. Check trait methods
        if let Some(traits) = self.class_uses_traits.get(type_name) {
            // In PHP, later traits override earlier ones
            for trait_name in traits.iter().rev() {
                if let Some(methods) = self.trait_methods.get(trait_name) {
                    if methods.iter().any(|m| m == method_name) {
                        return Some(trait_name.clone());
                    }
                }
            }
        }

        // 3. Check parent class
        if let Some(parent) = self.class_extends.get(type_name) {
            // Recursively check parent
            return self.resolve_method(parent, method_name);
        }

        None
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut chain = vec![type_name.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(type_name.to_string());

        // Add parent class chain
        let mut current = type_name;
        while let Some(parent) = self.class_extends.get(current) {
            if visited.contains(parent) {
                break; // Prevent infinite loop
            }
            chain.push(parent.clone());
            visited.insert(parent.clone());
            current = parent;
        }

        // Add implemented interfaces
        if let Some(interfaces) = self.class_implements.get(type_name) {
            for interface in interfaces {
                if !visited.contains(interface) {
                    chain.push(interface.clone());
                }
            }
        }

        // Add used traits
        if let Some(traits) = self.class_uses_traits.get(type_name) {
            for trait_name in traits {
                if !visited.contains(trait_name) {
                    chain.push(trait_name.clone());
                }
            }
        }

        chain
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        // Check direct parent
        if let Some(direct_parent) = self.class_extends.get(child) {
            if direct_parent == parent {
                return true;
            }
            // Recursively check parent's parents
            if self.is_subtype(direct_parent, parent) {
                return true;
            }
        }

        // Check interfaces
        if let Some(interfaces) = self.class_implements.get(child) {
            if interfaces.iter().any(|i| i == parent) {
                return true;
            }
        }

        // Check traits
        if let Some(traits) = self.class_uses_traits.get(child) {
            if traits.iter().any(|t| t == parent) {
                return true;
            }
        }

        false
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods.insert(type_name, methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut all_methods = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Add own methods
        if let Some(methods) = self.type_methods.get(type_name) {
            for method in methods {
                if seen.insert(method.clone()) {
                    all_methods.push(method.clone());
                }
            }
        }

        // Add trait methods
        if let Some(traits) = self.class_uses_traits.get(type_name) {
            for trait_name in traits {
                if let Some(methods) = self.trait_methods.get(trait_name) {
                    for method in methods {
                        if seen.insert(method.clone()) {
                            all_methods.push(method.clone());
                        }
                    }
                }
            }
        }

        // Add parent methods recursively
        if let Some(parent) = self.class_extends.get(type_name) {
            for method in self.get_all_methods(parent) {
                if seen.insert(method.clone()) {
                    all_methods.push(method);
                }
            }
        }

        all_methods
    }
}
