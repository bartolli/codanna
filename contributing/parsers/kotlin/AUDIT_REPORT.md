# Kotlin Parser Symbol Extraction Coverage Report

*Generated: 2025-12-01 15:24:41 UTC*

## Summary
- Key nodes: 17/17 (100%)
- Symbol kinds extracted: 8

> **Note:** Key nodes are symbol-producing constructs (classes, functions, imports).

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
| import_header | 158 | ✅ implemented |
| import_list | 157 | ✅ implemented |
| delegation_specifier | 169 | ✅ implemented |
| annotation | 304 | ✅ implemented |
| modifiers | 289 | ✅ implemented |
| infix_expression | 234 | ✅ implemented |

## Legend

- ✅ **implemented**: node type is handled by the parser
- ⚠️ **gap**: node exists in grammar but parser does not currently extract it
- ⭕ **not found**: node isn't present in the audited sample; add fixtures to verify

## Recommended Actions

All tracked nodes are currently implemented ✅
