//! Clojure-specific symbol resolution
//!
//! Clojure has a simpler scoping model than OOP languages:
//! - Namespace-level vars (def, defn, defmacro)
//! - Local bindings (let, loop, fn params)
//! - Imported vars (require, use)

use crate::parsing::resolution::ImportBinding;
use crate::parsing::{Import, ResolutionScope, ScopeLevel, ScopeType};
use crate::{FileId, RelationKind, SymbolId};
use std::collections::HashMap;

/// Clojure resolution context
///
/// Clojure has a simpler scoping model than OOP languages:
/// - Namespace-level vars (def, defn, defmacro)
/// - Local bindings (let, loop, fn params)
/// - Imported vars (require, use)
pub struct ClojureResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,

    /// Local bindings (let, fn params)
    local_scope: HashMap<String, SymbolId>,

    /// Namespace-level symbols
    namespace_scope: HashMap<String, SymbolId>,

    /// Imported/referred symbols
    imported_scope: HashMap<String, SymbolId>,

    /// Current namespace name
    current_namespace: Option<String>,

    /// Scope stack for nested contexts
    scope_stack: Vec<ScopeType>,

    /// Import bindings keyed by visible name
    import_bindings: HashMap<String, ImportBinding>,
}

impl ClojureResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scope: HashMap::new(),
            namespace_scope: HashMap::new(),
            imported_scope: HashMap::new(),
            current_namespace: None,
            scope_stack: Vec::new(),
            import_bindings: HashMap::new(),
        }
    }

    /// Set the current namespace
    pub fn set_namespace(&mut self, ns: String) {
        self.current_namespace = Some(ns);
    }

    /// Get the current namespace
    pub fn current_namespace(&self) -> Option<&str> {
        self.current_namespace.as_deref()
    }
}

impl ResolutionScope for ClojureResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Resolution order for Clojure:
        // 1. Local bindings (let, fn params)
        // 2. Namespace vars
        // 3. Imported/referred vars

        // 1. Local scope
        if let Some(&id) = self.local_scope.get(name) {
            return Some(id);
        }

        // 2. Namespace scope
        if let Some(&id) = self.namespace_scope.get(name) {
            return Some(id);
        }

        // 3. Imported scope
        if let Some(&id) = self.imported_scope.get(name) {
            return Some(id);
        }

        // 4. Check for qualified name lookup (namespace/var)
        if name.contains('/') {
            let parts: Vec<&str> = name.splitn(2, '/').collect();
            if parts.len() == 2 {
                let ns_alias = parts[0];
                let var_name = parts[1];

                // Try to resolve via alias first
                if let Some(&id) = self.imported_scope.get(ns_alias) {
                    // Found the namespace, now look for the var
                    // This is a simplified approach - in a real impl we'd look up
                    // the actual namespace and find the var there
                    return Some(id);
                }

                // Try direct namespace.var lookup
                let full_name = format!("{ns_alias}.{var_name}");
                if let Some(&id) = self.namespace_scope.get(&full_name) {
                    return Some(id);
                }
            }
        }

        None
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                self.local_scope.insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                self.namespace_scope.insert(name, symbol_id);
            }
            ScopeLevel::Package => {
                self.imported_scope.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                self.namespace_scope.insert(name, symbol_id);
            }
        }
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);
    }

    fn exit_scope(&mut self) {
        if let Some(scope) = self.scope_stack.pop() {
            if matches!(scope, ScopeType::Function { .. }) {
                self.clear_local_scope();
            }
        }
    }

    fn clear_local_scope(&mut self) {
        self.local_scope.clear();
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut symbols = Vec::new();

        for (name, &id) in &self.local_scope {
            symbols.push((name.clone(), id, ScopeLevel::Local));
        }
        for (name, &id) in &self.namespace_scope {
            symbols.push((name.clone(), id, ScopeLevel::Module));
        }
        for (name, &id) in &self.imported_scope {
            symbols.push((name.clone(), id, ScopeLevel::Package));
        }

        symbols
    }

    fn resolve_relationship(
        &self,
        _from_name: &str,
        to_name: &str,
        kind: RelationKind,
        _from_file: FileId,
    ) -> Option<SymbolId> {
        match kind {
            RelationKind::Calls => {
                // Handle qualified calls like clojure.string/join
                if to_name.contains('/') {
                    if let Some(id) = self.resolve(to_name) {
                        return Some(id);
                    }
                    // Try just the function name
                    if let Some(fn_name) = to_name.rsplit('/').next() {
                        return self.resolve(fn_name);
                    }
                }
                self.resolve(to_name)
            }
            RelationKind::Defines => {
                // Protocol method definitions
                self.resolve(to_name)
            }
            RelationKind::Extends => {
                // Protocol extensions
                self.resolve(to_name)
            }
            _ => self.resolve(to_name),
        }
    }

    fn populate_imports(&mut self, imports: &[Import]) {
        for import in imports {
            // Handle aliased imports: [clojure.string :as str]
            if let Some(alias) = &import.alias {
                // Store both the alias and the full path
                self.import_bindings.insert(
                    alias.clone(),
                    ImportBinding {
                        import: import.clone(),
                        exposed_name: alias.clone(),
                        origin: crate::parsing::resolution::ImportOrigin::Unknown,
                        resolved_symbol: None,
                    },
                );
            } else {
                // Store using the last segment of the path
                let name = import
                    .path
                    .rsplit('.')
                    .next()
                    .unwrap_or(&import.path)
                    .to_string();
                self.import_bindings.insert(
                    name.clone(),
                    ImportBinding {
                        import: import.clone(),
                        exposed_name: name,
                        origin: crate::parsing::resolution::ImportOrigin::Unknown,
                        resolved_symbol: None,
                    },
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolution_context_creation() {
        let file_id = FileId::new(1).unwrap();
        let context = ClojureResolutionContext::new(file_id);
        assert!(context.current_namespace().is_none());
    }

    #[test]
    fn test_add_and_resolve_symbol() {
        let file_id = FileId::new(1).unwrap();
        let mut context = ClojureResolutionContext::new(file_id);

        let symbol_id = SymbolId::new(1).unwrap();
        context.add_symbol("my-fn".to_string(), symbol_id, ScopeLevel::Module);

        assert_eq!(context.resolve("my-fn"), Some(symbol_id));
        assert_eq!(context.resolve("unknown"), None);
    }

    #[test]
    fn test_scope_resolution_order() {
        let file_id = FileId::new(1).unwrap();
        let mut context = ClojureResolutionContext::new(file_id);

        let local_id = SymbolId::new(1).unwrap();
        let ns_id = SymbolId::new(2).unwrap();
        let import_id = SymbolId::new(3).unwrap();

        // Add same name at different scope levels
        context.add_symbol("x".to_string(), import_id, ScopeLevel::Package);
        context.add_symbol("x".to_string(), ns_id, ScopeLevel::Module);
        context.add_symbol("x".to_string(), local_id, ScopeLevel::Local);

        // Local should win
        assert_eq!(context.resolve("x"), Some(local_id));

        // Clear local, namespace should win
        context.clear_local_scope();
        assert_eq!(context.resolve("x"), Some(ns_id));
    }

    #[test]
    fn test_set_namespace() {
        let file_id = FileId::new(1).unwrap();
        let mut context = ClojureResolutionContext::new(file_id);

        context.set_namespace("my.app.core".to_string());
        assert_eq!(context.current_namespace(), Some("my.app.core"));
    }
}
