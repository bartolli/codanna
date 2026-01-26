# Lua Parser Symbol Extraction Coverage Report

*Generated: 2026-01-25 22:44:47 UTC*

## Summary
- Key nodes: 21/21 (100%)
- Symbol kinds extracted: 6

> **Note:** Key nodes are symbol-producing constructs (functions, tables, imports).

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| chunk | 72 | ✅ implemented |
| function_declaration | 92 | ✅ implemented |
| function_definition | 110 | ✅ implemented |
| variable_declaration | 98 | ✅ implemented |
| assignment_statement | 77 | ✅ implemented |
| table_constructor | 122 | ✅ implemented |
| field | 125 | ✅ implemented |
| function_call | 118 | ✅ implemented |
| method_index_expression | 119 | ✅ implemented |
| dot_index_expression | 117 | ✅ implemented |
| bracket_index_expression | 116 | ✅ implemented |
| for_statement | 88 | ✅ implemented |
| for_generic_clause | 89 | ✅ implemented |
| for_numeric_clause | 90 | ✅ implemented |
| while_statement | 83 | ✅ implemented |
| repeat_statement | 84 | ✅ implemented |
| if_statement | 85 | ✅ implemented |
| do_statement | 82 | ✅ implemented |
| block | 73 | ✅ implemented |
| return_statement | 75 | ✅ implemented |
| comment | 128 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

✨ **Excellent coverage!** All key nodes are implemented.
