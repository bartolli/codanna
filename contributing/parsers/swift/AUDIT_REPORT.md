# Swift Parser Symbol Extraction Coverage Report

*Generated: 2025-12-01 00:05:32 UTC*

## Summary
- Nodes in file: 201
- Nodes with symbol extraction: 201
- Symbol kinds extracted: 11

> **Note:** This focuses on nodes that produce indexable symbols used for IDE features.

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 398 | ✅ implemented |
| protocol_declaration | 438 | ✅ implemented |
| function_declaration | 388 | ✅ implemented |
| init_declaration | 442 | ✅ implemented |
| infix_expression | 276 | ✅ implemented |
| deinit_declaration | 443 | ✅ implemented |
| property_declaration | 378 | ✅ implemented |
| enum_entry | 435 | ✅ implemented |
| typealias_declaration | 386 | ✅ implemented |
| subscript_declaration | 444 | ✅ implemented |
| import_declaration | 374 | ✅ implemented |
| visibility_modifier | 483 | ✅ implemented |
| modifiers | 475 | ✅ implemented |
| inheritance_specifier | 402 | ✅ implemented |
| type_constraint | 408 | ✅ implemented |
| associatedtype_declaration | 458 | ✅ implemented |
| where_keyword | 199 | ✅ implemented |
| switch_statement | 324 | ✅ implemented |
| switch_entry | 325 | ✅ implemented |
| willset_didset_block | 383 | ✅ implemented |
| as_expression | 270 | ✅ implemented |
| dictionary_type | 248 | ✅ implemented |
| boolean_literal | 220 | ✅ implemented |
| ternary_expression | 297 | ✅ implemented |
| while_statement | 357 | ✅ implemented |
| opaque_type | 253 | ✅ implemented |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

All tracked nodes are currently implemented
