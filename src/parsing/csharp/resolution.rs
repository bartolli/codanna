//! C#-specific symbol resolution implementation
//!
//! Provides the resolution context and logic for looking up C# symbols based on
//! their scope, imports (using directives), and namespace hierarchy.
//!
//! # Resolution Order
//!
//! C# symbols are resolved in the following order:
//! 1. Local scope (method parameters, local variables)
//! 2. Class/struct member scope
//! 3. Namespace scope (current namespace)
//! 4. Imported symbols (using directives)
//! 5. Global symbols
//! 6. Qualified names (Namespace.Type or Type.Member)
//!
//! This order follows C# language specification for name resolution.

use crate::parsing::resolution::{ImportBinding, ResolutionScope};
use crate::parsing::{ScopeLevel, ScopeType};
use crate::{FileId, SymbolId};
use std::collections::HashMap;

/// C#-specific resolution context
///
/// C# has these resolution features:
/// 1. Namespace scoping
/// 2. Using directives
/// 3. Type vs value space separation (limited compared to TypeScript)
/// 4. Class member visibility
/// 5. Assembly boundaries
pub struct CSharpResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,

    /// Local scope (method parameters, local variables)
    local_scope: HashMap<String, SymbolId>,

    /// Class/struct member scope
    member_scope: HashMap<String, SymbolId>,

    /// Namespace-level symbols
    namespace_symbols: HashMap<String, SymbolId>,

    /// Imported symbols from using directives
    imported_symbols: HashMap<String, SymbolId>,

    /// Global symbols (accessible from anywhere)
    global_symbols: HashMap<String, SymbolId>,

    /// Track nested scopes (methods, blocks, etc.)
    scope_stack: Vec<ScopeType>,

    /// Using directive tracking (namespace -> alias if any)
    using_directives: Vec<(String, Option<String>)>,

    /// Binding info for imports keyed by visible name
    import_bindings: HashMap<String, ImportBinding>,
}

impl CSharpResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scope: HashMap::new(),
            member_scope: HashMap::new(),
            namespace_symbols: HashMap::new(),
            imported_symbols: HashMap::new(),
            global_symbols: HashMap::new(),
            scope_stack: Vec::new(),
            using_directives: Vec::new(),
            import_bindings: HashMap::new(),
        }
    }

    /// Add a using directive
    pub fn add_using(&mut self, namespace: String, alias: Option<String>) {
        self.using_directives.push((namespace, alias));
    }

    /// Add an imported symbol from a using directive
    pub fn add_import_symbol(&mut self, name: String, symbol_id: SymbolId, _is_type_only: bool) {
        // C# doesn't have type-only imports like TypeScript
        self.imported_symbols.insert(name, symbol_id);
    }

    /// Add a symbol with proper scope context
    pub fn add_symbol_with_context(
        &mut self,
        name: String,
        symbol_id: SymbolId,
        scope_context: Option<&crate::symbol::ScopeContext>,
    ) {
        use crate::symbol::ScopeContext;

        match scope_context {
            Some(ScopeContext::Local { .. }) => {
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::ClassMember { .. }) => {
                self.member_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Parameter) => {
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Module) => {
                self.namespace_symbols.insert(name, symbol_id);
            }
            Some(ScopeContext::Package) => {
                self.imported_symbols.insert(name, symbol_id);
            }
            Some(ScopeContext::Global) => {
                self.global_symbols.insert(name, symbol_id);
            }
            None => {
                // Default to local scope if no context
                self.local_scope.insert(name, symbol_id);
            }
        }
    }
}

impl ResolutionScope for CSharpResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                self.local_scope.insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                self.namespace_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Package => {
                self.imported_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                self.global_symbols.insert(name, symbol_id);
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // C# resolution order:
        // 1. Local scope (method parameters, local variables)
        // 2. Class/struct member scope
        // 3. Namespace scope (current namespace)
        // 4. Imported symbols (using directives)
        // 5. Global symbols

        // 1. Check local scope
        if let Some(&id) = self.local_scope.get(name) {
            return Some(id);
        }

        // 2. Check member scope
        if let Some(&id) = self.member_scope.get(name) {
            return Some(id);
        }

        // 3. Check namespace symbols
        if let Some(&id) = self.namespace_symbols.get(name) {
            return Some(id);
        }

        // 4. Check imported symbols
        if let Some(&id) = self.imported_symbols.get(name) {
            return Some(id);
        }

        // 5. Check global symbols
        if let Some(&id) = self.global_symbols.get(name) {
            return Some(id);
        }

        // 6. Handle qualified names (Namespace.Type or Type.Member)
        if name.contains('.') {
            // First try to resolve the full qualified name
            if let Some(&id) = self.imported_symbols.get(name) {
                return Some(id);
            }
            if let Some(&id) = self.namespace_symbols.get(name) {
                return Some(id);
            }
            if let Some(&id) = self.global_symbols.get(name) {
                return Some(id);
            }

            // Try to resolve as Type.Member
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() == 2 {
                let type_name = parts[0];
                let member_name = parts[1];

                // Check if we have the type in our scope
                if self.resolve(type_name).is_some() {
                    // Type exists, try to resolve the member
                    return self.resolve(member_name);
                }

                // Check using aliases
                for (namespace, alias) in &self.using_directives {
                    if let Some(alias_name) = alias {
                        if alias_name == type_name {
                            // This is a using alias, resolve in the target namespace
                            let qualified_name = format!("{namespace}.{member_name}");
                            return self.resolve(&qualified_name);
                        }
                    }
                }
            }
        }

        None
    }

    fn clear_local_scope(&mut self) {
        self.local_scope.clear();
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);
    }

    fn exit_scope(&mut self) {
        self.scope_stack.pop();
        // Clear locals when exiting method/block scope
        if matches!(
            self.scope_stack.last(),
            None | Some(ScopeType::Module | ScopeType::Global)
        ) {
            self.clear_local_scope();
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut symbols = Vec::new();

        // Add all symbols with their appropriate scope levels
        for (name, &id) in &self.local_scope {
            symbols.push((name.clone(), id, ScopeLevel::Local));
        }
        for (name, &id) in &self.imported_symbols {
            symbols.push((name.clone(), id, ScopeLevel::Package));
        }
        for (name, &id) in &self.namespace_symbols {
            symbols.push((name.clone(), id, ScopeLevel::Module));
        }
        for (name, &id) in &self.global_symbols {
            symbols.push((name.clone(), id, ScopeLevel::Global));
        }

        symbols
    }

    fn resolve_relationship(
        &self,
        _from_name: &str,
        to_name: &str,
        kind: crate::RelationKind,
        _from_file: FileId,
    ) -> Option<SymbolId> {
        use crate::RelationKind;

        match kind {
            RelationKind::Implements => {
                // C#: classes and structs implement interfaces
                self.resolve(to_name)
            }
            RelationKind::Extends => {
                // C#: classes inherit from classes
                self.resolve(to_name)
            }
            RelationKind::Uses | RelationKind::References => {
                // General usage/reference
                self.resolve(to_name)
            }
            RelationKind::Calls => {
                // Method call resolution
                self.resolve(to_name)
            }
            RelationKind::Defines => {
                // Definition relationship
                self.resolve(to_name)
            }
            RelationKind::CalledBy
            | RelationKind::ExtendedBy
            | RelationKind::ImplementedBy
            | RelationKind::UsedBy
            | RelationKind::DefinedIn
            | RelationKind::ReferencedBy => {
                // Reverse relationships - typically used for finding references
                self.resolve(to_name)
            }
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
