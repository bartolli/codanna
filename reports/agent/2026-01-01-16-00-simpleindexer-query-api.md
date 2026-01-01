# Research Report: SimpleIndexer Read-Only/Query API

**Date**: 2026-01-01 16:00
**Agent**: Research-Agent-v5
**Model**: Opus 4.5
**Source File**: `/Users/bartolli/Projects/codanna/src/indexing/simple.rs` (5888 lines)

## Summary

SimpleIndexer provides a comprehensive query API for symbol lookup, relationship traversal, and search operations. Most query methods delegate to DocumentIndex (Tantivy) with optional enrichment from symbol_cache (fast name lookups) and semantic_search (doc comment embeddings). Several methods can be delegated directly to DocumentIndex with minimal wrapper logic.

## Key Findings

### 1. Symbol Query Methods

#### find_symbol (Line 1642-1661)
```rust
pub fn find_symbol(&self, name: &str) -> Option<SymbolId>
```
- **Parameters**: `name: &str` - Symbol name to find
- **Returns**: `Option<SymbolId>` - First matching symbol ID
- **Storage**: Tries `symbol_cache` first (O(1) hash lookup), falls back to `document_index.find_symbols_by_name()`
- **Delegation**: Could delegate to DocumentIndex, but symbol_cache provides significant performance benefit

#### find_symbols_by_name (Line 1664-1679)
```rust
pub fn find_symbols_by_name(&self, name: &str, language_filter: Option<&str>) -> Vec<Symbol>
```
- **Parameters**: `name`, `language_filter`
- **Returns**: `Vec<Symbol>` - All matching symbols enriched with `language_id`
- **Storage**: `document_index.find_symbols_by_name()` + `file_languages` enrichment
- **Delegation**: Partial - DocumentIndex handles core query, but enrichment adds `language_id` from `file_languages` cache

#### get_symbol (Line 1681-1693)
```rust
pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol>
```
- **Parameters**: `id: SymbolId`
- **Returns**: `Option<Symbol>` - Enriched with `language_id`
- **Storage**: `document_index.find_symbol_by_id()` + `file_languages` enrichment
- **Delegation**: Partial - same enrichment pattern

#### get_all_symbols (Line 1943-1951)
```rust
pub fn get_all_symbols(&self) -> Vec<Symbol>
```
- **Returns**: `Vec<Symbol>` - All symbols (limit 10000)
- **Storage**: `document_index.get_all_symbols(10000)`
- **Delegation**: **Full** - Direct pass-through with fixed limit

#### get_symbols_by_file (Line 2071-2076)
```rust
pub fn get_symbols_by_file(&self, file_id: FileId) -> Vec<Symbol>
```
- **Parameters**: `file_id: FileId`
- **Returns**: `Vec<Symbol>`
- **Storage**: `document_index.find_symbols_by_file()`
- **Delegation**: **Full** - Direct pass-through

---

### 2. Relationship Query Methods

#### get_called_functions (Line 1700-1710)
```rust
pub fn get_called_functions(&self, symbol_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_from(symbol_id, RelationKind::Calls)` + `get_symbol()` for each target
- **Delegation**: **Full** - Could delegate if DocumentIndex provided joined results

#### get_called_functions_with_metadata (Line 1714-1728)
```rust
pub fn get_called_functions_with_metadata(&self, symbol_id: SymbolId) -> Vec<(Symbol, Option<RelationshipMetadata>)>
```
- **Storage**: Same as above but preserves `RelationshipMetadata` (call site line/column, receiver info)
- **Delegation**: **Full** - Same pattern

#### get_calling_functions (Line 1729-1739)
```rust
pub fn get_calling_functions(&self, symbol_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_to(symbol_id, RelationKind::Calls)`
- **Delegation**: **Full** - Direct pattern

#### get_calling_functions_with_metadata (Line 1743-1757)
```rust
pub fn get_calling_functions_with_metadata(&self, symbol_id: SymbolId) -> Vec<(Symbol, Option<RelationshipMetadata>)>
```
- **Storage**: Same as above with metadata
- **Delegation**: **Full**

#### get_symbol_context (Line 1762-1856)
```rust
pub fn get_symbol_context(
    &self,
    symbol_id: SymbolId,
    include: ContextIncludes,
) -> Option<SymbolContext>
```
- **Parameters**: `symbol_id`, `include` (bitflags for which relationships to load)
- **Returns**: `SymbolContext` - Aggregated symbol with file_path and relationships
- **Storage**: Composes multiple queries based on `include` flags (IMPLEMENTATIONS, DEFINITIONS, CALLS, CALLERS, EXTENDS, USES)
- **Delegation**: Complex orchestration - not suitable for direct delegation

#### get_implementations (Line 1859-1870)
```rust
pub fn get_implementations(&self, trait_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_to(trait_id, RelationKind::Implements)`
- **Delegation**: **Full**

#### get_implemented_traits (Line 1873-1885)
```rust
pub fn get_implemented_traits(&self, type_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_from(type_id, RelationKind::Implements)`
- **Delegation**: **Full**

#### get_extends (Line 1889-1900)
```rust
pub fn get_extends(&self, class_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_from(class_id, RelationKind::Extends)`
- **Delegation**: **Full**

#### get_extended_by (Line 1903-1914)
```rust
pub fn get_extended_by(&self, base_class_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_to(base_class_id, RelationKind::Extends)`
- **Delegation**: **Full**

#### get_uses (Line 1918-1929)
```rust
pub fn get_uses(&self, symbol_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_from(symbol_id, RelationKind::Uses)`
- **Delegation**: **Full**

#### get_used_by (Line 1932-1943)
```rust
pub fn get_used_by(&self, type_id: SymbolId) -> Vec<Symbol>
```
- **Storage**: `document_index.get_relationships_to(type_id, RelationKind::Uses)`
- **Delegation**: **Full**

---

### 3. Dependency Query Methods

#### get_dependencies (Line 1953-1982)
```rust
pub fn get_dependencies(&self, symbol_id: SymbolId) -> HashMap<RelationKind, Vec<Symbol>>
```
- **Returns**: Map of relationship kinds to dependent symbols
- **Storage**: Iterates over `[Calls, Uses, Implements, Defines]` using `document_index.get_relationships_from()`
- **Delegation**: Partial - aggregation logic lives in SimpleIndexer

#### get_dependents (Line 1985-2014)
```rust
pub fn get_dependents(&self, symbol_id: SymbolId) -> HashMap<RelationKind, Vec<Symbol>>
```
- **Returns**: Map of relationship kinds to depending symbols
- **Storage**: Iterates over `[Calls, Uses, Implements]` using `document_index.get_relationships_to()`
- **Delegation**: Partial

#### get_impact_radius (Line 2017-2063)
```rust
pub fn get_impact_radius(&self, symbol_id: SymbolId, max_depth: Option<usize>) -> Vec<SymbolId>
```
- **Parameters**: `symbol_id`, `max_depth` (default 2)
- **Returns**: All symbols affected by changes (BFS traversal)
- **Storage**: Uses `document_index.get_relationships_to()` for `[Calls, Uses, Implements, Extends]`
- **Delegation**: Complex algorithm - not suitable for delegation

---

### 4. Full-Text Search (Line 2402-2414)

```rust
pub fn search(
    &self,
    query: &str,
    limit: usize,
    kind_filter: Option<SymbolKind>,
    module_filter: Option<&str>,
    language_filter: Option<&str>,
) -> IndexResult<Vec<SearchResult>>
```
- **Storage**: `document_index.search()` with filters
- **Delegation**: **Full** - Direct pass-through with error mapping

---

### 5. Semantic Search Methods

#### semantic_search_docs (Line 2289-2297)
```rust
pub fn semantic_search_docs(&self, query: &str, limit: usize) -> IndexResult<Vec<(Symbol, f32)>>
```
- **Storage**: Delegates to `semantic_search_docs_with_language(query, limit, None)`
- **Delegation**: Wrapper only

#### semantic_search_docs_with_language (Line 2300-2328)
```rust
pub fn semantic_search_docs_with_language(
    &self,
    query: &str,
    limit: usize,
    language_filter: Option<&str>,
) -> IndexResult<Vec<(Symbol, f32)>>
```
- **Storage**: `semantic_search.search_with_language()` + `get_symbol()` for each result
- **Delegation**: Partial - requires both semantic_search and document_index

#### semantic_search_docs_with_threshold (Line 2330-2340)
```rust
pub fn semantic_search_docs_with_threshold(
    &self,
    query: &str,
    limit: usize,
    threshold: f32,
) -> IndexResult<Vec<(Symbol, f32)>>
```
- **Storage**: Same as above with score filtering
- **Delegation**: Wrapper

#### semantic_search_docs_with_threshold_and_language (Line 2342-2381)
```rust
pub fn semantic_search_docs_with_threshold_and_language(
    &self,
    query: &str,
    limit: usize,
    threshold: f32,
    language_filter: Option<&str>,
) -> IndexResult<Vec<(Symbol, f32)>>
```
- **Storage**: `semantic_search.search_with_language()` with threshold filter
- **Delegation**: Partial

---

### 6. Statistics Methods

#### symbol_count (Line 2066-2068)
```rust
pub fn symbol_count(&self) -> usize
```
- **Storage**: `document_index.count_symbols()`
- **Delegation**: **Full**

#### file_count (Line 2077-2079)
```rust
pub fn file_count(&self) -> u32
```
- **Storage**: `document_index.count_files() as u32`
- **Delegation**: **Full**

#### relationship_count (Line 2081-2083)
```rust
pub fn relationship_count(&self) -> usize
```
- **Storage**: `document_index.count_relationships()`
- **Delegation**: **Full**

#### document_count (Line 2416-2419)
```rust
pub fn document_count(&self) -> IndexResult<u64>
```
- **Storage**: `document_index.document_count()` (total Tantivy docs)
- **Delegation**: **Full**

---

## Storage Architecture

### Components
1. **DocumentIndex** (`document_index: DocumentIndex`) - Primary Tantivy-based storage for symbols, relationships, files
2. **symbol_cache** (`Option<Arc<ConcurrentSymbolCache>>`) - O(1) hash-based name lookup, memory-mapped file
3. **semantic_search** (`Option<Arc<Mutex<SimpleSemanticSearch>>>`) - Doc comment embeddings for natural language search
4. **file_languages** (`HashMap<FileId, LanguageId>`) - Runtime cache for language enrichment

### Data Flow
```
Query Request
    |
    v
[symbol_cache] --(hit)--> Return fast
    |
    (miss)
    v
[DocumentIndex/Tantivy] --> Symbol data
    |
    v
[file_languages] --> Enrich with language_id
    |
    v
Return enriched Symbol
```

---

## Delegation Summary

| Method | Can Delegate to DocumentIndex? |
|--------|-------------------------------|
| `find_symbol` | Partial (cache benefit) |
| `find_symbols_by_name` | Partial (enrichment) |
| `get_symbol` | Partial (enrichment) |
| `get_all_symbols` | **Full** |
| `get_symbols_by_file` | **Full** |
| `get_called_functions` | **Full** |
| `get_called_functions_with_metadata` | **Full** |
| `get_calling_functions` | **Full** |
| `get_calling_functions_with_metadata` | **Full** |
| `get_symbol_context` | No (orchestration) |
| `get_implementations` | **Full** |
| `get_implemented_traits` | **Full** |
| `get_extends` | **Full** |
| `get_extended_by` | **Full** |
| `get_uses` | **Full** |
| `get_used_by` | **Full** |
| `get_dependencies` | Partial (aggregation) |
| `get_dependents` | Partial (aggregation) |
| `get_impact_radius` | No (BFS algorithm) |
| `search` | **Full** |
| `semantic_search_docs*` | Partial (needs semantic_search) |
| `symbol_count` | **Full** |
| `file_count` | **Full** |
| `relationship_count` | **Full** |
| `document_count` | **Full** |

## Conclusions

1. **Most relationship query methods** (`get_called_functions`, `get_implementations`, etc.) are thin wrappers around DocumentIndex with a `get_symbol()` join pattern. These could be optimized by having DocumentIndex return joined Symbol data directly.

2. **Statistics methods** are pure pass-throughs to DocumentIndex - no additional logic.

3. **Symbol lookup methods** benefit from the `symbol_cache` fast path but require `language_id` enrichment from `file_languages`.

4. **Semantic search** requires the separate `SimpleSemanticSearch` component and cannot be fully delegated to DocumentIndex.

5. **Complex orchestration methods** (`get_symbol_context`, `get_impact_radius`) contain significant business logic and should remain in SimpleIndexer.
