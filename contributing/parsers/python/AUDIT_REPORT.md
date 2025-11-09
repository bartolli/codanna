# Python Parser Symbol Extraction Coverage Report

*Generated: 2025-11-09 01:47:11 UTC*

## Summary
- Nodes in file: 121
- Nodes with symbol extraction: 120
- Symbol kinds extracted: 6

> **Note**: This report tracks nodes that produce indexed symbols for code intelligence.
> For complete grammar coverage, see GRAMMAR_ANALYSIS.md

## Coverage Table

*Showing key nodes relevant for symbol extraction. Status determined by dynamic tracking.*

| Node Type | ID | Status |
|-----------|-----|--------|
| class_definition | 154 | ✅ implemented |
| function_definition | 145 | ✅ implemented |
| decorated_definition | 158 | ✅ implemented |
| assignment | 198 | ✅ implemented |
| augmented_assignment | - | ❌ not found |
| annotated_assignment | - | ❌ not found |
| typed_parameter | 207 | ✅ implemented |
| typed_default_parameter | 182 | ✅ implemented |
| parameters | 146 | ✅ implemented |
| import_statement | 111 | ✅ implemented |
| import_from_statement | 115 | ✅ implemented |
| aliased_import | - | ❌ not found |
| lambda | 73 | ✅ implemented |
| list_comprehension | 220 | ✅ implemented |
| dictionary_comprehension | 221 | ✅ implemented |
| set_comprehension | 222 | ✅ implemented |
| generator_expression | 223 | ✅ implemented |
| async_function_definition | - | ❌ not found |
| async_for_statement | - | ❌ not found |
| async_with_statement | - | ❌ not found |
| decorator | 159 | ✅ implemented |
| type_alias_statement | - | ❌ not found |
| type | 208 | ✅ implemented |
| global_statement | - | ❌ not found |
| nonlocal_statement | - | ❌ not found |
| with_statement | - | ❌ not found |
| for_statement | 137 | ✅ implemented |
| while_statement | - | ❌ not found |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `augmented_assignment`: Add example to comprehensive.py or verify node name
- `annotated_assignment`: Add example to comprehensive.py or verify node name
- `aliased_import`: Add example to comprehensive.py or verify node name
- `async_function_definition`: Add example to comprehensive.py or verify node name
- `async_for_statement`: Add example to comprehensive.py or verify node name
- `async_with_statement`: Add example to comprehensive.py or verify node name
- `type_alias_statement`: Add example to comprehensive.py or verify node name
- `global_statement`: Add example to comprehensive.py or verify node name
- `nonlocal_statement`: Add example to comprehensive.py or verify node name
- `with_statement`: Add example to comprehensive.py or verify node name
- `while_statement`: Add example to comprehensive.py or verify node name

