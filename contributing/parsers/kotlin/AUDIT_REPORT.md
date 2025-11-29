# Kotlin Parser Symbol Extraction Coverage Report

*Generated: 2025-11-29 00:46:16 UTC*

## Summary
- Nodes in file: 150
- Nodes with symbol extraction: 142
- Symbol kinds extracted: 8

> **Note:** This focuses on nodes that produce indexable symbols used for IDE features.

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 162 | ✅ implemented |
| object_declaration | 192 | ✅ implemented |
| interface | 18 | ✅ implemented |
| function_declaration | 183 | ✅ implemented |
| property_declaration | 186 | ✅ implemented |
| secondary_constructor | 193 | ✅ implemented |
| primary_constructor | 163 | ✅ implemented |
| companion_object | 179 | ✅ implemented |
| enum_class_body | 195 | ✅ implemented |
| type_alias | 160 | ✅ implemented |
| package_header | 156 | ✅ implemented |
| import_header | 158 | ⚠️ gap |
| import_list | 157 | ✅ implemented |
| delegation_specifier | 169 | ✅ implemented |
| annotation | 304 | ✅ implemented |
| modifiers | 289 | ✅ implemented |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

### Implementation Gaps
- `import_header`: add handling in `kotlin/parser.rs` if symbol extraction is required.

