# Research Report: SimpleIndexer Resolution Orchestration

**Date**: 2026-01-01 10:30
**Agent**: Research-Agent
**Model**: Sonnet 4.5

## Summary

SimpleIndexer uses a two-pass resolution system. During parsing, relationships are stored as unresolved (name-based). After all symbols are committed, `resolve_cross_file_relationships()` resolves them in two passes: Defines first, then Calls. This ensures instance method calls can query Defines relationships to find method targets.

## Key Findings

### 1. Unresolved Relationship Storage

During parsing, relationships are captured with names (not IDs) and stored for later resolution.

**Entry Point**: `add_relationships_by_name_with_range()`

```rust
fn add_relationships_by_name_with_range(
    &mut self,
    from_id: Option<SymbolId>,  // May have from_id already
    from_name: &str,
    to_name: &str,
    file_id: FileId,
    kind: RelationKind,
    metadata: Option<RelationshipMetadata>,
    to_range: Option<crate::Range>,  // Used for Defines disambiguation
) -> IndexResult<()>
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:1605-1636`

### 2. Two-Pass Resolution Architecture

**Main method**: `resolve_cross_file_relationships()`

```rust
fn resolve_cross_file_relationships(&mut self) -> IndexResult<()>
```

**Pass 1 (Defines)**:
1. Split relationships: `partition(|rel| rel.kind == RelationKind::Defines)`
2. Group by file_id
3. Build resolution context per file via `build_resolution_context(file_id)`
4. Resolve each relationship using `context.resolve_relationship()` or range-based lookup
5. **COMMIT to Tantivy** - critical for Pass 2

**Pass 2 (Calls and others)**:
1. Group remaining relationships by file_id
2. Build resolution context per file
3. For Calls relationships with method call data:
   - Lookup stored `MethodCall` from `method_call_resolvers`
   - Call `behavior.resolve_method_call()` for unified resolution
4. Otherwise fall back to `context.resolve()`

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2621-3150`

### 3. Getting Behavior for a File

```rust
fn get_behavior_for_file(
    &self,
    file_id: FileId,
) -> IndexResult<&dyn crate::parsing::LanguageBehavior>
```

Returns stored behavior from `file_behaviors` HashMap. Behaviors are stored during file indexing (line 731) based on language detection.

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:839-850`

### 4. Building Resolution Context

```rust
fn build_resolution_context(&self, file_id: FileId) -> IndexResult<Box<dyn ResolutionScope>>
```

**Logic**:
1. Get behavior for file
2. If `symbol_cache` available: use `behavior.build_resolution_context_with_cache()` (fast path)
3. Otherwise: use `behavior.build_resolution_context()` (Tantivy path)

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs:2605-2619`

### 5. Behavior's build_resolution_context()

Located in `language_behavior.rs`, the method:

1. Creates language-specific context via `create_resolution_context(file_id)`
2. Gets imports from Tantivy + in-memory state (merged)
3. Populates raw imports into context
4. Resolves each import to symbol ID, registers bindings
5. Adds file's module-level symbols
6. Adds same-package symbols (for Java/Kotlin/Go)
7. Adds visible global symbols (limit 10,000)
8. Calls `initialize_resolution_context()` hook

**Evidence**: `/Users/bartolli/Projects/codanna/src/parsing/language_behavior.rs:510-670`

### 6. Resolution Flow (Simplified)

```
Unresolved Relationship (from_name, to_name, kind, file_id)
    |
    v
resolve_cross_file_relationships()
    |
    +-- Pass 1: Defines
    |       |
    |       v
    |   build_resolution_context(file_id)
    |       |
    |       v
    |   context.resolve_relationship() or find_symbol_by_name_and_range()
    |       |
    |       v
    |   add_relationship_internal(from_id, to_id, rel)
    |       |
    |       v
    |   document_index.store_relationship() --> TANTIVY WRITE
    |       |
    |       v
    |   COMMIT (so Calls can query Defines)
    |
    +-- Pass 2: Calls, Extends, Implements, etc.
            |
            v
        build_resolution_context(file_id)
            |
            v
        For Calls with MethodCall data:
            behavior.resolve_method_call() --> May query Defines via Tantivy
        Otherwise:
            context.resolve()
            |
            v
        add_relationship_internal() --> TANTIVY WRITE
            |
            v
        COMMIT
```

## When Tantivy is Accessed vs Cache

### Tantivy Reads

| Operation | Location | Purpose |
|-----------|----------|---------|
| `find_symbol_by_id` | Pass 1 & 2 resolution | Fetch symbol for from_id or to_id |
| `find_symbols_by_name` | Name-based lookup | Fallback when from_id missing |
| `find_symbol_by_name_and_range` | Defines with overloads | Disambiguate by range |
| `get_imports_for_file` | build_resolution_context | Get persisted imports |
| `find_symbols_by_file` | build_resolution_context | Get file's symbols |
| `find_symbols_by_module` | build_resolution_context | Same-package symbols |
| `get_all_symbols(10000)` | build_resolution_context | Global visible symbols |
| `get_relationships_from` | resolve_instance_method | Query Defines for method lookup |

### Cache Usage

| Cache | Purpose |
|-------|---------|
| `symbol_lookup_cache` (local) | Avoid duplicate Tantivy queries for same name |
| `symbol_cache` (ConcurrentSymbolCache) | O(1) lookups in `build_resolution_context_with_cache` |
| `method_call_resolvers` | Stored MethodCall objects per file for enhanced resolution |
| `file_behaviors` | Language behaviors with import state |

### Tantivy Writes

| Operation | When |
|-----------|------|
| `store_relationship` | After successful resolution in both passes |
| Reverse relationship | Automatic after forward relationship |
| Commit | After Pass 1 (critical), After Pass 2 |

## Key Method Signatures

```rust
// Entry point for relationship collection
fn add_relationships_by_name_with_range(
    from_id: Option<SymbolId>,
    from_name: &str,
    to_name: &str,
    file_id: FileId,
    kind: RelationKind,
    metadata: Option<RelationshipMetadata>,
    to_range: Option<crate::Range>,
) -> IndexResult<()>

// Main resolution orchestrator
fn resolve_cross_file_relationships(&mut self) -> IndexResult<()>

// Context builder
fn build_resolution_context(&self, file_id: FileId) -> IndexResult<Box<dyn ResolutionScope>>

// Get language behavior
fn get_behavior_for_file(&self, file_id: FileId) -> IndexResult<&dyn LanguageBehavior>

// Write relationship to Tantivy
fn add_relationship_internal(
    from: SymbolId,
    to: SymbolId,
    rel: Relationship,
) -> IndexResult<()>

// Resolution scope methods
trait ResolutionScope {
    fn resolve(&self, name: &str) -> Option<SymbolId>;
    fn resolve_relationship(
        &self,
        from_name: &str,
        to_name: &str,
        kind: RelationKind,
        from_file: FileId,
    ) -> Option<SymbolId>;
}

// Language behavior methods
trait LanguageBehavior {
    fn build_resolution_context(
        &self,
        file_id: FileId,
        document_index: &DocumentIndex,
    ) -> IndexResult<Box<dyn ResolutionScope>>;

    fn build_resolution_context_with_cache(
        &self,
        file_id: FileId,
        cache: &ConcurrentSymbolCache,
        document_index: &DocumentIndex,
    ) -> IndexResult<Box<dyn ResolutionScope>>;

    fn resolve_method_call(
        &self,
        method_call: &MethodCall,
        receiver_types: &HashMap<String, String>,
        context: &dyn ResolutionScope,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId>;
}
```

## Architecture Notes

1. **Two-pass is critical**: Instance method resolution needs to query Defines. If `Calculator.add()` is called, resolution must find the Defines relationship from `Calculator` to `add` to resolve the call.

2. **Context per file**: Each file gets its own ResolutionScope built from imports + file symbols + package symbols + global symbols. This respects visibility rules.

3. **Cache optimization**: The `symbol_lookup_cache` in resolution prevents millions of duplicate Tantivy queries for the same symbol name.

4. **Range-based disambiguation**: For overloaded methods (same name, different signatures), the `to_range` field enables precise matching.

## Conclusions

The SimpleIndexer resolution system is well-structured with clear separation:
- Parsing phase: Collect unresolved relationships (name-based)
- Resolution phase: Two-pass with context-aware resolution
- Write phase: Store resolved relationships to Tantivy

For a parallel pipeline, the key insight is that:
1. All symbols must be available before resolution (currently via Tantivy commit)
2. Resolution context needs access to imports, file symbols, and global symbols
3. Pass 1 (Defines) must complete before Pass 2 (Calls) for method resolution
4. Tantivy is heavily used for querying during resolution - a cache-only approach needs to replicate this data
