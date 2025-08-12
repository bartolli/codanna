//! PHP language parser implementation
//!
//! This parser provides PHP language support for the codebase intelligence system.
//! It extracts symbols, relationships, and documentation from PHP source code using
//! tree-sitter for AST parsing.

use crate::indexing::Import;
use crate::parsing::{Language, LanguageParser, MethodCall};
use crate::{FileId, Range, Symbol, SymbolId, SymbolKind};
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
}

impl std::fmt::Debug for PhpParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhpParser")
            .field("language", &"PHP")
            .finish()
    }
}

impl PhpParser {
    /// Create a new PHP parser instance
    pub fn new() -> Result<Self, PhpParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .map_err(|e| PhpParseError::ParserInitFailed {
                reason: format!("tree-sitter error: {e}"),
            })?;

        Ok(Self { parser })
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
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut u32,
    ) {
        match node.kind() {
            "function_definition" => {
                if let Some(symbol) = self.process_function(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                // Process children to find nested functions
                self.process_children(node, code, file_id, symbols, counter);
            }
            "method_declaration" => {
                if let Some(symbol) = self.process_method(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                // Process children for nested elements
                self.process_children(node, code, file_id, symbols, counter);
            }
            "class_declaration" => {
                if let Some(symbol) = self.process_class(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                // Continue processing children to find methods inside the class
                self.process_children(node, code, file_id, symbols, counter);
            }
            "interface_declaration" => {
                if let Some(symbol) = self.process_interface(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                // Process children for interface methods
                self.process_children(node, code, file_id, symbols, counter);
            }
            "trait_declaration" => {
                if let Some(symbol) = self.process_trait(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                // Process children for trait methods
                self.process_children(node, code, file_id, symbols, counter);
            }
            "property_declaration" => {
                if let Some(symbol) = self.process_property(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "const_declaration" | "class_const_declaration" => {
                if let Some(symbol) = self.process_constant(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            _ => {
                // Recursively process children
                self.process_children(node, code, file_id, symbols, counter);
            }
        }
    }

    /// Process a function definition node
    fn process_function(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut u32,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        *counter += 1;
        let id = SymbolId(*counter);

        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Function,
            file_id,
            self.node_to_range(node),
        );
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
        Some(symbol)
    }

    /// Process a method declaration node
    fn process_method(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut u32,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        *counter += 1;
        let id = SymbolId(*counter);

        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Method,
            file_id,
            self.node_to_range(node),
        );
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
        Some(symbol)
    }

    /// Process a class declaration node
    fn process_class(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut u32,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        *counter += 1;
        let id = SymbolId(*counter);

        // Using Class for PHP classes
        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Class,
            file_id,
            self.node_to_range(node),
        );
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
        Some(symbol)
    }

    /// Process an interface declaration node
    fn process_interface(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut u32,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        *counter += 1;
        let id = SymbolId(*counter);

        // Using Interface for PHP interfaces
        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Interface,
            file_id,
            self.node_to_range(node),
        );
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
        Some(symbol)
    }

    /// Process a trait declaration node
    fn process_trait(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut u32,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        *counter += 1;
        let id = SymbolId(*counter);

        let mut symbol = Symbol::new(
            id,
            name,
            SymbolKind::Trait,
            file_id,
            self.node_to_range(node),
        );
        symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
        Some(symbol)
    }

    /// Process a property declaration node
    fn process_property(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut u32,
    ) -> Option<Symbol> {
        // Find the property element within the declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "property_element" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    // Remove $ prefix from property name if present
                    let clean_name = name.strip_prefix('$').unwrap_or(name);

                    *counter += 1;
                    let id = SymbolId(*counter);

                    let mut symbol = Symbol::new(
                        id,
                        clean_name,
                        SymbolKind::Field,
                        file_id,
                        self.node_to_range(node),
                    );
                    symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
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
        counter: &mut u32,
    ) -> Option<Symbol> {
        // Find the const element within the declaration
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "const_element" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];

                    *counter += 1;
                    let id = SymbolId(*counter);

                    let mut symbol = Symbol::new(
                        id,
                        name,
                        SymbolKind::Constant,
                        file_id,
                        self.node_to_range(node),
                    );
                    symbol.doc_comment = self.extract_doc_comment(&node, code).map(|s| s.into());
                    return Some(symbol);
                }
            }
        }
        None
    }

    /// Process children nodes recursively
    fn process_children(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut u32,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter);
        }
    }
}

impl LanguageParser for PhpParser {
    fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut u32) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut symbols = Vec::new();
        self.extract_symbols_from_node(
            tree.root_node(),
            code,
            file_id,
            &mut symbols,
            symbol_counter,
        );
        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
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

        let mut calls = Vec::new();
        self.extract_calls_from_node(tree.root_node(), code, "", &mut calls);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        self.find_calls(code)
            .into_iter()
            .map(|(caller, target, range)| MethodCall::from_legacy_format(caller, target, range))
            .collect()
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut implementations = Vec::new();
        self.extract_implementations_from_node(tree.root_node(), code, &mut implementations);
        implementations
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut uses = Vec::new();
        self.extract_uses_from_node(tree.root_node(), code, "", &mut uses);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut defines = Vec::new();
        self.extract_defines_from_node(tree.root_node(), code, &mut defines);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut imports = Vec::new();
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

        let mut variable_types = Vec::new();
        self.extract_variable_types_from_node(tree.root_node(), code, &mut variable_types);
        variable_types
    }
}

// Helper methods for PhpParser
impl PhpParser {
    fn extract_calls_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        current_context: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "function_call_expression" => {
                if let Some(function_node) = node.child_by_field_name("function") {
                    let function_name = &code[function_node.byte_range()];
                    let range = self.node_to_range(node);
                    calls.push((current_context, function_name, range));
                }
            }
            "member_call_expression" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = &code[name_node.byte_range()];
                    let range = self.node_to_range(node);
                    calls.push((current_context, method_name, range));
                }
            }
            "function_definition" | "method_declaration" => {
                let new_context = if let Some(name_node) = node.child_by_field_name("name") {
                    &code[name_node.byte_range()]
                } else {
                    current_context
                };

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
        current_context: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "typed_property_declaration" | "parameter_declaration" => {
                if let Some(type_node) = node.child_by_field_name("type") {
                    let type_name = &code[type_node.byte_range()];
                    let range = self.node_to_range(type_node);
                    uses.push((current_context, type_name, range));
                }
            }
            "function_definition" | "method_declaration" => {
                let new_context = if let Some(name_node) = node.child_by_field_name("name") {
                    &code[name_node.byte_range()]
                } else {
                    current_context
                };

                // Check return type
                if let Some(return_type) = node.child_by_field_name("return_type") {
                    let type_name = &code[return_type.byte_range()];
                    let range = self.node_to_range(return_type);
                    uses.push((new_context, type_name, range));
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

                    // Find methods within the type
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "method_declaration" {
                            if let Some(method_name_node) = child.child_by_field_name("name") {
                                let method_name = &code[method_name_node.byte_range()];
                                let range = self.node_to_range(method_name_node);
                                defines.push((type_name, method_name, range));
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
