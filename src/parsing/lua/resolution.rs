//! Lua-specific resolution and inheritance implementation

use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::symbol::ScopeContext;
use crate::{FileId, SymbolId};
use std::any::Any;
use std::collections::HashMap;

/// Lua-specific resolution context
#[derive(Debug)]
pub struct LuaResolutionContext {
    scope_stack: Vec<LuaScope>,
    imports: HashMap<String, SymbolId>,
    global_symbols: HashMap<String, SymbolId>,
    module_symbols: HashMap<String, SymbolId>,
}

#[derive(Debug)]
struct LuaScope {
    symbols: HashMap<String, SymbolId>,
    #[allow(dead_code)]
    scope_type: ScopeType,
}

impl Default for LuaResolutionContext {
    fn default() -> Self {
        Self {
            scope_stack: vec![LuaScope {
                symbols: HashMap::new(),
                scope_type: ScopeType::Module,
            }],
            imports: HashMap::new(),
            global_symbols: HashMap::new(),
            module_symbols: HashMap::new(),
        }
    }
}

impl LuaResolutionContext {
    pub fn new(_file_id: FileId) -> Self {
        Self {
            scope_stack: vec![LuaScope {
                symbols: HashMap::new(),
                scope_type: ScopeType::Module,
            }],
            imports: HashMap::new(),
            global_symbols: HashMap::new(),
            module_symbols: HashMap::new(),
        }
    }

    pub fn add_import_symbol(&mut self, name: String, symbol_id: SymbolId, _is_type_only: bool) {
        self.imports.insert(name, symbol_id);
    }

    pub fn add_symbol_with_context(
        &mut self,
        name: String,
        symbol_id: SymbolId,
        scope_context: Option<&ScopeContext>,
    ) {
        let scope_level = match scope_context {
            Some(ScopeContext::Global) => ScopeLevel::Global,
            Some(ScopeContext::Module) | Some(ScopeContext::Package) => ScopeLevel::Module,
            Some(ScopeContext::Local { hoisted: true, .. }) => ScopeLevel::Module,
            Some(ScopeContext::Local { hoisted: false, .. }) => ScopeLevel::Local,
            Some(ScopeContext::Parameter) => ScopeLevel::Local,
            Some(ScopeContext::ClassMember { .. }) => ScopeLevel::Module,
            None => ScopeLevel::Module,
        };

        self.add_symbol(name, symbol_id, scope_level);
    }
}

impl ResolutionScope for LuaResolutionContext {
    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Global => {
                self.global_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Module | ScopeLevel::Package => {
                self.module_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Local => {
                if let Some(current_scope) = self.scope_stack.last_mut() {
                    current_scope.symbols.insert(name, symbol_id);
                }
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        for scope in self.scope_stack.iter().rev() {
            if let Some(id) = scope.symbols.get(name) {
                return Some(*id);
            }
        }

        if let Some(id) = self.imports.get(name) {
            return Some(*id);
        }

        if let Some(id) = self.module_symbols.get(name) {
            return Some(*id);
        }

        if let Some(id) = self.global_symbols.get(name) {
            return Some(*id);
        }

        None
    }

    fn clear_local_scope(&mut self) {
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.symbols.clear();
        }
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(LuaScope {
            symbols: HashMap::new(),
            scope_type,
        });
    }

    fn exit_scope(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut result = Vec::new();

        for scope in &self.scope_stack {
            for (name, id) in &scope.symbols {
                result.push((name.clone(), *id, ScopeLevel::Local));
            }
        }

        for (name, id) in &self.module_symbols {
            result.push((name.clone(), *id, ScopeLevel::Module));
        }

        for (name, id) in &self.global_symbols {
            result.push((name.clone(), *id, ScopeLevel::Global));
        }

        result
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Lua inheritance resolver
#[derive(Debug, Default)]
pub struct LuaInheritanceResolver {
    inheritance: HashMap<String, Vec<(String, String)>>,
    type_methods: HashMap<String, Vec<String>>,
}

impl LuaInheritanceResolver {
    pub fn new() -> Self {
        Self {
            inheritance: HashMap::new(),
            type_methods: HashMap::new(),
        }
    }
}

impl InheritanceResolver for LuaInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        self.inheritance
            .entry(child)
            .or_default()
            .push((parent, kind.to_string()));
    }

    fn resolve_method(&self, type_name: &str, method: &str) -> Option<String> {
        let mut to_visit = vec![type_name.to_string()];
        let mut visited = std::collections::HashSet::new();

        while let Some(current) = to_visit.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }

            if let Some(methods) = self.type_methods.get(&current) {
                if methods.iter().any(|m| m == method) {
                    return Some(current);
                }
            }

            if let Some(parents) = self.inheritance.get(&current) {
                for (parent, _kind) in parents {
                    to_visit.push(parent.clone());
                }
            }
        }

        None
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut chain = vec![type_name.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(type_name.to_string());

        let mut to_visit = vec![type_name.to_string()];

        while let Some(current) = to_visit.pop() {
            if let Some(parents) = self.inheritance.get(&current) {
                for (parent, _kind) in parents {
                    if visited.insert(parent.clone()) {
                        chain.push(parent.clone());
                        to_visit.push(parent.clone());
                    }
                }
            }
        }

        chain
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        if child == parent {
            return true;
        }

        let chain = self.get_inheritance_chain(child);
        chain.contains(&parent.to_string())
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods.insert(type_name, methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut methods = Vec::new();
        let chain = self.get_inheritance_chain(type_name);

        for ancestor in chain {
            if let Some(type_methods) = self.type_methods.get(&ancestor) {
                for method in type_methods {
                    if !methods.contains(method) {
                        methods.push(method.clone());
                    }
                }
            }
        }

        methods
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lua_resolution_context() {
        let file_id = FileId::new(1).unwrap();
        let mut context = LuaResolutionContext::new(file_id);

        let sym1 = SymbolId::new(1).unwrap();
        let sym2 = SymbolId::new(2).unwrap();

        context.add_symbol("globalFunc".to_string(), sym1, ScopeLevel::Global);
        context.add_symbol("moduleVar".to_string(), sym2, ScopeLevel::Module);

        assert_eq!(context.resolve("globalFunc"), Some(sym1));
        assert_eq!(context.resolve("moduleVar"), Some(sym2));
        assert_eq!(context.resolve("unknown"), None);
    }

    #[test]
    fn test_lua_scope_stack() {
        let file_id = FileId::new(1).unwrap();
        let mut context = LuaResolutionContext::new(file_id);

        let outer = SymbolId::new(1).unwrap();
        let inner = SymbolId::new(2).unwrap();

        context.add_symbol("x".to_string(), outer, ScopeLevel::Local);

        context.enter_scope(ScopeType::Block);
        context.add_symbol("x".to_string(), inner, ScopeLevel::Local);

        assert_eq!(context.resolve("x"), Some(inner));

        context.exit_scope();

        assert_eq!(context.resolve("x"), Some(outer));
    }

    #[test]
    fn test_lua_inheritance_resolver() {
        let mut resolver = LuaInheritanceResolver::new();

        resolver.add_inheritance("Dog".to_string(), "Animal".to_string(), "metatable");
        resolver.add_inheritance("Cat".to_string(), "Animal".to_string(), "metatable");

        assert!(resolver.is_subtype("Dog", "Animal"));
        assert!(resolver.is_subtype("Cat", "Animal"));
        assert!(!resolver.is_subtype("Animal", "Dog"));

        let chain = resolver.get_inheritance_chain("Dog");
        assert!(chain.contains(&"Dog".to_string()));
        assert!(chain.contains(&"Animal".to_string()));

        resolver.add_type_methods("Animal".to_string(), vec!["speak".to_string()]);
        resolver.add_type_methods("Dog".to_string(), vec!["bark".to_string()]);

        let methods = resolver.get_all_methods("Dog");
        assert!(methods.contains(&"bark".to_string()));
        assert!(methods.contains(&"speak".to_string()));
    }
}
