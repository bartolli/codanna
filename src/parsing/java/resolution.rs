//! Java resolution context and inheritance resolver
//!
//! Implements Java's scoping rules and inheritance resolution.
//!
//! Java resolution order: local � class � file � imported � package
//!
//! TODO: Implement methods after exploring actual Java AST with tree-sitter.

use crate::parsing::{
    InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType, resolution::ImportBinding,
};
use crate::{FileId, RelationKind, SymbolId};
use std::any::Any;
use std::collections::{HashMap, HashSet};

/// Resolution context for Java
///
/// Java has a 5-tier scope system (simpler than Kotlin - no companion objects):
/// 1. Local scope - variables and parameters within methods/blocks
/// 2. Class scope - fields and methods of the current class
/// 3. File scope - other classes in the same file
/// 4. Imported scope - symbols from import statements
/// 5. Package scope - classes in the same package (package-private access)
pub struct JavaResolutionContext {
    #[allow(dead_code)]
    file_id: FileId,

    // 5-tier scope system with nested support
    local_scopes: Vec<HashMap<String, SymbolId>>, // 1. Local vars/params (nested blocks)
    class_scopes: Vec<HashMap<String, SymbolId>>, // 2. Class members (nested classes)
    file_scope: HashMap<String, SymbolId>,        // 3. Same-file classes
    imported_symbols: HashMap<String, SymbolId>,  // 4. Imports
    package_scope: HashMap<String, SymbolId>,     // 5. Same package

    // Scope stack for tracking context
    scope_stack: Vec<ScopeType>,

    // Import tracking
    imports: Vec<(String, Option<String>)>, // Raw imports (path, alias)
    import_bindings: HashMap<String, ImportBinding>,
}

impl JavaResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            local_scopes: Vec::new(),
            class_scopes: Vec::new(),
            file_scope: HashMap::new(),
            imported_symbols: HashMap::new(),
            package_scope: HashMap::new(),
            scope_stack: Vec::new(),
            imports: Vec::new(),
            import_bindings: HashMap::new(),
        }
    }

    // =========================================================================
    // HELPER METHODS (matching Kotlin's internal API)
    // =========================================================================

    /// Set expression types for type inference
    /// TODO: Implement if Java needs type inference
    pub fn set_expression_types(&mut self, _entries: HashMap<String, String>) {
        // TODO: Store expression type mappings if needed
    }

    /// Get mutable reference to current local scope
    fn current_local_scope_mut(&mut self) -> &mut HashMap<String, SymbolId> {
        if self.local_scopes.is_empty() {
            self.local_scopes.push(HashMap::new());
        }
        self.local_scopes.last_mut().unwrap()
    }

    /// Get mutable reference to current class scope
    fn current_class_scope_mut(&mut self) -> Option<&mut HashMap<String, SymbolId>> {
        self.class_scopes.last_mut()
    }

    /// Resolve in local scopes (innermost first)
    fn resolve_in_locals(&self, name: &str) -> Option<SymbolId> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    /// Resolve in class scopes (innermost class first)
    fn resolve_in_classes(&self, name: &str) -> Option<SymbolId> {
        for scope in self.class_scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    /// Get import binding for a name
    pub fn import_binding(&self, name: &str) -> Option<ImportBinding> {
        self.import_bindings.get(name).cloned()
    }

    /// Resolve expression type from inference
    /// TODO: Implement if Java needs type inference
    pub fn resolve_expression_type(&self, _expr: &str) -> Option<String> {
        // TODO: Look up inferred type for expression
        None
    }
}

impl ResolutionScope for JavaResolutionContext {
    /// Add symbol to appropriate scope level
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
                self.file_scope.entry(name).or_insert(symbol_id);
            }
            ScopeLevel::Package => {
                self.package_scope.insert(name, symbol_id);
            }
            ScopeLevel::Global => {
                self.imported_symbols.insert(name, symbol_id);
            }
        }
    }

    /// Resolve symbol name to ID
    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Java resolution order: local � class � file � imported � package

        // 1. Check local scopes (innermost first)
        if let Some(id) = self.resolve_in_locals(name) {
            return Some(id);
        }

        // 2. Check class scopes (innermost class first)
        if let Some(id) = self.resolve_in_classes(name) {
            return Some(id);
        }

        // 3. Check file-level scope (other classes in same file)
        if let Some(&id) = self.file_scope.get(name) {
            return Some(id);
        }

        // 4. Check imported symbols
        if let Some(&id) = self.imported_symbols.get(name) {
            return Some(id);
        }

        // 5. Check package scope (package-private)
        if let Some(&id) = self.package_scope.get(name) {
            return Some(id);
        }

        // 6. Handle qualified names (OuterClass.InnerClass, Type.member)
        if name.contains('.') {
            // Try full qualified name in all scopes
            if let Some(id) = self.imported_symbols.get(name) {
                return Some(*id);
            }
            if let Some(id) = self.package_scope.get(name) {
                return Some(*id);
            }

            // Split and try Type.Member resolution
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() == 2 {
                let type_name = parts[0];
                let member_name = parts[1];

                // Check if type exists in scope
                if self.resolve(type_name).is_some() {
                    // Type exists, try to resolve member in class scopes
                    return self.resolve_in_classes(member_name);
                }
            }
        }

        None
    }

    /// Clear local scope (called on scope exit)
    fn clear_local_scope(&mut self) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.clear();
        }
    }

    /// Enter new scope
    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);

        match scope_type {
            ScopeType::Function { .. } | ScopeType::Block => {
                self.local_scopes.push(HashMap::new());
            }
            ScopeType::Class => {
                self.class_scopes.push(HashMap::new());
            }
            _ => {
                // Other scope types don't need stack entry
            }
        }
    }

    /// Exit current scope
    fn exit_scope(&mut self) {
        if let Some(scope_type) = self.scope_stack.pop() {
            match scope_type {
                ScopeType::Function { .. } | ScopeType::Block => {
                    self.local_scopes.pop();
                }
                ScopeType::Class => {
                    self.class_scopes.pop();
                }
                _ => {
                    // Other scope types don't need stack exit
                }
            }
        }
    }

    /// Get all symbols in current scope
    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut result = Vec::new();

        // Collect from all local scopes
        for scope in &self.local_scopes {
            for (name, &id) in scope {
                result.push((name.clone(), id, ScopeLevel::Local));
            }
        }

        // Collect from all class scopes
        for scope in &self.class_scopes {
            for (name, &id) in scope {
                result.push((name.clone(), id, ScopeLevel::Module));
            }
        }

        // Collect from file scope
        for (name, &id) in &self.file_scope {
            result.push((name.clone(), id, ScopeLevel::Module));
        }

        // Collect from imported symbols
        for (name, &id) in &self.imported_symbols {
            result.push((name.clone(), id, ScopeLevel::Global));
        }

        result
    }

    /// Downcast to concrete type
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Resolve relationship (calls, extends, implements, etc.)
    ///
    /// Routes relationship resolution based on kind. Most relationships
    /// delegate to standard resolution, with special handling for:
    /// - Qualified method calls (Class.method syntax)
    /// - Static imports
    fn resolve_relationship(
        &self,
        _from_name: &str,
        to_name: &str,
        kind: RelationKind,
        _from_file: FileId,
    ) -> Option<SymbolId> {
        match kind {
            RelationKind::Extends => {
                // Java: classes extend one superclass
                // Just resolve the class name
                self.resolve(to_name)
            }
            RelationKind::Implements => {
                // Java: classes implement multiple interfaces
                // Just resolve the interface name
                self.resolve(to_name)
            }
            RelationKind::Calls => {
                // Java: handle Class.method patterns and package.Class.method
                if to_name.contains('.') {
                    // Qualified name like MyClass.staticMethod or java.lang.System.println
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
                // Java: type usage, field types, parameter types, etc.
                // Standard resolution (checks all scope tiers)
                self.resolve(to_name)
            }
            _ => {
                // For other relationship types (References, Defines, etc.),
                // use standard resolution
                self.resolve(to_name)
            }
        }
    }

    /// Populate imports into scope
    ///
    /// Processes import statements and stores them for later resolution.
    /// Java imports can be:
    /// - Single-type: import com.example.MyClass;
    /// - On-demand (wildcard): import com.example.*;
    /// - Static single: import static com.example.MyClass.method;
    /// - Static on-demand: import static com.example.MyClass.*;
    fn populate_imports(&mut self, imports: &[crate::parsing::Import]) {
        for import in imports {
            // Store raw import for later resolution
            self.imports
                .push((import.path.clone(), import.alias.clone()));

            // If import has an alias, we can immediately add it to our scope
            // (the actual symbol resolution happens later in the indexer)
            if let Some(alias) = &import.alias {
                // For aliased imports, we create a binding that will be resolved later
                // Example: import com.example.Foo as Bar;
                // This makes "Bar" available in the importing file
                self.import_bindings.insert(
                    alias.clone(),
                    ImportBinding {
                        import: import.clone(),
                        exposed_name: alias.clone(),
                        origin: crate::parsing::resolution::ImportOrigin::Unknown,
                        resolved_symbol: None,
                    },
                );
            } else if import.is_glob {
                // Wildcard imports (com.example.*) don't expose specific names yet
                // They'll be resolved when we see references to undefined symbols
            } else {
                // Single-type imports without alias
                // Extract the type name from the path
                // Example: import com.example.MyClass; exposes "MyClass"
                if let Some(type_name) = import.path.rsplit('.').next() {
                    self.import_bindings.insert(
                        type_name.to_string(),
                        ImportBinding {
                            import: import.clone(),
                            exposed_name: type_name.to_string(),
                            origin: crate::parsing::resolution::ImportOrigin::Unknown,
                            resolved_symbol: None,
                        },
                    );
                }
            }
        }
    }

    /// Register import binding
    /// TODO: Implement
    fn register_import_binding(&mut self, binding: ImportBinding) {
        self.import_bindings
            .insert(binding.exposed_name.clone(), binding);
    }
}

/// Inheritance resolver for Java
///
/// Tracks class inheritance (single) and interface implementation (multiple)
pub struct JavaInheritanceResolver {
    // Track inheritance relationships
    parents: HashMap<String, Vec<String>>, // child � [superclass, interfaces...]

    // Track methods defined by types
    type_methods: HashMap<String, HashSet<String>>, // type � methods

    // Track which types are interfaces vs classes
    interfaces: HashSet<String>, // Set of interface type names
}

impl JavaInheritanceResolver {
    pub fn new() -> Self {
        Self {
            parents: HashMap::new(),
            type_methods: HashMap::new(),
            interfaces: HashSet::new(),
        }
    }

    /// Register a type as an interface
    pub fn register_interface(&mut self, type_name: String) {
        self.interfaces.insert(type_name);
    }

    /// Register a type as a class (removes from interfaces if present)
    pub fn register_class(&mut self, type_name: String) {
        self.interfaces.remove(&type_name);
    }

    // =========================================================================
    // HELPER METHODS (matching Kotlin's internal API)
    // =========================================================================

    /// Resolve method recursively through inheritance chain
    fn resolve_method_recursive(
        &self,
        type_name: &str,
        method_name: &str,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        // Cycle detection
        if !visited.insert(type_name.to_string()) {
            return None;
        }

        // Check if method is defined on this type
        if self
            .type_methods
            .get(type_name)
            .is_some_and(|methods| methods.contains(method_name))
        {
            return Some(type_name.to_string());
        }

        // Search in parents (superclass and interfaces)
        if let Some(parents) = self.parents.get(type_name) {
            for parent in parents {
                if let Some(found) = self.resolve_method_recursive(parent, method_name, visited) {
                    return Some(found);
                }
            }
        }

        None
    }

    /// Collect full inheritance chain with cycle detection
    fn collect_chain(&self, ty: &str, visited: &mut HashSet<String>, out: &mut Vec<String>) {
        if visited.contains(ty) {
            return; // Cycle detected
        }
        visited.insert(ty.to_string());

        if let Some(parents) = self.parents.get(ty) {
            for parent in parents {
                out.push(parent.clone());
                self.collect_chain(parent, visited, out);
            }
        }
    }

    /// Gather all methods from type and parents with cycle detection
    fn gather_methods(&self, ty: &str, visited: &mut HashSet<String>, out: &mut HashSet<String>) {
        if visited.contains(ty) {
            return;
        }
        visited.insert(ty.to_string());

        if let Some(methods) = self.type_methods.get(ty) {
            out.extend(methods.iter().cloned());
        }

        if let Some(parents) = self.parents.get(ty) {
            for parent in parents {
                self.gather_methods(parent, visited, out);
            }
        }
    }

    /// Add methods defined by a type
    pub fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods
            .insert(type_name, methods.into_iter().collect());
    }

    /// Check if type is an interface
    pub fn is_interface(&self, type_name: &str) -> bool {
        self.interfaces.contains(type_name)
    }

    /// Check if type has a specific method (including inherited)
    pub fn type_has_method(&self, type_name: &str, method_name: &str) -> bool {
        let mut visited = HashSet::new();
        self.resolve_method_recursive(type_name, method_name, &mut visited)
            .is_some()
    }

    /// Check if child is subtype of parent (recursive with cycle detection)
    fn is_subtype_recursive(
        &self,
        child: &str,
        parent: &str,
        visited: &mut HashSet<String>,
    ) -> bool {
        if child == parent {
            return true;
        }

        if visited.contains(child) {
            return false; // Cycle detected
        }
        visited.insert(child.to_string());

        if let Some(parents) = self.parents.get(child) {
            for p in parents {
                if self.is_subtype_recursive(p, parent, visited) {
                    return true;
                }
            }
        }

        false
    }
}

impl Default for JavaInheritanceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl InheritanceResolver for JavaInheritanceResolver {
    /// Add inheritance relationship (extends or implements)
    fn add_inheritance(&mut self, child: String, parent: String, _kind: &str) {
        self.parents.entry(child).or_default().push(parent);
    }

    /// Resolve which type provides a method by walking inheritance chain
    fn resolve_method(&self, type_name: &str, method_name: &str) -> Option<String> {
        let mut visited = HashSet::new();
        self.resolve_method_recursive(type_name, method_name, &mut visited)
    }

    /// Get full inheritance chain for a type (DFS traversal)
    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.collect_chain(type_name, &mut visited, &mut result);
        result
    }

    /// Check if child is subtype of parent
    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        let mut visited = HashSet::new();
        self.is_subtype_recursive(child, parent, &mut visited)
    }

    /// Add methods defined by a type
    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.type_methods
            .insert(type_name, methods.into_iter().collect());
    }

    /// Get all methods accessible from a type (including inherited)
    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut result = HashSet::new();
        self.gather_methods(type_name, &mut visited, &mut result);
        result.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::Import;

    #[test]
    fn test_populate_imports_single_type() {
        let mut ctx = JavaResolutionContext::new(FileId(1));

        let imports = vec![
            Import {
                path: "com.example.MyClass".to_string(),
                alias: None,
                file_id: FileId(1),
                is_glob: false,
                is_type_only: false,
            },
            Import {
                path: "com.example.utils.Helper".to_string(),
                alias: None,
                file_id: FileId(1),
                is_glob: false,
                is_type_only: false,
            },
        ];

        ctx.populate_imports(&imports);

        // Check that type names are exposed
        assert!(ctx.import_binding("MyClass").is_some());
        assert!(ctx.import_binding("Helper").is_some());

        // Verify binding details
        let binding = ctx.import_binding("MyClass").unwrap();
        assert_eq!(binding.exposed_name, "MyClass");
        assert_eq!(binding.import.path, "com.example.MyClass");
    }

    #[test]
    fn test_populate_imports_with_alias() {
        let mut ctx = JavaResolutionContext::new(FileId(1));

        let imports = vec![Import {
            path: "com.example.LongClassName".to_string(),
            alias: Some("Short".to_string()),
            file_id: FileId(1),
            is_glob: false,
            is_type_only: false,
        }];

        ctx.populate_imports(&imports);

        // Aliased name should be available
        assert!(ctx.import_binding("Short").is_some());

        // Original name should NOT be available (only alias)
        assert!(ctx.import_binding("LongClassName").is_none());

        let binding = ctx.import_binding("Short").unwrap();
        assert_eq!(binding.exposed_name, "Short");
        assert_eq!(binding.import.path, "com.example.LongClassName");
    }

    #[test]
    fn test_populate_imports_wildcard() {
        let mut ctx = JavaResolutionContext::new(FileId(1));

        let imports = vec![Import {
            path: "com.example.*".to_string(),
            alias: None,
            file_id: FileId(1),
            is_glob: true,
            is_type_only: false,
        }];

        ctx.populate_imports(&imports);

        // Wildcard imports don't expose specific names immediately
        // They're stored for later resolution
        assert_eq!(ctx.imports.len(), 1);
        assert_eq!(ctx.imports[0].0, "com.example.*");
    }

    #[test]
    fn test_populate_imports_multiple() {
        let mut ctx = JavaResolutionContext::new(FileId(1));

        let imports = vec![
            Import {
                path: "java.util.List".to_string(),
                alias: None,
                file_id: FileId(1),
                is_glob: false,
                is_type_only: false,
            },
            Import {
                path: "java.util.ArrayList".to_string(),
                alias: Some("AL".to_string()),
                file_id: FileId(1),
                is_glob: false,
                is_type_only: false,
            },
            Import {
                path: "java.util.*".to_string(),
                alias: None,
                file_id: FileId(1),
                is_glob: true,
                is_type_only: false,
            },
        ];

        ctx.populate_imports(&imports);

        // Regular import
        assert!(ctx.import_binding("List").is_some());

        // Aliased import
        assert!(ctx.import_binding("AL").is_some());
        assert!(ctx.import_binding("ArrayList").is_none());

        // All imports stored
        assert_eq!(ctx.imports.len(), 3);
    }

    #[test]
    fn test_is_interface_tracking() {
        let mut resolver = JavaInheritanceResolver::new();

        // Register some interfaces
        resolver.register_interface("java.util.List".to_string());
        resolver.register_interface("java.io.Serializable".to_string());

        // Register some classes
        resolver.register_class("java.util.ArrayList".to_string());
        resolver.register_class("java.lang.String".to_string());

        // Test interface detection
        assert!(resolver.is_interface("java.util.List"));
        assert!(resolver.is_interface("java.io.Serializable"));
        assert!(!resolver.is_interface("java.util.ArrayList"));
        assert!(!resolver.is_interface("java.lang.String"));
        assert!(!resolver.is_interface("unknown.Type"));

        // Test that register_class removes from interfaces
        resolver.register_interface("com.example.Foo".to_string());
        assert!(resolver.is_interface("com.example.Foo"));

        resolver.register_class("com.example.Foo".to_string());
        assert!(!resolver.is_interface("com.example.Foo"));
    }

    #[test]
    fn test_resolve_relationship() {
        use crate::RelationKind;

        let mut ctx = JavaResolutionContext::new(FileId(1));

        // Setup some symbols in different scopes
        ctx.file_scope.insert("MyClass".to_string(), SymbolId(1));
        ctx.file_scope
            .insert("MyClass.staticMethod".to_string(), SymbolId(2));
        ctx.imported_symbols
            .insert("ImportedClass".to_string(), SymbolId(3));
        ctx.package_scope.insert("Helper".to_string(), SymbolId(4));

        // Test Extends relationship
        assert_eq!(
            ctx.resolve_relationship("", "MyClass", RelationKind::Extends, FileId(1)),
            Some(SymbolId(1))
        );

        // Test Implements relationship
        assert_eq!(
            ctx.resolve_relationship("", "ImportedClass", RelationKind::Implements, FileId(1)),
            Some(SymbolId(3))
        );

        // Test Calls relationship - qualified name
        assert_eq!(
            ctx.resolve_relationship("", "MyClass.staticMethod", RelationKind::Calls, FileId(1)),
            Some(SymbolId(2))
        );

        // Test Calls relationship - simple name
        assert_eq!(
            ctx.resolve_relationship("", "Helper", RelationKind::Calls, FileId(1)),
            Some(SymbolId(4))
        );

        // Test qualified call fallback (when full name not found, try method name)
        ctx.file_scope.insert("someMethod".to_string(), SymbolId(5));
        assert_eq!(
            ctx.resolve_relationship(
                "",
                "UnknownClass.someMethod",
                RelationKind::Calls,
                FileId(1)
            ),
            Some(SymbolId(5))
        );

        // Test Uses relationship
        assert_eq!(
            ctx.resolve_relationship("", "MyClass", RelationKind::Uses, FileId(1)),
            Some(SymbolId(1))
        );
    }
}
