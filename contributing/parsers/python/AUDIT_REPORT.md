# Python Parser Symbol Extraction Coverage Report

*Generated: 2026-01-12 21:49:41 UTC*

## Summary
- Key nodes: 23/23 (100%)
- Symbol kinds extracted: 6

> **Note:** Key nodes are symbol-producing constructs (classes, functions, imports).

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_definition | 154 | ✅ implemented |
| function_definition | 145 | ✅ implemented |
| decorated_definition | 158 | ✅ implemented |
| assignment | 198 | ✅ implemented |
| augmented_assignment | 199 | ✅ implemented |
| typed_parameter | 207 | ✅ implemented |
| typed_default_parameter | 182 | ✅ implemented |
| parameters | 146 | ✅ implemented |
| import_statement | 111 | ✅ implemented |
| import_from_statement | 115 | ✅ implemented |
| aliased_import | 117 | ✅ implemented |
| lambda | 73 | ✅ implemented |
| list_comprehension | 220 | ✅ implemented |
| dictionary_comprehension | 221 | ✅ implemented |
| set_comprehension | 222 | ✅ implemented |
| generator_expression | 223 | ✅ implemented |
| decorator | 159 | ✅ implemented |
| type | 208 | ✅ implemented |
| global_statement | 150 | ✅ implemented |
| nonlocal_statement | 151 | ✅ implemented |
| with_statement | 142 | ✅ implemented |
| for_statement | 137 | ✅ implemented |
| while_statement | 138 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

✨ **Excellent coverage!** All key nodes are implemented.
