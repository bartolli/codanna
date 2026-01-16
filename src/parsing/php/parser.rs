//! PHP language parser implementation
//!
//! This parser provides PHP language support for the codebase intelligence system.
//! It extracts symbols, relationships, and documentation from PHP source code using
//! tree-sitter for AST parsing.
//!
//! **Tree-sitter ABI Version**: ABI-14 (tree-sitter-php 0.23.4)
//!
//! Note: This parser uses ABI-14 (same as Python). When upgrading the tree-sitter-php
//! version, verify compatibility with node type names used in this implementation.

use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    Language, LanguageParser, MethodCall, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind};
use std::any::Any;
use thiserror::Error;
use tree_sitter::{Node, Parser};

/// PHP-specific parsing errors
#[derive(Error, Debug)]
pub enum PhpParseError {
    #[error(
        "Failed to initialize PHP parser: {reason}\nSuggestion: Ensure tree-sitter-php is properly installed and the version matches Cargo.toml"
    )]
    ParserInitFailed { reason: String },

    #[error(
        "Invalid PHP syntax at {location:?}: {details}\nSuggestion: Check for missing semicolons, unclosed brackets, or incorrect PHP tags"
    )]
    SyntaxError { location: Range, details: String },

    #[error(
        "Failed to parse type annotation: {annotation}\nSuggestion: Ensure type annotations follow PHP 7+ syntax (e.g., string, int, ?string, array)"
    )]
    InvalidTypeAnnotation { annotation: String },

    #[error(
        "Unsupported PHP feature at {location:?}: {feature}\nSuggestion: This parser currently supports PHP 7.0+ syntax. Consider simplifying the code or file an issue"
    )]
    UnsupportedFeature { feature: String, location: Range },
}

/// PHP language parser
pub struct PhpParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for PhpParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhpParser")
            .field("language", &"PHP")
            .finish()
    }
}

impl PhpParser {
    /// Parse PHP source code and extract all symbols
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        // Reset context for each file
        self.context = ParserContext::new();

        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut symbols = Vec::with_capacity(64); // Reasonable initial capacity for most PHP files
        self.extract_symbols_from_node(
            tree.root_node(),
            code,
            file_id,
            &mut symbols,
            symbol_counter,
            0,
        );
        symbols
    }

    /// Create a new PHP parser instance
    pub fn new() -> Result<Self, PhpParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .map_err(|e| PhpParseError::ParserInitFailed {
                reason: format!("tree-sitter error: {e}"),
            })?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Extract function name from function_definition node
    fn extract_function_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|n| &code[n.byte_range()])
    }

    /// Extract method name from method_declaration node
    fn extract_method_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|n| &code[n.byte_range()])
    }

    /// Extract class name from class_declaration node
    fn extract_class_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|n| &code[n.byte_range()])
    }

    /// Extract trait name from trait_declaration node
    fn extract_trait_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|n| &code[n.byte_range()])
    }

    #[cfg(test)]
    fn debug_parse(&mut self, code: &str) {
        let tree = self.parser.parse(code, None).unwrap();
        let root = tree.root_node();
        eprintln!("=== PHP Parse Debug ===");
        self.debug_node(root, code, 0);
    }

    #[cfg(test)]
    #[allow(clippy::only_used_in_recursion)]
    fn debug_node(&self, node: Node, code: &str, indent: usize) {
        let indent_str = "  ".repeat(indent);
        let text_preview = if node.child_count() == 0 {
            let text = &code[node.byte_range()];
            if text.len() > 50 {
                let truncated = crate::parsing::safe_truncate_str(text, 50);
                format!(" = '{truncated}'...")
            } else {
                format!(" = '{text}'")
            }
        } else {
            String::new()
        };

        eprintln!("{}{}{}", indent_str, node.kind(), text_preview);

        // Show interesting nodes in detail
        if matches!(
            node.kind(),
            "const_declaration" | "const_element" | "expression_statement"
        ) {
            eprintln!(
                "{}  ^^ Range: {:?}, Parent: {:?}",
                indent_str,
                node.byte_range(),
                node.parent().map(|p| p.kind())
            );
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.debug_node(child, code, indent + 1);
        }
    }

    /// Convert tree-sitter node to Range
    fn node_to_range(&self, node: Node) -> Range {
        let start_pos = node.start_position();
        let end_pos = node.end_position();
        Range {
            start_line: start_pos.row as u32,
            start_column: start_pos.column as u16,
            end_line: end_pos.row as u32,
            end_column: end_pos.column as u16,
        }
    }

    /// Extract symbols from AST node recursively
    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }
        match node.kind() {
            "function_definition" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract function name for parent tracking
                let func_name = self.extract_function_name(node, code);

                if let Some(symbol) = self.process_function(node, code, file_id, counter) {
                    symbols.push(symbol);
                }

                // Enter function scope for nested items
                self.context
                    .enter_scope(ScopeType::Function { hoisting: false });

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current function for parent tracking
                if let Some(name) = func_name {
                    self.context.set_current_function(Some(name.to_string()));
                }

                // Process children to find nested functions
                self.process_children(node, code, file_id, symbols, counter, depth);

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "method_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract method name for parent tracking
                let method_name = self.extract_method_name(node, code);

                if let Some(symbol) = self.process_method(node, code, file_id, counter) {
                    symbols.push(symbol);
                }

                // Enter function scope for method body
                self.context
                    .enter_scope(ScopeType::Function { hoisting: false });

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current function to the method name
                if let Some(name) = method_name {
                    self.context.set_current_function(Some(name.to_string()));
                }

                // Process children for nested elements
                self.process_children(node, code, file_id, symbols, counter, depth);

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "class_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract class name for parent tracking
                let class_name = self.extract_class_name(node, code);

                if let Some(symbol) = self.process_class(node, code, file_id, counter) {
                    symbols.push(symbol);
                }

                // Enter class scope for methods and properties
                self.context.enter_scope(ScopeType::Class);

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current class for parent tracking
                if let Some(name) = class_name {
                    self.context.set_current_class(Some(name.to_string()));
                }

                // Continue processing children to find methods inside the class
                self.process_children(node, code, file_id, symbols, counter, depth);

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "interface_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(symbol) = self.process_interface(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                // Enter interface scope (like class)
                self.context.enter_scope(ScopeType::Class);
                // Process children for interface methods
                self.process_children(node, code, file_id, symbols, counter, depth);
                self.context.exit_scope();
            }
            "trait_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract trait name for parent tracking
                let trait_name = self.extract_trait_name(node, code);

                if let Some(symbol) = self.process_trait(node, code, file_id, counter) {
                    symbols.push(symbol);
                }

                // Enter trait scope (like class)
                self.context.enter_scope(ScopeType::Class);

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current class to the trait name for parent tracking
                if let Some(name) = trait_name {
                    self.context.set_current_class(Some(name.to_string()));
                }

                // Process children for trait methods
                self.process_children(node, code, file_id, symbols, counter, depth);

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "property_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(symbol) = self.process_property(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "const_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Process const declarations - they contain const_element children
                // Process children to extract the const_elements
                self.process_children(node, code, file_id, symbols, counter, depth);
            }
            "const_element" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Process individual const elements
                // Check if we're at global scope (not inside a class)
                if self.is_global_scope(node) {
                    // The first child is the name, third child is the value
                    if let Some(name_node) = node.child(0) {
                        if name_node.kind() == "name" {
                            let name = &code[name_node.byte_range()];
                            let id = counter.next_id();

                            let mut symbol = Symbol::new(
                                id,
                                name,
                                SymbolKind::Constant,
                                file_id,
                                self.node_to_range(node),
                            );

                            // Set scope context
                            symbol.scope_context = Some(self.context.current_scope_context());

                            // Try to get the value (third child after name and =)
                            if let Some(value_node) = node.child(2) {
                                let value = &code[value_node.byte_range()];
                                symbol.signature = Some(format!("const {name} = {value}").into());
                            }

                            symbol.doc_comment =
                                self.extract_doc_comment(&node, code).map(Into::into);
                            symbols.push(symbol);
                        }
                    }
                } else {
                    // This is a class constant, handled elsewhere
                }
            }
            "class_const_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(symbol) = self.process_constant(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "expression_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Check for global constants via define() or global variables
                if let Some(child) = node.child(0) {
                    match child.kind() {
                        "function_call_expression" => {
                            // Check if it's a define() call
                            if self.is_define_call(child, code) && self.is_global_scope(node) {
                                if let Some(symbol) =
                                    self.process_define(child, code, file_id, counter)
                                {
                                    symbols.push(symbol);
                                }
                            }
                        }
                        "assignment_expression" => {
                            // Global variable assignment
                            if self.is_global_scope(node) {
                                if let Some(symbol) =
                                    self.process_global_assignment(child, code, file_id, counter)
                                {
                                    symbols.push(symbol);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // Continue processing children
                self.process_children(node, code, file_id, symbols, counter, depth);
            }
            _ => {
                // Track all nodes we encounter, even if not extracting symbols
                self.register_handled_node(node.kind(), node.kind_id());
                // Recursively process children
                self.process_children(node, code, file_id, symbols, counter, depth);
            }
        }
    }

    /// Extract function signature from a node, excluding the body
    fn extract_function_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract method signature from a node, excluding the body
    fn extract_method_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract class signature including extends/implements
    fn extract_class_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract trait signature
    fn extract_trait_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract interface signature
    fn extract_interface_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Process a function definition node
    fn process_function(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let id = counter.next_id();

        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Function,
            file_id,
            self.node_to_range(node),
        );
        // Set scope context
        symbol.scope_context = Some(self.context.current_scope_context());
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);

        // Extract and add function signature
        let signature = self.extract_function_signature(node, code);
        symbol.signature = Some(signature.into());

        Some(symbol)
    }

    /// Process a method declaration node
    fn process_method(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let id = counter.next_id();

        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Method,
            file_id,
            self.node_to_range(node),
        );
        // Set scope context
        symbol.scope_context = Some(self.context.current_scope_context());
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);

        // Extract and add method signature
        let signature = self.extract_method_signature(node, code);
        symbol.signature = Some(signature.into());

        Some(symbol)
    }

    /// Process a class declaration node
    fn process_class(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let id = counter.next_id();

        // Using Class for PHP classes
        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Class,
            file_id,
            self.node_to_range(node),
        );
        // Set scope context
        symbol.scope_context = Some(self.context.current_scope_context());
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);

        // Extract and add class signature
        let signature = self.extract_class_signature(node, code);
        symbol.signature = Some(signature.into());

        Some(symbol)
    }

    /// Process an interface declaration node
    fn process_interface(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let id = counter.next_id();

        // Using Interface for PHP interfaces
        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Interface,
            file_id,
            self.node_to_range(node),
        );
        // Set scope context
        symbol.scope_context = Some(self.context.current_scope_context());
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);

        // Extract and add interface signature
        let signature = self.extract_interface_signature(node, code);
        symbol.signature = Some(signature.into());

        Some(symbol)
    }

    /// Process a trait declaration node
    fn process_trait(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let id = counter.next_id();

        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Trait,
            file_id,
            self.node_to_range(node),
        );
        // Set scope context
        symbol.scope_context = Some(self.context.current_scope_context());
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);

        // Extract and add trait signature
        let signature = self.extract_trait_signature(node, code);
        symbol.signature = Some(signature.into());

        Some(symbol)
    }

    /// Process a property declaration node
    fn process_property(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Find the property element within the declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "property_element" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    // Remove $ prefix from property name if present
                    let clean_name = name.strip_prefix('$').unwrap_or(name);

                    let id = counter.next_id();

                    let mut symbol = Symbol::new(
                        id,
                        clean_name,
                        SymbolKind::Field,
                        file_id,
                        self.node_to_range(node),
                    );
                    // Set scope context
                    symbol.scope_context = Some(self.context.current_scope_context());
                    symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);
                    return Some(symbol);
                }
            }
        }
        None
    }

    /// Process a constant declaration node
    fn process_constant(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Find the const element within the declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "const_element" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];

                    let id = counter.next_id();

                    let mut symbol = Symbol::new(
                        id,
                        name,
                        SymbolKind::Constant,
                        file_id,
                        self.node_to_range(node),
                    );
                    symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);
                    return Some(symbol);
                }
            }
        }
        None
    }

    /// Process children nodes recursively
    fn process_children(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
        }
    }

    /// Check if a node is at global scope (not inside a class, function, etc.)
    fn is_global_scope(&self, node: Node) -> bool {
        let mut parent = node.parent();
        while let Some(p) = parent {
            match p.kind() {
                "class_declaration"
                | "function_definition"
                | "method_declaration"
                | "interface_declaration"
                | "trait_declaration" => return false,
                "program" => return true,
                _ => parent = p.parent(),
            }
        }
        true
    }

    /// Check if a function call is a define() call
    fn is_define_call(&self, node: Node, code: &str) -> bool {
        if let Some(function_node) = node.child_by_field_name("function") {
            let function_name = &code[function_node.byte_range()];
            function_name == "define"
        } else {
            false
        }
    }

    /// Process a define() function call to extract a constant
    fn process_define(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Get the arguments of the define() call
        let arguments = node.child_by_field_name("arguments")?;

        // Find the first argument (constant name)
        let mut cursor = arguments.walk();
        let mut arg_count = 0;
        let mut name_str = String::new();
        let mut value_str = String::new();

        for child in arguments.children(&mut cursor) {
            if child.kind() == "argument" {
                if let Some(arg_child) = child.child(0) {
                    let arg_text = &code[arg_child.byte_range()];
                    if arg_count == 0 {
                        // First argument is the constant name (remove quotes)
                        name_str = arg_text.trim_matches('"').trim_matches('\'').to_string();
                    } else if arg_count == 1 {
                        // Second argument is the value
                        value_str = arg_text.to_string();
                    }
                    arg_count += 1;
                }
            }
        }

        if name_str.is_empty() {
            return None;
        }

        let id = counter.next_id();
        let mut symbol = Symbol::new(
            id,
            name_str.as_str(),
            SymbolKind::Constant,
            file_id,
            self.node_to_range(node),
        );

        // Set scope context
        symbol.scope_context = Some(self.context.current_scope_context());

        // Add signature with value if available
        if !value_str.is_empty() {
            symbol.signature = Some(format!("define('{name_str}', {value_str})").into());
        }

        symbol.doc_comment = self.extract_doc_comment(&node, code).map(Into::into);
        Some(symbol)
    }

    /// Process a global variable assignment
    fn process_global_assignment(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Get the left side of the assignment (variable name)
        let left = node.child_by_field_name("left")?;

        // Handle simple variable assignments
        if left.kind() == "variable_name" {
            let name = &code[left.byte_range()];
            // Remove the $ prefix for the symbol name
            let clean_name = name.strip_prefix('$').unwrap_or(name);

            let id = counter.next_id();
            let mut symbol = Symbol::new(
                id,
                clean_name,
                SymbolKind::Variable,
                file_id,
                self.node_to_range(node),
            );

            // Set scope context
            symbol.scope_context = Some(self.context.current_scope_context());

            // Try to extract the value as a simple signature
            if let Some(right) = node.child_by_field_name("right") {
                let value_preview = &code[right.byte_range()];
                // Store full signature for semantic quality
                symbol.signature = Some(format!("${clean_name} = {value_preview}").into());
            }

            return Some(symbol);
        }

        None
    }
}

impl NodeTracker for PhpParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<crate::parsing::HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for PhpParser {
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
        // Look for a comment node immediately before this node
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let comment_text = &code[prev.byte_range()];
                // PHP doc comments start with /** or //
                if comment_text.starts_with("/**") {
                    // Remove /** and */ and clean up
                    let cleaned = comment_text
                        .strip_prefix("/**")
                        .and_then(|s| s.strip_suffix("*/"))
                        .map(|s| {
                            s.lines()
                                .map(|line| line.trim().trim_start_matches('*').trim())
                                .filter(|line| !line.is_empty())
                                .collect::<Vec<_>>()
                                .join("\n")
                        });
                    return cleaned;
                } else if comment_text.starts_with("//") {
                    // Single line comment
                    return Some(
                        comment_text
                            .strip_prefix("//")
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    );
                }
            }
        }
        None
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut calls = Vec::with_capacity(32); // Typical function has <32 calls
        self.extract_calls_from_node(tree.root_node(), code, None, &mut calls);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        self.find_calls(code)
            .into_iter()
            .map(|(caller, target, range)| MethodCall::new(caller, target, range))
            .collect()
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut implementations = Vec::with_capacity(8); // Most classes implement few interfaces
        self.extract_implementations_from_node(tree.root_node(), code, &mut implementations);
        implementations
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut uses = Vec::with_capacity(16); // Typical number of type uses
        self.extract_uses_from_node(tree.root_node(), code, None, &mut uses);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut defines = Vec::with_capacity(16); // Typical class has <16 methods
        self.extract_defines_from_node(tree.root_node(), code, &mut defines);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut imports = Vec::with_capacity(16); // Typical file has <16 imports
        Self::extract_imports_from_node(tree.root_node(), code, file_id, &mut imports);
        imports
    }

    fn language(&self) -> Language {
        Language::Php
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut variable_types = Vec::with_capacity(16); // Typical function parameters
        self.extract_variable_types_from_node(tree.root_node(), code, &mut variable_types);
        variable_types
    }
}

// Helper methods for PhpParser
impl PhpParser {
    fn extract_calls_from_node<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        current_context: Option<&'a str>,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "function_call_expression" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(function_node) = node.child_by_field_name("function") {
                    let function_name = &code[function_node.byte_range()];
                    let range = self.node_to_range(node);
                    if let Some(context) = current_context {
                        calls.push((context, function_name, range));
                    }
                }
                // Also recurse to find nested calls like transform(getData())
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_calls_from_node(child, code, current_context, calls);
                }
            }
            "member_call_expression" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Method calls like $obj->method() or $this->method()
                // Extract the method name from the "name" field
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = &code[name_node.byte_range()];
                    let range = self.node_to_range(node);
                    if let Some(context) = current_context {
                        calls.push((context, method_name, range));
                    }
                }
                // Also recurse to find nested calls in arguments
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_calls_from_node(child, code, current_context, calls);
                }
            }
            "scoped_call_expression" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Static method calls like ClassName::method() or self::method()
                // Extract the method name from the "name" field
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = &code[name_node.byte_range()];
                    let range = self.node_to_range(node);
                    if let Some(context) = current_context {
                        calls.push((context, method_name, range));
                    }
                }
                // Also recurse to find nested calls in arguments
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_calls_from_node(child, code, current_context, calls);
                }
            }
            "function_definition" | "method_declaration" => {
                let new_context = node
                    .child_by_field_name("name")
                    .map(|name_node| &code[name_node.byte_range()])
                    .or(current_context);

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_calls_from_node(child, code, new_context, calls);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_calls_from_node(child, code, current_context, calls);
                }
            }
        }
    }

    fn extract_implementations_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if node.kind() == "class_declaration" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let class_name = &code[name_node.byte_range()];

                // Check for implements clause
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "base_clause" {
                        let mut base_cursor = child.walk();
                        for base_child in child.children(&mut base_cursor) {
                            if base_child.kind() == "name" {
                                let interface_name = &code[base_child.byte_range()];
                                let range = self.node_to_range(base_child);
                                implementations.push((class_name, interface_name, range));
                            }
                        }
                    }
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_implementations_from_node(child, code, implementations);
        }
    }

    fn extract_uses_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        current_context: Option<&'a str>,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "typed_property_declaration" | "parameter_declaration" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    let type_name = &code[type_node.byte_range()];
                    let range = self.node_to_range(type_node);
                    if let Some(context) = current_context {
                        uses.push((context, type_name, range));
                    }
                }
            }
            "function_definition" | "method_declaration" => {
                let new_context = node
                    .child_by_field_name("name")
                    .map(|name_node| &code[name_node.byte_range()])
                    .or(current_context);

                // Check return type
                if let Some(return_type) = node.child_by_field_name("return_type") {
                    let type_name = &code[return_type.byte_range()];
                    let range = self.node_to_range(return_type);
                    if let Some(context) = new_context {
                        uses.push((context, type_name, range));
                    }
                }

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_uses_from_node(child, code, new_context, uses);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_uses_from_node(child, code, current_context, uses);
                }
            }
        }
    }

    fn extract_defines_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "class_declaration" | "interface_declaration" | "trait_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let type_name = &code[name_node.byte_range()];

                    // Find methods within the type - they're inside declaration_list
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "declaration_list" {
                            // Methods are inside declaration_list, not direct children
                            let mut decl_cursor = child.walk();
                            for decl_child in child.children(&mut decl_cursor) {
                                if decl_child.kind() == "method_declaration" {
                                    if let Some(method_name_node) =
                                        decl_child.child_by_field_name("name")
                                    {
                                        let method_name = &code[method_name_node.byte_range()];
                                        let range = self.node_to_range(method_name_node);
                                        defines.push((type_name, method_name, range));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_defines_from_node(child, code, defines);
        }
    }

    fn extract_imports_from_node(
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if node.kind() == "namespace_use_declaration" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "namespace_use_clause" {
                    let mut path = String::new();
                    let mut alias = None;

                    let mut clause_cursor = child.walk();
                    for clause_child in child.children(&mut clause_cursor) {
                        match clause_child.kind() {
                            "qualified_name" => {
                                path = code[clause_child.byte_range()].to_string();
                            }
                            "namespace_aliasing_clause" => {
                                if let Some(alias_node) = clause_child.child(1) {
                                    alias = Some(code[alias_node.byte_range()].to_string());
                                }
                            }
                            _ => {}
                        }
                    }

                    if !path.is_empty() {
                        imports.push(Import {
                            path,
                            alias,
                            is_glob: false,
                            file_id,
                            is_type_only: false,
                        });
                    }
                }
            }
        }

        // Also handle require/include statements
        if matches!(
            node.kind(),
            "require_expression"
                | "require_once_expression"
                | "include_expression"
                | "include_once_expression"
        ) {
            if let Some(argument) = node.child(1) {
                if argument.kind() == "string" {
                    let path = code[argument.byte_range()]
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    imports.push(Import {
                        path,
                        alias: None,
                        is_glob: false,
                        file_id,
                        is_type_only: false,
                    });
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::extract_imports_from_node(child, code, file_id, imports);
        }
    }

    fn extract_variable_types_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        variable_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if node.kind() == "simple_parameter" {
            let mut type_name = None;
            let mut var_name = None;

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "type_list" | "named_type" | "primitive_type" => {
                        type_name = Some(&code[child.byte_range()]);
                    }
                    "variable_name" => {
                        let raw_name = &code[child.byte_range()];
                        var_name = Some(raw_name.trim_start_matches('$'));
                    }
                    _ => {}
                }
            }

            if let (Some(var), Some(typ)) = (var_name, type_name) {
                let range = self.node_to_range(node);
                variable_types.push((var, typ, range));
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_variable_types_from_node(child, code, variable_types);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_php_const_declarations() {
        let code = r#"<?php
const DEFAULT_TIMEOUT = 30;
const CACHE_TTL = 3600;
define('MAX_CONNECTIONS', 100);
$globalVar = 'test';
"#;

        let mut parser = PhpParser::new().unwrap();
        parser.debug_parse(code);

        // Now parse for real
        let mut counter = SymbolCounter::new();
        let file_id = FileId(1);
        let symbols = parser.parse(code, file_id, &mut counter);

        eprintln!("\n=== Extracted Symbols ===");
        for symbol in &symbols {
            eprintln!(
                "  {} ({:?}) at line {}",
                symbol.name.as_ref(),
                symbol.kind,
                symbol.range.start_line
            );
        }

        // Check we found the constants
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "MAX_CONNECTIONS"),
            "Should find MAX_CONNECTIONS from define()"
        );
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "DEFAULT_TIMEOUT"),
            "Should find DEFAULT_TIMEOUT from const"
        );
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "CACHE_TTL"),
            "Should find CACHE_TTL from const"
        );
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "globalVar"),
            "Should find globalVar"
        );
    }
}
