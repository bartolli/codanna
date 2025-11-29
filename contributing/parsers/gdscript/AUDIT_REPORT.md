# GDScript Parser Symbol Extraction Coverage Report

*Generated: 2025-11-29 00:46:16 UTC*

## Summary
- Nodes in file: 76
- Nodes with symbol extraction: 14
- Symbol kinds extracted: 7

> **Note:** This focuses on nodes that produce indexable symbols used for IDE features.

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_definition | 159 | ✅ implemented |
| class_name_statement | 152 | ✅ implemented |
| extends_statement | 153 | ✅ implemented |
| function_definition | 201 | ✅ implemented |
| constructor_definition | - | ⭕ not found |
| signal_statement | 151 | ✅ implemented |
| variable_statement | 143 | ✅ implemented |
| const_statement | 146 | ✅ implemented |
| enum_definition | 165 | ✅ implemented |
| match_statement | 169 | ✅ implemented |
| for_statement | 157 | ✅ implemented |
| while_statement | 158 | ✅ implemented |
| if_statement | 154 | ✅ implemented |
| tool_statement | - | ⭕ not found |
| export_variable_statement | - | ⭕ not found |
| annotation | 125 | ✅ implemented |
| annotations | 127 | ✅ implemented |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

### Missing Samples
- `constructor_definition`: include representative code in audit fixtures to track coverage.
- `tool_statement`: include representative code in audit fixtures to track coverage.
- `export_variable_statement`: include representative code in audit fixtures to track coverage.

