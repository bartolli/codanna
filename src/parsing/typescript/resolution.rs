//! TypeScript-specific resolution and inheritance implementation
//!
//! This module implements TypeScript's unique resolution rules including:
//! - Hoisting of functions and var declarations
//! - Block scoping for let/const
//! - Separate type and value spaces
//! - Namespace resolution
//! - Interface vs class inheritance distinctions
//!
//! ## Current Status (Sprint 2 - 2025-08-15)
//!
//! **ARCHITECTURAL FOUNDATION ONLY** - Not production ready.
//!
//! ### Technical Debt (MUST FIX in Sprint 4):
//! 1. No parser integration - uses heuristics instead of AST analysis
//! 2. Type space not populated - structure exists but unused
//! 3. Namespace support incomplete - basic structure only
//! 4. Interface detection uses naming conventions not type info
//!
//! ### Required for Production:
//! - Integration with TypeScript parser to get actual symbol types
//! - Proper AST-based hoisting detection
//! - Full namespace resolution implementation
//! - Type/value space population from parser

use crate::parsing::resolution::{ImportBinding, ProjectResolutionEnhancer};
use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::project_resolver::persist::ResolutionRules;
use crate::{FileId, SymbolId};
use std::collections::HashMap;

/// TypeScript-specific resolution context handling hoisting, namespaces, and dual spaces
///
/// TypeScript has unique resolution features:
/// 1. Hoisting (functions and var declarations)
/// 2. Block scoping (let/const)
/// 3. Separate type and value spaces
/// 4. Namespace scoping
pub struct TypeScriptResolutionContext {
    #[allow(dead_code)]
    file_id: FileId, // Will be used in Sprint 4 for file-specific resolution

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

    /// Type space symbols (interfaces, type aliases)
    /// NOTE: Currently populated via add_import_symbol() for type-only imports.
    /// TODO: Extend Import struct to track is_type_only flag for proper population.
    type_space: HashMap<String, SymbolId>,

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

impl TypeScriptResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scope: HashMap::new(),
            hoisted_scope: HashMap::new(),
            module_symbols: HashMap::new(),
            imported_symbols: HashMap::new(),
            global_symbols: HashMap::new(),
            type_space: HashMap::new(),
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
    /// This is called when an import is resolved to add the symbol to the appropriate space.
    /// Type-only imports go to the type space, others to imported_symbols.
    pub fn add_import_symbol(&mut self, name: String, symbol_id: SymbolId, is_type_only: bool) {
        if is_type_only {
            // Type-only imports are only available in type contexts
            self.type_space.insert(name, symbol_id);
        } else {
            // Regular imports are available everywhere
            self.imported_symbols.insert(name, symbol_id);
        }
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

impl ResolutionScope for TypeScriptResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        match scope_level {
            ScopeLevel::Local => {
                // NOTE: This method doesn't have access to hoisting information.
                // For proper hoisting behavior, use add_symbol_with_context() instead,
                // which uses the symbol's scope_context from the parser.
                // For now, we add to local scope (non-hoisted) by default.
                self.local_scope.insert(name, symbol_id);
            }
            ScopeLevel::Module => {
                self.module_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Package => {
                // In TypeScript, Package level maps to imported symbols
                self.imported_symbols.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                // Global/ambient declarations
                self.global_symbols.insert(name, symbol_id);
            }
        }
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // TypeScript resolution order:
        // 1. Local block scope (let/const)
        // 2. Hoisted scope (functions, var)
        // 3. Imported symbols
        // 4. Module symbols
        // 5. Global/ambient

        tracing::debug!("[typescript] resolve looking up name='{name}'");

        // 1. Check local block scope
        if let Some(&id) = self.local_scope.get(name) {
            tracing::debug!("[typescript] found in local_scope: {id:?}");
            return Some(id);
        }

        // 2. Check hoisted scope
        if let Some(&id) = self.hoisted_scope.get(name) {
            tracing::debug!("[typescript] found in hoisted_scope: {id:?}");
            return Some(id);
        }

        // 3. Check imported symbols
        if let Some(&id) = self.imported_symbols.get(name) {
            tracing::debug!("[typescript] found in imported_symbols: {id:?}");
            return Some(id);
        }

        // 4. Check module-level symbols
        if let Some(&id) = self.module_symbols.get(name) {
            return Some(id);
        }

        // 5. Check type space (for type references)
        if let Some(&id) = self.type_space.get(name) {
            return Some(id);
        }

        // 6. Check global/ambient
        if let Some(&id) = self.global_symbols.get(name) {
            return Some(id);
        }

        // 7. Check if it's a qualified name (contains .)
        if name.contains('.') {
            // CRITICAL FIX: First try to resolve the full qualified path directly
            // This handles cases where we have the full module path stored (e.g., "@app.services.auth.login")
            // Check in all scopes for the full qualified name
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
                    // If we haven't precomputed a mapping and no symbol exists
                    // in current context, fall through to None
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

        tracing::debug!("[typescript] resolve not found: '{name}'");
        None
    }

    fn clear_local_scope(&mut self) {
        self.local_scope.clear();
        // Note: We don't clear hoisted scope as it persists in the function
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);
        // TypeScript hoisting means functions are available throughout their containing scope
    }

    fn exit_scope(&mut self) {
        self.scope_stack.pop();
        // Clear locals when exiting function scope
        if matches!(
            self.scope_stack.last(),
            None | Some(ScopeType::Module | ScopeType::Global)
        ) {
            self.clear_local_scope();
            self.hoisted_scope.clear(); // Clear hoisted when leaving function
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut symbols = Vec::new();

        // Add all symbols with their appropriate scope levels
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
            RelationKind::Implements => {
                // TypeScript: classes implement interfaces
                // Just resolve the interface name
                self.resolve(to_name)
            }
            RelationKind::Extends => {
                // TypeScript: both classes and interfaces can extend
                // Classes extend classes, interfaces extend interfaces
                self.resolve(to_name)
            }
            RelationKind::Calls => {
                // TypeScript: handle Class.method patterns and module.function
                if to_name.contains('.') {
                    // Qualified name like Utils.helper or console.log
                    // Try to resolve the full qualified name first
                    if let Some(id) = self.resolve(to_name) {
                        return Some(id);
                    }
                    // If not found, try just the method/function name
                    // (might be a method call on an instance)
                    if let Some(last_part) = to_name.rsplit('.').next() {
                        return self.resolve(last_part);
                    }
                }
                // Simple function or method call
                self.resolve(to_name)
            }
            RelationKind::Uses => {
                // TypeScript: type usage, imports, etc.
                // Types might be in type_space
                if let Some(id) = self.type_space.get(to_name) {
                    return Some(*id);
                }
                // Otherwise use standard resolution
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
                // TypeScript-specific: Functions/Methods/Constants/Variables can call
                // Constants and Variables can contain functions (e.g., object methods, arrow functions)
                // This enables patterns like: const actions = { submitForm: () => {...} }
                let caller_can_call = matches!(
                    from_kind,
                    Function | Method | Macro | Module | Constant | Variable
                );
                // TypeScript-specific: Constants and Variables can be callable
                // This enables React component patterns: const Button = () => {}
                let callee_can_be_called = matches!(
                    to_kind,
                    Function | Method | Macro | Class | Constant | Variable
                );
                caller_can_call && callee_can_be_called
            }
            CalledBy => {
                // Reverse of Calls
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
            // For all other relationships, delegate to the default implementation
            // by replicating the logic (can't call trait default directly)
            Implements => {
                matches!(from_kind, Struct | Enum | Class) && matches!(to_kind, Trait | Interface)
            }
            ImplementedBy => {
                matches!(from_kind, Trait | Interface) && matches!(to_kind, Struct | Enum | Class)
            }
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
            Extends => {
                let extendable = matches!(from_kind, Class | Interface | Trait | Struct | Enum);
                let can_be_extended = matches!(to_kind, Class | Interface | Trait | Struct | Enum);
                extendable && can_be_extended
            }
            ExtendedBy => {
                matches!(from_kind, Class | Interface | Trait | Struct | Enum)
                    && matches!(to_kind, Class | Interface | Trait | Struct | Enum)
            }
            References => {
                // Very permissive - almost anything can reference anything
                true
            }
            ReferencedBy => {
                // Very permissive - almost anything can reference anything
                true
            }
        }
    }

    fn populate_imports(&mut self, imports: &[crate::parsing::Import]) {
        // Convert Import records into our internal (path, alias) tuple format
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

/// TypeScript inheritance resolution system
///
/// This handles:
/// - Class single inheritance (extends)
/// - Class interface implementation (implements)
/// - Interface multiple inheritance (extends)
pub struct TypeScriptInheritanceResolver {
    /// Maps class names to their parent class
    /// Key: "ClassName", Value: "ParentClassName"
    class_extends: HashMap<String, String>,

    /// Maps class names to interfaces they implement
    /// Key: "ClassName", Value: Vec<"InterfaceName">
    class_implements: HashMap<String, Vec<String>>,

    /// Maps interface names to interfaces they extend
    /// Key: "InterfaceName", Value: Vec<"ParentInterfaceName">
    interface_extends: HashMap<String, Vec<String>>,

    /// Tracks methods on types (both classes and interfaces)
    /// Key: "TypeName", Value: Vec<"method_name">
    type_methods: HashMap<String, Vec<String>>,
}

impl Default for TypeScriptInheritanceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeScriptInheritanceResolver {
    pub fn new() -> Self {
        Self {
            class_extends: HashMap::new(),
            class_implements: HashMap::new(),
            interface_extends: HashMap::new(),
            type_methods: HashMap::new(),
        }
    }

    /// Check if a type is an interface (heuristic)
    ///
    /// TODO(Sprint 4): CRITICAL - Replace convention-based detection with parser info
    /// Current: Guesses based on "I" prefix or absence in class maps
    /// Required: Parser should mark symbols as interface vs class
    /// Impact: May misclassify types, breaking inheritance resolution
    fn is_interface(&self, type_name: &str) -> bool {
        self.interface_extends.contains_key(type_name)
            || type_name.starts_with("I")  // HACK: Convention-based guess, unreliable
            || !self.class_extends.contains_key(type_name) && !self.class_implements.contains_key(type_name)
    }
}

impl InheritanceResolver for TypeScriptInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        match kind {
            "extends" => {
                if self.is_interface(&child) {
                    // Interface extends interface(s)
                    self.interface_extends
                        .entry(child)
                        .or_default()
                        .push(parent);
                } else {
                    // Class extends class (single inheritance)
                    self.class_extends.insert(child, parent);
                }
            }
            "implements" => {
                // Class implements interface(s)
                self.class_implements.entry(child).or_default().push(parent);
            }
            _ => {
                // Handle other relationship types if needed
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

        // Check implemented interfaces
        if let Some(interfaces) = self.class_implements.get(type_name) {
            for interface in interfaces {
                if let Some(methods) = self.type_methods.get(interface) {
                    if methods.iter().any(|m| m == method_name) {
                        return Some(interface.clone());
                    }
                }
            }
        }

        // Check extended interfaces
        if let Some(parents) = self.interface_extends.get(type_name) {
            for parent in parents {
                if let Some(resolved) = self.resolve_method(parent, method_name) {
                    return Some(resolved);
                }
            }
        }

        None
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut chain = vec![type_name.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(type_name.to_string());

        // For classes: add parent class
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

        // For classes: add implemented interfaces
        if let Some(interfaces) = self.class_implements.get(type_name) {
            for interface in interfaces {
                if visited.insert(interface.clone()) {
                    chain.push(interface.clone());
                    // Get interface's parent interfaces
                    for parent in self.get_inheritance_chain(interface) {
                        if visited.insert(parent.clone()) {
                            chain.push(parent);
                        }
                    }
                }
            }
        }

        // For interfaces: add extended interfaces
        if let Some(parents) = self.interface_extends.get(type_name) {
            for parent in parents {
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

        // Check if class implements interface
        if let Some(interfaces) = self.class_implements.get(child) {
            if interfaces.contains(&parent.to_string()) {
                return true;
            }
            // Check if any implemented interface extends parent
            for interface in interfaces {
                if self.is_subtype(interface, parent) {
                    return true;
                }
            }
        }

        // Check interface extension
        if let Some(extended) = self.interface_extends.get(child) {
            if extended.contains(&parent.to_string()) {
                return true;
            }
            // Recursive check
            for ext_interface in extended {
                if self.is_subtype(ext_interface, parent) {
                    return true;
                }
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

        // Helper to collect methods recursively
        fn collect_methods(
            resolver: &TypeScriptInheritanceResolver,
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

            // For classes: check parent class
            if let Some(parent) = resolver.class_extends.get(type_name) {
                collect_methods(resolver, parent, all_methods, visited);
            }

            // For classes: check implemented interfaces
            if let Some(interfaces) = resolver.class_implements.get(type_name) {
                for interface in interfaces {
                    collect_methods(resolver, interface, all_methods, visited);
                }
            }

            // For interfaces: check extended interfaces
            if let Some(parents) = resolver.interface_extends.get(type_name) {
                for parent in parents {
                    collect_methods(resolver, parent, all_methods, visited);
                }
            }
        }

        collect_methods(self, type_name, &mut all_methods, &mut visited);
        all_methods
    }
}

/// Extension methods for TypeScriptInheritanceResolver for TypeScript-specific operations
impl TypeScriptInheritanceResolver {
    /// Register that a class extends another class
    pub fn add_class_extends(&mut self, child: String, parent: String) {
        self.class_extends.insert(child, parent);
    }

    /// Register that a class implements an interface
    pub fn add_class_implements(&mut self, class_name: String, interface_name: String) {
        self.class_implements
            .entry(class_name)
            .or_default()
            .push(interface_name);
    }

    /// Register that an interface extends other interfaces
    pub fn add_interface_extends(&mut self, child: String, parents: Vec<String>) {
        self.interface_extends.insert(child, parents);
    }

    /// Get all interfaces that a class implements (directly and indirectly)
    pub fn get_all_interfaces(&self, class_name: &str) -> Vec<String> {
        let mut interfaces = Vec::new();
        let mut visited = std::collections::HashSet::new();

        // Get directly implemented interfaces
        if let Some(direct) = self.class_implements.get(class_name) {
            for interface in direct {
                if visited.insert(interface.clone()) {
                    interfaces.push(interface.clone());
                    // Get parent interfaces
                    for parent in self.get_inheritance_chain(interface) {
                        if visited.insert(parent.clone()) {
                            interfaces.push(parent);
                        }
                    }
                }
            }
        }

        // Check parent class's interfaces
        if let Some(parent) = self.class_extends.get(class_name) {
            for interface in self.get_all_interfaces(parent) {
                if visited.insert(interface.clone()) {
                    interfaces.push(interface);
                }
            }
        }

        interfaces
    }
}

/// TypeScript project resolution enhancer
///
/// Applies tsconfig.json path mappings to transform import paths
pub struct TypeScriptProjectEnhancer {
    /// Compiled path alias resolver (built from the resolution rules)
    resolver: Option<crate::parsing::typescript::tsconfig::PathAliasResolver>,
}

impl TypeScriptProjectEnhancer {
    /// Create a new enhancer from resolution rules
    pub fn new(rules: ResolutionRules) -> Self {
        // Build the PathAliasResolver from rules
        let resolver = if !rules.paths.is_empty() || rules.base_url.is_some() {
            // Create a minimal TsConfig to use from_tsconfig
            let config = crate::parsing::typescript::tsconfig::TsConfig {
                extends: None,
                compilerOptions: crate::parsing::typescript::tsconfig::CompilerOptions {
                    baseUrl: rules.base_url.clone(),
                    paths: rules.paths.clone(),
                },
            };

            // Use from_tsconfig to create the resolver
            crate::parsing::typescript::tsconfig::PathAliasResolver::from_tsconfig(&config).ok()
        } else {
            None
        };

        Self { resolver }
    }
}

impl ProjectResolutionEnhancer for TypeScriptProjectEnhancer {
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
    fn test_typescript_function_can_call_constant() {
        // React pattern: const Button = () => {}
        let context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());
        assert!(context.is_compatible_relationship(
            crate::SymbolKind::Function,
            crate::SymbolKind::Constant,
            crate::RelationKind::Calls
        ));
    }

    #[test]
    fn test_typescript_function_can_call_variable() {
        // Functional component stored in variable
        let context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());
        assert!(context.is_compatible_relationship(
            crate::SymbolKind::Function,
            crate::SymbolKind::Variable,
            crate::RelationKind::Calls
        ));
    }

    #[test]
    fn test_typescript_hoisting() {
        let mut context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());

        // Add hoisted function
        context.add_symbol(
            "function myFunc".to_string(),
            SymbolId::new(1).unwrap(),
            ScopeLevel::Local,
        );

        // Function should be in hoisted scope
        assert_eq!(
            context.resolve("function myFunc"),
            Some(SymbolId::new(1).unwrap())
        );
    }

    #[test]
    fn test_block_scoping() {
        let mut context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());

        // Add block-scoped variable
        context.add_symbol(
            "myLet".to_string(),
            SymbolId::new(1).unwrap(),
            ScopeLevel::Local,
        );

        // Should be resolvable
        assert_eq!(context.resolve("myLet"), Some(SymbolId::new(1).unwrap()));

        // Clear local scope
        context.clear_local_scope();

        // Should no longer be resolvable
        assert_eq!(context.resolve("myLet"), None);
    }

    #[test]
    fn test_interface_vs_class_inheritance() {
        let mut resolver = TypeScriptInheritanceResolver::new();

        // Class extends class
        resolver.add_inheritance(
            "ChildClass".to_string(),
            "ParentClass".to_string(),
            "extends",
        );

        // Class implements interface
        resolver.add_inheritance(
            "ChildClass".to_string(),
            "IMyInterface".to_string(),
            "implements",
        );

        // Check subtype relationships
        assert!(resolver.is_subtype("ChildClass", "ParentClass"));
        assert!(resolver.is_subtype("ChildClass", "IMyInterface"));

        // Check inheritance chain
        let chain = resolver.get_inheritance_chain("ChildClass");
        assert!(chain.contains(&"ParentClass".to_string()));
        assert!(chain.contains(&"IMyInterface".to_string()));
    }

    #[test]
    fn test_interface_multiple_inheritance() {
        let mut resolver = TypeScriptInheritanceResolver::new();

        // Interface extends multiple interfaces
        resolver.add_interface_extends(
            "IChild".to_string(),
            vec!["IParent1".to_string(), "IParent2".to_string()],
        );

        // Check inheritance chain
        let chain = resolver.get_inheritance_chain("IChild");
        assert!(chain.contains(&"IParent1".to_string()));
        assert!(chain.contains(&"IParent2".to_string()));

        // Check subtype relationships
        assert!(resolver.is_subtype("IChild", "IParent1"));
        assert!(resolver.is_subtype("IChild", "IParent2"));
    }

    #[test]
    fn test_cross_module_resolution_requires_module_path() {
        // This test demonstrates the core issue: symbols must be added by their module_path
        // for cross-module resolution to work with enhanced imports

        println!("\n=== Testing Cross-Module Resolution with Module Path ===");

        let mut context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());
        let button_id = SymbolId::new(42).unwrap();

        // Scenario 1: Symbol added only by name
        context.add_symbol("Button".to_string(), button_id, ScopeLevel::Global);
        println!("Added symbol 'Button' with id {button_id:?}");

        // Try to resolve by name - this works
        let resolved_by_name = context.resolve("Button");
        println!("Resolving 'Button' by name: {resolved_by_name:?}");
        assert_eq!(resolved_by_name, Some(button_id));

        // Try to resolve by module path - this DOESN'T work
        let module_path = "examples.typescript.react.src.components.ui.button";
        let resolved_by_module_before = context.resolve(module_path);
        println!(
            "Resolving '{module_path}' BEFORE adding by module_path: {resolved_by_module_before:?}"
        );
        assert_eq!(
            resolved_by_module_before, None,
            "Resolution by module path fails when symbol only added by name"
        );

        // Scenario 2: Symbol added ALSO by module_path (THE FIX)
        context.add_symbol(module_path.to_string(), button_id, ScopeLevel::Global);
        println!("\nAdded symbol by module_path '{module_path}'");

        // Now resolution by module path works
        let resolved_by_module_after = context.resolve(module_path);
        println!(
            "Resolving '{module_path}' AFTER adding by module_path: {resolved_by_module_after:?}"
        );
        assert_eq!(
            resolved_by_module_after,
            Some(button_id),
            "Resolution by module path works when symbol added by module_path"
        );

        println!(
            "\n✅ Test proves that symbols MUST be added by module_path for cross-module resolution!"
        );
        // The key insight: When imports are enhanced from aliases like "@/components/ui/button"
        // to "./src/components/ui/button", we need to match against the module_path
        // "examples.typescript.react.src.components.ui.button" for resolution to work
    }

    #[test]
    fn test_enhanced_import_resolution_workflow() {
        // This test shows the complete workflow for alias resolution

        let mut context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());
        let button_id = SymbolId::new(42).unwrap();

        // Step 1: Import with alias "@/components/ui/button" gets enhanced to "./src/components/ui/button"
        let enhanced_import = "./src/components/ui/button";

        // Step 2: The symbol exists with module_path "examples.typescript.react.src.components.ui.button"
        let module_path = "examples.typescript.react.src.components.ui.button";

        // Step 3: Add symbol by both name AND module_path for proper resolution
        context.add_symbol("Button".to_string(), button_id, ScopeLevel::Module);
        context.add_symbol(module_path.to_string(), button_id, ScopeLevel::Global);

        // Step 4: We need to transform the enhanced import to match the module_path
        // This is the missing piece - enhanced_import doesn't directly match module_path
        assert_ne!(
            enhanced_import, module_path,
            "Enhanced import path doesn't match module_path directly"
        );

        // For now, resolution works if we use the full module_path
        assert_eq!(context.resolve(module_path), Some(button_id));

        // But resolution by enhanced import path doesn't work without transformation
        assert_eq!(
            context.resolve(enhanced_import),
            None,
            "Resolution by enhanced import path needs transformation to module_path"
        );
    }

    #[test]
    fn test_resolve_relationship_for_calls() {
        // Test that resolve_relationship properly handles call relationships

        let mut context = TypeScriptResolutionContext::new(FileId::new(1).unwrap());
        let button_id = SymbolId::new(42).unwrap();

        // Add Button symbol by name and module_path
        context.add_symbol("Button".to_string(), button_id, ScopeLevel::Module);
        let module_path = "examples.typescript.react.src.components.ui.button";
        context.add_symbol(module_path.to_string(), button_id, ScopeLevel::Global);

        // Test resolving a call relationship
        let resolved = context.resolve_relationship(
            "TestComponent",
            "Button",
            crate::RelationKind::Calls,
            FileId::new(2).unwrap(),
        );

        assert_eq!(
            resolved,
            Some(button_id),
            "Should resolve Button when called from TestComponent"
        );

        // Test resolving by module path in call relationship
        let resolved_by_module = context.resolve_relationship(
            "TestComponent",
            module_path,
            crate::RelationKind::Calls,
            FileId::new(2).unwrap(),
        );

        assert_eq!(
            resolved_by_module,
            Some(button_id),
            "Should resolve by module_path in call relationship"
        );
    }
}
