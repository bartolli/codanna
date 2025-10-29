//! GDScript-specific resolution context and inheritance resolver
//!
//! Provides lightweight scoping and inheritance tracking tailored for Godot's
//! GDScript language. GDScript shares similarities with Python-style modules
//! and classes, but has its own surface syntax and export semantics.

use crate::parsing::resolution::{ImportBinding, InheritanceResolver, ResolutionScope};
use crate::parsing::{ScopeLevel, ScopeType};
use crate::{FileId, SymbolId};
use std::collections::{HashMap, HashSet};

/// Resolution context implementing GDScript scoping rules
pub struct GdscriptResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,
    /// Stack of local scopes (functions/blocks)
    local_scopes: Vec<HashMap<String, SymbolId>>,
    /// Stack of class member scopes (innermost class last)
    class_scopes: Vec<HashMap<String, SymbolId>>,
    /// Script (module) scope
    module_scope: HashMap<String, SymbolId>,
    /// Globally exposed symbols (e.g., via `class_name`)
    global_scope: HashMap<String, SymbolId>,
    /// Imported symbols (via `@tool` or preload/import statements)
    import_scope: HashMap<String, SymbolId>,
    /// Active scope stack for contextual decisions
    scope_stack: Vec<ScopeType>,
    /// Registered import bindings available to the script
    import_bindings: HashMap<String, ImportBinding>,
}

impl GdscriptResolutionContext {
    /// Create a new resolution context for a file
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scopes: Vec::new(),
            class_scopes: Vec::new(),
            module_scope: HashMap::new(),
            global_scope: HashMap::new(),
            import_scope: HashMap::new(),
            scope_stack: vec![ScopeType::Global],
            import_bindings: HashMap::new(),
        }
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
}

impl ResolutionScope for GdscriptResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                self.current_local_scope_mut().insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                // If we're inside a class, treat as class member; otherwise module-level
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
        // Check local scopes first (closest scope wins)
        if let Some(id) = self.resolve_in_locals(name) {
            return Some(id);
        }

        // Then class members (from innermost class outward)
        if let Some(id) = self.resolve_in_classes(name) {
            return Some(id);
        }

        // Module-level definitions
        if let Some(&id) = self.module_scope.get(name) {
            return Some(id);
        }

        // Imported names
        if let Some(&id) = self.import_scope.get(name) {
            return Some(id);
        }

        // Global exports (class_name, autoloads)
        if let Some(&id) = self.global_scope.get(name) {
            return Some(id);
        }

        // Handle qualified access like "Player.move"
        if let Some((head, tail)) = name.split_once('.') {
            // Prefer resolving the head, then attempt tail within class scope
            if let Some(class_id) = self.resolve(head) {
                // If the head resolves to the current class, search class scope for the member
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
                }
                _ => {}
            }
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut results = Vec::new();

        if let Some(local) = self.local_scopes.last() {
            for (name, &id) in local {
                results.push((name.clone(), id, ScopeLevel::Local));
            }
        }

        if let Some(class_scope) = self.class_scopes.last() {
            for (name, &id) in class_scope {
                results.push((name.clone(), id, ScopeLevel::Module));
            }
        }

        for (name, &id) in &self.module_scope {
            results.push((name.clone(), id, ScopeLevel::Module));
        }

        for (name, &id) in &self.import_scope {
            results.push((name.clone(), id, ScopeLevel::Package));
        }

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
        // GDScript imports (preload/load) do not map directly to symbol IDs at this stage.
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

/// Minimal inheritance resolver for GDScript's single inheritance tree
#[derive(Default)]
pub struct GdscriptInheritanceResolver {
    /// child -> parents
    parents: HashMap<String, Vec<String>>,
    /// type -> methods defined directly on that type
    type_methods: HashMap<String, HashSet<String>>,
}

impl GdscriptInheritanceResolver {
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
            return None;
        }

        if self
            .type_methods
            .get(ty)
            .is_some_and(|methods| methods.contains(method))
        {
            return Some(ty.to_string());
        }

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
            return;
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
            return;
        }

        if let Some(methods) = self.type_methods.get(ty) {
            out.extend(methods.iter().cloned());
        }

        if let Some(parents) = self.parents.get(ty) {
            for parent in parents {
                self.gather_methods(parent, visited, out);
            }
        }
    }
}

impl InheritanceResolver for GdscriptInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, _kind: &str) {
        self.parents.entry(child).or_default().push(parent);
    }

    fn resolve_method(&self, type_name: &str, method: &str) -> Option<String> {
        self.resolve_method_recursive(type_name, method, &mut HashSet::new())
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut chain = Vec::new();
        self.collect_chain(type_name, &mut visited, &mut chain);
        chain
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        self.get_inheritance_chain(child)
            .contains(&parent.to_string())
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        let entry = self.type_methods.entry(type_name).or_default();
        entry.extend(methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut collected = HashSet::new();
        self.gather_methods(type_name, &mut visited, &mut collected);
        collected.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolution_context_basic_scopes() {
        let file_id = FileId::new(1).unwrap();
        let mut context = GdscriptResolutionContext::new(file_id);

        let player_id = SymbolId::new(10).unwrap();
        context.add_symbol("Player".to_string(), player_id, ScopeLevel::Module);
        assert_eq!(context.resolve("Player"), Some(player_id));

        context.enter_scope(ScopeType::Class);
        let move_id = SymbolId::new(20).unwrap();
        context.add_symbol("move".to_string(), move_id, ScopeLevel::Module);
        assert_eq!(context.resolve("move"), Some(move_id));

        context.enter_scope(ScopeType::function());
        let temp_id = SymbolId::new(30).unwrap();
        context.add_symbol("temp".to_string(), temp_id, ScopeLevel::Local);
        assert_eq!(context.resolve("temp"), Some(temp_id));

        context.exit_scope(); // function
        assert!(context.resolve("temp").is_none());

        context.exit_scope(); // class
        assert_eq!(context.resolve("Player"), Some(player_id));
        assert_eq!(context.resolve("move"), Some(move_id));
    }

    #[test]
    fn test_inheritance_resolver() {
        let mut resolver = GdscriptInheritanceResolver::new();
        resolver.add_inheritance(
            "Player".to_string(),
            "CharacterBody2D".to_string(),
            "extends",
        );
        resolver.add_inheritance(
            "CharacterBody2D".to_string(),
            "Node2D".to_string(),
            "extends",
        );
        resolver.add_type_methods(
            "CharacterBody2D".to_string(),
            vec!["physics_process".to_string()],
        );
        resolver.add_type_methods("Player".to_string(), vec!["jump".to_string()]);

        assert!(resolver.is_subtype("Player", "Node2D"));
        assert_eq!(
            resolver.resolve_method("Player", "physics_process"),
            Some("CharacterBody2D".to_string())
        );
        let mut methods = resolver.get_all_methods("Player");
        methods.sort();
        assert_eq!(
            methods,
            vec!["jump".to_string(), "physics_process".to_string()]
        );
    }
}
