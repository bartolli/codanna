//! Go parser implementation
//!
//! Uses tree-sitter-go crate’s LANGUAGE constant (converted via .into()).
//!
//! Note: This parser uses ABI-15 (upgraded from ABI-14).
//! When migrating or updating the parser, ensure compatibility with ABI-15 features.

use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, LanguageParser, MethodCall, NodeTracker, NodeTrackingState, ParserContext,
    ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use tree_sitter::{Node, Parser};

use super::resolution::GoResolutionContext;

/// Go language parser
pub struct GoParser {
    parser: Parser,
    context: ParserContext,
    resolution_context: Option<GoResolutionContext>,
    node_tracker: NodeTrackingState,
}

impl GoParser {
    /// Parse Go source code and extract all symbols (functions, structs, interfaces, variables, etc.)
    ///
    /// This method handles the complete Go language syntax including:
    /// - Package declarations and imports
    /// - Function and method declarations (with receivers)
    /// - Struct and interface type definitions
    /// - Variable and constant declarations
    /// - Generic types and constraints (Go 1.18+)
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        // Reset context for each file
        self.context = ParserContext::new();
        self.resolution_context = Some(GoResolutionContext::new(file_id));
        let mut symbols = Vec::new();

        match self.parser.parse(code, None) {
            Some(tree) => {
                let root_node = tree.root_node();
                self.extract_symbols_from_node(
                    root_node,
                    code,
                    file_id,
                    symbol_counter,
                    &mut symbols,
                    "", // Module path will be determined by behavior
                    0,
                );
            }
            None => {
                eprintln!("Failed to parse Go file");
            }
        }

        symbols
    }

    /// Helper to create a symbol with all optional fields
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

        // Set scope context based on parser's current scope
        symbol.scope_context = Some(self.context.current_scope_context());

        symbol
    }

    /// Create a new Go parser
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        let lang = tree_sitter_go::LANGUAGE;
        parser
            .set_language(&lang.into())
            .map_err(|e| format!("Failed to set Go language: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            resolution_context: None,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Extract symbols from a Go AST node recursively
    ///
    /// This is the main symbol extraction method that handles all Go language constructs:
    /// - Functions and methods (with receivers)
    /// - Type declarations (structs, interfaces, type aliases)
    /// - Variable and constant declarations
    /// - Maintains proper scope context and parent-child relationships
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
                // Extract function name for parent tracking
                let func_name = node
                    .child_by_field_name("name")
                    .map(|n| code[n.byte_range()].to_string());

                if let Some(symbol) =
                    self.process_function(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                }
                // Note: In Go, function declarations are hoisted
                // But we process nested symbols in the function's scope
                self.context.enter_scope(ScopeType::hoisting_function());

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());
                // Set current function for parent tracking BEFORE processing children
                self.context.set_current_function(func_name.clone());

                // Process function parameters
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.process_method_parameters(
                        params,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                    );
                }

                // Process children for nested functions and body
                for child in node.children(&mut node.walk()) {
                    if child.kind() != "identifier"
                        && child.kind() != "parameter_list"
                        && child.kind() != "parameters"
                    {
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

                // Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "method_declaration" => {
                self.register_node_recursively(node);
                // Extract method name for parent tracking
                let method_name = node
                    .child_by_field_name("name")
                    .map(|n| code[n.byte_range()].to_string());

                if let Some(symbol) =
                    self.process_method_declaration(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                }

                // Enter method scope for processing nested symbols
                self.context.enter_scope(ScopeType::hoisting_function());

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());
                // Set current function for parent tracking
                self.context.set_current_function(method_name.clone());

                // Process method receiver to add receiver scope
                if let Some(receiver) = node.child_by_field_name("receiver") {
                    self.process_method_receiver(
                        receiver,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                    );
                }

                // Process method parameters
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.process_method_parameters(
                        params,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                    );
                }

                // Process children (body, etc.)
                for child in node.children(&mut node.walk()) {
                    if child.kind() != "identifier"
                        && child.kind() != "parameter_list"
                        && child.kind() != "parameters"
                        && child.kind() != "receiver"
                    // Skip receiver, already processed
                    {
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

                // Exit scope and restore context
                self.context.exit_scope();
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "type_declaration" => {
                self.register_node_recursively(node);
                self.process_type_declaration(node, code, file_id, counter, symbols, module_path);
            }
            "var_declaration" => {
                self.register_node_recursively(node);
                self.process_var_declaration(node, code, file_id, counter, symbols, module_path);
            }
            "const_declaration" => {
                self.register_node_recursively(node);
                self.process_const_declaration(node, code, file_id, counter, symbols, module_path);
            }
            "package_clause" => {
                self.register_node_recursively(node);
                // Package name is handled by behavior module
            }
            "import_declaration" => {
                self.register_node_recursively(node);
                // Imports are processed by find_imports() method
            }
            "if_statement" => {
                self.register_handled_node("if_statement", node.kind_id());
                // Enter block scope for if statement
                self.context.enter_scope(ScopeType::Block);

                // Process if statement parts (condition, body, else)
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
            "for_statement" => {
                self.register_handled_node("for_statement", node.kind_id());
                // Enter block scope for for loop
                self.context.enter_scope(ScopeType::Block);

                // Check for range clause specifically
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "range_clause" {
                        self.process_range_clause(
                            child,
                            code,
                            file_id,
                            counter,
                            symbols,
                            module_path,
                            depth,
                        );
                    } else {
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

                self.context.exit_scope();
            }
            "switch_statement" | "type_switch_statement" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Enter block scope for switch statement
                self.context.enter_scope(ScopeType::Block);

                // Process switch statement parts
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
            "expression_case" | "default_case" | "type_case" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Enter block scope for switch case
                self.context.enter_scope(ScopeType::Block);

                // Process case body
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
                // Enter block scope for bare blocks
                self.context.enter_scope(ScopeType::Block);

                // Process block contents
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
            "short_var_declaration" => {
                self.register_handled_node("short_var_declaration", node.kind_id());
                // Process short variable declarations (:=) in current scope
                self.process_short_var_declaration(
                    node,
                    code,
                    file_id,
                    counter,
                    symbols,
                    module_path,
                );
            }
            _ => {
                // For unhandled node types, recursively process children
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
        }
    }

    /// Process a function declaration
    fn process_function(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let signature = self.extract_signature(node, code);
        let doc_comment = self.extract_doc_comment(&node, code);
        let visibility = self.determine_go_visibility(name);

        Some(self.create_symbol(
            counter.next_id(),
            name.to_string(),
            SymbolKind::Function,
            file_id,
            Range::new(
                node.start_position().row as u32,
                node.start_position().column as u16,
                node.end_position().row as u32,
                node.end_position().column as u16,
            ),
            Some(signature),
            doc_comment,
            module_path,
            visibility,
        ))
    }

    /// Process a Go type declaration (struct, interface, or type alias)
    fn process_type_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        // type_declaration contains type_spec nodes
        for child in node.children(&mut node.walk()) {
            if child.kind() == "type_spec" {
                self.register_handled_node("type_spec", child.kind_id());
                self.process_type_spec(child, code, file_id, counter, symbols, module_path);
            }
        }
    }

    /// Process a type_spec node (individual type definition)
    /// Process a Go type specification node
    ///
    /// Handles type definitions including structs, interfaces, and type aliases.
    /// Extracts type name, fields/methods, and generates appropriate signatures.
    fn process_type_spec(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let name = &code[name_node.byte_range()];
        let type_node = match node.child_by_field_name("type") {
            Some(n) => n,
            None => return,
        };

        match type_node.kind() {
            "struct_type" => {
                self.register_handled_node("struct_type", type_node.kind_id());
                // Handle struct type
                let signature = self.extract_struct_signature(node, code);
                let doc_comment = self.extract_doc_comment(&node, code);
                let visibility = self.determine_go_visibility(name);

                let symbol_id = counter.next_id();

                // Extract generic params before borrowing issues
                let generic_params = self.extract_generic_params_from_signature(&signature);

                let symbol = self.create_symbol(
                    symbol_id,
                    name.to_string(),
                    SymbolKind::Struct,
                    file_id,
                    Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    ),
                    Some(signature),
                    doc_comment,
                    module_path,
                    visibility,
                );

                // Register type in resolution context
                if let Some(ref mut res_ctx) = self.resolution_context {
                    use super::resolution::{TypeCategory, TypeInfo};
                    let type_info = TypeInfo {
                        name: name.to_string(),
                        symbol_id: Some(symbol_id),
                        package_path: Some(module_path.to_string()),
                        is_exported: visibility == Visibility::Public,
                        category: TypeCategory::Struct,
                        generic_params,
                        constraints: std::collections::HashMap::new(),
                    };
                    res_ctx.register_type(type_info);
                }

                symbols.push(symbol);

                // Extract struct fields
                self.extract_struct_fields(
                    type_node,
                    code,
                    file_id,
                    counter,
                    symbols,
                    module_path,
                    name,
                );
            }
            "interface_type" => {
                self.register_handled_node("interface_type", type_node.kind_id());
                // Handle interface type
                let signature = self.extract_interface_signature(node, code);
                let doc_comment = self.extract_doc_comment(&node, code);
                let visibility = self.determine_go_visibility(name);

                let symbol_id = counter.next_id();

                // Extract generic params before borrowing issues
                let generic_params = self.extract_generic_params_from_signature(&signature);

                let symbol = self.create_symbol(
                    symbol_id,
                    name.to_string(),
                    SymbolKind::Interface,
                    file_id,
                    Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    ),
                    Some(signature),
                    doc_comment,
                    module_path,
                    visibility,
                );

                // Register type in resolution context
                if let Some(ref mut res_ctx) = self.resolution_context {
                    use super::resolution::{TypeCategory, TypeInfo};
                    let type_info = TypeInfo {
                        name: name.to_string(),
                        symbol_id: Some(symbol_id),
                        package_path: Some(module_path.to_string()),
                        is_exported: visibility == Visibility::Public,
                        category: TypeCategory::Interface,
                        generic_params,
                        constraints: std::collections::HashMap::new(),
                    };
                    res_ctx.register_type(type_info);
                }

                symbols.push(symbol);

                // Extract interface methods
                self.extract_interface_methods(
                    type_node,
                    code,
                    file_id,
                    counter,
                    symbols,
                    module_path,
                    name,
                );
            }
            _ => {
                // Handle type alias
                let signature = &code[node.byte_range()];
                let doc_comment = self.extract_doc_comment(&node, code);
                let visibility = self.determine_go_visibility(name);

                let symbol_id = counter.next_id();

                // Extract generic params before borrowing issues
                let generic_params = self.extract_generic_params_from_signature(signature);

                let symbol = self.create_symbol(
                    symbol_id,
                    name.to_string(),
                    SymbolKind::TypeAlias,
                    file_id,
                    Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    ),
                    Some(signature.to_string()),
                    doc_comment,
                    module_path,
                    visibility,
                );

                // Register type in resolution context
                if let Some(ref mut res_ctx) = self.resolution_context {
                    use super::resolution::{TypeCategory, TypeInfo};
                    let type_info = TypeInfo {
                        name: name.to_string(),
                        symbol_id: Some(symbol_id),
                        package_path: Some(module_path.to_string()),
                        is_exported: visibility == Visibility::Public,
                        category: TypeCategory::Alias,
                        generic_params,
                        constraints: std::collections::HashMap::new(),
                    };
                    res_ctx.register_type(type_info);
                }

                symbols.push(symbol);
            }
        }
    }

    /// Extract struct fields from a struct_type node
    fn extract_struct_fields(
        &mut self,
        struct_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        struct_name: &str, // Used for generating qualified field names (e.g., StructName.FieldName)
    ) {
        // Look for field_declaration_list
        for child in struct_node.children(&mut struct_node.walk()) {
            if child.kind() == "field_declaration_list" {
                for field_child in child.children(&mut child.walk()) {
                    if field_child.kind() == "field_declaration" {
                        self.register_handled_node("field_declaration", field_child.kind_id());
                        self.process_struct_field(
                            field_child,
                            code,
                            file_id,
                            counter,
                            symbols,
                            module_path,
                            struct_name,
                        );
                    }
                }
            }
        }
    }

    /// Process a single struct field declaration
    fn process_struct_field(
        &mut self,
        field_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        struct_name: &str, // Used for generating qualified field names (e.g., StructName.FieldName)
    ) {
        // field_declaration may have multiple field names for the same type
        // e.g., "Width, Height float64"
        let mut field_names = Vec::new();
        let mut field_type = None;

        for child in field_node.children(&mut field_node.walk()) {
            match child.kind() {
                "field_identifier" => {
                    field_names.push(&code[child.byte_range()]);
                }
                "type_identifier" | "pointer_type" | "array_type" | "slice_type" | "map_type"
                | "channel_type" => {
                    field_type = Some(&code[child.byte_range()]);
                }
                _ => {}
            }
        }

        // Create symbols for each field name
        for field_name in field_names {
            let visibility = self.determine_go_visibility(field_name);
            let signature = match field_type {
                Some(typ) => format!("{field_name} {typ}"),
                None => field_name.to_string(),
            };

            // Generate qualified field name for better disambiguation
            let qualified_name = format!("{struct_name}.{field_name}");

            let symbol = self.create_symbol(
                counter.next_id(),
                qualified_name,
                SymbolKind::Field,
                file_id,
                Range::new(
                    field_node.start_position().row as u32,
                    field_node.start_position().column as u16,
                    field_node.end_position().row as u32,
                    field_node.end_position().column as u16,
                ),
                Some(signature),
                None,
                module_path,
                visibility,
            );
            symbols.push(symbol);
        }
    }

    /// Extract interface methods from an interface_type node
    fn extract_interface_methods(
        &mut self,
        interface_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        interface_name: &str, // Used for generating qualified method names for interface methods
    ) {
        // Look for method_elem nodes
        for child in interface_node.children(&mut interface_node.walk()) {
            if child.kind() == "method_elem" {
                self.process_interface_method(
                    child,
                    code,
                    file_id,
                    counter,
                    symbols,
                    module_path,
                    interface_name,
                );
            }
        }
    }

    /// Process a single interface method element
    fn process_interface_method(
        &mut self,
        method_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        interface_name: &str, // Used for generating qualified method names for interface methods
    ) {
        let method_name = method_node
            .children(&mut method_node.walk())
            .find(|n| n.kind() == "field_identifier")
            .map(|n| &code[n.byte_range()]);

        if let Some(name) = method_name {
            let signature = &code[method_node.byte_range()];
            let visibility = self.determine_go_visibility(name);

            // Generate qualified method name for better disambiguation
            let qualified_name = format!("{interface_name}.{name}");

            let symbol = self.create_symbol(
                counter.next_id(),
                qualified_name,
                SymbolKind::Method,
                file_id,
                Range::new(
                    method_node.start_position().row as u32,
                    method_node.start_position().column as u16,
                    method_node.end_position().row as u32,
                    method_node.end_position().column as u16,
                ),
                Some(signature.to_string()),
                None,
                module_path,
                visibility,
            );
            symbols.push(symbol);
        }
    }

    /// Process a Go method declaration (function with receiver)
    fn process_method_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let signature = self.extract_method_signature(node, code);
        let doc_comment = self.extract_doc_comment(&node, code);
        let visibility = self.determine_go_visibility(name);

        Some(self.create_symbol(
            counter.next_id(),
            name.to_string(),
            SymbolKind::Method,
            file_id,
            Range::new(
                node.start_position().row as u32,
                node.start_position().column as u16,
                node.end_position().row as u32,
                node.end_position().column as u16,
            ),
            Some(signature),
            doc_comment,
            module_path,
            visibility,
        ))
    }

    /// Process Go variable declarations
    fn process_var_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        // var_declaration contains var_spec nodes
        for child in node.children(&mut node.walk()) {
            if child.kind() == "var_spec" {
                self.register_handled_node("var_spec", child.kind_id());
                self.process_var_spec(child, code, file_id, counter, symbols, module_path);
            }
        }
    }

    /// Process a single variable specification
    fn process_var_spec(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        let mut var_names = Vec::new();
        let mut var_type = None;

        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" => {
                    var_names.push(&code[child.byte_range()]);
                }
                "type_identifier" | "pointer_type" | "array_type" | "slice_type" | "map_type"
                | "channel_type" => {
                    var_type = Some(&code[child.byte_range()]);
                }
                _ => {}
            }
        }

        // Create symbols for each variable name
        for var_name in var_names {
            let visibility = self.determine_go_visibility(var_name);
            let signature = match var_type {
                Some(typ) => format!("var {var_name} {typ}"),
                None => format!("var {var_name}"),
            };

            let symbol = self.create_symbol(
                counter.next_id(),
                var_name.to_string(),
                SymbolKind::Variable,
                file_id,
                Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                ),
                Some(signature),
                None,
                module_path,
                visibility,
            );
            symbols.push(symbol);
        }
    }

    /// Process Go constant declarations
    fn process_const_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        // const_declaration contains const_spec nodes
        for child in node.children(&mut node.walk()) {
            if child.kind() == "const_spec" {
                self.register_handled_node("const_spec", child.kind_id());
                self.process_const_spec(child, code, file_id, counter, symbols, module_path);
            }
        }
    }

    /// Process a single constant specification
    fn process_const_spec(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        let mut const_names = Vec::new();
        let mut const_type = None;

        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" => {
                    const_names.push(&code[child.byte_range()]);
                }
                "type_identifier" | "pointer_type" | "array_type" | "slice_type" | "map_type"
                | "channel_type" => {
                    const_type = Some(&code[child.byte_range()]);
                }
                _ => {}
            }
        }

        // Create symbols for each constant name
        for const_name in const_names {
            let visibility = self.determine_go_visibility(const_name);
            let signature = match const_type {
                Some(typ) => format!("const {const_name} {typ}"),
                None => format!("const {const_name}"),
            };

            let symbol = self.create_symbol(
                counter.next_id(),
                const_name.to_string(),
                SymbolKind::Constant,
                file_id,
                Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                ),
                Some(signature),
                None,
                module_path,
                visibility,
            );
            symbols.push(symbol);
        }
    }

    /// Process Go short variable declarations (:=)
    fn process_short_var_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        // short_var_declaration format: identifiers := expressions
        let mut var_names = Vec::new();

        // Extract variable names (left side of :=)
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "expression_list" => {
                    // Handle multiple variables: a, b := 1, 2
                    for expr_child in child.children(&mut child.walk()) {
                        if expr_child.kind() == "identifier" {
                            var_names.push(&code[expr_child.byte_range()]);
                        }
                    }
                }
                "identifier" => {
                    // Handle single variable: a := 1
                    var_names.push(&code[child.byte_range()]);
                }
                _ => {}
            }
        }

        // Create symbols for each variable in the short declaration
        // These variables are created in the current scope (function/block scope)
        for var_name in var_names {
            let visibility = self.determine_go_visibility(var_name);
            let signature = format!("{var_name} := ...");

            let mut symbol = self.create_symbol(
                counter.next_id(),
                var_name.to_string(),
                SymbolKind::Variable,
                file_id,
                Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                ),
                Some(signature),
                None,
                module_path,
                visibility,
            );

            // Mark as local variable (block or function scope)
            symbol.scope_context = Some(crate::symbol::ScopeContext::Local {
                hoisted: false, // Go doesn't have hoisting
                parent_name: self.context.current_function().map(|s| s.into()),
                parent_kind: Some(SymbolKind::Function),
            });

            symbols.push(symbol);
        }
    }

    /// Process method receiver to track receiver scope
    fn process_method_receiver(
        &mut self,
        receiver_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        // Method receivers in Go are parameter lists: func (r *Type) method()
        // Process each receiver parameter in the receiver scope

        for child in receiver_node.children(&mut receiver_node.walk()) {
            if child.kind() == "parameter_declaration" {
                self.register_handled_node("parameter_declaration", child.kind_id());
                // Extract receiver name and type
                let mut receiver_name = None;
                let mut receiver_type = None;

                for param_child in child.children(&mut child.walk()) {
                    match param_child.kind() {
                        "identifier" => {
                            receiver_name = Some(&code[param_child.byte_range()]);
                        }
                        "type_identifier" | "pointer_type" => {
                            receiver_type = Some(&code[param_child.byte_range()]);
                        }
                        _ => {}
                    }
                }

                if let Some(name) = receiver_name {
                    let visibility = self.determine_go_visibility(name);
                    let signature = match receiver_type {
                        Some(typ) => format!("{name} {typ}"),
                        None => name.to_string(),
                    };

                    let mut symbol = self.create_symbol(
                        counter.next_id(),
                        name.to_string(),
                        SymbolKind::Parameter,
                        file_id,
                        Range::new(
                            child.start_position().row as u32,
                            child.start_position().column as u16,
                            child.end_position().row as u32,
                            child.end_position().column as u16,
                        ),
                        Some(signature),
                        None,
                        module_path,
                        visibility,
                    );

                    // Mark as method receiver parameter
                    symbol.scope_context = Some(crate::symbol::ScopeContext::Parameter);

                    symbols.push(symbol);
                }
            }
        }
    }

    /// Process method parameters to track parameter scope
    fn process_method_parameters(
        &mut self,
        params_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        // Method parameters in Go are parameter lists: func Method(param1 Type, param2 Type)
        // Process each parameter in the parameter scope

        for child in params_node.children(&mut params_node.walk()) {
            if child.kind() == "parameter_declaration" {
                self.register_handled_node("parameter_declaration", child.kind_id());
                // Extract parameter name and type
                let mut param_names = Vec::new();
                let mut param_type = None;

                for param_child in child.children(&mut child.walk()) {
                    match param_child.kind() {
                        "identifier" => {
                            param_names.push(&code[param_child.byte_range()]);
                        }
                        "type_identifier" | "pointer_type" | "array_type" | "slice_type"
                        | "map_type" | "channel_type" => {
                            param_type = Some(&code[param_child.byte_range()]);
                        }
                        _ => {}
                    }
                }

                // Create symbols for each parameter name
                for param_name in param_names {
                    let visibility = self.determine_go_visibility(param_name);
                    let signature = match param_type {
                        Some(typ) => format!("{param_name} {typ}"),
                        None => param_name.to_string(),
                    };

                    let mut symbol = self.create_symbol(
                        counter.next_id(),
                        param_name.to_string(),
                        SymbolKind::Parameter,
                        file_id,
                        Range::new(
                            child.start_position().row as u32,
                            child.start_position().column as u16,
                            child.end_position().row as u32,
                            child.end_position().column as u16,
                        ),
                        Some(signature),
                        None,
                        module_path,
                        visibility,
                    );

                    // Mark as method/function parameter
                    symbol.scope_context = Some(crate::symbol::ScopeContext::Parameter);

                    symbols.push(symbol);
                }
            }
        }
    }

    /// Process range clause to extract range variables (for index, value := range items)
    fn process_range_clause(
        &mut self,
        range_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
        depth: usize,
    ) {
        // Range clause format: index, value := range items
        // Extract the variable names from the left side
        let mut range_vars = Vec::new();

        for child in range_node.children(&mut range_node.walk()) {
            match child.kind() {
                "expression_list" => {
                    // Multiple variables: index, value
                    for expr_child in child.children(&mut child.walk()) {
                        if expr_child.kind() == "identifier" {
                            range_vars.push(&code[expr_child.byte_range()]);
                        }
                    }
                }
                "identifier" => {
                    // Single variable: index
                    range_vars.push(&code[child.byte_range()]);
                }
                _ => {
                    // Also process non-range parts (e.g., the iterable expression)
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

        // Create symbols for range variables (these are in for loop block scope)
        for (i, var_name) in range_vars.iter().enumerate() {
            let visibility = self.determine_go_visibility(var_name);
            let signature = if i == 0 {
                format!("{var_name} := range (index)")
            } else {
                format!("{var_name} := range (value)")
            };

            let mut symbol = self.create_symbol(
                counter.next_id(),
                var_name.to_string(),
                SymbolKind::Variable,
                file_id,
                Range::new(
                    range_node.start_position().row as u32,
                    range_node.start_position().column as u16,
                    range_node.end_position().row as u32,
                    range_node.end_position().column as u16,
                ),
                Some(signature),
                None,
                module_path,
                visibility,
            );

            // Mark as local variable in for loop scope
            symbol.scope_context = Some(crate::symbol::ScopeContext::Local {
                hoisted: false, // Go doesn't have hoisting
                parent_name: self.context.current_function().map(|s| s.into()),
                parent_kind: Some(SymbolKind::Function),
            });

            symbols.push(symbol);
        }
    }

    /// Determine Go visibility based on capitalization
    fn determine_go_visibility(&self, name: &str) -> Visibility {
        if let Some(first_char) = name.chars().next() {
            if first_char.is_uppercase() {
                Visibility::Public
            } else {
                Visibility::Private
            }
        } else {
            Visibility::Private
        }
    }

    /// Extract signature for struct types
    fn extract_struct_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the struct body and exclude it, keeping only the header
        if let Some(type_node) = node.child_by_field_name("type") {
            if let Some(body) = type_node
                .children(&mut type_node.walk())
                .find(|n| n.kind() == "field_declaration_list")
            {
                end = body.start_byte();
            }
        }

        code[start..end].trim().to_string()
    }

    /// Extract signature for Go methods (with receiver)
    fn extract_method_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Try to find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract function signature for Go functions
    fn extract_signature(&self, node: Node, code: &str) -> String {
        // Extract the signature without the body
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Try to find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract interface signature for Go interfaces
    fn extract_interface_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the interface body and exclude it, keeping only the declaration
        if let Some(type_node) = node.child_by_field_name("type") {
            if let Some(body_start) = type_node
                .children(&mut type_node.walk())
                .find(|n| n.kind() == "method_elem" || n.kind() == "type_elem")
                .map(|n| n.start_byte())
            {
                end = body_start.saturating_sub(2); // Account for the opening brace
            }
        }

        code[start..end].trim().to_string()
    }

    // Go uses implicit interface implementation (structural typing)
    // There are no explicit "implements" declarations to detect at parse time
    // Implementation detection would require semantic analysis across files

    /// Extract imports from AST node recursively
    ///
    /// Processes import_declaration nodes and delegates to specialized handlers
    /// for different Go import styles (single, grouped, aliased, etc.)
    fn extract_imports_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "import_declaration" => {
                self.process_go_import_declaration(node, code, file_id, imports);
            }
            _ => {
                // Recurse into children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_imports_from_node(child, code, file_id, imports);
                }
            }
        }
    }

    /// Process a Go import declaration node
    fn process_go_import_declaration(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        // import_declaration can contain either a single import_spec or import_spec_list
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "import_spec" => {
                    self.process_go_import_spec(child, code, file_id, imports);
                }
                "import_spec_list" => {
                    // Process each import_spec in the list
                    for spec_child in child.children(&mut child.walk()) {
                        if spec_child.kind() == "import_spec" {
                            self.process_go_import_spec(spec_child, code, file_id, imports);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Process a single Go import_spec node
    fn process_go_import_spec(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        let mut import_path = None;
        let mut import_alias = None;
        let mut is_dot_import = false;
        let mut is_blank_import = false;

        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "interpreted_string_literal" | "raw_string_literal" => {
                    // This is the import path
                    let path_text = &code[child.byte_range()];
                    // Remove quotes
                    import_path =
                        Some(path_text.trim_matches(|c| c == '"' || c == '`').to_string());
                }
                "package_identifier" => {
                    // This is an alias (e.g., "import f 'fmt'")
                    import_alias = Some(code[child.byte_range()].to_string());
                }
                "dot" => {
                    // Dot import (e.g., "import . 'fmt'")
                    is_dot_import = true;
                }
                "blank_identifier" => {
                    // Blank import (e.g., "import _ 'database/sql'")
                    is_blank_import = true;
                }
                _ => {}
            }
        }

        if let Some(path) = import_path {
            let import = Import {
                path,
                alias: if is_dot_import {
                    Some(".".to_string())
                } else if is_blank_import {
                    Some("_".to_string())
                } else {
                    import_alias
                },
                file_id,
                is_glob: is_dot_import, // Dot imports are like glob imports
                is_type_only: false,    // Go doesn't have type-only imports
            };
            imports.push(import);
        }
    }

    // Helper methods for find_calls()
    #[allow(clippy::only_used_in_recursion)]
    /// Recursively extract function calls from Go AST nodes
    ///
    /// Traverses the syntax tree to find all function calls including:
    /// - Direct function calls: `functionName()`
    /// - Method calls: `receiver.method()`
    /// - Package-qualified calls: `pkg.Function()`
    fn extract_calls_recursive<'a>(
        &self,
        node: &tree_sitter::Node,
        code: &'a str,
        current_function: Option<&'a str>,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Handle function context - track which function we're inside
        let function_context = if node.kind() == "function_declaration"
            || node.kind() == "method_declaration"
            || node.kind() == "func_literal"
        {
            // Extract function name
            if let Some(name_node) = node.child_by_field_name("name") {
                Some(&code[name_node.byte_range()])
            } else {
                // Function literals might not have a name
                current_function
            }
        } else {
            current_function
        };

        // Check if this is a call expression
        if node.kind() == "call_expression" {
            // Skip if it's a method call (handled by find_method_calls)
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() != "selector_expression" {
                    // It's a regular function call
                    if let Some(fn_name) = Self::extract_function_name(&function_node, code) {
                        if let Some(context) = function_context {
                            let range = Range {
                                start_line: (node.start_position().row + 1) as u32,
                                start_column: node.start_position().column as u16,
                                end_line: (node.end_position().row + 1) as u32,
                                end_column: node.end_position().column as u16,
                            };
                            calls.push((context, fn_name, range));
                        }
                    }
                }
            }
        }

        // Recurse to children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_calls_recursive(&child, code, function_context, calls);
        }
    }

    fn extract_type_uses_recursive<'a>(
        &self,
        node: &tree_sitter::Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            // Go function and method declarations with parameters and return types
            "function_declaration" | "method_declaration" => {
                let context_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                // Check parameters
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.extract_go_parameter_types(params, code, context_name, uses);
                }

                // Check return type (Go uses "result" field)
                if let Some(result) = node.child_by_field_name("result") {
                    self.extract_go_type_reference(&result, code, context_name, uses);
                }
            }

            // Go struct types
            "struct_type" => {
                // Extract field types from struct
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "field_declaration_list" {
                        for field_child in child.children(&mut child.walk()) {
                            if field_child.kind() == "field_declaration" {
                                self.extract_go_field_types(&field_child, code, "struct", uses);
                            }
                        }
                    }
                }
            }

            // Go variable declarations
            "var_spec" | "const_spec" => {
                if let Some(identifier) = node
                    .children(&mut node.walk())
                    .find(|n| n.kind() == "identifier")
                {
                    let var_name = &code[identifier.byte_range()];

                    // Look for type reference
                    for child in node.children(&mut node.walk()) {
                        if matches!(
                            child.kind(),
                            "type_identifier"
                                | "pointer_type"
                                | "array_type"
                                | "slice_type"
                                | "map_type"
                                | "channel_type"
                        ) {
                            self.extract_go_type_reference(&child, code, var_name, uses);
                        }
                    }
                }
            }

            // Go function calls - look for calls to generic functions
            "call_expression" => {
                // Track generic function calls with type arguments
                if let Some(function_node) = node.child_by_field_name("function") {
                    let func_name = &code[function_node.byte_range()];

                    // Look for type arguments in generic function calls
                    // Go 1.18+ supports syntax like: MakeSlice[int](10) or MyFunc[T, U](args)
                    for child in node.children(&mut node.walk()) {
                        if child.kind() == "type_arguments" {
                            // Extract each type argument
                            for type_arg in child.children(&mut child.walk()) {
                                if matches!(
                                    type_arg.kind(),
                                    "type_identifier"
                                        | "pointer_type"
                                        | "array_type"
                                        | "slice_type"
                                        | "map_type"
                                ) {
                                    self.extract_go_type_reference(
                                        &type_arg, code, func_name, uses,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            _ => {}
        }

        // Recurse to children
        for child in node.children(&mut node.walk()) {
            self.extract_type_uses_recursive(&child, code, uses);
        }
    }

    /// Extract type references from Go parameter list
    fn extract_go_parameter_types<'a>(
        &self,
        params_node: tree_sitter::Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for param in params_node.children(&mut params_node.walk()) {
            if param.kind() == "parameter_declaration" {
                // Go parameters have type after name
                for child in param.children(&mut param.walk()) {
                    if matches!(
                        child.kind(),
                        "type_identifier"
                            | "pointer_type"
                            | "array_type"
                            | "slice_type"
                            | "map_type"
                            | "channel_type"
                    ) {
                        self.extract_go_type_reference(&child, code, context_name, uses);
                    }
                }
            }
        }
    }

    /// Extract type references from Go field declarations
    fn extract_go_field_types<'a>(
        &self,
        field_node: &tree_sitter::Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in field_node.children(&mut field_node.walk()) {
            if matches!(
                child.kind(),
                "type_identifier"
                    | "pointer_type"
                    | "array_type"
                    | "slice_type"
                    | "map_type"
                    | "channel_type"
            ) {
                self.extract_go_type_reference(&child, code, context_name, uses);
            }
        }
    }

    /// Extract Go type reference and add to uses
    fn extract_go_type_reference<'a>(
        &self,
        type_node: &tree_sitter::Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if let Some(type_name) = self.extract_go_type_name(type_node, code) {
            let range = Range::new(
                type_node.start_position().row as u32,
                type_node.start_position().column as u16,
                type_node.end_position().row as u32,
                type_node.end_position().column as u16,
            );
            uses.push((context_name, type_name, range));
        }
    }

    /// Extract type name from Go type node
    #[allow(clippy::only_used_in_recursion)]
    fn extract_go_type_name<'a>(&self, node: &tree_sitter::Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            "type_identifier" => Some(&code[node.byte_range()]),
            "qualified_type" => {
                // For qualified types like pkg.Type, get the full name
                Some(&code[node.byte_range()])
            }
            "pointer_type" => {
                // For pointer types like *User, get the underlying type
                if let Some(child) = node.children(&mut node.walk()).nth(1) {
                    self.extract_go_type_name(&child, code)
                } else {
                    None
                }
            }
            "array_type" | "slice_type" => {
                // For array/slice types like []User, get the element type
                if let Some(element_node) = node.child_by_field_name("element") {
                    self.extract_go_type_name(&element_node, code)
                } else {
                    None
                }
            }
            "map_type" => {
                // For map types like map[string]User, get the value type
                if let Some(value_node) = node.child_by_field_name("value") {
                    self.extract_go_type_name(&value_node, code)
                } else {
                    None
                }
            }
            "channel_type" => {
                // For channel types like chan User, get the element type
                if let Some(element_node) = node.child_by_field_name("element") {
                    self.extract_go_type_name(&element_node, code)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn extract_method_defines_recursive<'a>(
        &self,
        node: &tree_sitter::Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            // Go interface types with method elements
            "interface_type" => {
                // We need to get the interface name from the parent type_spec
                let interface_name = "interface"; // Default name, could be improved

                for child in node.children(&mut node.walk()) {
                    if child.kind() == "method_elem" {
                        // Extract method name from method_elem
                        if let Some(name_node) = child
                            .children(&mut child.walk())
                            .find(|n| n.kind() == "field_identifier")
                        {
                            let method_name = &code[name_node.byte_range()];
                            let range = Range::new(
                                child.start_position().row as u32,
                                child.start_position().column as u16,
                                child.end_position().row as u32,
                                child.end_position().column as u16,
                            );
                            defines.push((interface_name, method_name, range));
                        }
                    }
                }
            }

            // Go method declarations (methods with receivers)
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = &code[name_node.byte_range()];

                    // Extract receiver type for context
                    let receiver_type = if let Some(receiver) = node.child_by_field_name("receiver")
                    {
                        // Get the receiver type from the parameter list
                        receiver
                            .children(&mut receiver.walk())
                            .find(|n| matches!(n.kind(), "type_identifier" | "pointer_type"))
                            .map(|n| &code[n.byte_range()])
                            .unwrap_or("unknown")
                    } else {
                        "unknown"
                    };

                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    defines.push((receiver_type, method_name, range));
                }
            }

            _ => {}
        }

        // Recurse to children
        for child in node.children(&mut node.walk()) {
            self.extract_method_defines_recursive(&child, code, defines);
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn extract_method_calls_recursive(
        &self,
        node: &tree_sitter::Node,
        code: &str,
        current_function: Option<&str>,
        calls: &mut Vec<MethodCall>,
    ) {
        // Track function context for Go
        let function_context = if node.kind() == "function_declaration"
            || node.kind() == "method_declaration"
            || node.kind() == "func_literal"
        {
            // Extract function name
            if let Some(name_node) = node.child_by_field_name("name") {
                Some(&code[name_node.byte_range()])
            } else {
                current_function
            }
        } else {
            current_function
        };

        // Check for method calls (Go uses selector_expression)
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "selector_expression" {
                    // It's a method call!
                    if let Some((receiver, method_name, is_static)) =
                        self.extract_go_method_signature(&function_node, code)
                    {
                        if let Some(context) = function_context {
                            let range = Range {
                                start_line: (node.start_position().row + 1) as u32,
                                start_column: node.start_position().column as u16,
                                end_line: (node.end_position().row + 1) as u32,
                                end_column: node.end_position().column as u16,
                            };

                            let method_call = MethodCall {
                                caller: context.to_string(),
                                method_name: method_name.to_string(),
                                receiver: receiver.map(|r| r.to_string()),
                                is_static,
                                range,
                            };

                            calls.push(method_call);
                        }
                    }
                }
            }
        }

        // Recurse
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_method_calls_recursive(&child, code, function_context, calls);
        }
    }

    fn extract_go_method_signature<'a>(
        &self,
        selector_expr: &tree_sitter::Node,
        code: &'a str,
    ) -> Option<(Option<&'a str>, &'a str, bool)> {
        // selector_expression has 'operand' and 'field' fields
        let operand = selector_expr.child_by_field_name("operand");
        let field = selector_expr.child_by_field_name("field");

        match (operand, field) {
            (Some(obj), Some(prop)) => {
                let receiver = &code[obj.byte_range()];
                let method_name = &code[prop.byte_range()];

                // In Go, we can't easily distinguish between static and instance calls
                // without type information, so we'll assume instance calls
                let is_static = false;

                Some((Some(receiver), method_name, is_static))
            }
            _ => None,
        }
    }

    fn extract_function_name<'a>(node: &tree_sitter::Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            "identifier" => Some(&code[node.byte_range()]),
            "selector_expression" => {
                // For qualified function calls like pkg.Function()
                Some(&code[node.byte_range()])
            }
            _ => None,
        }
    }

    /// Extract generic type parameters from a signature
    /// Returns a list of type parameter names like ["T", "K", "V"]
    fn extract_generic_params_from_signature(&self, signature: &str) -> Vec<String> {
        let mut params = Vec::new();

        // Look for generic parameter section like [T any, K comparable, V SomeInterface]
        if let Some(start) = signature.find('[') {
            if let Some(end) = signature[start..].find(']') {
                let generic_section = &signature[start + 1..start + end];

                // Parse parameters separated by commas
                for param in generic_section.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }

                    // Extract just the parameter name (first word)
                    if let Some(param_name) = param.split_whitespace().next() {
                        params.push(param_name.to_string());
                    }
                }
            }
        }

        params
    }
}

impl LanguageParser for GoParser {
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
        // Extract Go-style documentation comments (//)
        // For type_spec nodes, check the parent type_declaration for comments
        // For other nodes, check the current node's previous siblings

        let comment_node = if node.kind() == "type_spec" {
            // For type_spec, check parent's (type_declaration) previous siblings
            node.parent()
        } else {
            // For other nodes, check current node's previous siblings
            Some(*node)
        };

        let search_node = comment_node?;

        let mut doc_lines = Vec::new();
        let mut current = search_node.prev_sibling();

        // Walk backwards through previous siblings to collect consecutive comment lines
        while let Some(sibling) = current {
            if sibling.kind() == "comment" {
                let comment_text = &code[sibling.byte_range()];

                // Check for Go-style line comments starting with //
                if comment_text.starts_with("//") {
                    // Extract the comment content (remove // and leading/trailing whitespace)
                    let content = comment_text.trim_start_matches("//").trim();

                    // Add to the beginning of doc_lines since we're walking backwards
                    doc_lines.insert(0, content.to_string());

                    // Continue to previous sibling to check for more comment lines
                    current = sibling.prev_sibling();
                } else {
                    // Found a non-Go comment (like /* */), stop collecting
                    break;
                }
            } else {
                // Found a non-comment node, stop collecting
                break;
            }
        }

        // If we found any documentation comments, join them and return
        if !doc_lines.is_empty() {
            // Filter out empty lines and join with newlines
            let filtered_lines: Vec<String> = doc_lines
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect();

            if !filtered_lines.is_empty() {
                let joined = filtered_lines.join("\n").trim().to_string();
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
        }

        None
    }

    /// Extract function calls from Go source code
    ///
    /// Returns tuples of (caller_context, called_function, range) for all function calls
    /// including method calls via dot notation and package-qualified calls.
    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut calls = Vec::new();

        // Track current function context
        self.extract_calls_recursive(&root, code, None, &mut calls);

        calls
    }

    /// Extract method calls from Go source code
    ///
    /// Returns MethodCall structs containing caller, method name, and position information
    /// for all method invocations including pointer receiver calls and chained calls.
    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut method_calls = Vec::new();

        self.extract_method_calls_recursive(&root, code, None, &mut method_calls);

        method_calls
    }

    /// Go uses implicit interface implementation (duck typing).
    /// Types implement interfaces by having matching method signatures,
    /// not through explicit declarations. This cannot be reliably detected
    /// through AST parsing alone - it requires cross-file semantic analysis.
    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Return empty vector - Go has no explicit implementation declarations
        Vec::new()
    }

    /// Go doesn't have class inheritance. Interfaces can embed other interfaces,
    /// but this is composition, not inheritance. Struct types can embed other types,
    /// but again, this is composition rather than inheritance.
    fn find_extends<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Return empty vector - Go has no class inheritance
        Vec::new()
    }

    /// Extract import declarations from Go source code
    ///
    /// Handles all Go import styles:
    /// - Standard imports: `import "fmt"`
    /// - Grouped imports: `import ( "fmt"; "os" )`
    /// - Aliased imports: `import f "fmt"`
    /// - Dot imports: `import . "fmt"`
    /// - Blank imports: `import _ "database/sql"`
    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let mut imports = Vec::new();

        if let Some(tree) = self.parser.parse(code, None) {
            let root = tree.root_node();
            self.extract_imports_from_node(root, code, file_id, &mut imports);
        }

        imports
    }

    /// Extract type usage references from Go source code
    ///
    /// Returns tuples of (context, type_name, range) for all type references
    /// including struct field types, function parameters, and return types.
    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut uses = Vec::new();

        self.extract_type_uses_recursive(&root, code, &mut uses);

        uses
    }

    /// Extract method definitions from Go source code
    ///
    /// Returns tuples of (receiver_type, method_name, range) for all method definitions
    /// with explicit receivers, distinguishing them from standalone functions.
    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut defines = Vec::new();

        self.extract_method_defines_recursive(&root, code, &mut defines);

        defines
    }

    fn language(&self) -> crate::parsing::Language {
        crate::parsing::Language::Go
    }
}

impl NodeTracker for GoParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id)
    }
}

impl GoParser {
    /// Recursively register all nodes for audit tracking
    /// This ensures child nodes (type_identifier, field_identifier, etc.) are counted
    fn register_node_recursively(&mut self, node: Node) {
        self.register_handled_node(node.kind(), node.kind_id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.register_node_recursively(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FileId;

    #[test]
    fn test_go_import_extraction() {
        println!("\n=== Go Import Extraction Test ===\n");

        let mut parser = GoParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
package main

import (
    "fmt"
    "strings"
    "net/http"
    utils "github.com/user/repo/utils"
    . "encoding/json"
    _ "database/sql"
    "path/filepath"
)
"#;

        println!("Test code:\n{code}");

        let imports = parser.find_imports(code, file_id);

        println!("\nExtracted {} imports:", imports.len());
        for (i, import) in imports.iter().enumerate() {
            println!(
                "  {}. {} -> {:?} (type_only: {})",
                i + 1,
                import.path,
                import.alias,
                import.is_type_only
            );
        }

        // Verify counts - Go should extract 7 imports
        assert_eq!(imports.len(), 7, "Should extract 7 imports");

        // Verify specific imports
        // Standard library import
        assert!(imports.iter().any(|i| i.path == "fmt" && i.alias.is_none()));

        // Aliased import
        assert!(imports.iter().any(
            |i| i.path == "github.com/user/repo/utils" && i.alias == Some("utils".to_string())
        ));

        // Dot import (not implemented as alias, but should be present)
        assert!(imports.iter().any(|i| i.path == "encoding/json"));

        // Blank import
        assert!(imports.iter().any(|i| i.path == "database/sql"));

        println!("=== PASSED ===\n");
    }

    #[test]
    fn test_go_generic_type_extraction() {
        println!("\n=== Go Generic Type Extraction Test ===\n");

        let mut parser = GoParser::new().unwrap();

        let code = r#"
package main

import "fmt"

// Generic function with type constraint
func Identity[T any](value T) T {
    return value
}

// Generic function with multiple type parameters
func Compare[T comparable](a, b T) bool {
    return a == b
}

// Generic struct
type Container[T any] struct {
    items []T
}

// Generic interface
type Processor[T any] interface {
    Process(T) error
}

func main() {
    // Instantiate generic types
    intContainer := Container[int]{items: []int{1, 2, 3}}
    stringContainer := Container[string]{items: []string{"a", "b"}}
    
    fmt.Println(Identity(42))
    fmt.Println(Compare("hello", "world"))
    fmt.Println(intContainer.items)
    fmt.Println(stringContainer.items)
}
"#;

        println!("Test code:\n{code}");

        // Parse the code to extract symbols
        let mut symbol_counter = SymbolCounter::new();
        let file_id = FileId::new(1).unwrap();
        let symbols = parser.parse(code, file_id, &mut symbol_counter);

        println!("\nExtracted {} symbols:", symbols.len());
        for (i, symbol) in symbols.iter().enumerate() {
            println!("  {}. {} ({:?})", i + 1, symbol.name, symbol.kind);
        }

        // Verify that generic functions are captured
        let generic_functions: Vec<_> = symbols
            .iter()
            .filter(|s| {
                matches!(s.kind, SymbolKind::Function)
                    && s.signature.as_ref().is_some_and(|sig| sig.contains("["))
            })
            .collect();

        assert!(
            !generic_functions.is_empty(),
            "Should find generic functions"
        );

        // Verify specific generic symbols
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "Identity"
                && s.signature
                    .as_ref()
                    .is_some_and(|sig| sig.contains("[T any]"))),
            "Should find Identity generic function"
        );

        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "Container"
                && s.signature
                    .as_ref()
                    .is_some_and(|sig| sig.contains("[T any]"))),
            "Should find Container generic struct"
        );

        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "Processor"
                && s.signature
                    .as_ref()
                    .is_some_and(|sig| sig.contains("[T any]"))),
            "Should find Processor generic interface"
        );

        println!("\n✅ Go generic type extraction test passed");
    }

    #[test]
    fn test_go_interface_implementation_behavior() {
        println!("\n=== Go Interface Implementation Behavior Test ===\n");

        let mut parser = GoParser::new().unwrap();

        let code = r#"
package main

import (
    "fmt"
    "io"
)

// Interface definitions
type Writer interface {
    Write([]byte) (int, error)
}

type Reader interface {
    Read([]byte) (int, error)
}

// Struct that implements interfaces through method signatures
type FileProcessor struct {
    filename string
    data     []byte
}

// These methods make FileProcessor implement Writer interface
func (f *FileProcessor) Write(data []byte) (int, error) {
    f.data = append(f.data, data...)
    return len(data), nil
}

// This method makes FileProcessor implement Reader interface  
func (f *FileProcessor) Read(data []byte) (int, error) {
    copy(data, f.data)
    return len(f.data), nil
}

// Interface embedding (composition, not inheritance)
type ReadWriter interface {
    Reader
    Writer
}

// Type embedding (composition, not inheritance)
type ExtendedProcessor struct {
    FileProcessor
    metadata map[string]string
}
"#;

        println!("Test code:\n{code}");

        // Go uses implicit interface implementation
        // No explicit "implements" declarations exist
        let implementations = parser.find_implementations(code);
        println!("\nImplementations found ({}):", implementations.len());
        assert_eq!(
            implementations.len(),
            0,
            "Go should have no explicit implementations"
        );

        // Go has no class inheritance
        // Interface/struct embedding is composition, not inheritance
        let extends = parser.find_extends(code);
        println!("Extends relationships found ({}):", extends.len());
        assert_eq!(extends.len(), 0, "Go should have no extends relationships");

        println!("\n✅ Go interface implementation behavior test passed");
        println!("✅ Verified that Go returns empty results for explicit relationships");
        println!("✅ Go's implicit interface implementation requires semantic analysis");
    }

    #[test]
    fn test_go_complex_import_patterns() {
        println!("\n=== Go Complex Import Patterns Test ===\n");

        let mut parser = GoParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
package main

import (
    "fmt"
    "net/http"
    "path/filepath"
    
    // Aliased import
    httputil "net/http/httputil"
    
    // Dot import
    . "math"
    
    // Blank import
    _ "net/http/pprof"
    
    // External module
    "github.com/gorilla/mux"
    "github.com/user/project/internal/config"
)

func main() {
    fmt.Println("Hello, World!")
}
"#;

        let imports = parser.find_imports(code, file_id);

        println!("Found {} imports in Go complex patterns", imports.len());
        for import in &imports {
            println!(
                "  - {} -> {:?} (type_only: {})",
                import.path, import.alias, import.is_type_only
            );
        }

        // Should have 8 imports: standard lib + external modules
        assert!(imports.len() >= 6, "Should have at least 6 imports");

        // Check for standard library imports
        assert!(
            imports.iter().any(|i| i.path == "fmt"),
            "Should find fmt import"
        );
        assert!(
            imports.iter().any(|i| i.path == "net/http"),
            "Should find net/http import"
        );

        // Check for aliased import
        assert!(
            imports
                .iter()
                .any(|i| i.path == "net/http/httputil" && i.alias == Some("httputil".to_string())),
            "Should find aliased httputil import"
        );

        // Check for dot import
        assert!(
            imports.iter().any(|i| i.path == "math"),
            "Should find math import"
        );

        // Check for blank import
        assert!(
            imports.iter().any(|i| i.path == "net/http/pprof"),
            "Should find pprof import"
        );

        // Check for external module
        assert!(
            imports.iter().any(|i| i.path == "github.com/gorilla/mux"),
            "Should find external module import"
        );

        println!("✅ Go complex patterns handled correctly");
    }

    #[test]
    fn test_go_import_path_formats() {
        println!("\n=== Go Import Path Formats Test ===\n");

        let mut parser = GoParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
package main

import (
    // Standard library imports
    "fmt"
    "strings"
    "net/http"
    "encoding/json"
    
    // External module imports
    "github.com/gin-gonic/gin"
    "github.com/user/repo/pkg/database"
    "go.uber.org/zap"
    
    // Relative imports (rare in Go but valid)
    "./internal/config"
    "../shared/utils"
)

func main() {
    fmt.Println("Hello, World!")
}
"#;

        let imports = parser.find_imports(code, file_id);

        println!("Go path formats found:");
        for import in &imports {
            println!("  - {}", import.path);
        }

        // Check standard library imports
        assert!(imports.iter().any(|i| i.path == "fmt"));
        assert!(imports.iter().any(|i| i.path == "strings"));
        assert!(imports.iter().any(|i| i.path == "net/http"));
        assert!(imports.iter().any(|i| i.path == "encoding/json"));

        // Check external module imports
        assert!(imports.iter().any(|i| i.path == "github.com/gin-gonic/gin"));
        assert!(imports.iter().any(|i| i.path == "go.uber.org/zap"));

        // Check relative imports
        assert!(imports.iter().any(|i| i.path == "./internal/config"));
        assert!(imports.iter().any(|i| i.path == "../shared/utils"));

        println!("✅ Various Go path formats extracted correctly");
    }

    #[test]
    fn test_go_visibility_variations() {
        println!("\n=== Go Visibility Variations Test ===\n");

        let mut parser = GoParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
package main

import "fmt"

// Exported (public) symbols - start with uppercase
const PublicConstant = "public"
var PublicVariable = 42

type PublicStruct struct {
    PublicField   string
    privateField  int
}

func PublicFunction() string {
    return "public function"
}

func (p *PublicStruct) PublicMethod() {
    fmt.Println("public method")
}

// Unexported (private) symbols - start with lowercase
const privateConstant = "private"
var privateVariable = 24

type privateStruct struct {
    field string
}

func privateFunction() string {
    return "private function"
}

func (p *privateStruct) privateMethod() {
    fmt.Println("private method")
}
"#;

        let mut symbol_counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut symbol_counter);

        println!("Go visibility symbols found:");
        for symbol in &symbols {
            let visibility_str = match symbol.visibility {
                Visibility::Public => "public",
                _ => "private",
            };
            println!(
                "  - {} ({:?}) -> {}",
                symbol.name, symbol.kind, visibility_str
            );
        }

        // Check for exported symbols
        let exported_symbols: Vec<_> = symbols
            .iter()
            .filter(|s| matches!(s.visibility, Visibility::Public))
            .collect();

        let unexported_symbols: Vec<_> = symbols
            .iter()
            .filter(|s| !matches!(s.visibility, Visibility::Public))
            .collect();

        assert!(!exported_symbols.is_empty(), "Should find exported symbols");
        assert!(
            !unexported_symbols.is_empty(),
            "Should find unexported symbols"
        );

        // Check specific exported symbols
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "PublicFunction"
                && matches!(s.visibility, Visibility::Public))
        );
        assert!(symbols.iter().any(
            |s| s.name.as_ref() == "PublicStruct" && matches!(s.visibility, Visibility::Public)
        ));

        // Check specific unexported symbols
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "privateFunction"
                && !matches!(s.visibility, Visibility::Public))
        );
        assert!(
            symbols.iter().any(|s| s.name.as_ref() == "privateStruct"
                && !matches!(s.visibility, Visibility::Public))
        );

        println!("✅ Go visibility variations handled correctly");
    }
}
