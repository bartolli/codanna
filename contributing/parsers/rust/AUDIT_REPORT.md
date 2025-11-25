# Rust Parser Coverage Report

*Generated: 2025-11-25 15:15:07 UTC*

## Summary
- Nodes in file: 143
- Nodes handled by parser: 13
- Symbol kinds extracted: 10

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| function_item | 188 | ✅ implemented |
| impl_item | 193 | ✅ implemented |
| trait_item | 194 | ✅ implemented |
| struct_item | 176 | ✅ implemented |
| enum_item | 178 | ✅ implemented |
| mod_item | 173 | ✅ implemented |
| const_item | 185 | ✅ implemented |
| static_item | 186 | ✅ implemented |
| type_alias | - | ❌ not found |
| macro_definition | 161 | ✅ implemented |
| macro_rules | - | ❌ not found |
| field_declaration | 182 | ✅ implemented |
| enum_variant | 180 | ✅ implemented |
| function_signature_item | 189 | ✅ implemented |
| associated_type | 195 | ⚠️ gap |
| use_declaration | 204 | ⚠️ gap |
| use_as_clause | - | ❌ not found |
| use_wildcard | 209 | ⚠️ gap |
| parameter | 213 | ⚠️ gap |
| type_parameter | 201 | ⚠️ gap |
| lifetime | 219 | ⚠️ gap |
| closure_expression | 281 | ⚠️ gap |
| async_block | - | ❌ not found |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 1: Implementation Gaps
These nodes exist in your code but aren't being captured:

- `associated_type`: Add parsing logic in parser.rs
- `use_declaration`: Add parsing logic in parser.rs
- `use_wildcard`: Add parsing logic in parser.rs
- `parameter`: Add parsing logic in parser.rs
- `type_parameter`: Add parsing logic in parser.rs
- `lifetime`: Add parsing logic in parser.rs
- `closure_expression`: Add parsing logic in parser.rs

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `type_alias`: Add example to comprehensive.rs or verify node name
- `macro_rules`: Add example to comprehensive.rs or verify node name
- `use_as_clause`: Add example to comprehensive.rs or verify node name
- `async_block`: Add example to comprehensive.rs or verify node name

