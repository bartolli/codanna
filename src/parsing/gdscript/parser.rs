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

const SCRIPT_SCOPE: &str = "<script>";

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

    /// Recursively find imports (extends, preload, class_name) in the AST
    fn find_imports_in_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "extends_statement" => {
                // Module-level extends statement
                if let Some(target) = node.named_child(0) {
                    let path = self.text_for_node(code, target).trim().to_string();
                    if !path.is_empty() {
                        imports.push(Import {
                            file_id,
                            path,
                            alias: None,
                            is_glob: false,
                            is_type_only: false,
                        });
                    }
                }
            }
            "class_definition" => {
                // Check for extends clause inside class
                if let Some(extends_node) = node.child_by_field_name("extends") {
                    if let Some(target) = extends_node.named_child(0) {
                        let path = self.text_for_node(code, target).trim().to_string();
                        if !path.is_empty() {
                            imports.push(Import {
                                file_id,
                                path,
                                alias: None,
                                is_glob: false,
                                is_type_only: false,
                            });
                        }
                    }
                }
            }
            "class_name_statement" => {
                // class_name makes this symbol globally available
                if let Some(name_node) = node.named_child(0) {
                    let class_name = self.text_for_node(code, name_node).trim().to_string();
                    if !class_name.is_empty() {
                        imports.push(Import {
                            file_id,
                            path: class_name,
                            alias: None,
                            is_glob: true, // Globally visible
                            is_type_only: false,
                        });
                    }
                }
            }
            "call" => {
                // Check if this is a preload() call
                // Structure: call node has identifier child "preload" and arguments
                if let Some(identifier_node) = node.child(0) {
                    if identifier_node.kind() == "identifier" {
                        let func_name = self.text_for_node(code, identifier_node).trim();
                        if func_name == "preload" {
                            // Find arguments node
                            let mut cursor = node.walk();
                            for child in node.children(&mut cursor) {
                                if child.kind() == "arguments" {
                                    // Get first string argument
                                    if let Some(string_node) = child.named_child(0) {
                                        let mut path = self
                                            .text_for_node(code, string_node)
                                            .trim()
                                            .to_string();
                                        // Remove quotes
                                        if (path.starts_with('"') && path.ends_with('"'))
                                            || (path.starts_with('\'') && path.ends_with('\''))
                                        {
                                            path = path[1..path.len() - 1].to_string();
                                        }
                                        if !path.is_empty() {
                                            imports.push(Import {
                                                file_id,
                                                path,
                                                alias: None,
                                                is_glob: false,
                                                is_type_only: false,
                                            });
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.find_imports_in_node(child, code, file_id, imports);
        }
    }

    /// Remove wrapping quotes from string literals.
    fn strip_string_quotes<'a>(&self, value: &'a str) -> &'a str {
        let bytes = value.as_bytes();
        if bytes.len() >= 2 {
            let first = bytes[0];
            let last = bytes[bytes.len() - 1];
            if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
                return &value[1..value.len() - 1];
            }
        }
        value
    }

    /// Extract the signal name from an `emit_signal("name")` call.
    fn extract_signal_name<'a>(&self, call_node: Node, code: &'a str) -> Option<&'a str> {
        let arguments = call_node.child_by_field_name("arguments").or_else(|| {
            let mut cursor = call_node.walk();
            for child in call_node.children(&mut cursor) {
                if child.kind() == "arguments" {
                    return Some(child);
                }
            }
            None
        })?;
        let first_arg = arguments.named_child(0)?;
        let raw = self.text_for_node(code, first_arg).trim();
        let name = self.strip_string_quotes(raw);
        if name.is_empty() { None } else { Some(name) }
    }

    /// Extract path from `preload("res://path.gd")`.
    fn extract_preload_path<'a>(&self, call_node: Node, code: &'a str) -> Option<&'a str> {
        let callee = call_node.child(0)?;
        let callee_text = self.text_for_node(code, callee).trim();
        if callee_text != "preload" && !callee_text.ends_with(".preload") {
            return None;
        }

        let arguments = call_node.child_by_field_name("arguments").or_else(|| {
            let mut cursor = call_node.walk();
            for child in call_node.children(&mut cursor) {
                if child.kind() == "arguments" {
                    return Some(child);
                }
            }
            None
        })?;
        let first_arg = arguments.named_child(0)?;
        let raw = self.text_for_node(code, first_arg).trim();
        let path = self.strip_string_quotes(raw);
        if path.is_empty() { None } else { Some(path) }
    }

    fn collect_call_targets<'a>(&self, node: Node, code: &'a str) -> Vec<&'a str> {
        let mut targets = Vec::new();
        if let Some(callee) = node.child(0) {
            let name = self.text_for_node(code, callee).trim();
            if name.is_empty() {
                return targets;
            }

            if name == "preload" || name.ends_with(".preload") {
                return targets;
            }

            if name == "emit_signal" || name.ends_with(".emit_signal") {
                if let Some(signal) = self.extract_signal_name(node, code) {
                    targets.push(signal);
                } else {
                    targets.push(name);
                }
            } else {
                targets.push(name);
            }
        }
        targets
    }

    fn collect_calls<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: Option<&'a str>,
    ) {
        match node.kind() {
            "class_definition" => {
                self.register_node(&node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_calls(child, code, calls, None);
                }
                return;
            }
            "function_definition" => {
                self.register_node(&node);
                let next_function = node
                    .child_by_field_name("name")
                    .map(|n| self.text_for_node(code, n).trim())
                    .filter(|name| !name.is_empty())
                    .or(current_function);

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_calls(child, code, calls, next_function);
                }
                return;
            }
            "constructor_definition" => {
                self.register_node(&node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_calls(child, code, calls, Some("_init"));
                }
                return;
            }
            "call" => {
                self.register_node(&node);
                let caller = current_function.unwrap_or(SCRIPT_SCOPE);
                let range = self.node_to_range(node);
                for target in self.collect_call_targets(node, code) {
                    calls.push((caller, target, range));
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_calls(child, code, calls, current_function);
        }
    }

    fn collect_uses<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: Option<&'a str>,
        current_class: Option<&'a str>,
    ) {
        match node.kind() {
            "class_definition" => {
                self.register_node(&node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let class_name = self.text_for_node(code, name_node).trim();

                    if let Some(extends_node) = node.child_by_field_name("extends") {
                        if let Some(target) = extends_node.named_child(0) {
                            let base =
                                self.strip_string_quotes(self.text_for_node(code, target).trim());
                            if !base.is_empty() {
                                uses.push((class_name, base, self.node_to_range(extends_node)));
                            }
                        }
                    }

                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.collect_uses(child, code, uses, None, Some(class_name));
                    }
                } else {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.collect_uses(child, code, uses, current_function, current_class);
                    }
                }
                return;
            }
            "function_definition" => {
                self.register_node(&node);
                let next_function = node
                    .child_by_field_name("name")
                    .map(|n| self.text_for_node(code, n).trim())
                    .filter(|name| !name.is_empty())
                    .or(current_function);

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_uses(child, code, uses, next_function, current_class);
                }
                return;
            }
            "constructor_definition" => {
                self.register_node(&node);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_uses(child, code, uses, Some("_init"), current_class);
                }
                return;
            }
            "extends_statement" => {
                self.register_node(&node);
                if let Some(target) = node.named_child(0) {
                    let base = self.strip_string_quotes(self.text_for_node(code, target).trim());
                    if !base.is_empty() {
                        let derived = current_class.unwrap_or(SCRIPT_SCOPE);
                        uses.push((derived, base, self.node_to_range(node)));
                    }
                }
            }
            "const_statement" | "variable_statement" => {
                self.register_node(&node);

                let binding_name = node
                    .child_by_field_name("name")
                    .map(|n| self.text_for_node(code, n).trim())
                    .filter(|name| !name.is_empty())
                    .or_else(|| {
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.is_named() && child.kind() == "identifier" {
                                let text = self.text_for_node(code, child).trim();
                                if !text.is_empty() {
                                    return Some(text);
                                }
                            }
                        }
                        None
                    });

                let value_node = node.child_by_field_name("value").or_else(|| {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "call" {
                            return Some(child);
                        }
                    }
                    None
                });

                if let (Some(binding), Some(value_node)) = (binding_name, value_node) {
                    if let Some(path) = self.extract_preload_path(value_node, code) {
                        let owner = current_function.unwrap_or(binding);
                        uses.push((owner, path, self.node_to_range(value_node)));
                    }
                }
                return;
            }
            "call" => {
                self.register_node(&node);
                if let Some(path) = self.extract_preload_path(node, code) {
                    let source = current_function.unwrap_or(SCRIPT_SCOPE);
                    uses.push((source, path, self.node_to_range(node)));
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_uses(child, code, uses, current_function, current_class);
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
                    let derived = current_class.unwrap_or(SCRIPT_SCOPE);
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

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut calls = Vec::new();
        self.collect_calls(tree.root_node(), code, &mut calls, None);
        calls
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

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut uses = Vec::new();
        self.collect_uses(tree.root_node(), code, &mut uses, None, None);
        uses
    }

    fn find_defines<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut imports = Vec::new();

        self.find_imports_in_node(root_node, code, file_id, &mut imports);

        imports
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
