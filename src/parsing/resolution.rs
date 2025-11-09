//! Language-agnostic resolution traits for symbol and inheritance resolution
//!
//! This module provides the trait abstractions that allow each language to implement
//! its own resolution logic while keeping the indexer language-agnostic.

use super::context::ScopeType;
use crate::{FileId, SymbolId, parsing::Import};
use std::collections::HashMap;

/// Scope levels that work across all languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeLevel {
    /// Function/block scope (local variables, parameters)
    Local,
    /// Module/file scope (module-level definitions)
    Module,
    /// Package/namespace scope (package-level visibility)
    Package,
    /// Global/project scope (public exports)
    Global,
}

/// Classification of where an import originates from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOrigin {
    /// The import refers to code defined within the current project/crate
    Internal,
    /// The import comes from an external dependency that is not indexed
    External,
    /// The origin could not be determined
    Unknown,
}

/// Binding information for an import exposed to a file
#[derive(Debug, Clone)]
pub struct ImportBinding {
    /// Original import record extracted from the parser or index
    pub import: Import,
    /// The name that becomes visible in the file (alias or trailing segment)
    pub exposed_name: String,
    /// Where the import originates from
    pub origin: ImportOrigin,
    /// Resolved local symbol, if any
    pub resolved_symbol: Option<SymbolId>,
}

/// Language-agnostic resolution scope
///
/// Each language implements this trait to provide its own scoping rules.
/// For example:
/// - Rust: local -> imports -> module -> crate
/// - Python: LEGB (Local, Enclosing, Global, Built-in)
/// - TypeScript: hoisting, namespaces, type vs value space
pub trait ResolutionScope: Send + Sync {
    /// Add a symbol to the scope at the specified level
    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel);

    /// Resolve a symbol name according to language-specific rules
    fn resolve(&self, name: &str) -> Option<SymbolId>;

    /// Clear the local scope (e.g., when exiting a function)
    fn clear_local_scope(&mut self);

    /// Enter a new scope (e.g., entering a function or block)
    fn enter_scope(&mut self, scope_type: ScopeType);

    /// Exit the current scope
    fn exit_scope(&mut self);

    /// Get all symbols currently in scope (for debugging)
    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)>;

    /// Get as Any for downcasting (needed for language-specific operations)
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Resolve a relationship-aware symbol reference
    ///
    /// This method allows languages to handle relationship-specific resolution logic.
    /// For example, Rust needs special handling for trait method definitions vs
    /// inherent method definitions.
    ///
    /// Default implementation just delegates to standard resolve() for backward compatibility.
    ///
    /// # Parameters
    /// - `from_name`: The source symbol name (e.g., "Display" for trait)
    /// - `to_name`: The target symbol name (e.g., "fmt" for method)
    /// - `kind`: The relationship kind (Defines, Calls, Implements, etc.)
    /// - `from_file`: The file where the relationship originates
    ///
    /// # Returns
    /// The resolved SymbolId if found, None otherwise
    fn resolve_relationship(
        &self,
        from_name: &str,
        to_name: &str,
        kind: crate::RelationKind,
        from_file: FileId,
    ) -> Option<SymbolId> {
        // Default: use standard resolution
        // Languages can override for relationship-specific logic
        let _ = (from_name, kind, from_file); // Unused in default impl
        self.resolve(to_name)
    }

    /// Populate import records into the resolution context
    ///
    /// Called during context building to load import statements from the index.
    /// Each language converts Import records into their internal representation
    /// for later use in is_external_import() checks.
    ///
    /// # Parameters
    /// - `imports`: List of Import records from the index for this file
    ///
    /// # Default Behavior
    /// Does nothing - languages that need import tracking must override this.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Rust context would call add_import() for each Import record
    /// for import in imports {
    ///     rust_context.add_import(import.path, import.alias);
    /// }
    /// ```
    fn populate_imports(&mut self, imports: &[crate::parsing::Import]) {
        let _ = imports; // Unused in default impl
    }

    /// Register a processed import binding for later queries
    ///
    /// Default implementation ignores the binding. Languages that need import-aware
    /// resolution should override to record the binding.
    fn register_import_binding(&mut self, binding: ImportBinding) {
        let _ = binding;
    }

    /// Retrieve a previously registered import binding by exposed name
    ///
    /// Implementations should return a cloned binding if one exists. Default returns None.
    fn import_binding(&self, _name: &str) -> Option<ImportBinding> {
        None
    }

    /// Resolve an expression (e.g., receiver) to a concrete type string if available
    ///
    /// Default implementation returns None. Languages can override to provide
    /// richer resolution data (e.g., Kotlin generic flow).
    fn resolve_expression_type(&self, _expr: &str) -> Option<String> {
        None
    }

    /// Check if a name refers to an external import
    ///
    /// This method determines if a symbol name comes from an external library
    /// or package that is not indexed in the current codebase.
    ///
    /// When true, the indexer will not attempt to resolve methods or members
    /// of this symbol, preventing incorrect resolution to local symbols with
    /// the same name.
    ///
    /// # Parameters
    /// - `name`: The symbol name to check (e.g., "ProgressBar", "React")
    ///
    /// # Returns
    /// true if the name is from an external import with no local symbol_id
    ///
    /// # Default Behavior
    /// Returns false - assumes all symbols are local or can be resolved.
    /// Languages should override this to check their import lists.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Rust: use indicatif::ProgressBar;
    /// context.is_external_import("ProgressBar") // true - external crate
    /// context.is_external_import("MyStruct")     // false - local symbol
    /// ```
    fn is_external_import(&self, name: &str) -> bool {
        if let Some(binding) = self.import_binding(name) {
            match binding.origin {
                ImportOrigin::External => true,
                ImportOrigin::Internal => binding.resolved_symbol.is_none(),
                ImportOrigin::Unknown => binding.resolved_symbol.is_none(),
            }
        } else {
            false
        }
    }

    /// Check if a relationship between two symbol kinds is valid
    ///
    /// This method defines which relationships are semantically valid for a language.
    /// For example, in most languages a Function can Call another Function, but
    /// may not be able to Call a Constant. Languages override this to define
    /// their specific semantics (e.g., TypeScript allowing Constants to be callable
    /// for React components).
    ///
    /// # Parameters
    /// - `from_kind`: The kind of the source symbol
    /// - `to_kind`: The kind of the target symbol
    /// - `rel_kind`: The type of relationship
    ///
    /// # Returns
    /// true if the relationship is valid, false otherwise
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
                // Executable code can call other executable code
                let caller_can_call = matches!(from_kind, Function | Method | Macro | Module);
                let callee_can_be_called = matches!(to_kind, Function | Method | Macro | Class);
                caller_can_call && callee_can_be_called
            }
            CalledBy => {
                // Reverse of Calls
                let caller_can_call = matches!(to_kind, Function | Method | Macro | Module);
                let callee_can_be_called = matches!(from_kind, Function | Method | Macro | Class);
                callee_can_be_called && caller_can_call
            }
            Implements => {
                // Types can implement interfaces/traits
                matches!(from_kind, Struct | Enum | Class) && matches!(to_kind, Trait | Interface)
            }
            ImplementedBy => {
                // Reverse of Implements
                matches!(from_kind, Trait | Interface) && matches!(to_kind, Struct | Enum | Class)
            }
            Uses => {
                // Most symbols can use/reference types and values
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
                // Reverse of Uses
                let can_use = matches!(
                    to_kind,
                    Function | Method | Struct | Class | Trait | Interface | Module | Enum
                );
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
                can_be_used && can_use
            }
            Defines => {
                // Containers can define members
                let container = matches!(
                    from_kind,
                    Trait | Interface | Module | Struct | Enum | Class
                );
                let member = matches!(to_kind, Method | Function | Constant | Field | Variable);
                container && member
            }
            DefinedIn => {
                // Reverse of Defines
                let member = matches!(from_kind, Method | Function | Constant | Field | Variable);
                let container =
                    matches!(to_kind, Trait | Interface | Module | Struct | Enum | Class);
                member && container
            }
            Extends => {
                // Types can extend other types (inheritance)
                let extendable = matches!(from_kind, Class | Interface | Trait | Struct | Enum);
                let can_be_extended = matches!(to_kind, Class | Interface | Trait | Struct | Enum);
                extendable && can_be_extended
            }
            ExtendedBy => {
                // Reverse of Extends
                matches!(from_kind, Class | Interface | Trait | Struct | Enum)
                    && matches!(to_kind, Class | Interface | Trait | Struct | Enum)
            }
            References => {
                // Very permissive - almost anything can reference anything
                true
            }
            ReferencedBy => {
                // Reverse of References - also permissive
                true
            }
        }
    }
}

/// Project-specific resolution enhancement
///
/// This trait allows languages to enhance their import resolution with
/// project configuration (tsconfig.json, pyproject.toml, go.mod, etc.)
pub trait ProjectResolutionEnhancer: Send + Sync {
    /// Transform an import path using project-specific rules
    ///
    /// Returns None if no transformation is needed (use original path)
    /// Returns Some(enhanced_path) if the import should be resolved differently
    ///
    /// # Examples
    /// - TypeScript: "@app/utils" -> "src/app/utils"
    /// - Python: ".utils" -> "mypackage.submodule.utils"
    /// - Go: "old/pkg" -> "../new/pkg"
    /// - PHP: "App\Utils" -> "src/Utils.php"
    fn enhance_import_path(&self, import_path: &str, from_file: FileId) -> Option<String>;

    /// Get all possible candidate paths for an import
    ///
    /// Some project configs support multiple target paths (e.g., TypeScript paths)
    fn get_import_candidates(&self, import_path: &str, from_file: FileId) -> Vec<String> {
        // Default: single enhancement or original
        if let Some(enhanced) = self.enhance_import_path(import_path, from_file) {
            vec![enhanced]
        } else {
            vec![import_path.to_string()]
        }
    }
}

/// Language-agnostic inheritance resolver
///
/// Each language implements this trait to handle its inheritance model:
/// - Rust: traits and inherent implementations
/// - TypeScript: interfaces and class extension
/// - Python: multiple inheritance with MRO
/// - PHP: traits and interfaces
/// - Go: interfaces and struct embedding
pub trait InheritanceResolver: Send + Sync {
    /// Add an inheritance relationship
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str);

    /// Resolve which parent provides a method
    fn resolve_method(&self, type_name: &str, method: &str) -> Option<String>;

    /// Get the inheritance chain for a type
    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String>;

    /// Check if one type is a subtype of another
    fn is_subtype(&self, child: &str, parent: &str) -> bool;

    /// Add methods that a type defines
    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>);

    /// Get all methods available on a type (including inherited)
    fn get_all_methods(&self, type_name: &str) -> Vec<String>;
}

/// Generic resolution context that wraps the existing ResolutionContext
///
/// This provides a default implementation that maintains backward compatibility
/// while allowing languages to override with their own logic.
pub struct GenericResolutionContext {
    #[allow(dead_code)]
    file_id: FileId, // Kept for future use when we need file-specific resolution
    symbols: HashMap<ScopeLevel, HashMap<String, SymbolId>>,
    scope_stack: Vec<ScopeType>,
    import_bindings: HashMap<String, ImportBinding>,
}

impl GenericResolutionContext {
    /// Create a new generic resolution context
    pub fn new(file_id: FileId) -> Self {
        let mut symbols = HashMap::new();
        symbols.insert(ScopeLevel::Local, HashMap::new());
        symbols.insert(ScopeLevel::Module, HashMap::new());
        symbols.insert(ScopeLevel::Package, HashMap::new());
        symbols.insert(ScopeLevel::Global, HashMap::new());

        Self {
            file_id,
            symbols,
            scope_stack: vec![ScopeType::Global],
            import_bindings: HashMap::new(),
        }
    }

    /// Wrap an existing ResolutionContext (for migration)
    pub fn from_existing(file_id: FileId) -> Self {
        Self::new(file_id)
    }
}

impl ResolutionScope for GenericResolutionContext {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        self.symbols
            .entry(scope_level)
            .or_default()
            .insert(name, symbol_id);
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Check scopes in order: Local -> Module -> Package -> Global
        for level in &[
            ScopeLevel::Local,
            ScopeLevel::Module,
            ScopeLevel::Package,
            ScopeLevel::Global,
        ] {
            if let Some(symbols) = self.symbols.get(level) {
                if let Some(&id) = symbols.get(name) {
                    return Some(id);
                }
            }
        }
        None
    }

    fn clear_local_scope(&mut self) {
        if let Some(local) = self.symbols.get_mut(&ScopeLevel::Local) {
            local.clear();
        }
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.scope_stack.push(scope_type);
    }

    fn exit_scope(&mut self) {
        self.scope_stack.pop();
        // Clear local scope when exiting a function
        if matches!(self.scope_stack.last(), Some(ScopeType::Function { .. })) {
            self.clear_local_scope();
        }
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        let mut result = Vec::new();
        for (&level, symbols) in &self.symbols {
            for (name, &id) in symbols {
                result.push((name.clone(), id, level));
            }
        }
        result
    }

    fn register_import_binding(&mut self, binding: ImportBinding) {
        self.import_bindings
            .insert(binding.exposed_name.clone(), binding);
    }

    fn import_binding(&self, name: &str) -> Option<ImportBinding> {
        self.import_bindings.get(name).cloned()
    }
}

/// Generic inheritance resolver that provides default implementation
///
/// This wraps existing trait resolution logic while allowing languages
/// to provide their own inheritance semantics.
pub struct GenericInheritanceResolver {
    /// Maps child to parent relationships
    inheritance: HashMap<String, Vec<(String, String)>>, // (parent, kind)
    /// Maps types to their methods
    type_methods: HashMap<String, Vec<String>>,
}

impl GenericInheritanceResolver {
    /// Create a new generic inheritance resolver
    pub fn new() -> Self {
        Self {
            inheritance: HashMap::new(),
            type_methods: HashMap::new(),
        }
    }
}

impl Default for GenericInheritanceResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl InheritanceResolver for GenericInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        self.inheritance
            .entry(child)
            .or_default()
            .push((parent, kind.to_string()));
    }

    fn resolve_method(&self, type_name: &str, method: &str) -> Option<String> {
        // First check if the type has the method directly
        if let Some(methods) = self.type_methods.get(type_name) {
            if methods.contains(&method.to_string()) {
                return Some(type_name.to_string());
            }
        }

        // Then check parent types
        if let Some(parents) = self.inheritance.get(type_name) {
            for (parent, _kind) in parents {
                if let Some(result) = self.resolve_method(parent, method) {
                    return Some(result);
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
    fn test_default_compatibility_function_calls_function() {
        let context = GenericResolutionContext::new(FileId::new(1).unwrap());
        assert!(context.is_compatible_relationship(
            crate::SymbolKind::Function,
            crate::SymbolKind::Function,
            crate::RelationKind::Calls
        ));
    }

    #[test]
    fn test_default_compatibility_function_cannot_call_constant() {
        let context = GenericResolutionContext::new(FileId::new(1).unwrap());
        assert!(!context.is_compatible_relationship(
            crate::SymbolKind::Function,
            crate::SymbolKind::Constant,
            crate::RelationKind::Calls
        ));
    }

    #[test]
    fn test_default_compatibility_class_extends_class() {
        let context = GenericResolutionContext::new(FileId::new(1).unwrap());
        assert!(context.is_compatible_relationship(
            crate::SymbolKind::Class,
            crate::SymbolKind::Class,
            crate::RelationKind::Extends
        ));
    }

    #[test]
    fn test_generic_resolution_context() {
        let mut ctx = GenericResolutionContext::new(FileId::new(1).unwrap());

        // Add symbols at different scope levels
        ctx.add_symbol(
            "local_var".to_string(),
            SymbolId::new(1).unwrap(),
            ScopeLevel::Local,
        );
        ctx.add_symbol(
            "module_fn".to_string(),
            SymbolId::new(2).unwrap(),
            ScopeLevel::Module,
        );
        ctx.add_symbol(
            "global_type".to_string(),
            SymbolId::new(3).unwrap(),
            ScopeLevel::Global,
        );

        // Test resolution order
        assert_eq!(ctx.resolve("local_var"), Some(SymbolId::new(1).unwrap()));
        assert_eq!(ctx.resolve("module_fn"), Some(SymbolId::new(2).unwrap()));
        assert_eq!(ctx.resolve("global_type"), Some(SymbolId::new(3).unwrap()));
        assert_eq!(ctx.resolve("unknown"), None);

        // Test scope clearing
        ctx.clear_local_scope();
        assert_eq!(ctx.resolve("local_var"), None);
        assert_eq!(ctx.resolve("module_fn"), Some(SymbolId::new(2).unwrap()));
    }

    #[test]
    fn test_generic_inheritance_resolver() {
        let mut resolver = GenericInheritanceResolver::new();

        // Set up a simple inheritance hierarchy
        resolver.add_inheritance("Child".to_string(), "Parent".to_string(), "extends");
        resolver.add_inheritance("Parent".to_string(), "GrandParent".to_string(), "extends");

        // Add methods
        resolver.add_type_methods("GrandParent".to_string(), vec!["method1".to_string()]);
        resolver.add_type_methods("Parent".to_string(), vec!["method2".to_string()]);
        resolver.add_type_methods("Child".to_string(), vec!["method3".to_string()]);

        // Test method resolution
        assert_eq!(
            resolver.resolve_method("Child", "method3"),
            Some("Child".to_string())
        );
        assert_eq!(
            resolver.resolve_method("Child", "method2"),
            Some("Parent".to_string())
        );
        assert_eq!(
            resolver.resolve_method("Child", "method1"),
            Some("GrandParent".to_string())
        );

        // Test inheritance chain
        let chain = resolver.get_inheritance_chain("Child");
        assert!(chain.contains(&"Child".to_string()));
        assert!(chain.contains(&"Parent".to_string()));
        assert!(chain.contains(&"GrandParent".to_string()));

        // Test subtype checking
        assert!(resolver.is_subtype("Child", "Parent"));
        assert!(resolver.is_subtype("Child", "GrandParent"));
        assert!(!resolver.is_subtype("Parent", "Child"));
    }
}
