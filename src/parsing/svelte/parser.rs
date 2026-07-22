//! Svelte language parser implementation
//!
//! Parses `.svelte` files by:
//! 1. Using the tree-sitter-svelte grammar to locate every `<script>` block
//!    (both the instance `<script>` and the module `<script module>`)
//! 2. Re-parsing each script body with the JavaScript parser, or the TypeScript
//!    parser when the block declares `lang="ts"` (the Svelte 5 default)
//! 3. Offsetting all symbol ranges back to file-level positions
//! 4. Extracting snippet function names directly from the Svelte AST

use crate::parsing::import::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    JavaScriptParser, LanguageParser, MethodCall, NodeTracker, NodeTrackingState, TypeScriptParser,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use std::collections::HashSet;
use tree_sitter::{Language, Node, Parser};

use crate::parsing::parser::HandledNode;
use crate::parsing::registry::LanguageId;

/// A `<script>` block located in a Svelte file, with the offset needed to map
/// script-relative ranges back to file-level positions.
struct ScriptBlock<'a> {
    /// Raw script body (the `raw_text` between the script tags).
    text: &'a str,
    row_off: u32,
    col_off: u16,
    /// True when the block declares `lang="ts"` (or `lang="typescript"`).
    is_typescript: bool,
}

pub struct SvelteParser {
    svelte_parser: Parser,
    js_parser: JavaScriptParser,
    ts_parser: TypeScriptParser,
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
        let ts_parser =
            TypeScriptParser::new().map_err(|e| format!("Failed to create TS sub-parser: {e}"))?;

        Ok(Self {
            svelte_parser,
            js_parser,
            ts_parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Locate every `<script>` block, returning each body plus its file-level
    /// offset and whether it is TypeScript. Both the instance `<script>` and a
    /// module `<script module>` block are returned, in document order.
    fn extract_scripts<'a>(&mut self, code: &'a str) -> Vec<ScriptBlock<'a>> {
        let Some(tree) = self.svelte_parser.parse(code, None) else {
            return Vec::new();
        };
        let root = tree.root_node();

        let mut scripts = Vec::new();
        let mut root_cursor = root.walk();
        for child in root.children(&mut root_cursor) {
            if child.kind() != "script_element" {
                continue;
            }
            self.register_handled_node(child.kind(), child.kind_id());
            let is_typescript = Self::script_is_typescript(child, code);

            let mut child_cursor = child.walk();
            for inner in child.children(&mut child_cursor) {
                if inner.kind() == "raw_text" {
                    self.register_handled_node(inner.kind(), inner.kind_id());
                    let row_off = inner.start_position().row as u32;
                    let col_off = inner.start_position().column as u16;
                    // SAFETY: byte_range is within code's bounds (same source)
                    let text = &code[inner.byte_range()];
                    scripts.push(ScriptBlock {
                        text,
                        row_off,
                        col_off,
                        is_typescript,
                    });
                }
            }
        }
        scripts
    }

    /// Inspect a `script_element`'s start tag for a `lang="ts"` attribute.
    fn script_is_typescript(script_element: Node, code: &str) -> bool {
        let mut tag_cursor = script_element.walk();
        for tag in script_element.children(&mut tag_cursor) {
            if tag.kind() != "start_tag" {
                continue;
            }
            let mut attr_cursor = tag.walk();
            for attr in tag.children(&mut attr_cursor) {
                if attr.kind() != "attribute" {
                    continue;
                }
                let mut is_lang = false;
                let mut is_ts = false;
                let mut part_cursor = attr.walk();
                for part in attr.children(&mut part_cursor) {
                    match part.kind() {
                        "attribute_name" => {
                            is_lang = code[part.byte_range()].trim() == "lang";
                        }
                        "quoted_attribute_value" | "attribute_value" => {
                            let value = code[part.byte_range()]
                                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                            is_ts = value == "ts" || value == "typescript";
                        }
                        _ => {}
                    }
                }
                if is_lang && is_ts {
                    return true;
                }
            }
        }
        false
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
        &mut self,
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
            self.register_handled_node(node.kind(), node.kind_id());
            let mut c = node.walk();
            'outer: for child in node.children(&mut c) {
                if child.kind() == "snippet_start" {
                    self.register_handled_node(child.kind(), child.kind_id());
                    let mut c2 = child.walk();
                    for inner in child.children(&mut c2) {
                        if inner.kind() == "snippet_name" {
                            self.register_handled_node(inner.kind(), inner.kind_id());
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
            self.collect_snippets(child, code, file_id, counter, out, depth + 1);
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

        // Re-parse each <script> block with the matching JS/TS parser and offset ranges.
        for script in self.extract_scripts(code) {
            let mut script_symbols = if script.is_typescript {
                self.ts_parser.parse(script.text, file_id, symbol_counter)
            } else {
                self.js_parser.parse(script.text, file_id, symbol_counter)
            };
            for sym in &mut script_symbols {
                sym.range = Self::offset_range(sym.range, script.row_off, script.col_off);
                sym.language_id = Some(LanguageId::new("svelte"));
            }
            symbols.extend(script_symbols);
        }

        // Collect snippet functions from the template.
        if let Some(tree) = self.svelte_parser.parse(code, None) {
            let root = tree.root_node();
            self.collect_snippets(root, code, file_id, symbol_counter, &mut symbols, 0);
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
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let calls = if script.is_typescript {
                self.ts_parser.find_calls(script.text)
            } else {
                self.js_parser.find_calls(script.text)
            };
            out.extend(calls.into_iter().map(|(caller, callee, r)| {
                (
                    caller,
                    callee,
                    Self::offset_range(r, script.row_off, script.col_off),
                )
            }));
        }
        out
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let calls = if script.is_typescript {
                self.ts_parser.find_method_calls(script.text)
            } else {
                self.js_parser.find_method_calls(script.text)
            };
            out.extend(calls.into_iter().map(|mut mc| {
                mc.range = Self::offset_range(mc.range, script.row_off, script.col_off);
                mc
            }));
        }
        out
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let rels = if script.is_typescript {
                self.ts_parser.find_implementations(script.text)
            } else {
                self.js_parser.find_implementations(script.text)
            };
            out.extend(
                rels.into_iter()
                    .map(|(a, b, r)| (a, b, Self::offset_range(r, script.row_off, script.col_off))),
            );
        }
        out
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let rels = if script.is_typescript {
                self.ts_parser.find_extends(script.text)
            } else {
                self.js_parser.find_extends(script.text)
            };
            out.extend(
                rels.into_iter()
                    .map(|(a, b, r)| (a, b, Self::offset_range(r, script.row_off, script.col_off))),
            );
        }
        out
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let rels = if script.is_typescript {
                self.ts_parser.find_uses(script.text)
            } else {
                self.js_parser.find_uses(script.text)
            };
            out.extend(
                rels.into_iter()
                    .map(|(a, b, r)| (a, b, Self::offset_range(r, script.row_off, script.col_off))),
            );
        }
        out
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let rels = if script.is_typescript {
                self.ts_parser.find_defines(script.text)
            } else {
                self.js_parser.find_defines(script.text)
            };
            out.extend(
                rels.into_iter()
                    .map(|(a, b, r)| (a, b, Self::offset_range(r, script.row_off, script.col_off))),
            );
        }
        out
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            // Import has no range field, so no offset needed.
            let imports = if script.is_typescript {
                self.ts_parser.find_imports(script.text, file_id)
            } else {
                self.js_parser.find_imports(script.text, file_id)
            };
            out.extend(imports);
        }
        out
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut out = Vec::new();
        for script in self.extract_scripts(code) {
            let types = if script.is_typescript {
                self.ts_parser.find_variable_types(script.text)
            } else {
                self.js_parser.find_variable_types(script.text)
            };
            out.extend(
                types
                    .into_iter()
                    .map(|(a, b, r)| (a, b, Self::offset_range(r, script.row_off, script.col_off))),
            );
        }
        out
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

    #[test]
    fn test_typescript_script_symbols() {
        // Svelte 5 defaults to `lang="ts"`; symbols must route through the TS parser.
        let mut parser = SvelteParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let code = r#"<script lang="ts">
    interface User {
        name: string;
    }

    export function greet(user: User): string {
        return `Hello ${user.name}`;
    }
</script>

<h1>Hello</h1>
"#;
        let symbols = parser.parse(code, file_id(), &mut counter);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(
            names.contains(&"greet"),
            "should extract greet function from TS block; got: {names:?}"
        );
        assert!(
            names.contains(&"User"),
            "should extract User interface (TS-only construct); got: {names:?}"
        );
    }

    #[test]
    fn test_module_and_instance_scripts() {
        // Both the module `<script module>` and the instance `<script>` must be parsed.
        let mut parser = SvelteParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let code = r#"<script module>
    export const VERSION = "1.0.0";
</script>

<script lang="ts">
    export function start(): void {}
</script>

<p>{VERSION}</p>
"#;
        let symbols = parser.parse(code, file_id(), &mut counter);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(
            names.contains(&"VERSION"),
            "should extract VERSION from module script; got: {names:?}"
        );
        assert!(
            names.contains(&"start"),
            "should extract start from instance script; got: {names:?}"
        );
    }

    #[test]
    fn test_typescript_imports() {
        let mut parser = SvelteParser::new().unwrap();
        let code = r#"<script lang="ts">
    import type { User } from './types.ts';
    import { load } from './api.ts';
</script>
"#;
        let imports = parser.find_imports(code, file_id());
        let paths: Vec<&String> = imports.iter().map(|i| &i.path).collect();
        assert!(
            paths.iter().any(|p| p.contains("api")),
            "should find api import from TS block; got: {paths:?}"
        );
    }
}
