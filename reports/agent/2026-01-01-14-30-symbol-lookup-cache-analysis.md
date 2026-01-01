# Research Report: SymbolLookupCache and PipelineSymbolCache Analysis

**Date**: 2026-01-01 14:30
**Agent**: Research-Agent-v5
**Model**: Sonnet 4.5

## Summary

The `SymbolLookupCache` struct in `types.rs` implements the `PipelineSymbolCache` trait from `resolution.rs`. The current implementation already supports all five parameters for multi-tier resolution. The RESOLVE stage correctly calls `cache.resolve(name, from_file, to_range, imports, language_id)`.

## Key Findings

### 1. PipelineSymbolCache Trait Definition

The trait is fully defined with the expected signature.

**Location**: `/Users/bartolli/Projects/codanna/src/parsing/resolution.rs:596-641`

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

**Parameters accepted**:
- `name: &str` - Symbol name to resolve
- `from_file: FileId` - File where reference appears
- `to_range: Option<&Range>` - Call site location for scope ordering
- `imports: &[Import]` - Visible imports for import matching
- `language_id: LanguageId` - Language filter

### 2. SymbolLookupCache Implementation

**Location**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:326-430`

The struct has three DashMap indexes:
```rust
pub struct SymbolLookupCache {
    by_id: DashMap<SymbolId, Symbol>,
    by_name: DashMap<Box<str>, Vec<SymbolId>>,
    by_file_id: DashMap<FileId, Vec<SymbolId>>,
}
```

**Methods on SymbolLookupCache**:
| Method | Signature | Purpose |
|--------|-----------|---------|
| `new()` | `fn new() -> Self` | Create empty cache |
| `with_capacity(symbols)` | `fn with_capacity(usize) -> Self` | Pre-allocated cache |
| `insert(symbol)` | `fn insert(&self, Symbol)` | Add symbol to all indexes |
| `get(id)` | `fn get(SymbolId) -> Option<Symbol>` | Direct ID lookup |
| `get_ref(id)` | `fn get_ref(SymbolId) -> Option<Ref<...>>` | Zero-copy lookup |
| `lookup_candidates(name)` | `fn lookup_candidates(&str) -> Vec<SymbolId>` | Name-based lookup |
| `symbols_in_file(file_id)` | `fn symbols_in_file(FileId) -> Vec<SymbolId>` | File's symbols |
| `file_count()` | `fn file_count() -> usize` | Number of files |
| `len()` | `fn len() -> usize` | Number of symbols |
| `is_empty()` | `fn is_empty() -> bool` | Empty check |
| `unique_names()` | `fn unique_names() -> usize` | Distinct names |

### 3. resolve() Implementation - Four-Tier Resolution

**Location**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/types.rs:433-523`

```rust
impl PipelineSymbolCache for SymbolLookupCache {
    fn resolve(
        &self,
        name: &str,
        from_file: FileId,
        to_range: Option<&Range>,
        imports: &[Import],
        language_id: LanguageId,
    ) -> ResolveResult { ... }
}
```

**Tier 1 - Local**: Same file + defined before to_range
- Filters by `sym.file_id == from_file`
- Uses `to_range` for ordering (prefers symbols defined before call site)

**Tier 2 - Import**: Matches import alias or path segment
- Checks `import.alias == Some(name)`
- Checks last segment of import path
- Calls `find_by_import_path()` for resolution

**Tier 3 - Same Language**: Filters by language_id
- `sym.language_id == Some(&language_id)`

**Tier 4 - Cross-file**: Any visible symbol (fallback)

### 4. RESOLVE Stage Usage

**Location**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/resolve.rs:98-113`

```rust
fn resolve_one(&self, unresolved: &UnresolvedRelationship, context: &ResolutionContext) -> Option<ResolvedRelationship> {
    let result = self.symbol_cache.resolve(
        &unresolved.to_name,
        context.file_id,
        unresolved.to_range.as_ref(),
        &context.imports,
        context.language_id,
    );
    ...
}
```

All five parameters are correctly passed from `ResolutionContext` and `UnresolvedRelationship`.

### 5. lookup_candidates() vs resolve()

| Aspect | `lookup_candidates()` | `resolve()` |
|--------|----------------------|-------------|
| Input | `name: &str` | name + file_id + to_range + imports + language_id |
| Output | `Vec<SymbolId>` | `ResolveResult` (Found/Ambiguous/NotFound) |
| Logic | Simple name lookup in `by_name` index | Multi-tier filtering with disambiguation |
| Use case | Internal building block | Full resolution with context |

`lookup_candidates()` is called internally by `resolve()` as the first step.

## Architecture/Patterns

### Resolution Flow

```
RESOLVE Stage
    |
    v
cache.resolve(name, from_file, to_range, imports, language_id)
    |
    +--> lookup_candidates(name) -> Vec<SymbolId>
    |
    +--> Tier 1: Filter by file_id + to_range ordering
    |
    +--> Tier 2: Match imports (alias or path segment)
    |       |
    |       +--> find_by_import_path(path, language_id)
    |
    +--> Tier 3: Filter by language_id
    |
    +--> Tier 4: Return any candidate
    |
    v
ResolveResult::Found | Ambiguous | NotFound
```

### Disambiguation Flow

When `resolve()` returns `Ambiguous`, the RESOLVE stage's `disambiguate()` method takes over:

**Location**: `/Users/bartolli/Projects/codanna/src/indexing/pipeline/stages/resolve.rs:157-221`

- Uses `find_closest_by_range()` for local shadowing
- Uses `is_imported()` with `LanguageBehavior::import_matches_symbol()`

## Current Gaps Analysis

### What Works

1. `to_range` is used for scope ordering in Tier 1 (local symbols)
2. `language_id` filtering is implemented in Tier 3
3. Import path matching exists in Tier 2 via `find_by_import_path()`
4. All parameters flow correctly from RESOLVE stage to cache

### What Could Be Improved

1. **Import path matching in Tier 2**: `find_by_import_path()` does naive path substring matching. Does not use `LanguageBehavior::import_matches_symbol()` for language-specific path resolution (e.g., tsconfig paths).

2. **Module path matching in Tier 3**: Currently skipped entirely. Could add module path prefix matching as an intermediate tier.

3. **Visibility filtering**: No visibility checks (public/private) in any tier. Cross-file resolution could respect visibility.

4. **Symbol kind compatibility**: No check whether the resolved symbol kind is valid for the relationship kind (e.g., Calls should resolve to callable symbols).

## Conclusions

The `PipelineSymbolCache::resolve()` signature is complete with all five parameters for multi-tier resolution:
- `to_range` for disambiguation
- `language_id` for filtering
- `imports` for import path matching

The RESOLVE stage correctly calls `cache.resolve(name, from_file, to_range, &imports, language_id)` at line 106.

**No missing parameters for the target call signature.** The implementation exists and handles all tiers. Potential improvements relate to matching logic quality (using behaviors for import matching) rather than missing API surface.
