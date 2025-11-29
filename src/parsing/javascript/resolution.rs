//! JavaScript-specific resolution and inheritance implementation
//!
//! This module implements JavaScript's resolution rules including:
//! - Hoisting of functions and var declarations
//! - Block scoping for let/const
//! - Namespace resolution (for ES modules)
//! - Class inheritance (extends only, no interfaces)
//!
//! Note: JavaScript is similar to TypeScript but lacks:
//! - Type-only imports
//! - Interfaces
//! - Protected visibility
//! - Abstract classes

use crate::debug_print;
use crate::parsing::resolution::ImportBinding;
use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::{FileId, SymbolId};
use std::collections::HashMap;

/// JavaScript-specific resolution context handling hoisting, namespaces
///
/// JavaScript has similar resolution to TypeScript but without type spaces:
/// 1. Hoisting (functions and var declarations)
/// 2. Block scoping (let/const)
/// 3. Namespace scoping (ES modules)
pub struct JavaScriptResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,

    /// Local block-scoped bindings (let/const)
    local_scope: HashMap<String, SymbolId>,

    /// Hoisted declarations (functions and var)
    hoisted_scope: HashMap<String, SymbolId>,

    /// Module-level exports and declarations
    module_symbols: HashMap<String, SymbolId>,

    /// Imported symbols from other modules
    imported_symbols: HashMap<String, SymbolId>,

    /// Global/ambient declarations
    global_symbols: HashMap<String, SymbolId>,

    /// Track nested scopes (blocks, functions, etc.)
    scope_stack: Vec<ScopeType>,

    /// Import tracking (path -> alias)
    imports: Vec<(String, Option<String>)>,

    /// Precomputed qualified name resolution for namespace imports
    /// e.g., alias "Utils" → { "Utils.helper" => SymbolId }
    qualified_names: HashMap<String, SymbolId>,
    /// Namespace alias to target module path (normalized, dots)
    namespace_aliases: HashMap<String, String>,

    /// Binding info for imports keyed by visible name
    import_bindings: HashMap<String, ImportBinding>,
}

impl JavaScriptResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scope: HashMap::new(),
            hoisted_scope: HashMap::new(),
            module_symbols: HashMap::new(),
            imported_symbols: HashMap::new(),
            global_symbols: HashMap::new(),
            scope_stack: Vec::new(),
            imports: Vec::new(),
            qualified_names: HashMap::new(),
            namespace_aliases: HashMap::new(),
            import_bindings: HashMap::new(),
        }
    }

    /// Add an import (import statement)
    pub fn add_import(&mut self, path: String, alias: Option<String>) {
        self.imports.push((path, alias));
    }

    /// Record a namespace alias mapping (e.g., import * as React from 'react')
    pub fn add_namespace_alias(&mut self, alias: String, target_module: String) {
        self.namespace_aliases.insert(alias, target_module);
    }

    /// Add a qualified name binding for fast resolution (e.g., "Utils.helper" -> id)
    pub fn add_qualified_name(&mut self, qualified: String, symbol_id: SymbolId) {
        self.qualified_names.insert(qualified, symbol_id);
    }

    /// Add an imported symbol to the context
    ///
    /// JavaScript doesn't have type-only imports, so all imports are value imports
    pub fn add_import_symbol(&mut self, name: String, symbol_id: SymbolId, _is_type_only: bool) {
        // All imports are regular imports in JavaScript (no type-only imports)
        self.imported_symbols.insert(name, symbol_id);
    }

    /// Add a symbol with proper scope context
    ///
    /// This method uses the symbol's scope_context to determine proper placement.
    /// Functions are hoisted, let/const are block-scoped, var is function-scoped.
    pub fn add_symbol_with_context(
        &mut self,
        name: String,
        symbol_id: SymbolId,
        scope_context: Option<&crate::symbol::ScopeContext>,
    ) {
        use crate::symbol::ScopeContext;

        match scope_context {
            Some(ScopeContext::Local { hoisted: true, .. }) => {
                // Hoisted declarations (functions, var)
                self.hoisted_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Local { hoisted: false, .. }) => {
                // Block-scoped declarations (let, const, arrow functions)
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::ClassMember { .. }) => {
                // Class members go to local scope within the class
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Parameter) => {
                // Function parameters are local
                self.local_scope.insert(name, symbol_id);
            }
            Some(ScopeContext::Module) => {
                // Module-level declarations
                self.module_symbols.insert(name, symbol_id);
            }
            Some(ScopeContext::Package) => {
                // Imported symbols
                self.imported_symbols.insert(name, symbol_id);
            }
            Some(ScopeContext::Global) => {
                // Global/ambient declarations
                self.global_symbols.insert(name, symbol_id);
            }
            None => {
                // Default to local scope if no context
                self.local_scope.insert(name, symbol_id);
            }
        }
    }
}

impl ResolutionScope for JavaScriptResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                // For proper hoisting behavior, use add_symbol_with_context() instead
                self.local_scope.insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                self.module_symbols.insert(name, symbol_id);
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
        // JavaScript resolution order:
        // 1. Local block scope (let/const)
        // 2. Hoisted scope (functions, var)
        // 3. Imported symbols
        // 4. Module symbols
        // 5. Global/ambient

        debug_print!(self, "[resolve] Looking up name='{}'", name);

        // 1. Check local block scope
        if let Some(&id) = self.local_scope.get(name) {
            debug_print!(self, "[resolve] Found in local_scope: {:?}", id);
            return Some(id);
        }

        // 2. Check hoisted scope
        if let Some(&id) = self.hoisted_scope.get(name) {
            debug_print!(self, "[resolve] Found in hoisted_scope: {:?}", id);
            return Some(id);
        }

        // 3. Check imported symbols
        if let Some(&id) = self.imported_symbols.get(name) {
            debug_print!(self, "[resolve] Found in imported_symbols: {:?}", id);
            return Some(id);
        }

        // 4. Check module-level symbols
        if let Some(&id) = self.module_symbols.get(name) {
            return Some(id);
        }

        // 5. Check global/ambient
        if let Some(&id) = self.global_symbols.get(name) {
            return Some(id);
        }

        // 6. Check if it's a qualified name (contains .)
        if name.contains('.') {
            // First try to resolve the full qualified path directly
            if let Some(&id) = self.imported_symbols.get(name) {
                return Some(id);
            }
            if let Some(&id) = self.module_symbols.get(name) {
                return Some(id);
            }
            if let Some(&id) = self.global_symbols.get(name) {
                return Some(id);
            }

            // If full path not found, try to resolve as a 2-part path
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() == 2 {
                let class_or_module = parts[0];
                let method_or_prop = parts[1];
                // Namespace import alias (e.g., React.useEffect)
                if let Some(_module) = self.namespace_aliases.get(class_or_module) {
                    // For namespace imports, attempt to resolve the member by name
                    if let Some(&id) = self
                        .local_scope
                        .get(method_or_prop)
                        .or_else(|| self.hoisted_scope.get(method_or_prop))
                        .or_else(|| self.imported_symbols.get(method_or_prop))
                        .or_else(|| self.module_symbols.get(method_or_prop))
                        .or_else(|| self.global_symbols.get(method_or_prop))
                    {
                        return Some(id);
                    }
                }

                // Check if type exists in our codebase (class or module symbol)
                if self.resolve(class_or_module).is_some() {
                    // Type exists, resolve the method/property by name
                    return self.resolve(method_or_prop);
                }
                // External library or unresolved alias - return None
                return None;
            }
        }

        debug_print!(self, "[resolve] NOT FOUND: '{}'", name);
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
        // Clear locals when exiting function scope
        if matches!(
            self.scope_stack.last(),
            None | Some(ScopeType::Module | ScopeType::Global)
        ) {
            self.clear_local_scope();
            self.hoisted_scope.clear();
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut symbols = Vec::new();

        for (name, &id) in &self.local_scope {
            symbols.push((name.clone(), id, ScopeLevel::Local));
        }
        for (name, &id) in &self.hoisted_scope {
            symbols.push((name.clone(), id, ScopeLevel::Local));
        }
        for (name, &id) in &self.imported_symbols {
            symbols.push((name.clone(), id, ScopeLevel::Package));
        }
        for (name, &id) in &self.module_symbols {
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
            RelationKind::Extends => {
                // JavaScript: classes can extend other classes
                self.resolve(to_name)
            }
            RelationKind::Calls => {
                // JavaScript: handle Class.method patterns and module.function
                if to_name.contains('.') {
                    // Try to resolve the full qualified name first
                    if let Some(id) = self.resolve(to_name) {
                        return Some(id);
                    }
                    // If not found, try just the method/function name
                    if let Some(last_part) = to_name.rsplit('.').next() {
                        return self.resolve(last_part);
                    }
                }
                // Simple function or method call
                self.resolve(to_name)
            }
            RelationKind::Uses => {
                // JavaScript: imports, variable usage, etc.
                self.resolve(to_name)
            }
            _ => {
                // For other relationship types, use standard resolution
                self.resolve(to_name)
            }
        }
    }

    fn is_compatible_relationship(
        &self,
        from_kind: crate::SymbolKind,
        to_kind: crate::SymbolKind,
        rel_kind: crate::RelationKind,
    ) -> bool {
        use crate::RelationKind::*;
        use crate::SymbolKind::*;

        match rel_kind {
            Calls => {
                // JavaScript: Functions/Methods/Constants/Variables can call
                let caller_can_call = matches!(
                    from_kind,
                    Function | Method | Macro | Module | Constant | Variable
                );
                // JavaScript: Constants and Variables can be callable (React patterns)
                let callee_can_be_called = matches!(
                    to_kind,
                    Function | Method | Macro | Class | Constant | Variable
                );
                caller_can_call && callee_can_be_called
            }
            CalledBy => {
                let caller_can_call = matches!(
                    to_kind,
                    Function | Method | Macro | Module | Constant | Variable
                );
                let callee_can_be_called = matches!(
                    from_kind,
                    Function | Method | Macro | Class | Constant | Variable
                );
                callee_can_be_called && caller_can_call
            }
            Extends => {
                // JavaScript: only classes can extend classes (no interfaces)
                matches!(from_kind, Class) && matches!(to_kind, Class)
            }
            ExtendedBy => matches!(from_kind, Class) && matches!(to_kind, Class),
            Uses => {
                let can_use = matches!(
                    from_kind,
                    Function | Method | Struct | Class | Trait | Interface | Module | Enum
                );
                let can_be_used = matches!(
                    to_kind,
                    Struct
                        | Enum
                        | Class
                        | Trait
                        | Interface
                        | TypeAlias
                        | Constant
                        | Variable
                        | Function
                        | Method
                );
                can_use && can_be_used
            }
            UsedBy => {
                let can_be_used = matches!(
                    from_kind,
                    Struct
                        | Enum
                        | Class
                        | Trait
                        | Interface
                        | TypeAlias
                        | Constant
                        | Variable
                        | Function
                        | Method
                );
                let can_use = matches!(
                    to_kind,
                    Function | Method | Struct | Class | Trait | Interface | Module | Enum
                );
                can_be_used && can_use
            }
            Defines => {
                let container = matches!(
                    from_kind,
                    Trait | Interface | Module | Struct | Enum | Class
                );
                let member = matches!(to_kind, Method | Function | Constant | Field | Variable);
                container && member
            }
            DefinedIn => {
                let member = matches!(from_kind, Method | Function | Constant | Field | Variable);
                let container =
                    matches!(to_kind, Trait | Interface | Module | Struct | Enum | Class);
                member && container
            }
            References => true,
            ReferencedBy => true,
            // JavaScript doesn't support Implements/ImplementedBy (no interfaces)
            Implements | ImplementedBy => false,
        }
    }

    fn populate_imports(&mut self, imports: &[crate::parsing::Import]) {
        for import in imports {
            self.add_import(import.path.clone(), import.alias.clone());
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

/// JavaScript inheritance resolution system
///
/// This handles:
/// - Class single inheritance (extends only)
/// - No interfaces (JavaScript doesn't have them)
pub struct JavaScriptInheritanceResolver {
    /// Maps class names to their parent class
    class_extends: HashMap<String, String>,

    /// Tracks methods on classes
    type_methods: HashMap<String, Vec<String>>,
}

impl Default for JavaScriptInheritanceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaScriptInheritanceResolver {
    pub fn new() -> Self {
        Self {
            class_extends: HashMap::new(),
            type_methods: HashMap::new(),
        }
    }
}

impl InheritanceResolver for JavaScriptInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        match kind {
            "extends" => {
                // Class extends class (single inheritance)
                self.class_extends.insert(child, parent);
            }
            _ => {
                // JavaScript only supports extends, ignore other relationships
            }
        }
    }

    fn resolve_method(&self, type_name: &str, method_name: &str) -> Option<String> {
        // Check if the type has this method
        if let Some(methods) = self.type_methods.get(type_name) {
            if methods.iter().any(|m| m == method_name) {
                return Some(type_name.to_string());
            }
        }

        // Check parent class
        if let Some(parent) = self.class_extends.get(type_name) {
            if let Some(resolved) = self.resolve_method(parent, method_name) {
                return Some(resolved);
            }
        }

        None
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut chain = vec![type_name.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(type_name.to_string());

        // Add parent class
        if let Some(parent) = self.class_extends.get(type_name) {
            if visited.insert(parent.clone()) {
                chain.push(parent.clone());
                // Recursively get parent's chain
                for ancestor in self.get_inheritance_chain(parent) {
                    if visited.insert(ancestor.clone()) {
                        chain.push(ancestor);
                    }
                }
            }
        }

        chain
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        // Check direct class extension
        if let Some(direct_parent) = self.class_extends.get(child) {
            if direct_parent == parent {
                return true;
            }
            // Recursive check
            if self.is_subtype(direct_parent, parent) {
                return true;
            }
        }

        false
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods
            .entry(type_name)
            .or_default()
            .extend(methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut all_methods = Vec::new();
        let mut visited = std::collections::HashSet::new();

        fn collect_methods(
            resolver: &JavaScriptInheritanceResolver,
            type_name: &str,
            all_methods: &mut Vec<String>,
            visited: &mut std::collections::HashSet<String>,
        ) {
            if !visited.insert(type_name.to_string()) {
                return;
            }

            // Add this type's methods
            if let Some(methods) = resolver.type_methods.get(type_name) {
                for method in methods {
                    if !all_methods.contains(method) {
                        all_methods.push(method.clone());
                    }
                }
            }

            // Check parent class
            if let Some(parent) = resolver.class_extends.get(type_name) {
                collect_methods(resolver, parent, all_methods, visited);
            }
        }

        collect_methods(self, type_name, &mut all_methods, &mut visited);
        all_methods
    }
}

/// JavaScript project resolution enhancer
///
/// Applies jsconfig.json path mappings to transform import paths.
/// Mirrors TypeScript's TypeScriptProjectEnhancer architecture.
pub struct JavaScriptProjectEnhancer {
    /// Compiled path alias resolver (built from the resolution rules)
    resolver: Option<crate::parsing::javascript::jsconfig::PathAliasResolver>,
}

impl JavaScriptProjectEnhancer {
    /// Create a new enhancer from resolution rules
    pub fn new(rules: crate::project_resolver::persist::ResolutionRules) -> Self {
        // Build the PathAliasResolver from rules
        let resolver = if !rules.paths.is_empty() || rules.base_url.is_some() {
            // Create a minimal JsConfig to use from_jsconfig
            let config = crate::parsing::javascript::jsconfig::JsConfig {
                extends: None,
                compilerOptions: crate::parsing::javascript::jsconfig::CompilerOptions {
                    baseUrl: rules.base_url.clone(),
                    paths: rules.paths.clone(),
                },
            };

            // Use from_jsconfig to create the resolver
            crate::parsing::javascript::jsconfig::PathAliasResolver::from_jsconfig(&config).ok()
        } else {
            None
        };

        Self { resolver }
    }
}

impl crate::parsing::resolution::ProjectResolutionEnhancer for JavaScriptProjectEnhancer {
    fn enhance_import_path(&self, import_path: &str, _from_file: FileId) -> Option<String> {
        // Skip relative imports - they don't need enhancement
        if import_path.starts_with("./") || import_path.starts_with("../") {
            return None;
        }

        // Use the resolver to transform the path
        if let Some(ref resolver) = self.resolver {
            // Get candidates and return the first one
            let candidates = resolver.resolve_import(import_path);
            candidates.into_iter().next()
        } else {
            None
        }
    }

    fn get_import_candidates(&self, import_path: &str, _from_file: FileId) -> Vec<String> {
        // Skip relative imports
        if import_path.starts_with("./") || import_path.starts_with("../") {
            return vec![import_path.to_string()];
        }

        // Use the resolver to get all candidates
        if let Some(ref resolver) = self.resolver {
            let candidates = resolver.resolve_import(import_path);
            if !candidates.is_empty() {
                candidates
            } else {
                vec![import_path.to_string()]
            }
        } else {
            vec![import_path.to_string()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_javascript_function_can_call_constant() {
        // React pattern: const Button = () => {}
        let context = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
        assert!(context.is_compatible_relationship(
            crate::SymbolKind::Function,
            crate::SymbolKind::Constant,
            crate::RelationKind::Calls
        ));
    }

    #[test]
    fn test_javascript_hoisting() {
        let mut context = JavaScriptResolutionContext::new(FileId::new(1).unwrap());

        // Add hoisted function
        context.add_symbol(
            "function myFunc".to_string(),
            SymbolId::new(1).unwrap(),
            ScopeLevel::Local,
        );

        assert_eq!(
            context.resolve("function myFunc"),
            Some(SymbolId::new(1).unwrap())
        );
    }

    #[test]
    fn test_javascript_no_implements() {
        // JavaScript doesn't support implements relationship
        let context = JavaScriptResolutionContext::new(FileId::new(1).unwrap());
        assert!(!context.is_compatible_relationship(
            crate::SymbolKind::Class,
            crate::SymbolKind::Interface,
            crate::RelationKind::Implements
        ));
    }

    #[test]
    fn test_class_inheritance() {
        let mut resolver = JavaScriptInheritanceResolver::new();

        // Class extends class
        resolver.add_inheritance(
            "ChildClass".to_string(),
            "ParentClass".to_string(),
            "extends",
        );

        // Check subtype relationship
        assert!(resolver.is_subtype("ChildClass", "ParentClass"));

        // Check inheritance chain
        let chain = resolver.get_inheritance_chain("ChildClass");
        assert!(chain.contains(&"ParentClass".to_string()));
    }
}
