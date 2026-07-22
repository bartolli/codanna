use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Import, LanguageParser, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use tree_sitter::{Node, Parser};

pub struct NixParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

fn range_from_node(node: &Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range::new(
        start.row as u32,
        start.column as u16,
        end.row as u32,
        end.column as u16,
    )
}

impl NixParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        let lang = tree_sitter_nix::LANGUAGE;
        parser
            .set_language(&lang.into())
            .map_err(|e| format!("Failed to set Nix language: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
    }

    fn create_symbol(
        &self,
        id: crate::types::SymbolId,
        name: String,
        kind: SymbolKind,
        file_id: FileId,
        range: Range,
        signature: Option<String>,
        doc_comment: Option<String>,
        module_path: &str,
        visibility: Visibility,
    ) -> Symbol {
        let mut symbol = Symbol::new(id, name, kind, file_id, range);
        if let Some(sig) = signature {
            symbol = symbol.with_signature(sig);
        }
        if let Some(doc) = doc_comment {
            symbol = symbol.with_doc(doc);
        }
        if !module_path.is_empty() {
            symbol = symbol.with_module_path(module_path);
        }
        symbol = symbol.with_visibility(visibility);
        symbol.scope_context = Some(self.context.current_scope_context());
        symbol
    }

    fn node_text<'a>(&self, node: &Node, code: &'a str) -> &'a str {
        &code[node.byte_range()]
    }

    /// Check whether a binding's `expression` child is a function_expression (lambda).
    fn value_is_function(node: Node) -> bool {
        if let Some(expr) = node.child_by_field_name("expression") {
            let kind = expr.kind();
            if kind == "function_expression" {
                return true;
            }
            if kind == "parenthesized_expression" {
                let mut cursor = expr.walk();
                for child in expr.children(&mut cursor) {
                    if child.kind() == "function_expression" {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        self.node_tracker
            .register_handled_node(node.kind(), node.kind_id());

        match node.kind() {
            // ── root ─────────────────────────────────────────────────────────
            "source_code" => {
                // source_code has field `expression:` pointing to the root expr
                if let Some(expr) = node.child_by_field_name("expression") {
                    self.extract_symbols_from_node(
                        expr,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                        depth + 1,
                    );
                } else {
                    self.recurse_children(
                        node,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                        depth,
                    );
                }
            }

            // ── binding_set (container inside attrsets / let) ────────────────
            "binding_set" => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── binding ──────────────────────────────────────────────────────
            "binding" => {
                self.process_binding(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── attrset_expression ────────────────────────────────────────────
            "attrset_expression" => {
                self.context.enter_scope(ScopeType::Class);
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
                self.context.exit_scope();
            }

            // ── rec_attrset_expression ────────────────────────────────────────
            "rec_attrset_expression" => {
                self.context.enter_scope(ScopeType::Class);
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
                self.context.exit_scope();
            }

            // ── let_expression ────────────────────────────────────────────────
            "let_expression" => {
                self.context.enter_scope(ScopeType::Block);
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
                self.context.exit_scope();
            }

            // ── function_expression (lambda) ──────────────────────────────────
            // Fields confirmed: `universal:` (simple `x:` param), `formals:`, `body:`
            "function_expression" => {
                self.context.enter_scope(ScopeType::hoisting_function());

                if let Some(formals) = node.child_by_field_name("formals") {
                    self.process_formals(formals, code, file_id, counter, symbols, module_path);
                } else if let Some(param) = node.child_by_field_name("universal") {
                    // simple `x: body` form — param is an identifier
                    if param.kind() == "identifier" {
                        let name = self.node_text(&param, code).to_string();
                        let sym = self.create_symbol(
                            counter.next_id(),
                            name.clone(),
                            SymbolKind::Parameter,
                            file_id,
                            range_from_node(&param),
                            Some(name),
                            None,
                            module_path,
                            Visibility::Private,
                        );
                        symbols.push(sym);
                    }
                }

                if let Some(body) = node.child_by_field_name("body") {
                    self.extract_symbols_from_node(
                        body,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                        depth + 1,
                    );
                }
                self.context.exit_scope();
            }

            // ── inherit ───────────────────────────────────────────────────────
            // `inherit a b c;` — names inside `inherited_attrs`
            "inherit" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "inherited_attrs" {
                        let mut c2 = child.walk();
                        for attr in child.children(&mut c2) {
                            if attr.kind() == "identifier" {
                                let name = self.node_text(&attr, code).to_string();
                                let sym = self.create_symbol(
                                    counter.next_id(),
                                    name.clone(),
                                    SymbolKind::Variable,
                                    file_id,
                                    range_from_node(&attr),
                                    Some(name),
                                    None,
                                    module_path,
                                    Visibility::Public,
                                );
                                symbols.push(sym);
                            }
                        }
                    } else if child.kind() == "identifier" {
                        // some grammar versions put identifiers directly
                        let name = self.node_text(&child, code).to_string();
                        let sym = self.create_symbol(
                            counter.next_id(),
                            name.clone(),
                            SymbolKind::Variable,
                            file_id,
                            range_from_node(&child),
                            Some(name),
                            None,
                            module_path,
                            Visibility::Public,
                        );
                        symbols.push(sym);
                    }
                }
            }

            // ── inherit_from ──────────────────────────────────────────────────
            // `inherit (src) a b c;`
            "inherit_from" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "inherited_attrs" {
                        let mut c2 = child.walk();
                        for attr in child.children(&mut c2) {
                            if attr.kind() == "identifier" {
                                let name = self.node_text(&attr, code).to_string();
                                let sym = self.create_symbol(
                                    counter.next_id(),
                                    name.clone(),
                                    SymbolKind::Variable,
                                    file_id,
                                    range_from_node(&attr),
                                    Some(name),
                                    None,
                                    module_path,
                                    Visibility::Public,
                                );
                                symbols.push(sym);
                            }
                        }
                    }
                }
            }

            // ── apply_expression (function call / import) ─────────────────────
            "apply_expression" => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── select_expression (a.b.c) ─────────────────────────────────────
            "select_expression" => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── with_expression ───────────────────────────────────────────────
            "with_expression" => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── if_expression ─────────────────────────────────────────────────
            "if_expression" | "assert_expression" => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── variable_expression — leaf, no symbols emitted ────────────────
            "variable_expression" => {}

            // ── ERROR — recurse ───────────────────────────────────────────────
            "ERROR" => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }

            // ── everything else — pass through ────────────────────────────────
            _ => {
                self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            }
        }
    }

    fn recurse_children(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        depth: usize,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(
                child,
                code,
                file_id,
                counter,
                symbols,
                module_path,
                depth + 1,
            );
        }
    }

    fn process_binding(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        depth: usize,
    ) {
        // Key field is `attrpath:`, value field is `expression:`
        let Some(key_node) = node.child_by_field_name("attrpath") else {
            self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            return;
        };

        let name = self.node_text(&key_node, code).to_string();
        // Only emit symbols for simple single-component names; skip `a.b.c` paths
        if name.contains('.') || name.contains('"') || name.contains('$') {
            self.recurse_children(node, code, file_id, counter, symbols, module_path, depth);
            return;
        }

        let is_func = Self::value_is_function(node);
        let (kind, visibility) = if is_func {
            (SymbolKind::Function, Visibility::Public)
        } else if name.chars().all(|c| c.is_uppercase() || c == '_') && name.len() > 1 {
            (SymbolKind::Constant, Visibility::Public)
        } else {
            (SymbolKind::Variable, Visibility::Public)
        };

        let doc_comment = self.extract_nix_doc_comment(&node, code);
        let range = range_from_node(&node);
        let sym = self.create_symbol(
            counter.next_id(),
            name.clone(),
            kind,
            file_id,
            range,
            Some(name),
            doc_comment,
            module_path,
            visibility,
        );
        symbols.push(sym);

        // Recurse into value so nested structures are also visited
        if let Some(value) = node.child_by_field_name("expression") {
            self.extract_symbols_from_node(
                value,
                code,
                file_id,
                counter,
                symbols,
                module_path,
                depth + 1,
            );
        }
    }

    fn process_formals(
        &mut self,
        formals: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        self.node_tracker
            .register_handled_node(formals.kind(), formals.kind_id());

        let mut cursor = formals.walk();
        for child in formals.children(&mut cursor) {
            if child.kind() == "formal" {
                self.node_tracker
                    .register_handled_node(child.kind(), child.kind_id());
                // formal has field `name:` (identifier)
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = self.node_text(&name_node, code).to_string();
                    let sym = self.create_symbol(
                        counter.next_id(),
                        name.clone(),
                        SymbolKind::Parameter,
                        file_id,
                        range_from_node(&name_node),
                        Some(name),
                        None,
                        module_path,
                        Visibility::Private,
                    );
                    symbols.push(sym);
                } else {
                    // fallback: first identifier child
                    let mut c2 = child.walk();
                    for fc in child.children(&mut c2) {
                        if fc.kind() == "identifier" {
                            let name = self.node_text(&fc, code).to_string();
                            let sym = self.create_symbol(
                                counter.next_id(),
                                name.clone(),
                                SymbolKind::Parameter,
                                file_id,
                                range_from_node(&fc),
                                Some(name),
                                None,
                                module_path,
                                Visibility::Private,
                            );
                            symbols.push(sym);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn extract_nix_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        let mut prev = node.prev_sibling();
        let mut comments = Vec::new();

        while let Some(sibling) = prev {
            if sibling.kind() == "comment" {
                let text = &code[sibling.byte_range()];
                let trimmed = text.trim_start_matches('#').trim();
                if !trimmed.is_empty() {
                    comments.push(trimmed.to_string());
                }
                prev = sibling.prev_sibling();
            } else {
                break;
            }
        }

        if comments.is_empty() {
            return None;
        }
        comments.reverse();
        Some(comments.join("\n"))
    }
}

impl NodeTracker for NixParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for NixParser {
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        self.context = ParserContext::new();
        let mut symbols = Vec::new();

        if let Some(tree) = self.parser.parse(code, None) {
            let root_node = tree.root_node();
            self.extract_symbols_from_node(
                root_node,
                code,
                file_id,
                symbol_counter,
                &mut symbols,
                "",
                0,
            );
        }

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.extract_nix_doc_comment(node, code)
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let Some(tree) = self.parser.parse(code, None) else {
            return Vec::new();
        };

        let mut results = Vec::new();
        Self::collect_calls(tree.root_node(), code, &mut results);
        results
    }

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_uses<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_defines<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let Some(tree) = self.parser.parse(code, None) else {
            return Vec::new();
        };

        let mut imports = Vec::new();
        Self::collect_imports(tree.root_node(), code, file_id, &mut imports);
        imports
    }

    fn language(&self) -> crate::parsing::Language {
        crate::parsing::Language::Nix
    }
}

impl NixParser {
    fn collect_calls<'a>(node: Node, code: &'a str, results: &mut Vec<(&'a str, &'a str, Range)>) {
        if node.kind() == "apply_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                if matches!(func.kind(), "variable_expression" | "select_expression") {
                    let callee = &code[func.byte_range()];
                    results.push(("", callee, range_from_node(&node)));
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::collect_calls(child, code, results);
        }
    }

    fn collect_imports(node: Node, code: &str, file_id: FileId, imports: &mut Vec<Import>) {
        // `import ./path` or `import <nixpkgs>`
        if node.kind() == "apply_expression" {
            if let Some(func) = node.child_by_field_name("function") {
                let func_text = &code[func.byte_range()];
                if func_text == "import" {
                    if let Some(arg) = node.child_by_field_name("argument") {
                        let raw = &code[arg.byte_range()];
                        let path = raw
                            .trim_matches('<')
                            .trim_matches('>')
                            .trim_matches('"')
                            .to_string();
                        if !path.is_empty() {
                            imports.push(Import {
                                path,
                                alias: None,
                                file_id,
                                is_glob: false,
                                is_type_only: false,
                            });
                            return;
                        }
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::collect_imports(child, code, file_id, imports);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SymbolCounter;
    use std::collections::HashMap;

    /// Phase 0 — AST node discovery for tree-sitter-nix.
    /// Run with: `cargo test explore_nix_abi15 -- --nocapture`
    #[test]
    fn explore_nix_abi15() {
        let code = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/nix/comprehensive.nix"
        ))
        .unwrap_or_else(|_| r#"{ x = 1; add = a: b: a + b; }"#.to_string());

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_nix::LANGUAGE.into())
            .expect("failed to set Nix language");

        let tree = parser.parse(&code, None).expect("parse failed");

        let mut registry: HashMap<String, u16> = HashMap::new();
        discover_nodes(tree.root_node(), &mut registry);

        let mut sorted: Vec<_> = registry.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_str());

        println!(
            "\n=== tree-sitter-nix node kinds ({} total) ===",
            sorted.len()
        );
        for (kind, id) in &sorted {
            println!("  [{id:4}]  {kind}");
        }

        println!("\n=== Parse tree (first 80 fragments) ===");
        let s_expr = tree.root_node().to_sexp();
        for (i, fragment) in s_expr.split('(').take(80).enumerate() {
            println!("{i:3}  ({fragment}");
        }

        assert!(!registry.is_empty());
        assert!(
            registry.contains_key("source_code"),
            "Expected source_code root node"
        );
        assert!(registry.contains_key("binding"), "Expected binding node");
    }

    fn discover_nodes(node: tree_sitter::Node, registry: &mut HashMap<String, u16>) {
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            registry.insert(current.kind().to_string(), current.kind_id());
            let mut cursor = current.walk();
            for child in current.children(&mut cursor) {
                stack.push(child);
            }
        }
    }

    #[test]
    fn test_nix_parser_creation() {
        assert!(NixParser::new().is_ok());
    }

    #[test]
    fn test_nix_parse_attrset_bindings() {
        let mut parser = NixParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();

        let code = r#"{ x = 1; y = 2; }"#;
        let symbols = parser.parse(code, file_id, &mut counter);

        let names: Vec<_> = symbols.iter().map(|s| s.name.as_ref()).collect();
        println!("symbols from attrset: {names:?}");
        assert!(names.contains(&"x"), "Expected x in {names:?}");
        assert!(names.contains(&"y"), "Expected y in {names:?}");
    }

    #[test]
    fn test_nix_parse_function_binding() {
        let mut parser = NixParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();

        let code = r#"{ add = a: b: a + b; name = "hello"; }"#;
        let symbols = parser.parse(code, file_id, &mut counter);

        let names: Vec<_> = symbols.iter().map(|s| s.name.as_ref()).collect();
        println!("symbols from attrset with lambda: {names:?}");

        let add_sym = symbols.iter().find(|s| s.name.as_ref() == "add");
        assert!(add_sym.is_some(), "Expected add symbol");
        assert_eq!(add_sym.unwrap().kind, SymbolKind::Function);
    }

    #[test]
    fn test_nix_parse_let_expression() {
        let mut parser = NixParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();

        let code = r#"let x = 1; f = a: a + 1; in f x"#;
        let symbols = parser.parse(code, file_id, &mut counter);

        let names: Vec<_> = symbols.iter().map(|s| s.name.as_ref()).collect();
        println!("symbols from let: {names:?}");
        assert!(names.contains(&"x"), "Expected x");
        assert!(names.contains(&"f"), "Expected f");
    }

    #[test]
    fn test_nix_find_imports() {
        let mut parser = NixParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"{ pkgs = import <nixpkgs> {}; local = import ./local.nix; }"#;
        let imports = parser.find_imports(code, file_id);

        println!("imports: {imports:?}");
        assert!(!imports.is_empty(), "Expected at least one import");
    }

    #[test]
    fn test_nix_language() {
        let parser = NixParser::new().unwrap();
        assert_eq!(parser.language(), crate::parsing::Language::Nix);
    }
}
