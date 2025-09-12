# C++ Parser Coverage Report

*Generated: 2025-09-12 02:35:25 UTC*

## Summary
- Nodes in file: 131
- Nodes handled by parser: 131
- Symbol kinds extracted: 2

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| translation_unit | 219 | ✅ implemented |
| function_definition | 254 | ✅ implemented |
| class_specifier | 379 | ✅ implemented |
| struct_specifier | - | ❌ not found |
| union_specifier | - | ❌ not found |
| enum_specifier | - | ❌ not found |
| namespace_definition | 430 | ✅ implemented |
| template_declaration | 386 | ✅ implemented |
| template_instantiation | - | ❌ not found |
| function_declarator | 286 | ✅ implemented |
| init_declarator | 294 | ✅ implemented |
| parameter_declaration | 311 | ✅ implemented |
| field_declaration | 307 | ✅ implemented |
| access_specifier | 411 | ✅ implemented |
| base_class_clause | 383 | ✅ implemented |
| constructor_definition | - | ❌ not found |
| destructor_definition | - | ❌ not found |
| operator_overload | - | ❌ not found |
| lambda_expression | 464 | ✅ implemented |
| using_declaration | - | ❌ not found |
| typedef_declaration | - | ❌ not found |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `struct_specifier`: Add example to comprehensive.cpp or verify node name
- `union_specifier`: Add example to comprehensive.cpp or verify node name
- `enum_specifier`: Add example to comprehensive.cpp or verify node name
- `template_instantiation`: Add example to comprehensive.cpp or verify node name
- `constructor_definition`: Add example to comprehensive.cpp or verify node name
- `destructor_definition`: Add example to comprehensive.cpp or verify node name
- `operator_overload`: Add example to comprehensive.cpp or verify node name
- `using_declaration`: Add example to comprehensive.cpp or verify node name
- `typedef_declaration`: Add example to comprehensive.cpp or verify node name

