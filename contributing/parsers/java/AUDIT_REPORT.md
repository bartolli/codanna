# Java Parser Symbol Extraction Coverage Report

*Generated: 2026-01-12 21:49:41 UTC*

## Summary
- Key nodes: 13/13 (100%)
- Symbol kinds extracted: 5

> **Note:** Key nodes are symbol-producing constructs (classes, functions, imports).

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 233 | ✅ implemented |
| interface_declaration | 255 | ✅ implemented |
| enum_declaration | 229 | ✅ implemented |
| annotation_type_declaration | 251 | ✅ implemented |
| method_declaration | 279 | ✅ implemented |
| constructor_declaration | 244 | ✅ implemented |
| field_declaration | 249 | ✅ implemented |
| package_declaration | 226 | ✅ implemented |
| import_declaration | 227 | ✅ implemented |
| modifiers | 234 | ✅ implemented |
| formal_parameters | 273 | ✅ implemented |
| type_parameters | 235 | ✅ implemented |
| marker_annotation | 210 | ✅ implemented |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

All tracked nodes are currently implemented ✅
