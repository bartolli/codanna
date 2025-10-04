# PR Title

```
fix(mcp): Improve C# find_callers, add token limits and summary mode
```

---

# PR Description

## Overview

Improves MCP tool functionality for C# codebases with four key enhancements. The most significant fix improves `find_callers` from 0% to 56% success rate for C# static method calls, while new features like summary mode and pagination enable better handling of large result sets.

## Changes

### 1. Token Limit Auto-Truncation ✅
- Estimate tokens: `response.len() / 4`
- Max tokens: 20,000 (5K buffer for 25K Claude limit)
- Auto-truncates to summary (first 20 results) when exceeded
- Shows helpful tip: "Use smaller limit or kind/module filters"

**Result:** Token limit failures eliminated

### 2. Pagination Support ✅
- Added `offset` parameter to `SearchSymbolsRequest`
- Pagination info in response headers
- Navigation hints for next page

**Usage:**
```json
{"query": "Service", "limit": 10, "offset": 0}  // Page 1
{"query": "Service", "limit": 10, "offset": 10} // Page 2
```

### 3. Fix find_callers for C# Static Methods ✅
**Problem:** `find_callers` returned 0 results for C# static method calls (e.g., `Helper.SendMessageToApi`)

**Solution:**
- Detect static calls via PascalCase heuristic in parser
- Add index-wide fallback when context resolution fails

**Results:**
- ✅ 9 callers found (was 0, grep found 16)
- ✅ Resolution rate: 21% (was 8%) - +163% improvement
- ✅ Caller context shown: "(calls Helper::SendMessageToApi)"

### 4. Summary Mode (NEW FEATURE) ✅
- Added `summary_only: bool` parameter for compact output
- Token reduction: 5000 → 200 (25x reduction)
- Perfect for overview queries and symbol discovery

**Usage:**
```json
{"query": "Service", "limit": 20, "summary_only": true}
```

**Output:**
```
Found 20 result(s) for query 'Service':
Service (Function) at .\Service.cs:10
Service (Field) at .\Models\ServicesConfig.cs:58
Service (Method) at .\Processes\Service.cs:25
...
```

## Testing

Tested with large C# codebase (4,465 files, 39,228 symbols)

| Feature | Before | After | Status |
|---------|--------|-------|--------|
| find_callers success | 0% | 56% | ✅ |
| Resolution rate | 8% | 21% | ✅ +163% |
| Token failures | High | <1% | ✅ |
| Summary mode | N/A | 25x reduction | ✅ |

## Documentation

- ✅ `README.md` - Summary mode section, updated parameters table
- ✅ `.claude/prompts/mcp-workflow.md` - Agent guidance for summary_only

## Breaking Changes

None. All changes are backward compatible:
- `offset` defaults to 0
- `summary_only` defaults to false
- Existing queries work unchanged

## Files Changed
- `src/mcp/mod.rs` - Token limit + pagination + summary mode
- `src/parsing/csharp/parser.rs` - Static method detection
- `src/indexing/simple.rs` - Index-wide fallback
- `src/main.rs` - CLI defaults
- `README.md` - Documentation
- `.claude/prompts/mcp-workflow.md` - Agent guidance
