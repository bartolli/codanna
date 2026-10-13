# Svelte Parser Symbol Extraction Coverage Report

*Generated: 2026-05-22 19:33:09 UTC*

## Summary
- Key nodes: 5/20 (25%)
- Symbol kinds extracted: 4

> **Note:** Svelte delegates `<script>` bodies to the JS/TS parsers; the nodes below are the Svelte-level constructs handled directly.

## Coverage Table

| Node Type | ID | Status |
|-----------|-----|--------|
| document | 52 | ⚠️ gap |
| script_element | 56 | ✅ implemented |
| style_element | - | ❌ not found |
| start_tag | 58 | ⚠️ gap |
| end_tag | 62 | ⚠️ gap |
| raw_text | 47 | ✅ implemented |
| attribute | 64 | ⚠️ gap |
| attribute_name | 9 | ⚠️ gap |
| quoted_attribute_value | 65 | ⚠️ gap |
| element | 55 | ⚠️ gap |
| expression_tag | 107 | ⚠️ gap |
| render_tag | 114 | ⚠️ gap |
| snippet_statement | 100 | ✅ implemented |
| snippet_start | 102 | ✅ implemented |
| snippet_name | 32 | ✅ implemented |
| if_statement | 68 | ⚠️ gap |
| each_statement | 79 | ⚠️ gap |
| await_statement | - | ❌ not found |
| key_statement | - | ❌ not found |
| comment | 48 | ⚠️ gap |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 1: Implementation Gaps
These nodes exist in your code but aren't being captured:

- `document`: Add parsing logic in parser.rs
- `start_tag`: Add parsing logic in parser.rs
- `end_tag`: Add parsing logic in parser.rs
- `attribute`: Add parsing logic in parser.rs
- `attribute_name`: Add parsing logic in parser.rs
- `quoted_attribute_value`: Add parsing logic in parser.rs
- `element`: Add parsing logic in parser.rs
- `expression_tag`: Add parsing logic in parser.rs
- `render_tag`: Add parsing logic in parser.rs
- `if_statement`: Add parsing logic in parser.rs
- `each_statement`: Add parsing logic in parser.rs
- `comment`: Add parsing logic in parser.rs

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `style_element`: Add example to comprehensive.svelte or verify node name
- `await_statement`: Add example to comprehensive.svelte or verify node name
- `key_statement`: Add example to comprehensive.svelte or verify node name

