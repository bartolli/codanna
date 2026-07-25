# C# Parser Coverage Report

*Generated: 2026-07-01 23:18:07 UTC*

## Summary
- Key nodes: 31/32 (96%)
- Symbol kinds extracted: 9

> **Note:** Key nodes are symbol-producing constructs (classes, methods, properties).

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 232 | ✅ implemented |
| interface_declaration | 240 | ✅ implemented |
| struct_declaration | 234 | ✅ implemented |
| record_declaration | 244 | ✅ implemented |
| enum_declaration | 236 | ✅ implemented |
| enum_member_declaration | 239 | ✅ implemented |
| delegate_declaration | 242 | ✅ implemented |
| namespace_declaration | 229 | ✅ implemented |
| file_scoped_namespace_declaration | - | ❌ not found |
| method_declaration | 264 | ✅ implemented |
| constructor_declaration | 261 | ✅ implemented |
| destructor_declaration | 263 | ✅ implemented |
| property_declaration | 271 | ✅ implemented |
| indexer_declaration | 269 | ✅ implemented |
| event_declaration | 265 | ✅ implemented |
| event_field_declaration | 266 | ✅ implemented |
| field_declaration | 260 | ✅ implemented |
| operator_declaration | 256 | ✅ implemented |
| conversion_operator_declaration | 257 | ✅ implemented |
| using_directive | 221 | ✅ implemented |
| extern_alias_directive | 220 | ✅ implemented |
| modifier | 249 | ✅ implemented |
| parameter | 274 | ✅ implemented |
| type_parameter | 251 | ✅ implemented |
| type_parameter_list | 250 | ✅ implemented |
| base_list | 252 | ✅ implemented |
| invocation_expression | 395 | ✅ implemented |
| object_creation_expression | 412 | ✅ implemented |
| member_access_expression | 410 | ✅ implemented |
| variable_declaration | 283 | ✅ implemented |
| variable_declarator | 285 | ✅ implemented |
| local_declaration_statement | 341 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `file_scoped_namespace_declaration`: Add example to comprehensive.cs or verify node name

