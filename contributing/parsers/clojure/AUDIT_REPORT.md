# Clojure Parser Symbol Extraction Coverage Report

*Generated: 2026-03-15 18:41:01 UTC*

## Summary
- Key nodes: 15/15 (100%)
- Symbol kinds extracted: 7

> **Note:** Key nodes are symbol-producing constructs (lists containing def forms).

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| list_lit | 45 | ✅ implemented |
| sym_lit | 41 | ✅ implemented |
| vec_lit | 49 | ✅ implemented |
| map_lit | 47 | ✅ implemented |
| kwd_lit | 39 | ✅ implemented |
| str_lit | 40 | ✅ implemented |
| comment | 2 | ✅ implemented |
| meta_lit | 43 | ✅ implemented |
| num_lit | 4 | ✅ implemented |
| nil_lit | 12 | ✅ implemented |
| bool_lit | 13 | ✅ implemented |
| set_lit | 51 | ✅ implemented |
| anon_fn_lit | 53 | ✅ implemented |
| regex_lit | 54 | ✅ implemented |
| read_cond_lit | 55 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

✨ **Excellent coverage!** All key nodes are implemented.
