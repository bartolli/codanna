# C# Parser Coverage Report

*Generated: 2025-11-25 15:15:07 UTC*

## Summary
- Nodes in file: 142
- Nodes handled by parser: 142
- Symbol kinds extracted: 9

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 231 | ✅ implemented |
| interface_declaration | 236 | ✅ implemented |
| struct_declaration | 232 | ✅ implemented |
| record_declaration | 238 | ✅ implemented |
| enum_declaration | 233 | ✅ implemented |
| enum_member_declaration | 235 | ✅ implemented |
| delegate_declaration | 237 | ✅ implemented |
| namespace_declaration | 228 | ✅ implemented |
| file_scoped_namespace_declaration | - | ❌ not found |
| method_declaration | 255 | ✅ implemented |
| constructor_declaration | 253 | ✅ implemented |
| destructor_declaration | 254 | ✅ implemented |
| property_declaration | 262 | ✅ implemented |
| indexer_declaration | 260 | ✅ implemented |
| event_declaration | 256 | ✅ implemented |
| event_field_declaration | 257 | ✅ implemented |
| field_declaration | 252 | ✅ implemented |
| operator_declaration | 248 | ✅ implemented |
| conversion_operator_declaration | 249 | ✅ implemented |
| using_directive | 221 | ✅ implemented |
| extern_alias_directive | 220 | ✅ implemented |
| modifier | 241 | ✅ implemented |
| parameter | 265 | ✅ implemented |
| type_parameter | 243 | ✅ implemented |
| type_parameter_list | 242 | ✅ implemented |
| base_list | 244 | ✅ implemented |
| invocation_expression | 380 | ✅ implemented |
| object_creation_expression | 396 | ✅ implemented |
| member_access_expression | 394 | ✅ implemented |
| variable_declaration | 274 | ✅ implemented |
| variable_declarator | 276 | ✅ implemented |
| local_declaration_statement | 330 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `file_scoped_namespace_declaration`: Add example to comprehensive.cs or verify node name

