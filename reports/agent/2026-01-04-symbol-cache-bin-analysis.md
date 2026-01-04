# Research Report: symbol_cache.bin Usage Analysis

**Date**: 2026-01-04
**Agent**: Research-Agent-v5
**Model**: Opus 4.5

## Summary

The `symbol_cache.bin` file is created during indexing but is effectively unused at query time. All MCP handlers and CLI commands use `find_symbols_by_name()` which queries Tantivy directly, bypassing the cache. The singular `find_symbol()` method (which uses the cache) has only one active caller and that caller is itself unused.

## Key Findings

### 1. Creation Points

The cache is built in 4 places, all after indexing operations:

| Location | Method | Trigger |
|----------|--------|---------|
| `src/indexing/facade.rs:986` | `index_directory()` | After indexing a directory |
| `src/indexing/facade.rs:1058` | `index_directory_with_options()` | After advanced indexing |
| `src/indexing/facade.rs:1117` | `sync_with_config()` | After each directory sync |
| `src/cli/commands/index.rs:258` | `save_index()` | Before saving index |

**Creation Flow:**
```
build_symbol_cache() [facade.rs:856]
  -> SymbolHashCache::build_from_symbols() [symbol_cache.rs]
     -> Writes to {index_base}/symbol_cache.bin
  -> SymbolHashCache::open()
  -> ConcurrentSymbolCache::new()
     -> Stored in facade.symbol_cache
```

**Evidence**: `/Users/bartolli/Projects/codanna/src/indexing/facade.rs:855-880`

### 2. Loading Point

The cache is loaded once on startup:

| Location | Method |
|----------|--------|
| `src/storage/persistence.rs:108` | `load_facade()` |

This calls `facade.load_symbol_cache()` which opens the `.bin` file and creates a `ConcurrentSymbolCache` wrapper.

**Evidence**: `/Users/bartolli/Projects/codanna/src/storage/persistence.rs:100-112`

### 3. Usage Points (Minimal)

**Active Usage:**
| Location | Method | Uses Cache? | Notes |
|----------|--------|-------------|-------|
| `src/indexing/facade.rs:253` | `find_symbol()` | Yes | Returns single `SymbolId` |

**The `find_symbol()` method has only ONE caller:**
- `src/cli/commands/mcp.rs:38` function `run()` - but grep shows it calls `server.find_symbol()` (MCP method), NOT `facade.find_symbol()`.

**Dead Code - `build_resolution_context_with_cache()`:**
- Defined in `src/parsing/language_behavior.rs:674-970`
- Takes `ConcurrentSymbolCache` parameter
- **Has zero callers** - never invoked anywhere

### 4. Query Paths (All Bypass Cache)

Every query operation uses `find_symbols_by_name()` which goes directly to Tantivy:

| Component | Method Used | Cache Used? |
|-----------|-------------|-------------|
| MCP `find_symbol` handler | `find_symbols_by_name()` | No |
| MCP `get_calls` handler | `find_symbols_by_name()` | No |
| MCP `find_callers` handler | `find_symbols_by_name()` | No |
| CLI retrieve commands | `find_symbols_by_name()` | No |
| `src/retrieve.rs` | `find_symbols_by_name()` | No |
| Language behaviors | `find_symbols_by_name()` | No |

**Evidence**: grep found 45+ calls to `find_symbols_by_name()` across the codebase, and 0 calls to `facade.find_symbol()`.

## Architecture Analysis

```
Query Flow (ACTUAL):
  MCP/CLI -> find_symbols_by_name() -> Tantivy -> Vec<Symbol>
                                         ^
                                         |
                                    NO CACHE USED

Query Flow (DESIGNED but unused):
  ??? -> find_symbol() -> symbol_cache (if present) -> SymbolId
                            |
                            v (fallback)
                         Tantivy
```

## Conclusions

### Dead Code Identified

1. **`facade.find_symbol()`** - Has cache-first logic but no external callers
2. **`build_resolution_context_with_cache()`** - 300-line method with zero callers
3. **`ConcurrentSymbolCache` wrapper** - Created on load, never read
4. **JavaScript/TypeScript behavior overrides** - Override `build_resolution_context_with_cache()` but parent method is never called

### Disk/Memory Overhead

- `symbol_cache.bin` is written after every indexing operation
- File is memory-mapped on startup via `load_symbol_cache()`
- Neither is ever read for queries

### Recommendation: Remove

**Remove the following:**

1. **`symbol_cache.bin` creation** - Remove calls to `build_symbol_cache()` in:
   - `facade.rs:986`, `1058`, `1117`
   - `cli/commands/index.rs:258`

2. **`symbol_cache.bin` loading** - Remove call in:
   - `persistence.rs:108`

3. **Dead methods:**
   - `IndexFacade::build_symbol_cache()`
   - `IndexFacade::load_symbol_cache()`
   - `IndexFacade::clear_symbol_cache()`
   - `IndexFacade::find_symbol()` (singular)
   - `LanguageBehavior::build_resolution_context_with_cache()`
   - JavaScript/TypeScript overrides of above

4. **Structs/types:**
   - `ConcurrentSymbolCache`
   - Potentially `SymbolHashCache` if not used elsewhere

**Impact:**
- Faster indexing (no cache file write)
- Simpler codebase (remove ~400 lines)
- No query performance regression (already using Tantivy)
- Reduced memory footprint (no mmap of unused file)

**Migration path:**
- The new parallel pipeline uses `PipelineSymbolCache` (in-memory DashMap) during indexing
- Query operations already use Tantivy
- No feature gap
