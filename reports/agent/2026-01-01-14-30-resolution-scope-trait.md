# Research Report: ResolutionScope Trait and Implementations

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Sonnet 4.5

## Summary

The `ResolutionScope` trait is the core abstraction for symbol resolution, defined in `src/parsing/resolution.rs`. It provides a language-agnostic interface with a `resolve(&str) -> Option<SymbolId>` method that only takes a name. There is an extended `resolve_relationship()` method that adds relationship context (from_name, to_name, kind, from_file) but still lacks `to_range` and `language_id` disambiguation. The parallel pipeline uses a separate `PipelineSymbolCache` trait with a richer `resolve()` signature that includes all context needed for multi-tier resolution.

## Key Findings

### 1. ResolutionScope Trait Definition

The trait is defined at `src/parsing/resolution.rs:56-330` with these core methods:

```rust
pub trait ResolutionScope: Send + Sync {
    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel);
    fn resolve(&self, name: &str) -> Option<SymbolId>;
    fn clear_local_scope(&mut self);
    fn enter_scope(&mut self, scope_type: ScopeType);
    fn exit_scope(&mut self);
    fn symbols_in_scope(&self) -> Vec<(String, SymbolId, ScopeLevel)>;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    // Extended method with relationship context
    fn resolve_relationship(
        &self,
        from_name: &str,
        to_name: &str,
        kind: crate::RelationKind,
        from_file: FileId,
    ) -> Option<SymbolId>;

    // Import handling
    fn populate_imports(&mut self, imports: &[Import]);
    fn register_import_binding(&mut self, binding: ImportBinding);
    fn import_binding(&self, _name: &str) -> Option<ImportBinding>;
    fn is_external_import(&self, name: &str) -> bool;

    // Expression type resolution
    fn resolve_expression_type(&self, _expr: &str) -> Option<String>;

    // Relationship validation
    fn is_compatible_relationship(
        &self,
        from_kind: SymbolKind,
        to_kind: SymbolKind,
        rel_kind: RelationKind,
    ) -> bool;
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/resolution.rs:56-200`

### 2. The resolve() Method is Name-Only

The core `resolve(&str)` method takes only a symbol name:

```rust
fn resolve(&self, name: &str) -> Option<SymbolId>;
```

Each language implements its own resolution order. For example:

- **Rust** (`src/parsing/rust/resolution.rs:118-180`): local -> imported -> module -> crate, then qualified path handling
- **TypeScript** (`src/parsing/typescript/resolution.rs:209-280`): local_scope -> hoisted_scope -> imported_symbols -> module_symbols -> type_space -> global_symbols
- **Go** (`src/parsing/go/resolution.rs:967-1010`): local_scope -> package_symbols -> imported_symbols

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/rust/resolution.rs:118-180`

### 3. resolve_relationship() Adds Limited Context

The `resolve_relationship` method (trait line 94-107) provides:
- `from_name`: Source symbol name
- `to_name`: Target symbol name
- `kind`: RelationKind (Defines, Calls, Implements, etc.)
- `from_file`: FileId where relationship originates

Default implementation just delegates to `resolve(to_name)`. Languages override for relationship-specific logic:

**Rust** (`src/parsing/rust/resolution.rs:216-260`):
```rust
fn resolve_relationship(&self, _from_name: &str, to_name: &str,
                        kind: RelationKind, _from_file: FileId) -> Option<SymbolId> {
    match kind {
        RelationKind::Defines => { /* Trait method handling */ }
        RelationKind::Calls => { /* Qualified name handling */ }
        _ => self.resolve(to_name)
    }
}
```

**TypeScript** (`src/parsing/typescript/resolution.rs:342-395`):
```rust
fn resolve_relationship(&self, _from_name: &str, to_name: &str,
                        kind: RelationKind, _from_file: FileId) -> Option<SymbolId> {
    match kind {
        RelationKind::Implements | RelationKind::Extends => self.resolve(to_name),
        RelationKind::Calls => { /* Qualified name handling */ }
        RelationKind::Uses => { /* Type space lookup */ }
        _ => self.resolve(to_name)
    }
}
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/resolution.rs:94-107`

### 4. PipelineSymbolCache Has Richer Resolution

For the parallel pipeline, a separate trait `PipelineSymbolCache` (`src/parsing/resolution.rs:596-645`) provides multi-tier resolution:

```rust
pub trait PipelineSymbolCache: Send + Sync {
    fn resolve(
        &self,
        name: &str,
        from_file: FileId,
        to_range: Option<&Range>,
        imports: &[Import],
        language_id: LanguageId,
    ) -> ResolveResult;

    fn get(&self, id: SymbolId) -> Option<Symbol>;
    fn symbols_in_file(&self, file_id: FileId) -> Vec<SymbolId>;
    fn lookup_candidates(&self, name: &str) -> Vec<SymbolId>;
}
```

Priority order documented:
1. **Local**: Same file + matching name + defined before `to_range`
2. **Import**: Name matches import alias or last segment
3. **Same module**: Module path prefix match
4. **Cross-file**: Public symbol, same language

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/resolution.rs:596-645`

### 5. Context Held by Scope vs Received at Resolution

**Held by ResolutionScope context** (set during construction):
- `file_id`: The file this context is for
- Symbol maps: `local_scope`, `imported_symbols`, `module_symbols`, etc.
- `scope_stack`: Current scope nesting
- `imports`: List of import statements
- `import_bindings`: Resolved import bindings

**Received at resolution time**:
- `name`: Symbol name to resolve
- For `resolve_relationship`: `from_name`, `kind`, `from_file`

**NOT received** (gap for multi-context resolution):
- `to_range`: Call site location
- `language_id`: Language filter

**Evidence**: Struct definitions at:
- `/Users/bartolli/Projects/codanna/src/parsing/rust/resolution.rs:22-50`
- `/Users/bartolli/Projects/codanna/src/parsing/typescript/resolution.rs:35-80`

## Gaps for Multi-Context Resolution

### Gap 1: No to_range in ResolutionScope

The `resolve()` and `resolve_relationship()` methods lack `to_range` parameter. This prevents:
- Shadowing disambiguation (local variable defined after call site should not shadow)
- Scope-based resolution (which symbol is in scope at the call site)

The parallel pipeline's `PipelineSymbolCache.resolve()` accepts `to_range: Option<&Range>` but this is a separate trait, not integrated into `ResolutionScope`.

### Gap 2: No language_id Filtering

ResolutionScope methods do not accept `language_id`. Cross-language pollution is possible:
- A TypeScript file could resolve to a Rust symbol with the same name
- The context holds `file_id` but not `language_id`

`PipelineSymbolCache.resolve()` accepts `language_id: LanguageId` for filtering.

### Gap 3: Two Parallel Systems

There are effectively two resolution systems:
1. **ResolutionScope**: Used by `SimpleIndexer`, name-based, language-aware via context type
2. **PipelineSymbolCache**: Used by parallel pipeline, multi-tier, explicit parameters

This creates maintenance burden and divergent behavior.

### Gap 4: resolve_relationship Has Dead Parameters

The `resolve_relationship` signature accepts `from_name` and `from_file`, but most implementations ignore them:

```rust
fn resolve_relationship(&self, _from_name: &str, to_name: &str,
                        kind: RelationKind, _from_file: FileId) -> Option<SymbolId>
```

These parameters could be used for:
- Scoped resolution (is `from_name` a trait? use trait method scope)
- File-relative imports

## Recommendations

1. **Add `to_range` to `resolve_relationship`**:
   ```rust
   fn resolve_relationship(
       &self,
       from_name: &str,
       to_name: &str,
       kind: RelationKind,
       from_file: FileId,
       to_range: Option<&Range>,  // NEW
   ) -> Option<SymbolId>;
   ```

2. **Add `language_id` to ResolutionScope**:
   Store `language_id` in the context struct (already have `file_id`), use for filtering candidates.

3. **Consider unifying interfaces**:
   Either make `ResolutionScope` accept the same parameters as `PipelineSymbolCache.resolve()`, or extract a shared trait.

## Language Implementations

15 implementations of `ResolutionScope` exist:
- `GenericResolutionContext` (default)
- `RustResolutionContext`
- `TypeScriptResolutionContext`
- `JavaScriptResolutionContext`
- `GoResolutionContext`
- `PythonResolutionContext`
- `CResolutionContext`
- `CppResolutionContext`
- `CSharpResolutionContext`
- `JavaResolutionContext`
- `KotlinResolutionContext`
- `SwiftResolutionContext`
- `PhpResolutionContext`
- `GdscriptResolutionContext`
- `NoOpScope` (pipeline placeholder)

**Evidence**: Grep for `impl ResolutionScope for` returns all 15 implementations.
