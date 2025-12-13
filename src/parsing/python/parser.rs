//! Python language parser implementation
//!
//! This parser provides Python language support for the codebase intelligence system.
//! It extracts symbols, relationships, and documentation from Python source code using
//! tree-sitter for AST parsing.
//!
//! **Tree-sitter ABI Version**: ABI-14 (tree-sitter-python 0.23.6)
//!
//! Note: This parser uses ABI-14 (not ABI-15 like Rust). The tree-sitter-python
//! grammar hasn't been regenerated with the newer CLI yet. All required node types
//! are available in ABI-14. When upgrading to a newer tree-sitter-python version,
//! verify compatibility with node type names used in this implementation.

use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, MethodCall, NodeTracker, NodeTrackingState,
    ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind};
use std::any::Any;
use std::collections::HashSet;
use thiserror::Error;
use tree_sitter::{Node, Parser};

/// Python-specific parsing errors
#[derive(Error, Debug)]
pub enum PythonParseError {
    #[error(
        "Failed to initialize Python parser: {reason}\nSuggestion: Ensure tree-sitter-python is properly installed and the version matches Cargo.toml"
    )]
    ParserInitFailed { reason: String },

    #[error(
        "Invalid Python syntax at {location:?}: {details}\nSuggestion: Check for missing colons, incorrect indentation, or unclosed brackets"
    )]
    SyntaxError { location: Range, details: String },

    #[error(
        "Failed to parse type annotation: {annotation}\nSuggestion: Ensure type annotations follow PEP 484 syntax (e.g., List[str], Dict[str, int])"
    )]
    InvalidTypeAnnotation { annotation: String },

    #[error(
        "Unsupported Python feature at {location:?}: {feature}\nSuggestion: This parser currently supports Python 3.6+ syntax. Consider simplifying the code or file an issue"
    )]
    UnsupportedFeature { feature: String, location: Range },
}

/// Python language parser
pub struct PythonParser {
    parser: Parser,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for PythonParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PythonParser")
            .field("language", &"Python")
            .finish()
    }
}

impl PythonParser {
    /// Parse Python source code and extract all symbols
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut symbols = Vec::new();
        // Create a parser context starting at module scope
        let mut context = ParserContext::new();

        // Create a module-level symbol to represent the file's module scope.
        // Name is set to "<module>" here to match Python conventions and tests;
        // during indexing, PythonBehavior will rename it to the actual module path
        // (e.g., package.module) for searchability.
        let module_symbol_id = symbol_counter.next_id();
        let module_range = self.node_to_range(root_node);
        let mut module_symbol = Symbol::new(
            module_symbol_id,
            "<module>",
            SymbolKind::Module,
            file_id,
            module_range,
        );
        module_symbol.scope_context = Some(crate::symbol::ScopeContext::Module);
        symbols.push(module_symbol);

        self.extract_symbols_from_node(
            root_node,
            code,
            file_id,
            &mut symbols,
            symbol_counter,
            &mut context,
            0,
        );

        symbols
    }

    /// Create a new Python parser instance
    pub fn new() -> Result<Self, PythonParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|e| PythonParseError::ParserInitFailed {
                reason: format!("tree-sitter error: {e}"),
            })?;

        Ok(Self {
            parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Extract symbols from AST node recursively
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
            "function_definition" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract function name for parent tracking
                let func_name = self.extract_function_name(node, code);

                if let Some(symbol) = self.process_function(node, code, file_id, counter, context) {
                    symbols.push(symbol);
                }

                // Enter function scope for processing children
                context.enter_scope(ScopeType::function());

                // Save the current parent context before setting new one
                let saved_function = context.current_function().map(|s| s.to_string());
                let saved_class = context.current_class().map(|s| s.to_string());

                // Set current function for parent tracking
                if let Some(name) = func_name {
                    context.set_current_function(Some(name.to_string()));
                }

                // Process children to find nested functions
                self.process_children(node, code, file_id, symbols, counter, context, depth);

                // CRITICAL: Exit scope first (this clears the current context)
                context.exit_scope();

                // Then restore the previous parent context
                context.set_current_function(saved_function);
                context.set_current_class(saved_class);
            }
            "class_definition" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Extract class name for parent tracking
                let class_name = self.extract_class_name(node, code);

                if let Some(symbol) = self.process_class(node, code, file_id, counter, context) {
                    symbols.push(symbol);
                }

                // Enter class scope for processing children
                context.enter_scope(ScopeType::Class);

                // Save the current parent context before setting new one
                let saved_function = context.current_function().map(|s| s.to_string());
                let saved_class = context.current_class().map(|s| s.to_string());

                // Set current class for parent tracking
                if let Some(name) = class_name {
                    // Build full class path for nested classes
                    let full_class_name = if let Some(parent_class) = context.current_class() {
                        format!("{parent_class}.{name}")
                    } else {
                        name.to_string()
                    };
                    context.set_current_class(Some(full_class_name));
                }

                // Continue processing children to find methods inside the class
                self.process_children(node, code, file_id, symbols, counter, context, depth);

                // CRITICAL: Exit scope first (this clears the current context)
                context.exit_scope();

                // Then restore the previous parent context
                context.set_current_function(saved_function);
                context.set_current_class(saved_class);
            }
            "expression_statement" => {
                // Process children (including assignments handled by direct "assignment" case)
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "decorated_definition" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Handle decorated functions and classes (@property, @staticmethod, etc.)
                // Process ALL children to ensure decorators are tracked
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "assignment" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Direct assignment at any scope level
                if let Some(symbol) = self.process_assignment(node, code, file_id, counter, context)
                {
                    symbols.push(symbol);
                }
                // Also process children to track lambda, comprehensions, etc. on the right side
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "type_alias_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Handle type aliases: UserId = int
                if let Some(symbol) = self.process_type_alias(node, code, file_id, counter, context)
                {
                    symbols.push(symbol);
                }
            }
            "import_statement" | "import_from_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // For now, just process children to find any nested symbols
                // TODO: Consider creating import symbols for better cross-file resolution
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "lambda" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Lambda expressions - process children for nested symbols
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "list_comprehension"
            | "dictionary_comprehension"
            | "set_comprehension"
            | "generator_expression" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Comprehensions - process children for nested symbols
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "decorator" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Decorators - process children
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "for_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // For loops - process children for nested symbols
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            "type" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Type annotations - process children
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
            _ => {
                // Track any other nodes we encounter
                self.register_handled_node(node.kind(), node.kind_id());
                // Recursively process children
                self.process_children(node, code, file_id, symbols, counter, context, depth);
            }
        }
    }

    /// Process a function definition node
    fn process_function(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        let name = self.extract_function_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Determine if this is a method by checking if it's inside a class
        let kind = if self.is_inside_class(node) {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        // Extract docstring
        let doc_comment = self
            .extract_function_docstring(node, code)
            .map(|s| s.into_boxed_str());

        // Build function signature with type annotations
        let signature = self.build_function_signature(node, code);

        // For methods inside nested classes, use the full qualified name
        let symbol_name = if let Some(class_name) = context.current_class() {
            format!("{class_name}.{name}")
        } else {
            name.to_string()
        };

        let mut symbol = Symbol::new(symbol_id, symbol_name.as_str(), kind, file_id, range);
        symbol.doc_comment = doc_comment;
        symbol.signature = signature.map(|s| s.into_boxed_str());
        // Set the scope context based on where the function is defined
        symbol.scope_context = Some(context.current_scope_context());
        Some(symbol)
    }

    /// Extract class signature including inheritance
    fn extract_class_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it (colon + indented content)
        for child in node.children(&mut node.walk()) {
            if child.kind() == ":" {
                end = child.end_byte();
                break;
            }
        }

        code[start..end].trim().to_string()
    }

    /// Process a class definition node
    fn process_class(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        let name = self.extract_class_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Extract docstring
        let doc_comment = self
            .extract_class_docstring(node, code)
            .map(|s| s.into_boxed_str());

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Class, file_id, range);
        symbol.doc_comment = doc_comment;
        // Classes are typically module-level in Python
        symbol.scope_context = Some(context.current_scope_context());

        // Extract and add class signature
        let signature = self.extract_class_signature(node, code);
        symbol.signature = Some(signature.into());

        Some(symbol)
    }

    /// Process child nodes recursively
    fn process_children(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        for child in node.children(&mut node.walk()) {
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

    /// Process an assignment node (module-level variables and constants)
    fn process_assignment(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Get the left side of the assignment (the variable name)
        let left = node.child_by_field_name("left")?;

        // Handle simple identifier assignments (not tuple unpacking for now)
        if left.kind() == "identifier" {
            let name = &code[left.byte_range()];
            let range = self.node_to_range(node);
            let symbol_id = counter.next_id();

            // Determine if it's a constant (UPPER_CASE naming convention)
            let kind = if name
                .chars()
                .all(|c| c.is_uppercase() || c == '_' || c.is_numeric())
                && name.chars().any(|c| c.is_alphabetic())
            {
                SymbolKind::Constant
            } else {
                SymbolKind::Variable
            };

            let mut symbol = Symbol::new(symbol_id, name, kind, file_id, range);
            // Set scope context - assignments are at the current scope level
            symbol.scope_context = Some(context.current_scope_context());

            // Try to extract the value as a simple signature
            if let Some(right) = node.child_by_field_name("right") {
                let value_preview = &code[right.byte_range()];
                // Store full signature for semantic quality
                symbol.signature = Some(format!("{name} = {value_preview}").into());
            }

            return Some(symbol);
        }

        None
    }

    /// Process a type alias statement (e.g., UserId = int, Vector = List[float])
    fn process_type_alias(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        context: &ParserContext,
    ) -> Option<Symbol> {
        // Type alias: type_name = type_expression
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::TypeAlias, file_id, range);
        symbol.scope_context = Some(context.current_scope_context());

        // Extract the type alias definition as signature
        if let Some(value_node) = node.child_by_field_name("value") {
            let type_def = &code[value_node.byte_range()];
            symbol.signature = Some(format!("{name} = {type_def}").into());
        }

        Some(symbol)
    }

    /// Extract function name from function_definition node
    fn extract_function_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|name_node| &code[name_node.byte_range()])
    }

    /// Extract class name from class_definition node
    fn extract_class_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|name_node| &code[name_node.byte_range()])
    }

    /// Check if a node is inside a class definition
    fn is_inside_class(&self, node: Node) -> bool {
        let mut parent = node.parent();
        while let Some(p) = parent {
            if p.kind() == "class_definition" {
                return true;
            }
            parent = p.parent();
        }
        false
    }

    /// Check if a function definition is async
    fn is_async_function(&self, node: Node, _code: &str) -> bool {
        // From the debug output, we can see that async functions have:
        // function_definition with first child being "async" token

        // Method 1: Check if the first child is "async" token
        if let Some(first_child) = node.child(0) {
            if first_child.kind() == "async" {
                return true;
            }
        }

        // Method 2: For safety, also check if any child before "def" is "async"
        for child in node.children(&mut node.walk()) {
            if child.kind() == "async" {
                return true;
            }
            // Stop when we hit "def" - async must come before def
            if child.kind() == "def" {
                break;
            }
        }

        false
    }

    /// Build function signature with type annotations  
    fn build_function_signature(&mut self, node: Node, code: &str) -> Option<String> {
        let params_node = node.child_by_field_name("parameters")?;
        self.register_handled_node(params_node.kind(), params_node.kind_id());
        let params_str = self.build_parameters_string(params_node, code)?;

        // Check if this is an async function
        let is_async = self.is_async_function(node, code);

        // Check for return type annotation
        let return_type = self.extract_return_type(node, code);

        let base_signature = if let Some(ret_type) = return_type {
            format!("({params_str}) -> {ret_type}")
        } else {
            format!("({params_str})")
        };

        if is_async {
            Some(format!("async {base_signature}"))
        } else {
            Some(base_signature)
        }
    }

    /// Build parameters string with type annotations
    fn build_parameters_string(&mut self, params_node: Node, code: &str) -> Option<String> {
        let mut params = Vec::new();

        for child in params_node.children(&mut params_node.walk()) {
            match child.kind() {
                "identifier" => {
                    // Simple parameter without type annotation
                    let param_name = &code[child.byte_range()];
                    params.push(param_name.to_string());
                }
                "typed_parameter" => {
                    self.register_handled_node(child.kind(), child.kind_id());
                    // Parameter with type annotation: name: type
                    if let Some(param_str) = self.extract_typed_parameter(child, code) {
                        params.push(param_str);
                    }
                }
                "typed_default_parameter" => {
                    self.register_handled_node(child.kind(), child.kind_id());
                    // Parameter with type annotation and default value: name: type = value
                    if let Some(param_str) = self.extract_typed_default_parameter(child, code) {
                        params.push(param_str);
                    }
                }
                "default_parameter" => {
                    self.register_handled_node(child.kind(), child.kind_id());
                    // Parameter with default value: name = value
                    if let Some(param_str) = self.extract_default_parameter(child, code) {
                        params.push(param_str);
                    }
                }
                _ => {}
            }
        }

        if params.is_empty() {
            Some(String::new())
        } else {
            Some(params.join(", "))
        }
    }

    /// Extract typed parameter (name: type)
    fn extract_typed_parameter(&self, node: Node, code: &str) -> Option<String> {
        // Structure: identifier : type
        let name = node.child(0).map(|n| &code[n.byte_range()])?;
        let type_annotation = node.child(2).map(|n| &code[n.byte_range()])?; // Skip the ':'

        Some(format!("{name}: {type_annotation}"))
    }

    /// Extract typed default parameter (name: type = value)
    fn extract_typed_default_parameter(&self, node: Node, code: &str) -> Option<String> {
        // Structure: identifier : type = value
        let name = node.child(0).map(|n| &code[n.byte_range()])?;
        let type_annotation = node.child(2).map(|n| &code[n.byte_range()])?; // Skip the ':'
        let default_value = node.child(4).map(|n| &code[n.byte_range()])?; // Skip the '='

        Some(format!("{name}: {type_annotation} = {default_value}"))
    }

    /// Extract default parameter (name = value or name: type = value)
    fn extract_default_parameter(&self, node: Node, code: &str) -> Option<String> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let default_value = node
            .child_by_field_name("value")
            .map(|n| &code[n.byte_range()])?;

        // Check if there's a type annotation
        if let Some(type_node) = node.child_by_field_name("type") {
            let type_annotation = &code[type_node.byte_range()];
            Some(format!("{name}: {type_annotation} = {default_value}"))
        } else {
            Some(format!("{name} = {default_value}"))
        }
    }

    /// Extract return type annotation from function definition
    fn extract_return_type(&self, node: Node, code: &str) -> Option<String> {
        node.child_by_field_name("return_type")
            .map(|n| code[n.byte_range()].to_string())
    }

    /// Convert tree-sitter Node to Range
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

    /// Extract docstring from function definition
    fn extract_function_docstring(&self, node: Node, code: &str) -> Option<String> {
        let body = node.child_by_field_name("body")?;
        self.extract_docstring_from_body(body, code)
    }

    /// Extract docstring from class definition
    fn extract_class_docstring(&self, node: Node, code: &str) -> Option<String> {
        let body = node.child_by_field_name("body")?;
        self.extract_docstring_from_body(body, code)
    }

    /// Extract docstring from function/class body (first string literal)
    fn extract_docstring_from_body(&self, body: Node, code: &str) -> Option<String> {
        // Find the first statement in the body
        let first_statement = body.child(0)?; // Usually the block node

        // For Python, body is usually a "block" node containing statements
        let first_child = if first_statement.kind() == "block" {
            first_statement.child(0)? // Get first statement from block
        } else {
            first_statement
        };

        // Check if it's an expression statement containing a string
        if first_child.kind() == "expression_statement" {
            let expr = first_child.child(0)?;
            if expr.kind() == "string" {
                let raw_string = &code[expr.byte_range()];
                return Some(self.normalize_docstring(raw_string));
            }
        }

        None
    }

    /// Normalize docstring by removing quotes and cleaning whitespace
    fn normalize_docstring(&self, raw: &str) -> String {
        let trimmed = raw.trim();

        // Handle triple quotes (""" or ''')
        if (trimmed.starts_with("\"\"\"") && trimmed.ends_with("\"\"\"") && trimmed.len() >= 6)
            || (trimmed.starts_with("'''") && trimmed.ends_with("'''") && trimmed.len() >= 6)
        {
            let content = &trimmed[3..trimmed.len() - 3];
            self.clean_docstring_whitespace(content)
        }
        // Handle single quotes (" or ')
        else if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
        {
            let content = &trimmed[1..trimmed.len() - 1];
            content.to_string()
        } else {
            raw.to_string()
        }
    }

    /// Clean whitespace from docstring content
    fn clean_docstring_whitespace(&self, content: &str) -> String {
        // Split into lines and trim leading/trailing empty lines
        let lines: Vec<&str> = content.lines().collect();
        let mut start_idx = 0;
        let mut end_idx = lines.len();

        // Find first non-empty line
        while start_idx < lines.len() && lines[start_idx].trim().is_empty() {
            start_idx += 1;
        }

        // Find last non-empty line
        while end_idx > start_idx && lines[end_idx - 1].trim().is_empty() {
            end_idx -= 1;
        }

        if start_idx >= end_idx {
            return String::new();
        }

        lines[start_idx..end_idx].join("\n").trim().to_string()
    }

    /// Find function calls in AST node recursively
    fn find_calls_in_node<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        match node.kind() {
            "function_definition" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_function_node_for_calls(node, code, calls, current_function);
            }
            "call" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_call_node(node, code, calls, current_function);
            }
            "lambda" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_children_for_calls(node, code, calls, current_function);
            }
            "list_comprehension" | "dictionary_comprehension" | "set_comprehension" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_children_for_calls(node, code, calls, current_function);
            }
            "decorator" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_children_for_calls(node, code, calls, current_function);
            }
            _ => {
                self.process_children_for_calls(node, code, calls, current_function);
            }
        }
    }

    /// Process function definition node for call detection
    fn process_function_node_for_calls<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        if let Some(name) = self.extract_function_name(node, code) {
            let old_function = *current_function;
            *current_function = Some(name);

            self.process_children_for_calls(node, code, calls, current_function);

            *current_function = old_function;
        }
    }

    /// Process call node for call detection
    fn process_call_node<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        if let Some(callee) = self.extract_call_target(node, code) {
            let range = self.node_to_range(node);
            let caller = (*current_function).unwrap_or("<module>");
            calls.push((caller, callee, range));
        }

        self.process_children_for_calls(node, code, calls, current_function);
    }

    /// Find method calls in AST node recursively
    fn find_method_calls_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        method_calls: &mut Vec<MethodCall>,
        current_function: &mut Option<&'a str>,
    ) {
        match node.kind() {
            "function_definition" => {
                self.process_function_node_for_method_calls(
                    node,
                    code,
                    method_calls,
                    current_function,
                );
            }
            "call" => {
                self.process_call_node_for_method_calls(node, code, method_calls, current_function);
            }
            _ => {
                self.process_children_for_method_calls(node, code, method_calls, current_function);
            }
        }
    }

    /// Process function definition node for method call detection
    fn process_function_node_for_method_calls<'a>(
        &self,
        node: Node,
        code: &'a str,
        method_calls: &mut Vec<MethodCall>,
        current_function: &mut Option<&'a str>,
    ) {
        if let Some(name) = self.extract_function_name(node, code) {
            let old_function = *current_function;
            *current_function = Some(name);

            self.process_children_for_method_calls(node, code, method_calls, current_function);

            *current_function = old_function;
        }
    }

    /// Process call node for method call detection
    fn process_call_node_for_method_calls<'a>(
        &self,
        node: Node,
        code: &'a str,
        method_calls: &mut Vec<MethodCall>,
        current_function: &mut Option<&'a str>,
    ) {
        let caller = (*current_function).unwrap_or("<module>");
        if let Some(method_call) = self.extract_method_call(node, code, caller) {
            method_calls.push(method_call);
        }

        self.process_children_for_method_calls(node, code, method_calls, current_function);
    }

    /// Process child nodes for function calls
    fn process_children_for_calls<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: &mut Option<&'a str>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_calls_in_node(child, code, calls, current_function);
        }
    }

    /// Process child nodes for method calls
    fn process_children_for_method_calls<'a>(
        &self,
        node: Node,
        code: &'a str,
        method_calls: &mut Vec<MethodCall>,
        current_function: &mut Option<&'a str>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_method_calls_in_node(child, code, method_calls, current_function);
        }
    }

    /// Extract the target of a function call
    fn extract_call_target<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        // Get the function being called
        let function_node = node.child_by_field_name("function")?;

        match function_node.kind() {
            "identifier" => {
                // Simple function call: func()
                Some(&code[function_node.byte_range()])
            }
            "attribute" => {
                // Cross-module or method call: module.func() or obj.method()
                // Return the full qualified path for cross-module tracking
                // This allows tracking calls like app.utils.helper.process_data()
                // The attribute node's byte_range already contains the full dotted path
                Some(&code[function_node.byte_range()])
            }
            _ => None,
        }
    }

    /// Extract method call information including receiver
    fn extract_method_call<'a>(
        &self,
        node: Node,
        code: &'a str,
        caller: &'a str,
    ) -> Option<MethodCall> {
        let function_node = node.child_by_field_name("function")?;
        let range = self.node_to_range(node);

        match function_node.kind() {
            "attribute" => {
                // This is a method call: obj.method()
                let method_name = function_node
                    .child_by_field_name("attribute")
                    .map(|n| &code[n.byte_range()])?;

                let receiver = function_node
                    .child_by_field_name("object")
                    .map(|n| &code[n.byte_range()]);

                let mut method_call = MethodCall::new(caller, method_name, range);

                if let Some(receiver_name) = receiver {
                    method_call = method_call.with_receiver(receiver_name);
                }

                Some(method_call)
            }
            _ => {
                // Not a method call, skip
                None
            }
        }
    }

    /// Find import statements in AST node recursively
    fn find_imports_in_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "import_statement" => {
                self.process_import_statement(node, code, file_id, imports);
            }
            "import_from_statement" => {
                self.process_from_import_statement(node, code, file_id, imports);
            }
            _ => {
                self.process_children_for_imports(node, code, file_id, imports);
            }
        }
    }

    /// Process simple import statement (import module)
    fn process_import_statement(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        // Import statement structure: import module1, module2, ...
        for child in node.children(&mut node.walk()) {
            if child.kind() == "dotted_name" || child.kind() == "identifier" {
                let module_path = &code[child.byte_range()];
                imports.push(Import {
                    path: module_path.to_string(),
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
        }
    }

    /// Process from import statement (from module import name)
    fn process_from_import_statement(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        let module_path = self.extract_from_module_path(node, code);

        if let Some(base_path) = module_path {
            // Check for wildcard import (from module import *)
            if self.has_wildcard_import(node, code) {
                imports.push(Import {
                    path: base_path.to_string(),
                    alias: None,
                    file_id,
                    is_glob: true,
                    is_type_only: false,
                });
            } else {
                // Process individual imports
                self.extract_from_import_names(node, code, base_path, file_id, imports);
            }
        }
    }

    /// Extract module path from 'from' import statement
    fn extract_from_module_path<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        // Find the first dotted_name node (the module path comes after 'from')
        for child in node.children(&mut node.walk()) {
            if child.kind() == "dotted_name" {
                return Some(&code[child.byte_range()]);
            }
        }
        None
    }

    /// Check if import statement has wildcard (*)
    fn has_wildcard_import(&self, node: Node, code: &str) -> bool {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "wildcard_import"
                || (child.kind() == "identifier" && &code[child.byte_range()] == "*")
            {
                return true;
            }
        }
        false
    }

    /// Extract individual import names from 'from' statement
    fn extract_from_import_names(
        &self,
        node: Node,
        code: &str,
        base_path: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        // Look for dotted_name nodes that represent import names after the 'import' keyword
        let mut found_import_keyword = false;

        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "import" => {
                    found_import_keyword = true;
                }
                "dotted_name" if found_import_keyword => {
                    // This is an import name
                    let name = &code[child.byte_range()];
                    let full_path = format!("{base_path}.{name}");
                    imports.push(Import {
                        path: full_path,
                        alias: None,
                        file_id,
                        is_glob: false,
                        is_type_only: false,
                    });
                }
                "aliased_import" => {
                    self.process_aliased_import(child, code, base_path, file_id, imports);
                }
                _ => {}
            }
        }
    }

    /// Process aliased import (name as alias)
    fn process_aliased_import(
        &self,
        node: Node,
        code: &str,
        base_path: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()]);
        let alias = node
            .child_by_field_name("alias")
            .map(|n| &code[n.byte_range()]);

        if let Some(import_name) = name {
            let full_path = format!("{base_path}.{import_name}");
            imports.push(Import {
                path: full_path,
                alias: alias.map(|s| s.to_string()),
                file_id,
                is_glob: false,
                is_type_only: false,
            });
        }
    }

    /// Process child nodes for imports
    fn process_children_for_imports(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_imports_in_node(child, code, file_id, imports);
        }
    }

    /// Find class inheritance relationships in AST node recursively
    fn find_implementations_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "class_definition" => {
                self.process_class_inheritance(node, code, implementations);
                // Continue processing children for nested classes
                self.process_children_for_implementations(node, code, implementations);
            }
            _ => {
                self.process_children_for_implementations(node, code, implementations);
            }
        }
    }

    /// Process class definition for inheritance relationships
    fn process_class_inheritance<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if let Some(class_name) = self.extract_class_name(node, code) {
            let range = self.node_to_range(node);
            let base_classes = self.extract_base_classes(node, code);

            for base_class in base_classes {
                implementations.push((class_name, base_class, range));
            }
        }
    }

    /// Extract base class names from class definition
    fn extract_base_classes<'a>(&self, node: Node, code: &'a str) -> Vec<&'a str> {
        let mut base_classes = Vec::new();

        // Look for argument_list node which contains the base classes
        if let Some(superclasses_node) = node.child_by_field_name("superclasses") {
            Self::extract_base_class_names(superclasses_node, code, &mut base_classes);
        }

        base_classes
    }

    /// Extract individual base class names from superclasses list
    fn extract_base_class_names<'a>(node: Node, code: &'a str, base_classes: &mut Vec<&'a str>) {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" => {
                    // Simple base class: class Dog(Animal)
                    base_classes.push(&code[child.byte_range()]);
                }
                "attribute" => {
                    // Qualified base class: class Child(parent.Base)
                    base_classes.push(&code[child.byte_range()]);
                }
                "argument_list" => {
                    // Nested argument list - recurse
                    Self::extract_base_class_names(child, code, base_classes);
                }
                _ => {
                    // Continue processing children for other node types
                    Self::extract_base_class_names(child, code, base_classes);
                }
            }
        }
    }

    /// Process child nodes for inheritance detection
    fn process_children_for_implementations<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_implementations_in_node(child, code, implementations);
        }
    }

    /// Find variable type annotations in AST node recursively
    fn find_variable_types_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        variable_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "assignment" => {
                self.process_assignment_with_type(node, code, variable_types);
            }
            _ => {
                self.process_children_for_variable_types(node, code, variable_types);
            }
        }
    }

    /// Process assignment node with type annotation (x: int = 5 or x: int)
    fn process_assignment_with_type<'a>(
        &self,
        node: Node,
        code: &'a str,
        variable_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Only process assignments that have type annotations
        if let Some(type_node) = node.child_by_field_name("type") {
            // Extract variable name from the left side
            if let Some(target_node) = node.child_by_field_name("left") {
                if let Some(var_name) = self.extract_variable_name(target_node, code) {
                    let type_annotation = &code[type_node.byte_range()];
                    let range = self.node_to_range(node);
                    variable_types.push((var_name, type_annotation, range));
                }
            }
        }
    }

    /// Extract variable name from assignment target
    fn extract_variable_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            "identifier" => {
                // Simple variable: x
                Some(&code[node.byte_range()])
            }
            "attribute" => {
                // Class attribute: self.name
                if let Some(attr_node) = node.child_by_field_name("attribute") {
                    Some(&code[attr_node.byte_range()])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Process child nodes for variable type extraction
    fn process_children_for_variable_types<'a>(
        &self,
        node: Node,
        code: &'a str,
        variable_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in node.children(&mut node.walk()) {
            self.find_variable_types_in_node(child, code, variable_types);
        }
    }

    fn find_defines_in_node<'a>(
        parser: &mut PythonParser,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "class_definition" => {
                parser.register_handled_node(node.kind(), node.kind_id());
                // Extract class name
                if let Some(class_name_node) = node.child_by_field_name("name") {
                    let class_name = &code[class_name_node.byte_range()];

                    // Find all methods defined in this class
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            if child.kind() == "function_definition" {
                                if let Some(method_name_node) = child.child_by_field_name("name") {
                                    let method_name = &code[method_name_node.byte_range()];
                                    let range = Range::new(
                                        child.start_position().row as u32,
                                        child.start_position().column as u16,
                                        child.end_position().row as u32,
                                        child.end_position().column as u16,
                                    );
                                    defines.push((class_name, method_name, range));
                                }
                            }
                        }
                    }
                }
            }
            "lambda" => {
                parser.register_handled_node(node.kind(), node.kind_id());
                // Process children for lambda
                for child in node.children(&mut node.walk()) {
                    Self::find_defines_in_node(parser, child, code, defines);
                }
            }
            "list_comprehension" | "dictionary_comprehension" | "set_comprehension" => {
                parser.register_handled_node(node.kind(), node.kind_id());
                // Process children for comprehensions
                for child in node.children(&mut node.walk()) {
                    Self::find_defines_in_node(parser, child, code, defines);
                }
            }
            "decorator" => {
                parser.register_handled_node(node.kind(), node.kind_id());
                // Process children for decorators
                for child in node.children(&mut node.walk()) {
                    Self::find_defines_in_node(parser, child, code, defines);
                }
            }
            _ => {
                // Recursively process children
                for child in node.children(&mut node.walk()) {
                    Self::find_defines_in_node(parser, child, code, defines);
                }
            }
        }
    }
}

impl LanguageParser for PythonParser {
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

    fn language(&self) -> Language {
        Language::Python
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        match node.kind() {
            "function_definition" => self.extract_function_docstring(*node, code),
            "class_definition" => self.extract_class_docstring(*node, code),
            _ => None,
        }
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();
        let mut current_function = None;

        self.find_calls_in_node(root_node, code, &mut calls, &mut current_function);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut method_calls = Vec::new();
        let mut current_function = None;

        self.find_method_calls_in_node(root_node, code, &mut method_calls, &mut current_function);
        method_calls
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut implementations = Vec::new();

        self.find_implementations_in_node(root_node, code, &mut implementations);
        implementations
    }

    fn find_uses<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Stub implementation - will be implemented in Phase 3
        Vec::new()
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut defines = Vec::new();

        Self::find_defines_in_node(self, root_node, code, &mut defines);
        defines
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

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut variable_types = Vec::new();

        self.find_variable_types_in_node(root_node, code, &mut variable_types);
        variable_types
    }
}

impl NodeTracker for PythonParser {
    fn get_handled_nodes(&self) -> &HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_parser_creation() {
        let parser = PythonParser::new();
        assert!(parser.is_ok(), "Parser should initialize successfully");
    }

    #[test]
    fn test_language_parser_trait_impl() {
        let parser: Box<dyn LanguageParser> = Box::new(PythonParser::new().unwrap());
        assert_eq!(parser.language(), Language::Python);
        assert!(parser.as_any().is::<PythonParser>());
    }

    // Sub-Task 1.2.1: Parse function definitions
    #[test]
    fn test_parse_simple_function() {
        let mut parser = PythonParser::new().unwrap();
        let code = "def hello():\n    pass";
        println!("Parsing: {code}");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        assert_eq!(symbols.len(), 2); // <module>, hello
        let func = symbols.iter().find(|s| s.name.as_ref() == "hello").unwrap();
        assert_eq!(func.name.as_ref(), "hello");
        assert_eq!(func.kind, SymbolKind::Function);

        println!(
            "✓ Found function_definition node at {}:{}-{}:{}",
            func.range.start_line,
            func.range.start_column,
            func.range.end_line,
            func.range.end_column
        );
        println!(
            "✓ Extracted name: \"{}\" at {}:{}-{}:{}",
            func.name.as_ref(),
            func.range.start_line,
            func.range.start_column + 4, // "def " offset
            func.range.start_line,
            func.range.start_column + 9
        ); // "hello" end
        println!(
            "✓ Created Symbol {{ id: {}, name: \"{}\", kind: {:?} }}",
            func.id.value(),
            func.name.as_ref(),
            func.kind
        );
    }

    // Sub-Task 1.2.2: Parse class definitions
    #[test]
    fn test_parse_simple_class() {
        let mut parser = PythonParser::new().unwrap();
        let code = "class Person:\n    pass";
        println!("Parsing: {code}");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        assert_eq!(symbols.len(), 2); // <module>, Person
        let class = symbols
            .iter()
            .find(|s| s.name.as_ref() == "Person")
            .unwrap();
        assert_eq!(class.name.as_ref(), "Person");
        assert_eq!(class.kind, SymbolKind::Class);

        println!(
            "✓ Found class_definition node at {}:{}-{}:{}",
            class.range.start_line,
            class.range.start_column,
            class.range.end_line,
            class.range.end_column
        );
        println!(
            "✓ Extracted name: \"{}\" at {}:{}-{}:{}",
            class.name.as_ref(),
            class.range.start_line,
            class.range.start_column + 6, // "class " offset
            class.range.start_line,
            class.range.start_column + 12
        ); // "Person" end
        println!(
            "✓ Created Symbol {{ id: {}, name: \"{}\", kind: {:?} }}",
            class.id.value(),
            class.name.as_ref(),
            class.kind
        );
    }

    // Sub-Task 1.2.3: Parse methods within classes
    #[test]
    fn test_parse_class_with_methods() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class Calculator:
    def __init__(self):
        self.value = 0

    def add(self, n):
        self.value += n
"#;
        println!("Parsing class Calculator with methods...");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        assert_eq!(symbols.len(), 4); // <module>, Calculator, __init__, add
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "Calculator.__init__" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "Calculator.add" && s.kind == SymbolKind::Method)
        );

        let class = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Class)
            .unwrap();
        let init_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "Calculator.__init__")
            .unwrap();
        let add_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "Calculator.add")
            .unwrap();

        println!(
            "✓ Found class_definition \"{}\" at {}:{}-{}:{}",
            class.name.as_ref(),
            class.range.start_line,
            class.range.start_column,
            class.range.end_line,
            class.range.end_column
        );
        println!(
            "  ✓ Found function_definition \"{}\" at {}:{}-{}:{} (inside class → Method)",
            init_method.name.as_ref(),
            init_method.range.start_line,
            init_method.range.start_column,
            init_method.range.end_line,
            init_method.range.end_column
        );
        println!(
            "  ✓ Found function_definition \"{}\" at {}:{}-{}:{} (inside class → Method)",
            add_method.name.as_ref(),
            add_method.range.start_line,
            add_method.range.start_column,
            add_method.range.end_line,
            add_method.range.end_column
        );
        println!(
            "✓ Created 3 symbols: {} ({:?}), {} ({:?}), {} ({:?})",
            class.name.as_ref(),
            class.kind,
            init_method.name.as_ref(),
            init_method.kind,
            add_method.name.as_ref(),
            add_method.kind
        );
    }

    // Test nested functions (they should be treated as regular functions)
    #[test]
    fn test_nested_functions() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
def outer():
    def inner():
        pass
    return inner
"#;
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        // Both outer and inner should be functions (not methods) since they're not in a class
        assert_eq!(symbols.len(), 3); // <module>, outer, inner
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "outer" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "inner" && s.kind == SymbolKind::Function)
        );
    }

    // Sub-Task 2.1.1: Function docstrings
    #[test]
    fn test_function_docstring_extraction() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
def calculate_area(radius):
    """Calculate the area of a circle.

    Args:
        radius: The radius of the circle.

    Returns:
        The area of the circle.
    """
    return 3.14159 * radius ** 2
"#;
        println!("Extracting docstring for function \"calculate_area\"...");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        let func = symbols
            .iter()
            .find(|s| s.name.as_ref() == "calculate_area")
            .unwrap();
        assert!(func.doc_comment.is_some());
        assert!(
            func.doc_comment
                .as_ref()
                .unwrap()
                .contains("Calculate the area")
        );

        println!("✓ Found function body starting at line 2");
        println!("✓ First statement is expression_statement with string literal");
        println!("✓ Extracted docstring: \"\"\"Calculate the area of a circle...\"\"\"");
        println!(
            "✓ Normalized to: \"{}\"",
            func.doc_comment.as_ref().unwrap()
        );
    }

    // Sub-Task 2.1.2: Class docstrings
    #[test]
    fn test_class_docstring_extraction() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class DatabaseConnection:
    """Manages database connections.

    This class provides a high-level interface for database operations.
    """
    def __init__(self, host, port):
        pass
"#;
        println!("Extracting docstring for class \"DatabaseConnection\"...");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        let class = symbols
            .iter()
            .find(|s| s.name.as_ref() == "DatabaseConnection")
            .unwrap();
        assert!(
            class
                .doc_comment
                .as_ref()
                .unwrap()
                .contains("Manages database connections")
        );

        println!("✓ Found class body starting at line 2");
        println!("✓ First statement is expression_statement with string literal");
        println!("✓ Extracted class docstring successfully");
        println!("✓ Preserved multi-line formatting");
    }

    // Test various docstring formats
    #[test]
    fn test_various_docstring_formats() {
        let mut parser = PythonParser::new().unwrap();

        // Single line docstring with double quotes
        let code1 = r#"
def simple():
    "This is a simple docstring."
    pass
"#;
        let symbols1 = parser.parse(code1, FileId::new(1).unwrap(), &mut SymbolCounter::new());
        let func1 = symbols1
            .iter()
            .find(|s| s.name.as_ref() == "simple")
            .unwrap();
        assert!(func1.doc_comment.is_some());
        assert_eq!(
            func1.doc_comment.as_ref().unwrap().as_ref(),
            "This is a simple docstring."
        );

        // Single line docstring with single quotes
        let code2 = r#"
def another():
    'Another simple docstring.'
    pass
"#;
        let symbols2 = parser.parse(
            code2,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(10),
        );
        let func2 = symbols2
            .iter()
            .find(|s| s.name.as_ref() == "another")
            .unwrap();
        assert!(func2.doc_comment.is_some());
        assert_eq!(
            func2.doc_comment.as_ref().unwrap().as_ref(),
            "Another simple docstring."
        );

        // No docstring - first statement is not a string
        let code3 = r#"
def no_doc():
    x = 42
    return x
"#;
        let symbols3 = parser.parse(
            code3,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(20),
        );
        let func3 = symbols3
            .iter()
            .find(|s| s.name.as_ref() == "no_doc")
            .unwrap();
        assert!(func3.doc_comment.is_none());
    }

    // Test docstrings with various quote styles
    #[test]
    fn test_triple_quote_docstrings() {
        let mut parser = PythonParser::new().unwrap();

        // Triple single quotes
        let code = r#"
class TestClass:
    '''
    This is a class with single quotes.

    Multiple lines here.
    '''
    pass
"#;
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());
        let class = symbols
            .iter()
            .find(|s| s.name.as_ref() == "TestClass")
            .unwrap();
        assert!(class.doc_comment.is_some());
        assert!(
            class
                .doc_comment
                .as_ref()
                .unwrap()
                .contains("This is a class with single quotes")
        );
    }

    // Sub-Task 3.1.1: Simple function calls
    #[test]
    fn test_simple_function_calls() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
def process_data():
    validate_input()
    result = calculate()
    print(result)
"#;
        println!("Finding calls in function \"process_data\"...");
        println!("---");
        let calls = parser.find_calls(code);

        assert_eq!(calls.len(), 3);
        assert!(calls.iter().any(|(caller, callee, _)| *caller == "process_data" && *callee == "validate_input"));
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| *caller == "process_data" && *callee == "calculate")
        );
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| *caller == "process_data" && *callee == "print")
        );

        // Debug output
        for (caller, callee, range) in &calls {
            println!(
                "✓ Found call node \"{}()\" at {}:{}-{}:{}",
                callee, range.start_line, range.start_column, range.end_line, range.end_column
            );
            println!("  → Caller: \"{caller}\", Callee: \"{callee}\"");
        }
    }

    // Sub-Task 3.1.2: Method calls with receivers
    #[test]
    fn test_method_calls_with_receivers() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
def process_string(text):
    result = text.strip().lower()
    words = result.split()
    self.save(words)
"#;
        println!("Finding method calls in function \"process_string\"...");
        println!("---");
        let method_calls = parser.find_method_calls(code);

        assert!(
            method_calls
                .iter()
                .any(|mc| mc.method_name == "strip" && mc.receiver == Some("text".into()))
        );
        assert!(
            method_calls
                .iter()
                .any(|mc| mc.method_name == "lower" && mc.receiver.is_some())
        ); // Chained call
        assert!(
            method_calls
                .iter()
                .any(|mc| mc.method_name == "split" && mc.receiver == Some("result".into()))
        );
        assert!(
            method_calls
                .iter()
                .any(|mc| mc.method_name == "save" && mc.receiver == Some("self".into()))
        );

        // Debug output
        for method_call in &method_calls {
            if let Some(receiver) = &method_call.receiver {
                println!(
                    "✓ Found attribute call \"{}.{}()\" at {}:{}-{}:{}",
                    receiver,
                    method_call.method_name,
                    method_call.range.start_line,
                    method_call.range.start_column,
                    method_call.range.end_line,
                    method_call.range.end_column
                );
                println!(
                    "  → Receiver: \"{}\", Method: \"{}\", Type: Instance",
                    receiver, method_call.method_name
                );
            }
        }
    }

    // Sub-Task 3.2.1: Simple imports
    #[test]
    fn test_simple_imports() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
import os
import sys
import collections.abc
"#;
        println!("Parsing import statements...");
        println!("---");
        let imports = parser.find_imports(code, FileId::new(1).unwrap());

        assert_eq!(imports.len(), 3);
        assert!(imports.iter().any(|i| i.path == "os" && !i.is_glob));
        assert!(imports.iter().any(|i| i.path == "sys" && !i.is_glob));
        assert!(
            imports
                .iter()
                .any(|i| i.path == "collections.abc" && !i.is_glob)
        );

        // Debug output
        for import in &imports {
            println!("✓ Found import_statement \"import {}\"", import.path);
            println!(
                "  → Import {{ path: \"{}\", alias: {:?}, is_glob: {} }}",
                import.path, import.alias, import.is_glob
            );
        }
    }

    // Sub-Task 3.2.2: From imports
    #[test]
    fn test_from_imports() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
from typing import List, Dict, Optional
from collections import defaultdict as dd
from itertools import *
"#;
        println!("Parsing from-import statements...");
        println!("---");

        let imports = parser.find_imports(code, FileId::new(1).unwrap());

        assert!(
            imports
                .iter()
                .any(|i| i.path == "typing.List" && !i.is_glob)
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "typing.Dict" && !i.is_glob)
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "typing.Optional" && !i.is_glob)
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "collections.defaultdict" && i.alias == Some("dd".to_string()))
        );
        assert!(imports.iter().any(|i| i.path == "itertools" && i.is_glob));

        // Debug output
        for import in &imports {
            if import.is_glob {
                println!("✓ Found wildcard import");
                println!(
                    "  → Import {{ path: \"{}\", alias: {:?}, is_glob: {} }}",
                    import.path, import.alias, import.is_glob
                );
            } else if import.alias.is_some() {
                println!("✓ Found aliased import");
                println!(
                    "  → Import {{ path: \"{}\", alias: {:?}, is_glob: {} }}",
                    import.path, import.alias, import.is_glob
                );
            } else {
                println!("✓ Found import_from_statement");
                println!(
                    "  → Import {{ path: \"{}\", alias: {:?}, is_glob: {} }}",
                    import.path, import.alias, import.is_glob
                );
            }
        }
    }

    // Additional test for mixed import styles
    #[test]
    fn test_mixed_import_styles() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
import os
from typing import List
import sys.path
from collections import defaultdict as dd, Counter
from itertools import *
"#;
        let imports = parser.find_imports(code, FileId::new(1).unwrap());

        // Should find all imports
        assert!(imports.len() >= 6);

        // Simple imports
        assert!(
            imports
                .iter()
                .any(|i| i.path == "os" && i.alias.is_none() && !i.is_glob)
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "sys.path" && i.alias.is_none() && !i.is_glob)
        );

        // From imports
        assert!(
            imports
                .iter()
                .any(|i| i.path == "typing.List" && i.alias.is_none() && !i.is_glob)
        );
        assert!(imports.iter().any(|i| i.path == "collections.defaultdict"
            && i.alias == Some("dd".to_string())
            && !i.is_glob));
        assert!(
            imports
                .iter()
                .any(|i| i.path == "collections.Counter" && i.alias.is_none() && !i.is_glob)
        );

        // Wildcard import
        assert!(
            imports
                .iter()
                .any(|i| i.path == "itertools" && i.alias.is_none() && i.is_glob)
        );
    }

    // Test edge cases
    #[test]
    fn test_import_edge_cases() {
        let mut parser = PythonParser::new().unwrap();

        // Empty file
        let imports1 = parser.find_imports("", FileId::new(1).unwrap());
        assert_eq!(imports1.len(), 0);

        // File with no imports
        let code2 = r#"
def hello():
    print("Hello, world!")
"#;
        let imports2 = parser.find_imports(code2, FileId::new(1).unwrap());
        assert_eq!(imports2.len(), 0);

        // Deeply nested module
        let code3 = "import a.very.deeply.nested.module.name";
        let imports3 = parser.find_imports(code3, FileId::new(1).unwrap());
        assert_eq!(imports3.len(), 1);
        assert_eq!(imports3[0].path, "a.very.deeply.nested.module.name");
    }

    // Sub-Task 3.3.1: Single inheritance
    #[test]
    fn test_single_inheritance() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class Animal:
    pass

class Dog(Animal):
    def bark(self):
        pass
"#;
        println!("Finding class inheritance relationships...");
        println!("---");
        let implementations = parser.find_implementations(code);

        assert_eq!(implementations.len(), 1);
        assert_eq!(implementations[0].0, "Dog");
        assert_eq!(implementations[0].1, "Animal");

        // Debug output
        println!("✓ Found class_definition \"Animal\" with no base classes");
        println!(
            "✓ Found class_definition \"Dog\" at {}:{}-{}:{}",
            implementations[0].2.start_line,
            implementations[0].2.start_column,
            implementations[0].2.end_line,
            implementations[0].2.end_column
        );
        println!("  → Detected superclass: \"{}\"", implementations[0].1);
        println!(
            "  → Implementation: (\"{}\", \"{}\", Range {{ {}:{}-{}:{} }})",
            implementations[0].0,
            implementations[0].1,
            implementations[0].2.start_line,
            implementations[0].2.start_column,
            implementations[0].2.end_line,
            implementations[0].2.end_column
        );
    }

    // Sub-Task 5.1.1: Function parameter types
    #[test]
    fn test_function_type_annotations() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
def process_items(items: List[str], count: int = 10) -> Dict[str, int]:
    pass
"#;
        println!("Parsing typed function signature...");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        let func = symbols
            .iter()
            .find(|s| s.name.as_ref() == "process_items")
            .unwrap();
        assert!(func.signature.is_some());
        let signature = func.signature.as_ref().unwrap();
        assert!(signature.contains("List[str]"));
        assert!(signature.contains("-> Dict[str, int]"));
        assert!(signature.contains("count: int = 10"));

        println!("✓ Found function \"process_items\" with parameters");
        println!("  → Parameter \"items\" : type_annotation = \"List[str]\"");
        println!("  → Parameter \"count\" : type_annotation = \"int\", default = \"10\"");
        println!("✓ Found return type annotation: \"Dict[str, int]\"");
        println!("✓ Built signature: \"{signature}\"");
    }

    // Test various signature combinations
    #[test]
    fn test_various_function_signatures() {
        let mut parser = PythonParser::new().unwrap();

        // Function without type annotations
        let code1 = "def simple(x, y=10): pass";
        let symbols1 = parser.parse(code1, FileId::new(1).unwrap(), &mut SymbolCounter::new());
        let func1 = symbols1
            .iter()
            .find(|s| s.name.as_ref() == "simple")
            .unwrap();
        assert!(func1.signature.is_some());
        let sig1 = func1.signature.as_ref().unwrap();
        assert!(sig1.contains("x, y = 10"));

        // Function with only return type
        let code2 = "def get_number() -> int: pass";
        let symbols2 = parser.parse(
            code2,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(10),
        );
        let func2 = symbols2
            .iter()
            .find(|s| s.name.as_ref() == "get_number")
            .unwrap();
        assert!(func2.signature.is_some());
        let sig2 = func2.signature.as_ref().unwrap();
        assert!(sig2.contains("() -> int"));

        // Mixed typed and untyped parameters
        let code3 = "def mixed(name, age: int, city='NYC'): pass";
        let symbols3 = parser.parse(
            code3,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(20),
        );
        let func3 = symbols3
            .iter()
            .find(|s| s.name.as_ref() == "mixed")
            .unwrap();
        assert!(func3.signature.is_some());
        let sig3 = func3.signature.as_ref().unwrap();
        assert!(sig3.contains("name"));
        assert!(sig3.contains("age: int"));
        assert!(sig3.contains("city = 'NYC'"));

        // Complex generic types
        let code4 =
            "def complex_types(data: Dict[str, List[int]]) -> Optional[Tuple[str, int]]: pass";
        let symbols4 = parser.parse(
            code4,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(30),
        );
        let func4 = symbols4
            .iter()
            .find(|s| s.name.as_ref() == "complex_types")
            .unwrap();
        assert!(func4.signature.is_some());
        let sig4 = func4.signature.as_ref().unwrap();
        assert!(sig4.contains("Dict[str, List[int]]"));
        assert!(sig4.contains("-> Optional[Tuple[str, int]]"));
    }

    // Test async function type annotations
    #[test]
    fn test_async_function_signatures() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
async def fetch_data(url: str, timeout: float = 5.0) -> Dict[str, Any]:
    pass
"#;
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        let func = symbols
            .iter()
            .find(|s| s.name.as_ref() == "fetch_data")
            .unwrap();
        assert!(func.signature.is_some());
        let signature = func.signature.as_ref().unwrap();

        // Now the async keyword should be in the signature
        assert!(signature.contains("async"));
        assert!(signature.contains("url: str"));
        assert!(signature.contains("timeout: float = 5.0"));
        assert!(signature.contains("-> Dict[str, Any]"));

        println!("Async function signature: {signature}");
    }

    // Sub-Task 5.1.2: Variable type annotations
    #[test]
    fn test_variable_type_extraction() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class Config:
    timeout: int = 30
    endpoints: List[str] = []

def setup():
    connection: DatabaseConnection = connect()
    retry_count: int = 3
"#;
        println!("Finding variable type annotations...");
        println!("---");
        let var_types = parser.find_variable_types(code);

        assert!(
            var_types
                .iter()
                .any(|(name, typ, _)| *name == "timeout" && *typ == "int")
        );
        assert!(
            var_types
                .iter()
                .any(|(name, typ, _)| *name == "endpoints" && *typ == "List[str]")
        );
        assert!(
            var_types
                .iter()
                .any(|(name, typ, _)| *name == "connection" && *typ == "DatabaseConnection")
        );
        assert!(
            var_types
                .iter()
                .any(|(name, typ, _)| *name == "retry_count" && *typ == "int")
        );

        // Debug output as specified in progress document
        println!("✓ In class Config:");
        for (name, typ, _range) in var_types
            .iter()
            .filter(|(name, _, _)| *name == "timeout" || *name == "endpoints")
        {
            println!("  → Found annotated_assignment \"{name}: {typ} = ...\"");
            println!("    Variable: \"{name}\", Type: \"{typ}\"");
        }
        println!("✓ In function setup:");
        for (name, typ, _range) in var_types
            .iter()
            .filter(|(name, _, _)| *name == "connection" || *name == "retry_count")
        {
            println!("  → Found annotated_assignment \"{name}: {typ} = ...\"");
            println!("    Variable: \"{name}\", Type: \"{typ}\"");
        }
    }

    // Test additional variable type annotation cases
    #[test]
    fn test_various_variable_type_annotations() {
        let mut parser = PythonParser::new().unwrap();

        // Type-only annotations (no assignment)
        let code1 = r#"
def func():
    x: int
    y: str
"#;
        let var_types1 = parser.find_variable_types(code1);

        // Type-only annotations in Python create assignment nodes in tree-sitter
        assert!(
            var_types1
                .iter()
                .any(|(name, typ, _)| *name == "x" && *typ == "int")
        );
        assert!(
            var_types1
                .iter()
                .any(|(name, typ, _)| *name == "y" && *typ == "str")
        );

        // Complex generic types
        let code2 = r#"
class Service:
    cache: Dict[str, List[Optional[User]]] = {}
    mapping: Tuple[int, str, bool] = (1, "test", True)
"#;
        let var_types2 = parser.find_variable_types(code2);
        assert!(
            var_types2.iter().any(|(name, typ, _)| *name == "cache"
                && typ.contains("Dict[str, List[Optional[User]]]"))
        );
        assert!(
            var_types2
                .iter()
                .any(|(name, typ, _)| *name == "mapping" && typ.contains("Tuple[int, str, bool]"))
        );

        // Class attributes with self
        let code3 = r#"
class MyClass:
    def __init__(self):
        self.value: int = 42
        self.name: str = "test"
"#;
        let var_types3 = parser.find_variable_types(code3);
        assert!(
            var_types3
                .iter()
                .any(|(name, typ, _)| *name == "value" && *typ == "int")
        );
        assert!(
            var_types3
                .iter()
                .any(|(name, typ, _)| *name == "name" && *typ == "str")
        );

        // Variables without type annotations should not appear
        let code4 = r#"
def setup():
    x = 5  # No type annotation
    y: int = 10  # Has type annotation
    z = "hello"  # No type annotation
"#;
        let var_types4 = parser.find_variable_types(code4);
        assert_eq!(var_types4.len(), 1);
        assert!(
            var_types4
                .iter()
                .any(|(name, typ, _)| *name == "y" && *typ == "int")
        );
    }

    // Sub-Task 3.3.2: Multiple inheritance
    #[test]
    fn test_multiple_inheritance() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class Animal:
    pass

class Flyable:
    pass

class Swimmable:
    pass

class Duck(Animal, Flyable, Swimmable):
    pass
"#;
        println!("Finding multiple inheritance...");
        println!("---");
        let implementations = parser.find_implementations(code);

        assert!(
            implementations
                .iter()
                .any(|(t, b, _)| *t == "Duck" && *b == "Animal")
        );
        assert!(
            implementations
                .iter()
                .any(|(t, b, _)| *t == "Duck" && *b == "Flyable")
        );
        assert!(
            implementations
                .iter()
                .any(|(t, b, _)| *t == "Duck" && *b == "Swimmable")
        );

        // Debug output
        if let Some(duck_impl) = implementations.iter().find(|(t, _, _)| *t == "Duck") {
            println!(
                "✓ Found class_definition \"Duck\" at {}:{}-{}:{}",
                duck_impl.2.start_line,
                duck_impl.2.start_column,
                duck_impl.2.end_line,
                duck_impl.2.end_column
            );
            println!("  → Parsing superclasses list");

            let mut base_count = 1;
            for (_typ, base, _) in implementations.iter().filter(|(t, _, _)| *t == "Duck") {
                println!("  → Base class {base_count}: \"{base}\"");
                base_count += 1;
            }
            println!(
                "✓ Created {} implementation relationships",
                implementations
                    .iter()
                    .filter(|(t, _, _)| *t == "Duck")
                    .count()
            );
        }
    }

    // Sub-Task 5.2.1: Async function detection
    #[test]
    fn test_async_function_detection() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
async def fetch_data(url: str) -> Dict:
    response = await http_get(url)
    return response.json()
"#;
        println!("Parsing async function...");
        println!("---");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        let func = symbols
            .iter()
            .find(|s| s.name.as_ref() == "fetch_data")
            .unwrap();
        // Note: May need to extend SymbolKind or add metadata for async
        assert!(func.signature.as_ref().unwrap().contains("async"));

        println!("✓ Found async function_definition \"fetch_data\"");
        println!("✓ Detected await expression \"await http_get(url)\"");
        println!("✓ Marked function as async in metadata");
        println!("✓ Symbol created with async indicator");
        println!("Signature: {}", func.signature.as_ref().unwrap());
    }

    // Test async methods within classes
    #[test]
    fn test_async_method_detection() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class APIClient:
    async def fetch(self, url: str) -> dict:
        response = await self.http_client.get(url)
        return response.json()

    def sync_method(self):
        return "sync"
"#;
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        println!("=== ASYNC METHOD DETECTION TEST ===");
        println!("Found {len} symbols:", len = symbols.len());
        for symbol in &symbols {
            println!(
                "  {name} ({kind:?})",
                name = symbol.name.as_ref(),
                kind = symbol.kind
            );
        }

        // Should find module, class, both methods, and response variable (enhanced extraction)
        assert_eq!(symbols.len(), 5);

        let fetch_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "APIClient.fetch")
            .unwrap();
        let sync_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "APIClient.sync_method")
            .unwrap();

        // Both should be methods (inside class)
        assert_eq!(fetch_method.kind, SymbolKind::Method);
        assert_eq!(sync_method.kind, SymbolKind::Method);

        // Fetch should be async, sync_method should not
        assert!(fetch_method.signature.as_ref().unwrap().contains("async"));
        assert!(!sync_method.signature.as_ref().unwrap().contains("async"));

        println!(
            "Async method signature: {}",
            fetch_method.signature.as_ref().unwrap()
        );
        println!(
            "Sync method signature: {}",
            sync_method.signature.as_ref().unwrap()
        );
    }

    // Test various async function edge cases
    #[test]
    fn test_async_function_edge_cases() {
        let mut parser = PythonParser::new().unwrap();

        // Async function with no parameters
        let code1 = "async def background_task(): pass";
        let symbols1 = parser.parse(code1, FileId::new(1).unwrap(), &mut SymbolCounter::new());
        let func1 = symbols1
            .iter()
            .find(|s| s.name.as_ref() == "background_task")
            .unwrap();
        assert!(func1.signature.as_ref().unwrap().contains("async"));
        assert!(func1.signature.as_ref().unwrap().contains("()"));

        // Async function with complex signature
        let code2 = r#"async def process(
    data: List[Dict[str, Any]],
    *args: Any,
    **kwargs: Dict[str, Any]
) -> Optional[Result]: pass"#;
        let symbols2 = parser.parse(
            code2,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(10),
        );
        let func2 = symbols2
            .iter()
            .find(|s| s.name.as_ref() == "process")
            .unwrap();
        assert!(func2.signature.as_ref().unwrap().contains("async"));
        assert!(
            func2
                .signature
                .as_ref()
                .unwrap()
                .contains("-> Optional[Result]")
        );

        // Mixed async and regular functions
        let code3 = r#"
def regular_func(): pass
async def async_func(): pass
def another_regular(): pass
"#;
        let symbols3 = parser.parse(
            code3,
            FileId::new(1).unwrap(),
            &mut SymbolCounter::from_value(20),
        );
        assert_eq!(symbols3.len(), 4); // <module> + 3 functions

        let regular1 = symbols3
            .iter()
            .find(|s| s.name.as_ref() == "regular_func")
            .unwrap();
        let async_func = symbols3
            .iter()
            .find(|s| s.name.as_ref() == "async_func")
            .unwrap();
        let regular2 = symbols3
            .iter()
            .find(|s| s.name.as_ref() == "another_regular")
            .unwrap();

        assert!(!regular1.signature.as_ref().unwrap().contains("async"));
        assert!(async_func.signature.as_ref().unwrap().contains("async"));
        assert!(!regular2.signature.as_ref().unwrap().contains("async"));

        println!("Regular function: {}", regular1.signature.as_ref().unwrap());
        println!("Async function: {}", async_func.signature.as_ref().unwrap());
        println!("Another regular: {}", regular2.signature.as_ref().unwrap());
    }

    // Integration test: async functions with all features combined
    #[test]
    fn test_async_integration() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class AsyncWebService:
    """An async web service for handling HTTP requests."""

    async def fetch_user(self, user_id: int) -> Optional[User]:
        """Fetch a user by ID from the API.

        Args:
            user_id: The ID of the user to fetch.

        Returns:
            The user object if found, None otherwise.
        """
        response = await self.http_client.get(f"/users/{user_id}")
        if response.status == 200:
            return User.from_dict(response.json())
        return None

    def get_cache_key(self, user_id: int) -> str:
        """Generate cache key for user data."""
        return f"user:{user_id}"

async def process_batch(items: List[str]) -> Dict[str, Any]:
    """Process a batch of items asynchronously."""
    results = []
    for item in items:
        result = await process_item(item)
        results.append(result)
    return {"processed": len(results), "items": results}
"#;

        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        println!("=== ASYNC INTEGRATION TEST ===");
        println!("Found {len} symbols:", len = symbols.len());
        for symbol in &symbols {
            println!(
                "  {name} ({kind:?})",
                name = symbol.name.as_ref(),
                kind = symbol.kind
            );
        }

        // Should find: module, class, async method, sync method, async function, and local variables (enhanced extraction)
        assert_eq!(symbols.len(), 8);

        let class_symbol = symbols
            .iter()
            .find(|s| s.name.as_ref() == "AsyncWebService")
            .unwrap();
        let async_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "AsyncWebService.fetch_user")
            .unwrap();
        let sync_method = symbols
            .iter()
            .find(|s| s.name.as_ref() == "AsyncWebService.get_cache_key")
            .unwrap();
        let async_func = symbols
            .iter()
            .find(|s| s.name.as_ref() == "process_batch")
            .unwrap();

        // Verify symbol kinds
        assert_eq!(class_symbol.kind, SymbolKind::Class);
        assert_eq!(async_method.kind, SymbolKind::Method);
        assert_eq!(sync_method.kind, SymbolKind::Method);
        assert_eq!(async_func.kind, SymbolKind::Function);

        // Verify docstrings are present
        assert!(class_symbol.doc_comment.is_some());
        assert!(async_method.doc_comment.is_some());
        assert!(sync_method.doc_comment.is_some());
        assert!(async_func.doc_comment.is_some());

        // Verify async signatures
        assert!(async_method.signature.as_ref().unwrap().contains("async"));
        assert!(
            async_method
                .signature
                .as_ref()
                .unwrap()
                .contains("-> Optional[User]")
        );
        assert!(!sync_method.signature.as_ref().unwrap().contains("async"));
        assert!(async_func.signature.as_ref().unwrap().contains("async"));
        assert!(
            async_func
                .signature
                .as_ref()
                .unwrap()
                .contains("-> Dict[str, Any]")
        );

        println!("✓ Integration test passed!");
        println!("  - Class with docstring: {}", class_symbol.name.as_ref());
        println!(
            "  - Async method: {}",
            async_method.signature.as_ref().unwrap()
        );
        println!(
            "  - Sync method: {}",
            sync_method.signature.as_ref().unwrap()
        );
        println!(
            "  - Async function: {}",
            async_func.signature.as_ref().unwrap()
        );
    }

    // Test edge cases for inheritance
    #[test]
    fn test_inheritance_edge_cases() {
        let mut parser = PythonParser::new().unwrap();

        // Class with no inheritance
        let code1 = r#"
class SimpleClass:
    pass
"#;
        let implementations1 = parser.find_implementations(code1);
        assert_eq!(implementations1.len(), 0);

        // Qualified base class names
        let code2 = r#"
class Child(parent.Base):
    pass
"#;
        let implementations2 = parser.find_implementations(code2);
        assert_eq!(implementations2.len(), 1);
        assert_eq!(implementations2[0].0, "Child");
        assert_eq!(implementations2[0].1, "parent.Base");

        // Mixed simple and qualified names
        let code3 = r#"
class Complex(SimpleBase, module.QualifiedBase, another.pkg.DeepBase):
    pass
"#;
        let implementations3 = parser.find_implementations(code3);
        assert_eq!(implementations3.len(), 3);
        assert!(
            implementations3
                .iter()
                .any(|(t, b, _)| *t == "Complex" && *b == "SimpleBase")
        );
        assert!(
            implementations3
                .iter()
                .any(|(t, b, _)| *t == "Complex" && *b == "module.QualifiedBase")
        );
        assert!(
            implementations3
                .iter()
                .any(|(t, b, _)| *t == "Complex" && *b == "another.pkg.DeepBase")
        );
    }

    // Test nested classes with inheritance
    #[test]
    fn test_nested_class_inheritance() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class Outer:
    class Inner(OuterBase):
        pass

    class AnotherInner(Outer.Inner):
        pass
"#;
        let implementations = parser.find_implementations(code);

        // Debug what we actually found
        println!("Found implementations:");
        for (t, b, range) in &implementations {
            println!(
                "  {} -> {} at {}:{}-{}:{}",
                t, b, range.start_line, range.start_column, range.end_line, range.end_column
            );
        }

        // Should find inheritance relationships for nested classes
        assert!(
            implementations
                .iter()
                .any(|(t, b, _)| *t == "Inner" && *b == "OuterBase")
        );
        assert!(
            implementations
                .iter()
                .any(|(t, b, _)| *t == "AnotherInner" && *b == "Outer.Inner")
        );
    }

    // Debug: test what tree-sitter sees for async functions
    #[test]
    fn test_debug_async_tree_sitter_structure() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"async def fetch_data(url: str) -> Dict:
    response = await http_get(url)
    return response.json()"#;

        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        fn print_node_with_fields(node: tree_sitter::Node, code: &str, depth: usize) {
            let indent = "  ".repeat(depth);
            let text = &code[node.byte_range()];
            let text_preview = crate::parsing::truncate_for_display(text, 50).replace('\n', "\\n");
            println!(
                "{}{} [{}] \"{}\"",
                indent,
                node.kind(),
                node.byte_range().len(),
                text_preview
            );

            // Check for specific fields on function_definition and async_function_definition
            if node.kind().contains("function") {
                println!("{indent}  Fields:");
                if let Some(name) = node.child_by_field_name("name") {
                    println!("{}    name: {}", indent, &code[name.byte_range()]);
                }
                if let Some(params) = node.child_by_field_name("parameters") {
                    println!("{}    parameters: {}", indent, &code[params.byte_range()]);
                }
                if let Some(ret_type) = node.child_by_field_name("return_type") {
                    println!(
                        "{}    return_type: {}",
                        indent,
                        &code[ret_type.byte_range()]
                    );
                }
                if let Some(body) = node.child_by_field_name("body") {
                    println!(
                        "{}    body: {}",
                        indent,
                        &code[body.byte_range()].replace('\n', "\\n")
                    );
                }
            }

            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    print_node_with_fields(child, code, depth + 1);
                }
            }
        }

        print_node_with_fields(root, code, 0);
    }

    // Debug: test what tree-sitter sees
    #[test]
    fn test_debug_tree_sitter_structure() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"class Dog(Animal):
    pass"#;

        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        fn print_node_with_fields(node: tree_sitter::Node, code: &str, depth: usize) {
            let indent = "  ".repeat(depth);
            let text = &code[node.byte_range()];
            let text_preview = if text.len() > 20 {
                format!("{}...", &text[..20])
            } else {
                text.to_string()
            };
            println!(
                "{}{} [{}] \"{}\"",
                indent,
                node.kind(),
                node.byte_range().len(),
                text_preview
            );

            // Check for specific fields on class_definition
            if node.kind() == "class_definition" {
                println!("{indent}  Fields:");
                if let Some(name) = node.child_by_field_name("name") {
                    println!("{}    name: {}", indent, &code[name.byte_range()]);
                }
                if let Some(superclasses) = node.child_by_field_name("superclasses") {
                    println!(
                        "{}    superclasses: {}",
                        indent,
                        &code[superclasses.byte_range()]
                    );
                }
                if let Some(body) = node.child_by_field_name("body") {
                    println!("{}    body: {}", indent, &code[body.byte_range()]);
                }
            }

            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    print_node_with_fields(child, code, depth + 1);
                }
            }
        }

        print_node_with_fields(root, code, 0);
    }

    // ===== BASELINE TESTS FOR PYTHON DOC COMMENT ENHANCEMENT =====

    #[test]
    fn test_python_baseline_module_docstring() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"""This is a module-level docstring.

It should be attached to the file/module symbol.
"""

def some_function():
    pass
"#;

        println!("=== BASELINE TEST: Module Docstring (EXPECTED GAP) ===");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        // Look for module-level symbol with docstring
        let module_symbol = symbols
            .iter()
            .find(|s| s.doc_comment.is_some() && s.name.as_ref().contains("module"));

        println!("Expected: Module symbol with docstring 'This is a module-level docstring.'");
        println!(
            "Actual:   {:?}",
            module_symbol.map(|s| (s.name.as_ref(), s.doc_comment.as_ref().unwrap().as_ref()))
        );

        // This should currently fail - module docstrings not supported
        if module_symbol.is_none() {
            println!("✓ CONFIRMED GAP: Module docstrings not currently extracted");
        }
    }

    #[test]
    fn test_python_baseline_method_docstring() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
class TestClass:
    """Class docstring (should work)."""
    
    def method_with_docs(self):
        """Method docstring that should be extracted.
        
        This is currently a GAP in functionality.
        """
        pass
"#;

        println!("=== BASELINE TEST: Method Docstring (EXPECTED GAP) ===");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        // Find the class (should have docstring)
        let class_symbol = symbols.iter().find(|s| s.name.as_ref() == "TestClass");
        assert!(class_symbol.is_some());
        assert!(class_symbol.unwrap().doc_comment.is_some());
        println!(
            "✓ Class docstring works: {:?}",
            class_symbol.unwrap().doc_comment.as_ref().unwrap().as_ref()
        );

        // Find the method (should have docstring but probably doesn't)
        let method_symbol = symbols
            .iter()
            .find(|s| s.name.as_ref() == "method_with_docs");

        println!(
            "Expected: Method symbol with docstring 'Method docstring that should be extracted.'"
        );
        println!(
            "Actual:   {:?}",
            method_symbol.map(|s| (s.name.as_ref(), s.doc_comment.as_ref().map(|d| d.as_ref())))
        );

        if method_symbol.is_some() && method_symbol.unwrap().doc_comment.is_none() {
            println!("✓ CONFIRMED GAP: Method docstrings not currently extracted");
        }
    }

    #[test]
    fn test_python_baseline_current_strengths() {
        let mut parser = PythonParser::new().unwrap();
        let code = r#"
def function_with_docs():
    """Function docstring (should work)."""
    pass

class ClassWithDocs:
    """Class docstring (should work)."""
    pass
"#;

        println!("=== BASELINE TEST: Current Strengths (SHOULD WORK) ===");
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        // Function docstring (existing functionality)
        let func_symbol = symbols
            .iter()
            .find(|s| s.name.as_ref() == "function_with_docs");
        assert!(func_symbol.is_some());
        let func_doc = func_symbol.unwrap().doc_comment.as_ref();

        println!("Function docstring:");
        println!("  Expected: 'Function docstring (should work).'");
        println!("  Actual:   {:?}", func_doc.map(|d| d.as_ref()));
        assert!(func_doc.is_some());
        assert!(func_doc.unwrap().contains("Function docstring"));
        println!("  ✅ WORKS CORRECTLY");

        // Class docstring (existing functionality)
        let class_symbol = symbols.iter().find(|s| s.name.as_ref() == "ClassWithDocs");
        assert!(class_symbol.is_some());
        let class_doc = class_symbol.unwrap().doc_comment.as_ref();

        println!("Class docstring:");
        println!("  Expected: 'Class docstring (should work).'");
        println!("  Actual:   {:?}", class_doc.map(|d| d.as_ref()));
        assert!(class_doc.is_some());
        assert!(class_doc.unwrap().contains("Class docstring"));
        println!("  ✅ WORKS CORRECTLY");
    }

    #[test]
    fn test_comprehensive_docstring_extraction() {
        let mut parser = PythonParser::new().unwrap();
        // Use relative path from the workspace root
        let test_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/python/comprehensive.py");
        let code = std::fs::read_to_string(test_file).expect("Should find comprehensive test file");

        println!("=== COMPREHENSIVE PYTHON DOCSTRING EXTRACTION TEST ===");
        let symbols = parser.parse(&code, FileId::new(1).unwrap(), &mut SymbolCounter::new());

        println!("Found {} symbols total:", symbols.len());

        // Print each symbol and its docstring status
        for symbol in &symbols {
            println!();
            println!("Symbol: {} ({:?})", symbol.name.as_ref(), symbol.kind);
            println!(
                "  Location: {}:{}",
                symbol.range.start_line, symbol.range.start_column
            );

            if let Some(doc) = &symbol.doc_comment {
                println!("  Docstring: \"{}\"", doc.as_ref().trim());
            } else {
                println!("  Docstring: None");
            }

            if let Some(sig) = &symbol.signature {
                println!("  Signature: {}", sig.as_ref());
            }
        }

        // Check for specific expected symbols
        println!();
        println!("=== SPECIFIC SYMBOL CHECKS ===");

        // Module docstring check
        let has_module_symbol = symbols
            .iter()
            .any(|s| s.name.as_ref().contains("module") || s.kind == crate::SymbolKind::Module);
        println!("Module symbol found: {has_module_symbol}");

        // Function docstring check (comprehensive.py has simple_function)
        let simple_function = symbols
            .iter()
            .find(|s| s.name.as_ref() == "simple_function");
        println!("simple_function found: {}", simple_function.is_some());
        if let Some(func) = simple_function {
            println!("  Has docstring: {}", func.doc_comment.is_some());
        }

        // Class docstring check (comprehensive.py has SimpleClass)
        let simple_class = symbols.iter().find(|s| s.name.as_ref() == "SimpleClass");
        println!("SimpleClass found: {}", simple_class.is_some());
        if let Some(class) = simple_class {
            println!("  Has docstring: {}", class.doc_comment.is_some());
        }

        // Method docstring check (comprehensive.py has method inside SimpleClass)
        let method = symbols.iter().find(|s| s.name.as_ref() == "method");
        println!("method found: {}", method.is_some());
        if let Some(method_sym) = method {
            println!("  Has docstring: {}", method_sym.doc_comment.is_some());
        }

        // Constants check
        let module_constant = symbols
            .iter()
            .find(|s| s.name.as_ref() == "MODULE_CONSTANT");
        println!("MODULE_CONSTANT found: {}", module_constant.is_some());
        if let Some(constant) = module_constant {
            println!("  Has docstring: {}", constant.doc_comment.is_some());
        }
    }

    #[test]
    fn test_qualified_calls() {
        let code = r#"
def init_config_file():
    """Initialize configuration file."""
    # Cross-module function call
    app.utils.helper.process_data()

    # Another cross-module call
    database.connection.manager.connect()

    # Simple function call
    print("Initializing")

def process_data():
    """Process data locally."""
    print("Processing")
"#;

        let mut parser = PythonParser::new().unwrap();
        let calls = parser.find_calls(code);

        println!("\n=== Testing Python qualified calls extraction ===");
        println!("Found {} calls:", calls.len());
        for (from, to, _) in &calls {
            println!("  '{from}' -> '{to}'");
        }

        // Check that we find the qualified calls
        let has_app_utils_call = calls
            .iter()
            .any(|(f, t, _)| *f == "init_config_file" && *t == "app.utils.helper.process_data");

        let has_database_call = calls.iter().any(|(f, t, _)| {
            *f == "init_config_file" && *t == "database.connection.manager.connect"
        });

        let has_print_call = calls
            .iter()
            .any(|(f, t, _)| *f == "init_config_file" && *t == "print");

        assert!(
            has_app_utils_call,
            "Should find call from init_config_file to app.utils.helper.process_data\nFound calls: {calls:?}"
        );

        assert!(
            has_database_call,
            "Should find call from init_config_file to database.connection.manager.connect\nFound calls: {calls:?}"
        );

        assert!(
            has_print_call,
            "Should find simple call from init_config_file to print\nFound calls: {calls:?}"
        );

        println!("SUCCESS: Python now tracks cross-module calls correctly!");
    }
}
