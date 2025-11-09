//! Kotlin-specific resolution context and inheritance resolver
//!
//! Provides scoping and inheritance tracking tailored for Kotlin's language features,
//! including package-based modules, nested classes, companion objects, and interfaces.

use crate::parsing::resolution::{ImportBinding, InheritanceResolver, ResolutionScope};
use crate::parsing::{ScopeLevel, ScopeType};
use crate::{FileId, SymbolId};
use std::collections::{HashMap, HashSet};

/// Resolution context implementing Kotlin scoping rules
pub struct KotlinResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,
    /// Stack of local scopes (functions/blocks/lambdas)
    local_scopes: Vec<HashMap<String, SymbolId>>,
    /// Stack of class member scopes (for nested classes, innermost last)
    class_scopes: Vec<HashMap<String, SymbolId>>,
    /// Companion object scopes (parallel to class_scopes)
    companion_scopes: Vec<HashMap<String, SymbolId>>,
    /// File (top-level) scope
    module_scope: HashMap<String, SymbolId>,
    /// Package-level scope (imported symbols)
    import_scope: HashMap<String, SymbolId>,
    /// Global scope (stdlib, etc.)
    global_scope: HashMap<String, SymbolId>,
    /// Active scope stack for contextual decisions
    scope_stack: Vec<ScopeType>,
    /// Registered import bindings available to the file
    import_bindings: HashMap<String, ImportBinding>,
    /// Expression-to-type mappings for receiver inference
    expression_types: HashMap<String, String>,
}

impl KotlinResolutionContext {
    /// Create a new resolution context for a file
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scopes: Vec::new(),
            class_scopes: Vec::new(),
            companion_scopes: Vec::new(),
            module_scope: HashMap::new(),
            import_scope: HashMap::new(),
            global_scope: HashMap::new(),
            scope_stack: vec![ScopeType::Global],
            import_bindings: HashMap::new(),
            expression_types: HashMap::new(),
        }
    }

    /// Inject expression type mappings produced by the parser
    pub fn set_expression_types(&mut self, entries: HashMap<String, String>) {
        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "[KOTLIN-RESOLVE] Loaded {} expression types into resolution context for file {:?}",
                entries.len(),
                self.file_id
            );
        }
        self.expression_types = entries;
    }

    fn current_local_scope_mut(&mut self) -> &mut HashMap<String, SymbolId> {
        if self.local_scopes.is_empty() {
            self.local_scopes.push(HashMap::new());
        }
        self.local_scopes.last_mut().unwrap()
    }

    fn current_class_scope_mut(&mut self) -> Option<&mut HashMap<String, SymbolId>> {
        self.class_scopes.last_mut()
    }

    fn resolve_in_locals(&self, name: &str) -> Option<SymbolId> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    fn resolve_in_classes(&self, name: &str) -> Option<SymbolId> {
        for scope in self.class_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    fn resolve_in_companions(&self, name: &str) -> Option<SymbolId> {
        for scope in self.companion_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }
}

impl ResolutionScope for KotlinResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                self.current_local_scope_mut().insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                // If we're inside a class, treat as class member; otherwise file-level
                if matches!(self.scope_stack.last(), Some(ScopeType::Class)) {
                    if let Some(scope) = self.current_class_scope_mut() {
                        scope.insert(name.clone(), symbol_id);
                    }
                }
                self.module_scope.entry(name).or_insert(symbol_id);
            }
            ScopeLevel::Package => {
                self.import_scope.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                self.global_scope.insert(name.clone(), symbol_id);
                self.module_scope.entry(name).or_insert(symbol_id);
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Kotlin resolution order:
        // 1. Local scopes (innermost first)
        if let Some(id) = self.resolve_in_locals(name) {
            return Some(id);
        }

        // 2. Class members (from innermost class outward)
        if let Some(id) = self.resolve_in_classes(name) {
            return Some(id);
        }

        // 3. Companion object members
        if let Some(id) = self.resolve_in_companions(name) {
            return Some(id);
        }

        // 4. File-level definitions (top-level functions, properties)
        if let Some(&id) = self.module_scope.get(name) {
            return Some(id);
        }

        // 5. Imported symbols
        if let Some(&id) = self.import_scope.get(name) {
            return Some(id);
        }

        // 6. Global/stdlib
        if let Some(&id) = self.global_scope.get(name) {
            return Some(id);
        }

        // Handle qualified names like "MyClass.companion" or "Outer.Inner"
        if let Some((head, tail)) = name.split_once('.') {
            // Try to resolve the head first
            if let Some(class_id) = self.resolve(head) {
                // If head resolves, try to find the tail in class scope
                // This handles nested classes, companion objects, etc.
                if let Some(id) = self.resolve_in_classes(tail) {
                    return Some(id);
                }
                return Some(class_id);
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
                self.class_scopes.push(HashMap::new());
                self.companion_scopes.push(HashMap::new());
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
                    self.class_scopes.pop();
                    self.companion_scopes.pop();
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

        // Class scope
        if let Some(class_scope) = self.class_scopes.last() {
            for (name, &id) in class_scope {
                results.push((name.clone(), id, ScopeLevel::Module));
            }
        }

        // Companion scope
        if let Some(companion_scope) = self.companion_scopes.last() {
            for (name, &id) in companion_scope {
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

        // Global scope
        for (name, &id) in &self.global_scope {
            results.push((name.clone(), id, ScopeLevel::Global));
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
        // Kotlin imports are resolved through the behavior's import matching
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

    fn resolve_expression_type(&self, expr: &str) -> Option<String> {
        self.expression_types.get(expr).cloned()
    }
}

/// Inheritance resolver for Kotlin's class hierarchy
/// Supports single inheritance (extends) and multiple interface implementation
#[derive(Default)]
pub struct KotlinInheritanceResolver {
    /// child -> immediate parents (superclass + interfaces)
    parents: HashMap<String, Vec<String>>,
    /// type -> methods defined directly on that type
    type_methods: HashMap<String, HashSet<String>>,
}

impl KotlinInheritanceResolver {
    pub fn new() -> Self {
        Self::default()
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

        // Check if method is defined on this type
        if self
            .type_methods
            .get(ty)
            .is_some_and(|methods| methods.contains(method))
        {
            return Some(ty.to_string());
        }

        // Search in parents (superclass and interfaces)
        if let Some(parents) = self.parents.get(ty) {
            for parent in parents {
                if let Some(found) = self.resolve_method_recursive(parent, method, visited) {
                    return Some(found);
                }
            }
        }

        None
    }

    fn collect_chain(&self, ty: &str, visited: &mut HashSet<String>, out: &mut Vec<String>) {
        if !visited.insert(ty.to_string()) {
            return; // Cycle detection
        }
        if let Some(parents) = self.parents.get(ty) {
            for parent in parents {
                out.push(parent.clone());
                self.collect_chain(parent, visited, out);
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

        // Recursively gather from parents
        if let Some(parents) = self.parents.get(ty) {
            for parent in parents {
                self.gather_methods(parent, visited, out);
            }
        }
    }
}

impl InheritanceResolver for KotlinInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, _kind: &str) {
        self.parents.entry(child).or_default().push(parent);
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

impl KotlinInheritanceResolver {
    fn is_subtype_recursive(
        &self,
        child: &str,
        parent: &str,
        visited: &mut HashSet<String>,
    ) -> bool {
        if !visited.insert(child.to_string()) {
            return false; // Cycle detection
        }

        if let Some(parents) = self.parents.get(child) {
            for p in parents {
                if p == parent {
                    return true;
                }
                if self.is_subtype_recursive(p, parent, visited) {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolution_context() {
        let mut ctx = KotlinResolutionContext::new(FileId(1));

        // Add a module-level symbol
        ctx.add_symbol("topLevel".to_string(), SymbolId(1), ScopeLevel::Module);

        // Resolve it
        assert_eq!(ctx.resolve("topLevel"), Some(SymbolId(1)));

        // Enter a class scope
        ctx.enter_scope(ScopeType::Class);
        ctx.add_symbol("member".to_string(), SymbolId(2), ScopeLevel::Module);

        // Both should be visible
        assert_eq!(ctx.resolve("topLevel"), Some(SymbolId(1)));
        assert_eq!(ctx.resolve("member"), Some(SymbolId(2)));

        ctx.exit_scope();

        // Top-level still visible, member not visible outside class
        assert_eq!(ctx.resolve("topLevel"), Some(SymbolId(1)));
    }

    #[test]
    fn test_inheritance_resolver() {
        let mut resolver = KotlinInheritanceResolver::new();

        // Define inheritance: Child -> Parent -> GrandParent
        resolver.add_inheritance("Child".to_string(), "Parent".to_string(), "extends");
        resolver.add_inheritance("Parent".to_string(), "GrandParent".to_string(), "extends");

        // Add methods
        resolver.add_type_methods("GrandParent".to_string(), vec!["grandMethod".to_string()]);
        resolver.add_type_methods("Parent".to_string(), vec!["parentMethod".to_string()]);
        resolver.add_type_methods("Child".to_string(), vec!["childMethod".to_string()]);

        // Test method resolution
        assert_eq!(
            resolver.resolve_method("Child", "childMethod"),
            Some("Child".to_string())
        );
        assert_eq!(
            resolver.resolve_method("Child", "parentMethod"),
            Some("Parent".to_string())
        );
        assert_eq!(
            resolver.resolve_method("Child", "grandMethod"),
            Some("GrandParent".to_string())
        );

        // Test subtype checking
        assert!(resolver.is_subtype("Child", "Parent"));
        assert!(resolver.is_subtype("Child", "GrandParent"));
        assert!(!resolver.is_subtype("Parent", "Child"));
    }
}
