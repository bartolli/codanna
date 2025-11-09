# GDScript Parser Symbol Extraction Coverage Report

*Generated: 2025-11-09 01:47:11 UTC*

## Summary
- Nodes in file: 78
- Nodes with symbol extraction: 15
- Symbol kinds extracted: 7

> **Note:** This focuses on nodes that produce indexable symbols used for IDE features.

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_definition | 150 | ✅ implemented |
| class_name_statement | 143 | ✅ implemented |
| extends_statement | 144 | ✅ implemented |
| function_definition | 185 | ✅ implemented |
| constructor_definition | 187 | ✅ implemented |
| signal_statement | 142 | ✅ implemented |
| variable_statement | 134 | ✅ implemented |
| const_statement | 137 | ✅ implemented |
| enum_definition | 151 | ✅ implemented |
| match_statement | 155 | ✅ implemented |
| for_statement | 148 | ✅ implemented |
| while_statement | 149 | ✅ implemented |
| if_statement | 145 | ✅ implemented |
| tool_statement | - | ⭕ not found |
| export_variable_statement | - | ⭕ not found |
| annotation | 117 | ✅ implemented |
| annotations | 119 | ✅ implemented |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

### Missing Samples
- `tool_statement`: include representative code in audit fixtures to track coverage.
- `export_variable_statement`: include representative code in audit fixtures to track coverage.

