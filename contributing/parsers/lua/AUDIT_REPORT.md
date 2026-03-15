# Lua Parser Symbol Extraction Coverage Report

*Generated: 2026-03-15 18:41:01 UTC*

## Summary
- Key nodes: 21/21 (100%)
- Symbol kinds extracted: 6

> **Note:** Key nodes are symbol-producing constructs (functions, tables, imports).

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| chunk | 73 | ✅ implemented |
| function_declaration | 94 | ✅ implemented |
| function_definition | 114 | ✅ implemented |
| variable_declaration | 101 | ✅ implemented |
| assignment_statement | 78 | ✅ implemented |
| table_constructor | 127 | ✅ implemented |
| field | 130 | ✅ implemented |
| function_call | 123 | ✅ implemented |
| method_index_expression | 124 | ✅ implemented |
| dot_index_expression | 122 | ✅ implemented |
| bracket_index_expression | 121 | ✅ implemented |
| for_statement | 89 | ✅ implemented |
| for_generic_clause | 90 | ✅ implemented |
| for_numeric_clause | 91 | ✅ implemented |
| while_statement | 84 | ✅ implemented |
| repeat_statement | 85 | ✅ implemented |
| if_statement | 86 | ✅ implemented |
| do_statement | 83 | ✅ implemented |
| block | 74 | ✅ implemented |
| return_statement | 76 | ✅ implemented |
| comment | 133 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

✨ **Excellent coverage!** All key nodes are implemented.
