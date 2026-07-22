use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::symbol::ScopeContext;
use crate::{FileId, SymbolId};
use std::any::Any;
use std::collections::HashMap;

#[derive(Debug)]
pub struct NixResolutionContext {
    scope_stack: Vec<NixScope>,
    imports: HashMap<String, SymbolId>,
    global_symbols: HashMap<String, SymbolId>,
    module_symbols: HashMap<String, SymbolId>,
}

#[derive(Debug)]
struct NixScope {
    symbols: HashMap<String, SymbolId>,
    #[allow(dead_code)]
    scope_type: ScopeType,
}

impl Default for NixResolutionContext {
    fn default() -> Self {
        Self {
            scope_stack: vec![NixScope {
                symbols: HashMap::new(),
                scope_type: ScopeType::Module,
            }],
            imports: HashMap::new(),
            global_symbols: HashMap::new(),
            module_symbols: HashMap::new(),
        }
    }
}

impl NixResolutionContext {
    pub fn new(_file_id: FileId) -> Self {
        Self::default()
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

impl ResolutionScope for NixResolutionContext {
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
        self.scope_stack.push(NixScope {
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
        let mut seen = std::collections::HashSet::new();

        for scope in self.scope_stack.iter().rev() {
            for (name, id) in &scope.symbols {
                if seen.insert(name.clone()) {
                    result.push((name.clone(), *id, ScopeLevel::Local));
                }
            }
        }
        for (name, id) in &self.module_symbols {
            if seen.insert(name.clone()) {
                result.push((name.clone(), *id, ScopeLevel::Module));
            }
        }
        for (name, id) in &self.global_symbols {
            if seen.insert(name.clone()) {
                result.push((name.clone(), *id, ScopeLevel::Global));
            }
        }
        result
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug, Default)]
pub struct NixInheritanceResolver {
    inheritance: HashMap<String, Vec<(String, String)>>,
    type_methods: HashMap<String, Vec<String>>,
}

impl NixInheritanceResolver {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InheritanceResolver for NixInheritanceResolver {
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
                for (parent, _) in parents {
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
                for (parent, _) in parents {
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
        self.get_inheritance_chain(child)
            .contains(&parent.to_string())
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods.insert(type_name, methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut methods = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for ancestor in self.get_inheritance_chain(type_name) {
            if let Some(type_methods) = self.type_methods.get(&ancestor) {
                for method in type_methods {
                    if seen.insert(method.clone()) {
                        methods.push(method.clone());
                    }
                }
            }
        }
        methods
    }
}
