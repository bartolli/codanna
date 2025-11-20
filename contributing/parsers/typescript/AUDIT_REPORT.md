# TypeScript Parser Coverage Report

*Generated: 2025-11-20 14:36:43 UTC*

## Summary
- Nodes in file: 203
- Nodes handled by parser: 189
- Symbol kinds extracted: 9

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| class_declaration | 235 | ✅ implemented |
| interface_declaration | 301 | ✅ implemented |
| enum_declaration | 303 | ✅ implemented |
| type_alias_declaration | 306 | ✅ implemented |
| function_declaration | 238 | ✅ implemented |
| method_definition | 275 | ✅ implemented |
| public_field_definition | 280 | ✅ implemented |
| accessibility_modifier | 307 | ✅ implemented |
| variable_declaration | 189 | ✅ implemented |
| lexical_declaration | 190 | ✅ implemented |
| arrow_function | 241 | ✅ implemented |
| function_expression | 237 | ⚠️ gap |
| generator_function_declaration | 240 | ✅ implemented |
| import_statement | 180 | ✅ implemented |
| export_statement | 173 | ✅ implemented |
| namespace_import | 183 | ✅ implemented |
| named_imports | 184 | ✅ implemented |
| required_parameter | 309 | ✅ implemented |
| optional_parameter | 310 | ✅ implemented |
| rest_pattern | 274 | ✅ implemented |
| type_parameter | 354 | ✅ implemented |
| type_annotation | 315 | ✅ implemented |
| predefined_type | 348 | ✅ implemented |
| internal_module | 297 | ✅ implemented |
| module_declaration | - | ❌ not found |
| jsx_element | 225 | ✅ implemented |
| jsx_self_closing_element | 231 | ⚠️ gap |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 1: Implementation Gaps
These nodes exist in your code but aren't being captured:

- `function_expression`: Add parsing logic in parser.rs
- `jsx_self_closing_element`: Add parsing logic in parser.rs

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `module_declaration`: Add example to comprehensive.ts or verify node name

