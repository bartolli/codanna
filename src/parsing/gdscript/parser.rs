//! GDScript language parser implementation
//!
//! Provides basic symbol extraction for Godot's GDScript using tree-sitter.

use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind};
use std::any::Any;
use tree_sitter::{Node, Parser};

/// Parser for GDScript source files
pub struct GdscriptParser {
    parser: Parser,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for GdscriptParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GdscriptParser")
            .field("language", &"GDScript")
            .finish()
    }
}

impl GdscriptParser {
    /// Create a new parser instance
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_gdscript::LANGUAGE.into())
            .map_err(|e| format!("Failed to initialize GDScript parser: {e}"))?;

        Ok(Self {
            parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Convert a tree-sitter node into a Range
    fn node_to_range(&self, node: Node) -> Range {
        let start = node.start_position();
        let end = node.end_position();
        Range {
            start_line: start.row as u32,
            start_column: start.column as u16,
            end_line: end.row as u32,
            end_column: end.column as u16,
        }
    }

    /// Helper to register handled node kinds for audit tracking
    fn register_node(&mut self, node: &Node) {
        self.node_tracker
            .register_handled_node(node.kind(), node.kind_id());
    }

    /// Extract raw source text for a node
    fn text_for_node<'a>(&self, code: &'a str, node: Node) -> &'a str {
        &code[node.byte_range()]
    }

    /// Extract documentation comments prefixed with `##`
    fn doc_comment_for(&self, node: &Node, code: &str) -> Option<String> {
        let mut comments = Vec::new();
        let mut current = node.prev_named_sibling();

        while let Some(sibling) = current {
            if sibling.kind() != "comment" {
                break;
            }

            let raw = self.text_for_node(code, sibling).trim();
            if raw.starts_with("##") {
                let cleaned = raw.trim_start_matches('#').trim();
                comments.push(cleaned.to_string());
                current = sibling.prev_named_sibling();
            } else {
                break;
            }
        }

        if comments.is_empty() {
            None
        } else {
            comments.reverse();
            Some(comments.join("\n"))
        }
    }

    /// Process AST recursively and collect symbols
    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            "class_definition" => {
                self.handle_class_definition(node, code, file_id, symbols, counter, context, depth);
                return;
            }
            "function_definition" => {
                self.handle_function_definition(
                    node, code, file_id, symbols, counter, context, depth,
                );
                return;
            }
            "constructor_definition" => {
                self.handle_constructor_definition(
                    node, code, file_id, symbols, counter, context, depth,
                );
                return;
            }
            "signal_statement" => {
                self.handle_signal_statement(node, code, file_id, symbols, counter, context);
            }
            "enum_definition" => {
                self.register_node(&node);
            }
            "extends_statement"
            | "match_statement"
            | "for_statement"
            | "if_statement"
            | "while_statement"
            | "tool_statement"
            | "export_variable_statement"
            | "annotation"
            | "annotations" => {
                self.register_node(&node);
            }
            "variable_statement" => {
                self.handle_variable_statement(node, code, file_id, symbols, counter, context);
            }
            "const_statement" => {
                self.handle_const_statement(node, code, file_id, symbols, counter, context);
            }
            "class_name_statement" => {
                self.handle_class_name_statement(node, code, file_id, symbols, counter, context);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(
                child,
                code,
                file_id,
                symbols,
                counter,
                context,
                depth + 1,
            );
        }
    }

    fn handle_class_definition(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        self.register_node(&node);

        let name_node = match node.child_by_field_name("name") {
            Some(name) => name,
            None => return,
        };

        let class_name = self.text_for_node(code, name_node).trim();
        if class_name.is_empty() {
            return;
        }

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            class_name,
            SymbolKind::Class,
            file_id,
            self.node_to_range(node),
        );

        symbol.signature = Some(format!("class {class_name}").into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);

        let previous_class = context.current_class().map(|s| s.to_string());
        context.enter_scope(ScopeType::Class);
        context.set_current_class(Some(class_name.to_string()));

        if let Some(extends_node) = node.child_by_field_name("extends") {
            self.register_node(&extends_node);
        }

        if let Some(body) = node.child_by_field_name("body") {
            self.extract_symbols_from_node(
                body,
                code,
                file_id,
                symbols,
                counter,
                context,
                depth + 1,
            );
        }

        context.exit_scope();
        context.set_current_class(previous_class);
    }

    fn handle_function_definition(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        self.register_node(&node);

        let name_node = match node.child_by_field_name("name") {
            Some(name) => name,
            None => return,
        };

        let func_name = self.text_for_node(code, name_node).trim();
        if func_name.is_empty() {
            return;
        }

        let params = node
            .child_by_field_name("parameters")
            .map(|n| self.text_for_node(code, n).trim())
            .unwrap_or("()");

        let signature = if let Some(ret) = node.child_by_field_name("return_type") {
            let ret_text = self.text_for_node(code, ret).trim();
            format!("func {func_name}{params} -> {ret_text}")
        } else {
            format!("func {func_name}{params}")
        };

        let symbol_kind = if context.is_in_class() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            func_name,
            symbol_kind,
            file_id,
            self.node_to_range(node),
        );

        symbol.signature = Some(signature.into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);

        let previous_function = context.current_function().map(|s| s.to_string());
        context.enter_scope(ScopeType::function());
        context.set_current_function(Some(func_name.to_string()));

        if let Some(body) = node.child_by_field_name("body") {
            self.extract_symbols_from_node(
                body,
                code,
                file_id,
                symbols,
                counter,
                context,
                depth + 1,
            );
        }

        context.exit_scope();
        context.set_current_function(previous_function);
    }

    fn handle_constructor_definition(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        self.register_node(&node);

        let func_name = "_init";
        let params = node
            .child_by_field_name("parameters")
            .map(|n| self.text_for_node(code, n).trim())
            .unwrap_or("()");

        let signature = if let Some(ret) = node.child_by_field_name("return_type") {
            let ret_text = self.text_for_node(code, ret).trim();
            format!("func {func_name}{params} -> {ret_text}")
        } else {
            format!("func {func_name}{params}")
        };

        let symbol_kind = if context.is_in_class() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            func_name,
            symbol_kind,
            file_id,
            self.node_to_range(node),
        );

        symbol.signature = Some(signature.into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);

        let previous_function = context.current_function().map(|s| s.to_string());
        context.enter_scope(ScopeType::function());
        context.set_current_function(Some(func_name.to_string()));

        if let Some(body) = node.child_by_field_name("body") {
            self.extract_symbols_from_node(
                body,
                code,
                file_id,
                symbols,
                counter,
                context,
                depth + 1,
            );
        }

        context.exit_scope();
        context.set_current_function(previous_function);
    }

    fn handle_signal_statement(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) {
        self.register_node(&node);

        let name_node = match node.child_by_field_name("name") {
            Some(name) => name,
            None => return,
        };

        let signal_name = self.text_for_node(code, name_node).trim();
        if signal_name.is_empty() {
            return;
        }

        let params = node
            .child_by_field_name("parameters")
            .map(|n| self.text_for_node(code, n).trim())
            .unwrap_or("");

        let signature = if params.is_empty() {
            format!("signal {signal_name}")
        } else {
            format!("signal {signal_name}{params}")
        };

        let symbol_kind = if context.is_in_class() {
            SymbolKind::Field
        } else {
            SymbolKind::Constant
        };

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            signal_name,
            symbol_kind,
            file_id,
            self.node_to_range(node),
        );
        symbol.signature = Some(signature.into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);
    }

    fn handle_variable_statement(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) {
        self.register_node(&node);

        if context.is_in_function() {
            // Skip local variables
            return;
        }

        let name_node = match node.child_by_field_name("name") {
            Some(name) => name,
            None => return,
        };

        let var_name = self.text_for_node(code, name_node).trim();
        if var_name.is_empty() {
            return;
        }

        let symbol_kind = if context.is_in_class() {
            SymbolKind::Field
        } else {
            SymbolKind::Variable
        };

        let signature = self.text_for_node(code, node).trim();

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            var_name,
            symbol_kind,
            file_id,
            self.node_to_range(node),
        );
        symbol.signature = Some(signature.into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);
    }

    fn handle_const_statement(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) {
        self.register_node(&node);

        if context.is_in_function() {
            return;
        }

        let name_node = match node.child_by_field_name("name") {
            Some(name) => name,
            None => return,
        };

        let const_name = self.text_for_node(code, name_node).trim();
        if const_name.is_empty() {
            return;
        }

        let symbol_kind = if context.is_in_class() {
            SymbolKind::Field
        } else {
            SymbolKind::Constant
        };

        let signature = self.text_for_node(code, node).trim();

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            const_name,
            symbol_kind,
            file_id,
            self.node_to_range(node),
        );
        symbol.signature = Some(signature.into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);
    }

    fn handle_class_name_statement(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) {
        self.register_node(&node);

        let name_node = match node.child_by_field_name("name") {
            Some(name) => name,
            None => return,
        };

        let class_name = self.text_for_node(code, name_node).trim();
        if class_name.is_empty() {
            return;
        }

        let signature = format!("class_name {class_name}");

        let symbol_id = counter.next_id();
        let mut symbol = Symbol::new(
            symbol_id,
            class_name,
            SymbolKind::Class,
            file_id,
            self.node_to_range(node),
        );
        symbol.signature = Some(signature.into());
        if let Some(doc) = self.doc_comment_for(&node, code) {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(context.current_scope_context());
        symbols.push(symbol);
    }

    /// Recursively collect class inheritance relationships
    fn collect_extends<'a>(
        &self,
        node: Node,
        code: &'a str,
        results: &mut Vec<(&'a str, &'a str, Range)>,
        current_class: Option<&'a str>,
    ) {
        match node.kind() {
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let class_name = self.text_for_node(code, name_node).trim();

                    if let Some(extends_node) = node.child_by_field_name("extends") {
                        if let Some(target) = extends_node.named_child(0) {
                            let base = self.text_for_node(code, target).trim().trim_matches('"');
                            let range = self.node_to_range(extends_node);
                            results.push((class_name, base, range));
                        }
                    }

                    if let Some(body) = node.child_by_field_name("body") {
                        let mut cursor = body.walk();
                        for child in body.children(&mut cursor) {
                            self.collect_extends(child, code, results, Some(class_name));
                        }
                    }

                    return;
                }
            }
            "extends_statement" => {
                if let Some(target) = node.named_child(0) {
                    let base = self.text_for_node(code, target).trim().trim_matches('"');
                    let derived = current_class.unwrap_or("<script>");
                    let range = self.node_to_range(node);
                    results.push((derived, base, range));
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_extends(child, code, results, current_class);
        }
    }

    /// Access handled nodes for audit tooling
    pub fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }
}

impl LanguageParser for GdscriptParser {
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut symbols = Vec::new();
        let mut context = ParserContext::new();

        // Create a module-level symbol to represent the script itself
        let module_id = symbol_counter.next_id();
        let mut module_symbol = Symbol::new(
            module_id,
            "<script>",
            SymbolKind::Module,
            file_id,
            self.node_to_range(root),
        );
        module_symbol.scope_context = Some(crate::symbol::ScopeContext::Module);
        symbols.push(module_symbol);

        self.extract_symbols_from_node(
            root,
            code,
            file_id,
            &mut symbols,
            symbol_counter,
            &mut context,
            0,
        );

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.doc_comment_for(node, code)
    }

    fn find_calls<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut results = Vec::new();
        self.collect_extends(tree.root_node(), code, &mut results, None);
        results
    }

    fn find_uses<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_defines<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_imports(&mut self, _code: &str, _file_id: FileId) -> Vec<Import> {
        Vec::new()
    }

    fn language(&self) -> Language {
        Language::Gdscript
    }
}

impl NodeTracker for GdscriptParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}
