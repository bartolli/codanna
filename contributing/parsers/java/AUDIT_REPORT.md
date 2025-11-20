# Java Parser Symbol Extraction Coverage Report

*Generated: 2025-11-16 17:06:53 UTC*

## Summary
- Nodes in file: 92
- Nodes with symbol extraction: 87
- Symbol kinds extracted: 5

> **Note:** This focuses on nodes that produce indexable symbols used for IDE features.

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 233 | ✅ implemented |
| interface_declaration | 255 | ✅ implemented |
| enum_declaration | 229 | ✅ implemented |
| annotation_type_declaration | 251 | ⚠️ gap |
| method_declaration | 279 | ✅ implemented |
| constructor_declaration | 244 | ✅ implemented |
| field_declaration | 249 | ✅ implemented |
| package_declaration | 226 | ✅ implemented |
| import_declaration | 227 | ✅ implemented |
| modifiers | 234 | ✅ implemented |
| formal_parameters | 273 | ✅ implemented |
| type_parameters | 235 | ✅ implemented |
| annotation | - | ⭕ not found |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

### Implementation Gaps
- `annotation_type_declaration`: add handling in `java/parser.rs` if symbol extraction is required.

### Missing Samples
- `annotation`: include representative code in audit fixtures to track coverage.

