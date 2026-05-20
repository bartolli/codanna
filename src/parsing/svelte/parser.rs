//! Svelte language parser implementation
//!
//! Parses `.svelte` files by:
//! 1. Using the tree-sitter-svelte grammar to locate `<script>` blocks
//! 2. Re-parsing the script content with the JavaScript parser for symbol extraction
//! 3. Offsetting all symbol ranges back to file-level positions
//! 4. Extracting snippet function names directly from the Svelte AST

use crate::parsing::import::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    JavaScriptParser, LanguageParser, MethodCall, NodeTracker, NodeTrackingState,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use std::collections::HashSet;
use tree_sitter::{Language, Node, Parser};

use crate::parsing::parser::HandledNode;
use crate::parsing::registry::LanguageId;

pub struct SvelteParser {
    svelte_parser: Parser,
    js_parser: JavaScriptParser,
    node_tracker: NodeTrackingState,
}

impl SvelteParser {
    pub fn new() -> Result<Self, String> {
        let mut svelte_parser = Parser::new();
        let language: Language = tree_sitter_svelte_next::LANGUAGE.into();
        svelte_parser
            .set_language(&language)
            .map_err(|e| format!("Failed to set Svelte language: {e}"))?;

        let js_parser =
            JavaScriptParser::new().map_err(|e| format!("Failed to create JS sub-parser: {e}"))?;

        Ok(Self {
            svelte_parser,
            js_parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Locate the first `<script>` block and return its raw text plus file-level position offset.
    fn extract_script<'a>(&mut self, code: &'a str) -> Option<(&'a str, u32, u16)> {
        let tree = self.svelte_parser.parse(code, None)?;
        let root = tree.root_node();

        let mut root_cursor = root.walk();
        for child in root.children(&mut root_cursor) {
            if child.kind() == "script_element" {
                let mut child_cursor = child.walk();
                for inner in child.children(&mut child_cursor) {
                    if inner.kind() == "raw_text" {
                        let row_off = inner.start_position().row as u32;
                        let col_off = inner.start_position().column as u16;
                        // SAFETY: byte_range is within code's bounds (same source)
                        let script = &code[inner.byte_range()];
                        return Some((script, row_off, col_off));
                    }
                }
            }
        }
        None
    }

    /// Adjust a script-relative `Range` to be relative to the whole file.
    fn offset_range(r: Range, row_off: u32, col_off: u16) -> Range {
        Range::new(
            r.start_line + row_off,
            if r.start_line == 0 {
                r.start_column.saturating_add(col_off)
            } else {
                r.start_column
            },
            r.end_line + row_off,
            if r.end_line == 0 {
                r.end_column.saturating_add(col_off)
            } else {
                r.end_column
            },
        )
    }

    /// Walk the Svelte AST for `snippet_statement` nodes and emit a Function symbol per snippet.
    fn collect_snippets(
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        out: &mut Vec<Symbol>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        if node.kind() == "snippet_statement" {
            let mut c = node.walk();
            'outer: for child in node.children(&mut c) {
                if child.kind() == "snippet_start" {
                    let mut c2 = child.walk();
                    for inner in child.children(&mut c2) {
                        if inner.kind() == "snippet_name" {
                            let name = code[inner.byte_range()].to_string();
                            let id = counter.next_id();
                            let range = Range::new(
                                inner.start_position().row as u32,
                                inner.start_position().column as u16,
                                inner.end_position().row as u32,
                                inner.end_position().column as u16,
                            );
                            let mut sym =
                                Symbol::new(id, name, SymbolKind::Function, file_id, range);
                            sym.visibility = Visibility::Private;
                            sym.language_id = Some(LanguageId::new("svelte"));
                            out.push(sym);
                            break 'outer;
                        }
                    }
                }
            }
        }

        let mut c = node.walk();
        for child in node.children(&mut c) {
            Self::collect_snippets(child, code, file_id, counter, out, depth + 1);
        }
    }
}

impl NodeTracker for SvelteParser {
    fn get_handled_nodes(&self) -> &HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for SvelteParser {
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();

        // Re-parse the script block with the JS parser and offset ranges.
        if let Some((script, row_off, col_off)) = self.extract_script(code) {
            let mut js_symbols = self.js_parser.parse(script, file_id, symbol_counter);
            for sym in &mut js_symbols {
                sym.range = Self::offset_range(sym.range, row_off, col_off);
                sym.language_id = Some(LanguageId::new("svelte"));
            }
            symbols.extend(js_symbols);
        }

        // Collect snippet functions from the template.
        if let Some(tree) = self.svelte_parser.parse(code, None) {
            let root = tree.root_node();
            Self::collect_snippets(root, code, file_id, symbol_counter, &mut symbols, 0);
        }

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, _node: &Node, _code: &str) -> Option<String> {
        None
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_calls(script)
            .into_iter()
            .map(|(caller, callee, r)| (caller, callee, Self::offset_range(r, row_off, col_off)))
            .collect()
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_method_calls(script)
            .into_iter()
            .map(|mut mc| {
                mc.range = Self::offset_range(mc.range, row_off, col_off);
                mc
            })
            .collect()
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_implementations(script)
            .into_iter()
            .map(|(a, b, r)| (a, b, Self::offset_range(r, row_off, col_off)))
            .collect()
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_extends(script)
            .into_iter()
            .map(|(a, b, r)| (a, b, Self::offset_range(r, row_off, col_off)))
            .collect()
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_uses(script)
            .into_iter()
            .map(|(a, b, r)| (a, b, Self::offset_range(r, row_off, col_off)))
            .collect()
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_defines(script)
            .into_iter()
            .map(|(a, b, r)| (a, b, Self::offset_range(r, row_off, col_off)))
            .collect()
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let Some((script, _row_off, _col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        // Import has no range field, so no offset needed.
        self.js_parser.find_imports(script, file_id)
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some((script, row_off, col_off)) = self.extract_script(code) else {
            return Vec::new();
        };
        self.js_parser
            .find_variable_types(script)
            .into_iter()
            .map(|(a, b, r)| (a, b, Self::offset_range(r, row_off, col_off)))
            .collect()
    }

    fn language(&self) -> crate::parsing::Language {
        crate::parsing::Language::Svelte
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, SymbolCounter};

    fn file_id() -> FileId {
        FileId::new(1).unwrap()
    }

    #[test]
    fn test_svelte_parser_loads() {
        let parser = SvelteParser::new();
        assert!(parser.is_ok(), "SvelteParser should initialize");
    }

    #[test]
    fn test_validate_grammar_node_kinds() {
        let behavior = crate::parsing::svelte::SvelteBehavior::new();
        use crate::parsing::LanguageBehavior;
        assert!(
            behavior.validate_node_kind("script_element"),
            "script_element should exist in grammar"
        );
        assert!(
            behavior.validate_node_kind("element"),
            "element should exist in grammar"
        );
    }

    #[test]
    fn test_parse_script_symbols() {
        let mut parser = SvelteParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let code = r#"<script>
    export function greet(name) {
        return `Hello ${name}`;
    }

    let count = 0;
</script>

<h1>Hello</h1>
"#;
        let symbols = parser.parse(code, file_id(), &mut counter);
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "greet"),
            "should extract greet function; got: {:?}",
            symbols.iter().map(|s| s.name.as_ref()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_snippet_symbols() {
        let mut parser = SvelteParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let code = r#"<script>
    let items = [];
</script>

{#snippet card(item)}
    <div>{item.name}</div>
{/snippet}
"#;
        let symbols = parser.parse(code, file_id(), &mut counter);
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "card"),
            "should extract snippet 'card'; got: {:?}",
            symbols.iter().map(|s| s.name.as_ref()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_range_offset() {
        // Script starts at line 1 (0-indexed), col 0
        let r = Range::new(0, 4, 0, 9); // line 0, col 4-9 in script
        let offset = SvelteParser::offset_range(r, 1, 0);
        assert_eq!(offset.start_line, 1);
        assert_eq!(offset.start_column, 4);

        // Multi-line symbol: line 2 in script → line 3 in file
        let r2 = Range::new(2, 0, 4, 1);
        let offset2 = SvelteParser::offset_range(r2, 1, 0);
        assert_eq!(offset2.start_line, 3);
        assert_eq!(offset2.end_line, 5);
    }

    #[test]
    fn test_find_imports() {
        let mut parser = SvelteParser::new().unwrap();
        let code = r#"<script>
    import { add } from './math.js';
    import Component from './Component.svelte';
</script>
<Component />
"#;
        let fid = file_id();
        let imports = parser.find_imports(code, fid);
        assert!(
            imports.iter().any(|i| i.path.contains("math")),
            "should find math import; got: {:?}",
            imports.iter().map(|i| &i.path).collect::<Vec<_>>()
        );
    }
}
