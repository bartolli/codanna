# C Parser Coverage Report

*Generated: 2025-09-12 02:35:25 UTC*

## Summary
- Nodes in file: 145
- Nodes handled by parser: 29
- Symbol kinds extracted: 6

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| translation_unit | 161 | ✅ implemented |
| function_definition | 196 | ✅ implemented |
| declaration | 198 | ✅ implemented |
| struct_specifier | 249 | ✅ implemented |
| union_specifier | 250 | ✅ implemented |
| enum_specifier | 247 | ✅ implemented |
| typedef_declaration | - | ❌ not found |
| init_declarator | 240 | ✅ implemented |
| parameter_declaration | 260 | ✅ implemented |
| field_declaration | 253 | ✅ implemented |
| enumerator | 256 | ✅ implemented |
| macro_definition | - | ❌ not found |
| preproc_include | 164 | ✅ implemented |
| compound_statement | 241 | ✅ implemented |
| if_statement | 267 | ✅ implemented |
| while_statement | 271 | ✅ implemented |
| for_statement | 273 | ✅ implemented |
| do_statement | 272 | ✅ implemented |
| switch_statement | 269 | ✅ implemented |
| case_statement | 270 | ✅ implemented |
| expression_statement | 266 | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `typedef_declaration`: Add example to comprehensive.c or verify node name
- `macro_definition`: Add example to comprehensive.c or verify node name

