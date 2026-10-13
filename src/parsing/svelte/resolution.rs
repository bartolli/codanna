//! Svelte-specific resolution and inheritance implementation.
//!
//! Svelte components hold JavaScript or TypeScript inside their `<script>`
//! blocks, so symbol resolution follows the exact same rules: hoisting of
//! functions and `var` declarations, block scoping for `let`/`const`, and ES
//! module semantics. Rather than duplicate that logic, the Svelte context wraps
//! [`JavaScriptResolutionContext`] and forwards to it. This keeps Svelte aligned
//! with the standard four-file parser layout (parser/behavior/definition/
//! resolution) while reusing the battle-tested JS resolver.

use crate::parsing::javascript::resolution::{
    JavaScriptInheritanceResolver, JavaScriptResolutionContext,
};
use crate::parsing::resolution::ImportBinding;
use crate::parsing::{InheritanceResolver, ResolutionScope, ScopeLevel, ScopeType};
use crate::symbol::ScopeContext;
use crate::{FileId, RelationKind, SymbolId, SymbolKind};
use std::any::Any;

/// Svelte resolution context.
///
/// Delegates to [`JavaScriptResolutionContext`]; see the module docs for why.
pub struct SvelteResolutionContext {
    inner: JavaScriptResolutionContext,
}

impl SvelteResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            inner: JavaScriptResolutionContext::new(file_id),
        }
    }

    /// Track an imported symbol (mirrors the JS context API).
    pub fn add_import_symbol(&mut self, name: String, symbol_id: SymbolId, is_type_only: bool) {
        self.inner.add_import_symbol(name, symbol_id, is_type_only);
    }

    /// Add a symbol using its scope context so hoisting/block-scoping apply.
    pub fn add_symbol_with_context(
        &mut self,
        name: String,
        symbol_id: SymbolId,
        scope_context: Option<&ScopeContext>,
    ) {
        self.inner
            .add_symbol_with_context(name, symbol_id, scope_context);
    }
}

impl ResolutionScope for SvelteResolutionContext {
    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        self.inner.add_symbol(name, symbol_id, scope_level);
    }

    fn resolve(&self, name: &str) -> Option<SymbolId> {
        self.inner.resolve(name)
    }

    fn clear_local_scope(&mut self) {
        self.inner.clear_local_scope();
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.inner.enter_scope(scope_type);
    }

    fn exit_scope(&mut self) {
        self.inner.exit_scope();
    }

    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)> {
        self.inner.symbols_in_scope()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn resolve_relationship(
        &self,
        from_name: &str,
        to_name: &str,
        kind: RelationKind,
        from_file: FileId,
    ) -> Option<SymbolId> {
        self.inner
            .resolve_relationship(from_name, to_name, kind, from_file)
    }

    fn populate_imports(&mut self, imports: &[crate::parsing::Import]) {
        self.inner.populate_imports(imports);
    }

    fn register_import_binding(&mut self, binding: ImportBinding) {
        self.inner.register_import_binding(binding);
    }

    fn import_binding(&self, name: &str) -> Option<ImportBinding> {
        self.inner.import_binding(name)
    }

    fn is_compatible_relationship(
        &self,
        from_kind: SymbolKind,
        to_kind: SymbolKind,
        rel_kind: RelationKind,
    ) -> bool {
        self.inner
            .is_compatible_relationship(from_kind, to_kind, rel_kind)
    }
}

/// Svelte inheritance resolver.
///
/// Class inheritance inside `<script>` blocks is plain JS/TS `extends`, so this
/// forwards to [`JavaScriptInheritanceResolver`].
#[derive(Default)]
pub struct SvelteInheritanceResolver {
    inner: JavaScriptInheritanceResolver,
}

impl SvelteInheritanceResolver {
    pub fn new() -> Self {
        Self {
            inner: JavaScriptInheritanceResolver::new(),
        }
    }
}

impl InheritanceResolver for SvelteInheritanceResolver {
    fn add_inheritance(&mut self, child: String, parent: String, kind: &str) {
        self.inner.add_inheritance(child, parent, kind);
    }

    fn resolve_method(&self, type_name: &str, method: &str) -> Option<String> {
        self.inner.resolve_method(type_name, method)
    }

    fn get_inheritance_chain(&self, type_name: &str) -> Vec<String> {
        self.inner.get_inheritance_chain(type_name)
    }

    fn is_subtype(&self, child: &str, parent: &str) -> bool {
        self.inner.is_subtype(child, parent)
    }

    fn add_type_methods(&mut self, type_name: String, methods: Vec<String>) {
        self.inner.add_type_methods(type_name, methods);
    }

    fn get_all_methods(&self, type_name: &str) -> Vec<String> {
        self.inner.get_all_methods(type_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svelte_resolution_delegates_to_js() {
        let file_id = FileId::new(1).unwrap();
        let mut context = SvelteResolutionContext::new(file_id);

        let sym = SymbolId::new(1).unwrap();
        context.add_symbol("greet".to_string(), sym, ScopeLevel::Module);

        assert_eq!(context.resolve("greet"), Some(sym));
        assert_eq!(context.resolve("missing"), None);
    }

    #[test]
    fn test_svelte_inheritance_delegates_to_js() {
        let mut resolver = SvelteInheritanceResolver::new();
        resolver.add_inheritance("Child".to_string(), "Parent".to_string(), "extends");

        assert!(resolver.is_subtype("Child", "Parent"));
        assert!(!resolver.is_subtype("Parent", "Child"));
    }
}
