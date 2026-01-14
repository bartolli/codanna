//! Lua parser implementation
//!
//! Uses tree-sitter-lua crate's LANGUAGE constant for parsing Lua source code.

use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Import, LanguageParser, MethodCall, NodeTracker, NodeTrackingState, ParserContext,
    ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use tree_sitter::{Node, Parser, Tree};

/// Lua language parser
pub struct LuaParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

fn range_from_node(node: &Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range::new(
        start.row as u32 + 1,
        start.column as u16,
        end.row as u32 + 1,
        end.column as u16,
    )
}

impl LuaParser {
    /// Parse Lua source code and extract all symbols
    ///
    /// Handles function declarations (global and local), method definitions (colon syntax),
    /// variable declarations, table constructors, and field assignments.
    pub fn parse(
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

    /// Create a new Lua parser
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        let lang = tree_sitter_lua::LANGUAGE;
        parser
            .set_language(&lang.into())
            .map_err(|e| format!("Failed to set Lua language: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Extract symbols from a Lua AST node recursively
    ///
    /// Handles Lua-specific constructs:
    /// - Function declarations (global, local, and method syntax)
    /// - Variable declarations and assignments
    /// - Table constructors and field definitions
    /// - Control flow blocks (for, while, if, do)
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

        match node.kind() {
            "function_declaration" => {
                self.register_node_recursively(node);
                if let Some(symbol) =
                    self.process_function_declaration(node, code, file_id, counter, module_path)
                {
                    let func_name = symbol.name.to_string();
                    symbols.push(symbol);

                    self.context.enter_scope(ScopeType::hoisting_function());
                    let saved_function = self.context.current_function().map(|s| s.to_string());
                    self.context.set_current_function(Some(func_name));

                    if let Some(params) = node.child_by_field_name("parameters") {
                        self.process_parameters(
                            params,
                            code,
                            file_id,
                            counter,
                            symbols,
                            module_path,
                        );
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
                    self.context.set_current_function(saved_function);
                }
            }
            "function_definition" => {
                self.register_handled_node("function_definition", node.kind_id());
            }
            "variable_declaration" => {
                self.register_node_recursively(node);
                self.process_variable_declaration(
                    node,
                    code,
                    file_id,
                    counter,
                    symbols,
                    module_path,
                );
            }
            "assignment_statement" => {
                self.register_node_recursively(node);
                self.process_assignment(node, code, file_id, counter, symbols, module_path, depth);
            }
            "table_constructor" => {
                self.register_handled_node("table_constructor", node.kind_id());
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "field" {
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
            }
            "field" => {
                self.register_handled_node("field", node.kind_id());
                if let Some(symbol) =
                    self.process_table_field(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                }
            }
            "for_statement" | "while_statement" | "repeat_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.context.enter_scope(ScopeType::Block);
                for child in node.children(&mut node.walk()) {
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
                self.context.exit_scope();
            }
            "if_statement" => {
                self.register_handled_node("if_statement", node.kind_id());
                self.context.enter_scope(ScopeType::Block);
                for child in node.children(&mut node.walk()) {
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
                self.context.exit_scope();
            }
            "do_statement" => {
                self.register_handled_node("do_statement", node.kind_id());
                self.context.enter_scope(ScopeType::Block);
                for child in node.children(&mut node.walk()) {
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
                self.context.exit_scope();
            }
            "block" => {
                self.register_handled_node("block", node.kind_id());
                for child in node.children(&mut node.walk()) {
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
            "chunk" | "program" => {
                self.register_handled_node(node.kind(), node.kind_id());
                for child in node.children(&mut node.walk()) {
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
            "return_statement" | "break_statement" | "goto_statement" | "label_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
            }
            "function_call"
            | "method_index_expression"
            | "dot_index_expression"
            | "bracket_index_expression" => {
                self.register_handled_node(node.kind(), node.kind_id());
            }
            "comment" => {
                self.register_handled_node("comment", node.kind_id());
            }
            _ => {
                for child in node.children(&mut node.walk()) {
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
        }
    }

    fn process_function_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name_text = &code[name_node.byte_range()];

        let is_local = node
            .children(&mut node.walk())
            .any(|child| child.kind() == "local");

        let (name, kind, visibility) = if name_text.contains(':') {
            let parts: Vec<&str> = name_text.split(':').collect();
            let method_name = parts.last().unwrap_or(&name_text).to_string();
            let vis = if is_local {
                Visibility::Private
            } else {
                Visibility::Public
            };
            (method_name, SymbolKind::Method, vis)
        } else if name_text.contains('.') {
            let parts: Vec<&str> = name_text.split('.').collect();
            let func_name = parts.last().unwrap_or(&name_text).to_string();
            let vis = if is_local || func_name.starts_with('_') {
                Visibility::Private
            } else {
                Visibility::Public
            };
            (func_name, SymbolKind::Function, vis)
        } else {
            let vis = if is_local || name_text.starts_with('_') {
                Visibility::Private
            } else {
                Visibility::Public
            };
            (name_text.to_string(), SymbolKind::Function, vis)
        };

        let range = range_from_node(&node);
        let signature = if is_local {
            format!("local {}", self.extract_function_signature(node, code))
        } else {
            self.extract_function_signature(node, code)
        };
        let doc_comment = self.extract_lua_doc_comment(&node, code);

        Some(self.create_symbol(
            counter.next_id(),
            name,
            kind,
            file_id,
            range,
            Some(signature),
            doc_comment,
            module_path,
            visibility,
        ))
    }

    fn process_variable_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "assignment_statement" {
                for assign_child in child.children(&mut child.walk()) {
                    if assign_child.kind() == "variable_list" {
                        for var_child in assign_child.children(&mut assign_child.walk()) {
                            if var_child.kind() == "identifier" {
                                let name = code[var_child.byte_range()].to_string();
                                let range = range_from_node(&var_child);

                                let kind = if name.chars().all(|c| c.is_uppercase() || c == '_')
                                    && name.contains('_')
                                {
                                    SymbolKind::Constant
                                } else {
                                    SymbolKind::Variable
                                };

                                let signature = format!("local {name}");
                                let doc_comment = self.extract_lua_doc_comment(&node, code);

                                let symbol = self.create_symbol(
                                    counter.next_id(),
                                    name,
                                    kind,
                                    file_id,
                                    range,
                                    Some(signature),
                                    doc_comment,
                                    module_path,
                                    Visibility::Private,
                                );
                                symbols.push(symbol);
                            }
                        }
                    }
                }
            }
        }
    }

    fn process_assignment(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        depth: usize,
    ) {
        let mut has_function_value = false;
        for child in node.children(&mut node.walk()) {
            if child.kind() == "expression_list" {
                for expr_child in child.children(&mut child.walk()) {
                    if expr_child.kind() == "function_definition" {
                        has_function_value = true;
                        break;
                    }
                }
            }
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "variable_list" {
                for var_child in child.children(&mut child.walk()) {
                    match var_child.kind() {
                        "identifier" => {
                            let name = code[var_child.byte_range()].to_string();

                            if self.context.current_function().is_some() {
                                continue;
                            }

                            let range = range_from_node(&var_child);
                            let kind = if has_function_value {
                                SymbolKind::Function
                            } else if name.chars().all(|c| c.is_uppercase() || c == '_')
                                && name.contains('_')
                            {
                                SymbolKind::Constant
                            } else {
                                SymbolKind::Variable
                            };

                            let visibility = if name.starts_with('_') {
                                Visibility::Private
                            } else {
                                Visibility::Public
                            };

                            let doc_comment = self.extract_lua_doc_comment(&node, code);

                            let symbol = self.create_symbol(
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
                            symbols.push(symbol);
                        }
                        "dot_index_expression" => {
                            self.process_dot_index_assignment(
                                var_child,
                                node,
                                code,
                                file_id,
                                counter,
                                symbols,
                                module_path,
                                has_function_value,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "expression_list" {
                for expr_child in child.children(&mut child.walk()) {
                    if expr_child.kind() == "table_constructor" {
                        self.extract_symbols_from_node(
                            expr_child,
                            code,
                            file_id,
                            counter,
                            symbols,
                            module_path,
                            depth + 1,
                        );
                    }
                }
            }
        }
    }

    fn process_dot_index_assignment(
        &mut self,
        node: Node,
        parent_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        is_function: bool,
    ) {
        if let Some(field_node) = node.child_by_field_name("field") {
            let field_name = code[field_node.byte_range()].to_string();
            let range = range_from_node(&node);

            let kind = if is_function {
                SymbolKind::Function
            } else if field_name.chars().all(|c| c.is_uppercase() || c == '_')
                && field_name.contains('_')
            {
                SymbolKind::Constant
            } else {
                SymbolKind::Field
            };

            let visibility = if field_name.starts_with('_') {
                Visibility::Private
            } else {
                Visibility::Public
            };

            let signature = code[node.byte_range()].to_string();
            let doc_comment = self.extract_lua_doc_comment(&parent_node, code);

            let symbol = self.create_symbol(
                counter.next_id(),
                field_name,
                kind,
                file_id,
                range,
                Some(signature),
                doc_comment,
                module_path,
                visibility,
            );
            symbols.push(symbol);
        }
    }

    fn process_table_field(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = code[name_node.byte_range()].to_string();
            let range = range_from_node(&node);

            let mut is_function = false;
            if let Some(value_node) = node.child_by_field_name("value") {
                is_function = value_node.kind() == "function_definition";
            }

            let kind = if is_function {
                SymbolKind::Method
            } else {
                SymbolKind::Field
            };

            let visibility = if name.starts_with('_') {
                Visibility::Private
            } else {
                Visibility::Public
            };

            let signature = code[node.byte_range()].to_string();

            return Some(self.create_symbol(
                counter.next_id(),
                name,
                kind,
                file_id,
                range,
                Some(signature),
                None,
                module_path,
                visibility,
            ));
        }

        None
    }

    fn process_parameters(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "identifier" {
                let name = code[child.byte_range()].to_string();
                let range = range_from_node(&child);

                let symbol = self.create_symbol(
                    counter.next_id(),
                    name.clone(),
                    SymbolKind::Parameter,
                    file_id,
                    range,
                    Some(name),
                    None,
                    module_path,
                    Visibility::Private,
                );
                symbols.push(symbol);
            }
        }
    }

    fn extract_function_signature(&self, node: Node, code: &str) -> String {
        let mut sig = String::from("function");

        if let Some(name_node) = node.child_by_field_name("name") {
            sig.push(' ');
            sig.push_str(&code[name_node.byte_range()]);
        }

        if let Some(params_node) = node.child_by_field_name("parameters") {
            sig.push_str(&code[params_node.byte_range()]);
        }

        sig
    }

    fn extract_lua_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut current = node.prev_sibling();

        while let Some(sibling) = current {
            if sibling.kind() == "comment" {
                let comment_text = &code[sibling.byte_range()];

                if comment_text.starts_with("---") {
                    let content = comment_text.trim_start_matches("---").trim();
                    doc_lines.insert(0, content.to_string());
                    current = sibling.prev_sibling();
                } else if comment_text.starts_with("--") && !comment_text.starts_with("--[[") {
                    let content = comment_text.trim_start_matches("--").trim();
                    doc_lines.insert(0, content.to_string());
                    current = sibling.prev_sibling();
                } else if comment_text.starts_with("--[[") {
                    let content = comment_text
                        .trim_start_matches("--[[")
                        .trim_end_matches("]]")
                        .trim();
                    doc_lines.insert(0, content.to_string());
                    break;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if !doc_lines.is_empty() {
            let filtered: Vec<String> = doc_lines.into_iter().filter(|l| !l.is_empty()).collect();
            if !filtered.is_empty() {
                return Some(filtered.join("\n"));
            }
        }

        None
    }

    fn extract_method_calls_from_tree(&self, tree: &Tree, code: &str) -> Vec<MethodCall> {
        let mut calls = Vec::new();
        extract_method_calls_recursive(&tree.root_node(), code, &mut calls);
        calls
    }
}

fn extract_method_calls_recursive(node: &Node, code: &str, calls: &mut Vec<MethodCall>) {
    if node.kind() == "function_call" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if name_node.kind() == "method_index_expression" {
                if let Some(method_node) = name_node.child_by_field_name("method") {
                    let method_name = code[method_node.byte_range()].to_string();
                    let range = range_from_node(node);

                    let receiver = name_node
                        .child_by_field_name("table")
                        .map(|n| code[n.byte_range()].to_string());

                    calls.push(MethodCall {
                        caller: String::new(),
                        method_name,
                        receiver,
                        is_static: false,
                        range,
                        caller_range: Some(range),
                    });
                }
            }
        }
    }

    for child in node.children(&mut node.walk()) {
        extract_method_calls_recursive(&child, code, calls);
    }
}

fn extract_imports_recursive(node: &Node, code: &str, file_id: FileId, imports: &mut Vec<Import>) {
    // Look for variable_declaration containing require() calls
    // Pattern: local foo = require("module")
    if node.kind() == "variable_declaration" {
        let mut alias: Option<String> = None;
        let mut require_call: Option<Node> = None;

        for child in node.children(&mut node.walk()) {
            if child.kind() == "assignment_statement" {
                for assign_child in child.children(&mut child.walk()) {
                    if assign_child.kind() == "variable_list" {
                        // Get the variable name (alias)
                        for var_child in assign_child.children(&mut assign_child.walk()) {
                            if var_child.kind() == "identifier" {
                                alias = Some(code[var_child.byte_range()].to_string());
                                break;
                            }
                        }
                    } else if assign_child.kind() == "expression_list" {
                        // Check if value is a require() call
                        for expr_child in assign_child.children(&mut assign_child.walk()) {
                            if expr_child.kind() == "function_call" {
                                require_call = Some(expr_child);
                                break;
                            }
                        }
                    }
                }
            }
        }

        if let Some(call_node) = require_call {
            if let Some(import) = try_extract_require_call(&call_node, code, file_id, alias) {
                imports.push(import);
                return; // Don't recurse into this node again
            }
        }
    }

    // Also check for standalone require() calls (without assignment)
    if node.kind() == "function_call" {
        if let Some(import) = try_extract_require_call(node, code, file_id, None) {
            imports.push(import);
            return; // Found a require call, don't recurse
        }
    }

    for child in node.children(&mut node.walk()) {
        extract_imports_recursive(&child, code, file_id, imports);
    }
}

fn try_extract_require_call(
    node: &Node,
    code: &str,
    file_id: FileId,
    alias: Option<String>,
) -> Option<Import> {
    if node.kind() != "function_call" {
        return None;
    }

    let name_node = node.child_by_field_name("name")?;
    let func_name = &code[name_node.byte_range()];

    if func_name != "require" {
        return None;
    }

    let args_node = node.child_by_field_name("arguments")?;

    // Find the string argument inside the arguments
    for arg_child in args_node.children(&mut args_node.walk()) {
        if arg_child.kind() == "string" {
            // Extract string content (remove quotes)
            let full_string = &code[arg_child.byte_range()];
            let module_path = full_string
                .trim_start_matches('"')
                .trim_start_matches('\'')
                .trim_end_matches('"')
                .trim_end_matches('\'')
                .to_string();

            if !module_path.is_empty() {
                return Some(Import {
                    path: module_path,
                    alias,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
        }
    }

    None
}

impl NodeTracker for LuaParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LuaParser {
    fn register_node_recursively(&mut self, node: Node) {
        self.node_tracker
            .register_handled_node(node.kind(), node.kind_id());
        for child in node.children(&mut node.walk()) {
            self.register_node_recursively(child);
        }
    }
}

impl LanguageParser for LuaParser {
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        self.parse(code, file_id, symbol_counter)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.extract_lua_doc_comment(node, code)
    }

    fn find_calls<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    /// Extract method calls from Lua source code
    ///
    /// Returns MethodCall structs for colon-syntax method invocations (obj:method()).
    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        self.extract_method_calls_from_tree(&tree, code)
    }

    /// Lua uses duck typing - no explicit interface implementations
    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    /// Lua uses metatables for inheritance - no explicit extends declarations
    fn find_extends<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_uses<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_defines<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    /// Extract require() imports from Lua source code
    ///
    /// Parses patterns like:
    /// - `local foo = require("path.to.module")`
    /// - `local bar = require('module')`
    /// - `require("module")` (without assignment)
    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut imports = Vec::new();
        extract_imports_recursive(&tree.root_node(), code, file_id, &mut imports);
        imports
    }

    fn language(&self) -> crate::parsing::Language {
        crate::parsing::Language::Lua
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_global_function() {
        let mut parser = LuaParser::new().unwrap();
        let code = r#"
function hello(name)
    print("Hello, " .. name)
end
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(!symbols.is_empty());
        let func = symbols.iter().find(|s| s.name.as_ref() == "hello");
        assert!(func.is_some());
        assert_eq!(func.unwrap().kind, SymbolKind::Function);
    }

    #[test]
    fn test_parse_local_function() {
        let mut parser = LuaParser::new().unwrap();
        let code = r#"
local function helper()
    return 42
end
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let func = symbols.iter().find(|s| s.name.as_ref() == "helper");
        assert!(func.is_some());
        assert_eq!(func.unwrap().visibility, Visibility::Private);
    }

    #[test]
    fn test_parse_local_variable() {
        let mut parser = LuaParser::new().unwrap();
        let code = r#"
local counter = 0
local MAX_VALUE = 100
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter_sym = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter_sym);

        let var = symbols.iter().find(|s| s.name.as_ref() == "counter");
        assert!(var.is_some());
        assert_eq!(var.unwrap().kind, SymbolKind::Variable);

        let const_sym = symbols.iter().find(|s| s.name.as_ref() == "MAX_VALUE");
        assert!(const_sym.is_some());
        assert_eq!(const_sym.unwrap().kind, SymbolKind::Constant);
    }

    #[test]
    fn test_parse_method() {
        let mut parser = LuaParser::new().unwrap();
        let code = r#"
function MyClass:greet()
    return "Hello"
end
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let method = symbols.iter().find(|s| s.name.as_ref() == "greet");
        assert!(method.is_some());
        assert_eq!(method.unwrap().kind, SymbolKind::Method);
    }

    #[test]
    fn test_find_imports_with_alias() {
        use crate::parsing::LanguageParser;

        let mut parser = LuaParser::new().unwrap();
        let code = r#"
local json = require("cjson")
local utils = require("myapp.utils")
"#;

        let file_id = FileId::new(1).unwrap();
        let imports = parser.find_imports(code, file_id);

        assert_eq!(imports.len(), 2);

        let json_import = imports.iter().find(|i| i.path == "cjson");
        assert!(json_import.is_some());
        assert_eq!(json_import.unwrap().alias, Some("json".to_string()));

        let utils_import = imports.iter().find(|i| i.path == "myapp.utils");
        assert!(utils_import.is_some());
        assert_eq!(utils_import.unwrap().alias, Some("utils".to_string()));
    }

    #[test]
    fn test_find_imports_single_quotes() {
        use crate::parsing::LanguageParser;

        let mut parser = LuaParser::new().unwrap();
        let code = r#"
local foo = require('single.quoted')
"#;

        let file_id = FileId::new(1).unwrap();
        let imports = parser.find_imports(code, file_id);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "single.quoted");
        assert_eq!(imports[0].alias, Some("foo".to_string()));
    }

    #[test]
    fn test_find_imports_standalone() {
        use crate::parsing::LanguageParser;

        let mut parser = LuaParser::new().unwrap();
        let code = r#"
require("some.module")
"#;

        let file_id = FileId::new(1).unwrap();
        let imports = parser.find_imports(code, file_id);

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "some.module");
        assert_eq!(imports[0].alias, None);
    }
}
