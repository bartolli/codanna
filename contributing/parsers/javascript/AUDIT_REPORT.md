# JavaScript Parser Coverage Report

*Generated: 2025-11-24 03:07:00 UTC*

## Summary
- Nodes in grammar: 180+
- Nodes handled by parser: 150+
- Symbol kinds extracted: 5

## Coverage Table

| Node Type | Status |
|-----------|--------|
| class_declaration | ✅ implemented |
| method_definition | ✅ implemented |
| function_declaration | ✅ implemented |
| function_expression | ✅ implemented |
| arrow_function | ✅ implemented |
| variable_declaration | ✅ implemented |
| lexical_declaration | ✅ implemented |
| const | ✅ implemented |
| let | ✅ implemented |
| var | ✅ implemented |
| import_statement | ✅ implemented |
| export_statement | ✅ implemented |
| named_imports | ✅ implemented |
| namespace_import | ✅ implemented |
| class_heritage | ✅ implemented |
| jsx_element | ✅ implemented |
| jsx_self_closing_element | ✅ implemented |
| generator_function_declaration | ✅ implemented |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Symbol Kinds Extracted
- Class
- Function
- Variable
- Constant
- Method

## Notes

The JavaScript parser provides comprehensive coverage of modern JavaScript/ES6+ features including:
- Classes and inheritance
- Arrow functions and generators
- Modern module system (import/export)
- Block-scoped variables (let/const)
- JSX support for React
- Function hoisting semantics
