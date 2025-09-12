//! TypeScript parser implementation
//!
//! **Tree-sitter ABI Version**: ABI-14 (tree-sitter-typescript 0.24.4)
//!
//! Note: This parser uses ABI-14 with 383 node types and 40 fields.
//! When migrating or updating the parser, ensure compatibility with ABI-14 features.

use crate::parsing::Import;
use crate::parsing::{
    LanguageParser, MethodCall, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use tree_sitter::{Language, Node, Parser};

/// TypeScript language parser
pub struct TypeScriptParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

impl TypeScriptParser {
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

    /// Parse TypeScript source code and extract all symbols
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        // Reset context for each file
        self.context = ParserContext::new();
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
                );
            }
            None => {
                eprintln!("Failed to parse TypeScript file");
            }
        }

        symbols
    }

    /// Create a new TypeScript parser
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        // Use the TSX grammar so TSX/JSX syntax parses correctly. It also
        // handles plain TypeScript files, avoiding ERROR roots in TSX files.
        let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        parser
            .set_language(&language)
            .map_err(|e| format!("Failed to set TypeScript language: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Extract symbols from a TypeScript node
    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        match node.kind() {
            "function_declaration" => {
                // Register ALL child nodes for audit (including type_parameters, parameters, etc.)
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
                // Note: In TypeScript, function declarations are hoisted
                // But we process nested symbols in the function's scope
                self.context.enter_scope(ScopeType::hoisting_function());

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());
                // Set current function for parent tracking BEFORE processing children
                self.context.set_current_function(func_name.clone());

                // Process function body for nested symbols
                if let Some(body) = node.child_by_field_name("body") {
                    // Register the body node for audit tracking
                    self.register_handled_node(body.kind(), body.kind_id());
                    // Process the body using the standard extraction
                    // This ensures all nodes are properly registered
                    self.extract_symbols_from_node(
                        body,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                    );
                }

                // Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "class_declaration" | "abstract_class_declaration" => {
                // Register ALL child nodes for audit
                self.register_node_recursively(node);
                // Extract class name for parent tracking
                let class_name = node
                    .children(&mut node.walk())
                    .find(|n| n.kind() == "type_identifier")
                    .map(|n| code[n.byte_range()].to_string());

                if let Some(symbol) = self.process_class(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                    // Enter class scope for processing members
                    self.context.enter_scope(ScopeType::Class);

                    // Save the current parent context before setting new one
                    let saved_function = self.context.current_function().map(|s| s.to_string());
                    let saved_class = self.context.current_class().map(|s| s.to_string());

                    // Set current class for parent tracking
                    self.context.set_current_class(class_name.clone());

                    // Extract class members
                    self.extract_class_members(node, code, file_id, counter, symbols, module_path);

                    // Exit scope first (this clears the current context)
                    self.context.exit_scope();

                    // Then restore the previous parent context
                    self.context.set_current_function(saved_function);
                    self.context.set_current_class(saved_class);
                }
            }
            "interface_declaration" => {
                // Register ALL child nodes for audit
                self.register_node_recursively(node);
                if let Some(symbol) =
                    self.process_interface(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                }
            }
            "type_alias_declaration" => {
                // Register ALL child nodes for audit
                self.register_node_recursively(node);
                if let Some(symbol) =
                    self.process_type_alias(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                }
            }
            "enum_declaration" => {
                // Register ALL child nodes for audit
                self.register_node_recursively(node);
                if let Some(symbol) = self.process_enum(node, code, file_id, counter, module_path) {
                    symbols.push(symbol);
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                self.process_variable_declaration(
                    node,
                    code,
                    file_id,
                    counter,
                    symbols,
                    module_path,
                );
            }
            "arrow_function" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Handle arrow functions assigned to variables
                if let Some(symbol) =
                    self.process_arrow_function(node, code, file_id, counter, module_path)
                {
                    symbols.push(symbol);
                }
            }
            "ERROR" => {
                // ERROR nodes occur when tree-sitter can't parse something
                // (e.g., "use client" directive in React Server Components)
                // We still want to extract symbols from the children
                self.register_handled_node(node.kind(), node.kind_id());

                // Check if this looks like a fragmented function declaration
                // Pattern: identifier followed by formal_parameters
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();

                let mut i = 0;
                while i < children.len() {
                    let child = children[i];

                    // Check if this is an identifier followed by formal_parameters
                    if child.kind() == "identifier" && i + 1 < children.len() {
                        let next = children[i + 1];
                        if next.kind() == "formal_parameters" {
                            // This looks like a function declaration that got fragmented
                            // Extract it as a function
                            let func_name = &code[child.byte_range()];

                            // Create a synthetic function symbol
                            let symbol_id = counter.next_id();
                            let range = Range::new(
                                child.start_position().row as u32,
                                child.start_position().column as u16,
                                next.end_position().row as u32,
                                next.end_position().column as u16,
                            );

                            let mut symbol = Symbol::new(
                                symbol_id,
                                func_name.to_string(),
                                SymbolKind::Function,
                                file_id,
                                range,
                            );

                            symbol = symbol
                                .with_visibility(Visibility::Public)
                                .with_signature(format!("function {func_name}()"));

                            if !module_path.is_empty() {
                                symbol = symbol.with_module_path(module_path.to_string());
                            }

                            // Set scope context
                            symbol.scope_context = Some(self.context.current_scope_context());

                            symbols.push(symbol);

                            // Skip the formal_parameters node since we processed it
                            i += 2;
                            continue;
                        }
                    }

                    // Process child normally
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                    );
                    i += 1;
                }
            }
            _ => {
                // Track all nodes we encounter, even if not extracting symbols
                self.register_handled_node(node.kind(), node.kind_id());
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
        let visibility = self.determine_visibility(node, code);

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

    /// Process a class declaration
    fn process_class(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        // For abstract classes, the name isn't a field, it's a type_identifier child
        let name = node
            .children(&mut node.walk())
            .find(|n| n.kind() == "type_identifier")
            .map(|n| &code[n.byte_range()])?;

        let signature = self.extract_class_signature(node, code);
        let doc_comment = self.extract_doc_comment(&node, code);
        let visibility = self.determine_visibility(node, code);

        Some(self.create_symbol(
            counter.next_id(),
            name.to_string(),
            SymbolKind::Class,
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

    /// Extract class members (methods, properties)
    fn extract_class_members(
        &mut self,
        class_node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        if let Some(body) = class_node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                match child.kind() {
                    "method_definition" => {
                        self.register_handled_node(child.kind(), child.kind_id());
                        // Extract method name for parent tracking
                        let method_name = child
                            .child_by_field_name("name")
                            .map(|n| code[n.byte_range()].to_string());

                        if let Some(symbol) =
                            self.process_method(child, code, file_id, counter, module_path)
                        {
                            symbols.push(symbol);
                        }

                        // Also process the method body for nested classes/functions
                        if let Some(body) = child.child_by_field_name("body") {
                            // Enter function scope for method body
                            self.context
                                .enter_scope(ScopeType::Function { hoisting: false });

                            // Save the current parent context before setting new one
                            let saved_function =
                                self.context.current_function().map(|s| s.to_string());

                            // Set current function to the method name
                            self.context.set_current_function(method_name.clone());

                            // Register the body node for audit tracking
                            self.register_handled_node(body.kind(), body.kind_id());
                            // Process the body using standard extraction
                            self.extract_symbols_from_node(
                                body,
                                code,
                                file_id,
                                counter,
                                symbols,
                                module_path,
                            );

                            // Exit scope first (this clears the current context)
                            self.context.exit_scope();

                            // Then restore the previous parent context when exiting method
                            self.context.set_current_function(saved_function);
                        }
                    }
                    "public_field_definition" | "property_declaration" => {
                        self.register_handled_node(child.kind(), child.kind_id());
                        if let Some(symbol) =
                            self.process_property(child, code, file_id, counter, module_path)
                        {
                            symbols.push(symbol);
                        }
                    }
                    _ => {
                        self.register_handled_node(child.kind(), child.kind_id());
                    }
                }
            }
        }
    }

    /// Process an interface declaration
    fn process_interface(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let signature = self.extract_interface_signature(node, code);
        let doc_comment = self.extract_doc_comment(&node, code);
        let visibility = self.determine_visibility(node, code);

        Some(self.create_symbol(
            counter.next_id(),
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
        ))
    }

    /// Process a type alias declaration
    fn process_type_alias(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let signature = &code[node.byte_range()];
        let doc_comment = self.extract_doc_comment(&node, code);
        let visibility = self.determine_visibility(node, code);

        Some(self.create_symbol(
            counter.next_id(),
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
        ))
    }

    /// Process an enum declaration
    fn process_enum(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let signature = &code[node.byte_range()];
        let doc_comment = self.extract_doc_comment(&node, code);
        let visibility = self.determine_visibility(node, code);

        Some(self.create_symbol(
            counter.next_id(),
            name.to_string(),
            SymbolKind::Enum,
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
        ))
    }

    /// Process variable declarations
    fn process_variable_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        module_path: &str,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if name_node.kind() == "identifier" {
                        let name = &code[name_node.byte_range()];

                        // Check if this is an arrow function assignment
                        let is_arrow_function =
                            if let Some(value_node) = child.child_by_field_name("value") {
                                value_node.kind() == "arrow_function"
                            } else {
                                false
                            };

                        // Determine the kind based on whether it's a function or regular variable
                        let kind = if is_arrow_function {
                            SymbolKind::Function
                        } else if code[node.byte_range()].starts_with("const") {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        };

                        let visibility = self.determine_visibility(node, code);

                        // Extract JSDoc comment for const declarations
                        let doc_comment = self.extract_doc_comment(&node, code);

                        let mut symbol = self.create_symbol(
                            counter.next_id(),
                            name.to_string(),
                            kind,
                            file_id,
                            Range::new(
                                child.start_position().row as u32,
                                child.start_position().column as u16,
                                child.end_position().row as u32,
                                child.end_position().column as u16,
                            ),
                            None,
                            doc_comment,
                            module_path,
                            visibility,
                        );

                        // Override scope context for arrow functions - they are never hoisted
                        if is_arrow_function {
                            // Arrow functions are not hoisted, but keep the parent context that was already set
                            match symbol.scope_context {
                                Some(crate::symbol::ScopeContext::Local {
                                    parent_name,
                                    parent_kind,
                                    ..
                                }) => {
                                    symbol.scope_context =
                                        Some(crate::symbol::ScopeContext::Local {
                                            hoisted: false, // Arrow functions are never hoisted
                                            parent_name,    // Keep the parent context
                                            parent_kind,    // Keep the parent kind
                                        });
                                }
                                _ => {
                                    // If not already Local, make it Local with parent context
                                    let (parent_name, parent_kind) = if let Some(func_name) =
                                        self.context.current_function()
                                    {
                                        (Some(func_name.into()), Some(crate::SymbolKind::Function))
                                    } else if let Some(class_name) = self.context.current_class() {
                                        (Some(class_name.into()), Some(crate::SymbolKind::Class))
                                    } else {
                                        (None, None)
                                    };

                                    symbol.scope_context =
                                        Some(crate::symbol::ScopeContext::Local {
                                            hoisted: false,
                                            parent_name,
                                            parent_kind,
                                        });
                                }
                            }
                        }

                        symbols.push(symbol);

                        // CRITICAL FIX: Process arrow function body for nested symbols
                        if is_arrow_function {
                            if let Some(value_node) = child.child_by_field_name("value") {
                                if value_node.kind() == "arrow_function" {
                                    if let Some(body) = value_node.child_by_field_name("body") {
                                        // Save current context
                                        let saved_function =
                                            self.context.current_function().map(|s| s.to_string());
                                        let saved_class =
                                            self.context.current_class().map(|s| s.to_string());

                                        // Enter function scope for the arrow function
                                        self.context.enter_scope(ScopeType::function());
                                        self.context.set_current_function(Some(name.to_string()));

                                        // Register the body node for audit tracking
                                        self.register_handled_node(body.kind(), body.kind_id());
                                        // Process the body using standard extraction
                                        self.extract_symbols_from_node(
                                            body,
                                            code,
                                            file_id,
                                            counter,
                                            symbols,
                                            module_path,
                                        );

                                        // Exit scope and restore context
                                        self.context.exit_scope();
                                        self.context.set_current_function(saved_function);
                                        self.context.set_current_class(saved_class);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Process arrow functions
    fn process_arrow_function(
        &mut self,
        _node: Node,
        _code: &str,
        _file_id: FileId,
        _counter: &mut SymbolCounter,
        _module_path: &str,
    ) -> Option<Symbol> {
        // Arrow functions are typically anonymous
        // We'll handle named arrow functions when assigned to variables
        None
    }

    /// Process a method definition
    fn process_method(
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
        let visibility = self.determine_method_visibility(node, code);

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

    /// Process a property/field definition
    fn process_property(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
        module_path: &str,
    ) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

        let visibility = self.determine_method_visibility(node, code);
        let doc_comment = self.extract_doc_comment(&node, code);

        Some(self.create_symbol(
            counter.next_id(),
            name.to_string(),
            SymbolKind::Field,
            file_id,
            Range::new(
                node.start_position().row as u32,
                node.start_position().column as u16,
                node.end_position().row as u32,
                node.end_position().column as u16,
            ),
            None,
            doc_comment,
            module_path,
            visibility,
        ))
    }

    /// Extract function/method signature
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

    /// Extract class signature (with extends/implements)
    fn extract_class_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the class body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract interface signature
    fn extract_interface_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the interface body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Determine visibility based on export keywords
    fn determine_visibility(&self, node: Node, code: &str) -> Visibility {
        // 1) Ancestor check: many TS grammars wrap declarations in export_statement
        let mut anc = node.parent();
        for _ in 0..3 {
            // walk a few levels conservatively
            if let Some(a) = anc {
                if a.kind() == "export_statement" {
                    return Visibility::Public;
                }
                anc = a.parent();
            } else {
                break;
            }
        }

        // 2) Sibling check (rare, but safe)
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "export_statement" {
                return Visibility::Public;
            }
        }

        // 3) Token check: if the source preceding the node contains 'export '
        // This catches inline modifiers when export is not represented as a wrapper.
        let start = node.start_byte();
        let prefix_start = start.saturating_sub(10); // small window
        let prefix = &code[prefix_start..start];
        if prefix.contains("export ") || prefix.contains("export\n") {
            return Visibility::Public;
        }

        // Default: not exported
        Visibility::Private
    }

    /// Determine method/property visibility
    fn determine_method_visibility(&self, node: Node, code: &str) -> Visibility {
        let signature = &code[node.byte_range()];

        if signature.contains("private ") || signature.starts_with("#") {
            Visibility::Private
        } else if signature.contains("protected ") {
            Visibility::Module // Map TypeScript protected to Module visibility
        } else {
            Visibility::Public // Default for class members
        }
    }

    /// Find implementations (extends and implements) in TypeScript
    fn find_implementations_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
        extends_only: bool,
    ) {
        match node.kind() {
            "class_declaration" | "abstract_class_declaration" => {
                // Get class name first
                let class_name = node
                    .children(&mut node.walk())
                    .find(|n| n.kind() == "type_identifier")
                    .map(|n| &code[n.byte_range()]);

                if let Some(class_name) = class_name {
                    // Look for class_heritage child node (not a field!)
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "class_heritage" {
                            self.process_heritage_clauses(
                                child,
                                code,
                                class_name,
                                implementations,
                                extends_only,
                            );
                        }
                    }
                }
            }
            "interface_declaration" => {
                // Only process interface extends if we're looking for extends relationships
                if extends_only {
                    if let Some(interface_name_node) = node.child_by_field_name("name") {
                        let interface_name = &code[interface_name_node.byte_range()];

                        // Check for extends_type_clause - it's a child node, not a field!
                        // ABI-15 exploration shows: interfaces use "extends_type_clause" not "extends_clause"
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.kind() == "extends_type_clause" {
                                // Extract the extended interface(s)
                                // The extended type is in field "type"
                                if let Some(type_node) = child.child_by_field_name("type") {
                                    if let Some(base_name) = self.extract_type_name(type_node, code)
                                    {
                                        let range = Range::new(
                                            type_node.start_position().row as u32,
                                            type_node.start_position().column as u16,
                                            type_node.end_position().row as u32,
                                            type_node.end_position().column as u16,
                                        );
                                        implementations.push((interface_name, base_name, range));
                                    }
                                } else {
                                    // Fallback: process as before for multiple extends
                                    self.process_extends_clause(
                                        child,
                                        code,
                                        interface_name,
                                        implementations,
                                    );
                                }
                            }
                        }
                    }
                }
                // When extends_only = false, skip interfaces entirely since they don't implement
            }
            _ => {}
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.find_implementations_in_node(child, code, implementations, extends_only);
        }
    }

    /// Process heritage clauses (extends and implements)
    fn process_heritage_clauses<'a>(
        &self,
        heritage_node: Node,
        code: &'a str,
        class_name: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
        extends_only: bool,
    ) {
        let mut cursor = heritage_node.walk();
        for child in heritage_node.children(&mut cursor) {
            match child.kind() {
                "extends_clause" => {
                    // Process extends clause ONLY when looking for extends relationships
                    // This maintains separation between extends and implements
                    if extends_only {
                        let mut extends_cursor = child.walk();
                        for extends_child in child.children(&mut extends_cursor) {
                            if extends_child.kind() == "type_identifier"
                                || extends_child.kind() == "identifier"
                                || extends_child.kind() == "nested_type_identifier"
                                || extends_child.kind() == "generic_type"
                            {
                                if let Some(base_name) = self.extract_type_name(extends_child, code)
                                {
                                    let range = Range::new(
                                        extends_child.start_position().row as u32,
                                        extends_child.start_position().column as u16,
                                        extends_child.end_position().row as u32,
                                        extends_child.end_position().column as u16,
                                    );
                                    implementations.push((class_name, base_name, range));
                                }
                            }
                        }
                    }
                }
                "implements_clause" => {
                    // Only process implements clause when NOT looking for extends only
                    if !extends_only {
                        // Skip "implements" keyword, get all the interfaces
                        let mut impl_cursor = child.walk();
                        for impl_child in child.children(&mut impl_cursor) {
                            if impl_child.kind() == "type_identifier"
                                || impl_child.kind() == "identifier"
                                || impl_child.kind() == "nested_type_identifier"
                                || impl_child.kind() == "generic_type"
                            {
                                if let Some(interface_name) =
                                    self.extract_type_name(impl_child, code)
                                {
                                    let range = Range::new(
                                        impl_child.start_position().row as u32,
                                        impl_child.start_position().column as u16,
                                        impl_child.end_position().row as u32,
                                        impl_child.end_position().column as u16,
                                    );
                                    implementations.push((class_name, interface_name, range));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Process extends clause for interfaces
    fn process_extends_clause<'a>(
        &self,
        extends_node: Node,
        code: &'a str,
        interface_name: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        let mut cursor = extends_node.walk();
        for child in extends_node.children(&mut cursor) {
            if child.kind() == "type_identifier" || child.kind() == "nested_type_identifier" {
                if let Some(base_interface) = self.extract_type_name(child, code) {
                    let range = Range::new(
                        child.start_position().row as u32,
                        child.start_position().column as u16,
                        child.end_position().row as u32,
                        child.end_position().column as u16,
                    );
                    implementations.push((interface_name, base_interface, range));
                }
            }
        }
    }

    /// Extract type name from a type node
    #[allow(clippy::only_used_in_recursion)]
    fn extract_type_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            "type_identifier" | "identifier" => Some(&code[node.byte_range()]),
            "nested_type_identifier" => {
                // For qualified names like Namespace.Type, get the full name
                Some(&code[node.byte_range()])
            }
            "generic_type" => {
                // For generic types like Array<T>, get just the base type
                if let Some(name_node) = node.child_by_field_name("name") {
                    self.extract_type_name(name_node, code)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Extract imports from AST node recursively
    fn extract_imports_from_node(
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
            "export_statement" => {
                // Check if it's a re-export (has source)
                if node.child_by_field_name("source").is_some() {
                    self.process_export_statement(node, code, file_id, imports);
                }
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

    /// Process an import statement node
    fn process_import_statement(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "ENTERING process_import_statement, code: {}",
                &code[node.byte_range()]
            );
        }

        // Debug: print all children
        let mut cursor = node.walk();
        if crate::config::is_global_debug_enabled() {
            eprintln!("  Node has {} children:", node.child_count());
        }

        // Check if this is a type-only import (has 'type' keyword after 'import')
        let mut is_type_only = false;
        for (i, child) in node.children(&mut cursor).enumerate() {
            if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "    child[{}]: kind='{}', field_name={:?}",
                    i,
                    child.kind(),
                    node.field_name_for_child(i as u32)
                );
            }
            // Check for 'type' keyword (appears in type-only imports)
            if child.kind() == "type" && i == 1 {
                is_type_only = true;
                if crate::config::is_global_debug_enabled() {
                    eprintln!("    Detected type-only import!");
                }
            }
        }

        // Get the source (the module being imported from)
        let source_node = match node.child_by_field_name("source") {
            Some(n) => n,
            None => return,
        };

        let source_path = &code[source_node.byte_range()];
        let source_path = source_path.trim_matches(|c| c == '"' || c == '\'' || c == '`');

        // Process import clause (what's being imported)
        // Note: import_clause is not a named field, we need to find it by kind
        let import_clause = {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .find(|c| c.kind() == "import_clause")
        };

        if let Some(import_clause) = import_clause {
            if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "  Found import_clause: {}",
                    &code[import_clause.byte_range()]
                );
            }

            // Check for different import types
            let mut has_default = false;
            let mut has_named = false;
            let mut has_namespace = false;
            let mut default_name = None;
            let mut namespace_name = None;

            let mut cursor = import_clause.walk();
            for child in import_clause.children(&mut cursor) {
                if crate::config::is_global_debug_enabled() {
                    eprintln!(
                        "    Child kind: {}, text: {}",
                        child.kind(),
                        &code[child.byte_range()]
                    );
                }
                match child.kind() {
                    "identifier" => {
                        // Default import
                        has_default = true;
                        let name = code[child.byte_range()].to_string();
                        if crate::config::is_global_debug_enabled() {
                            eprintln!("      Setting default_name = {name}");
                        }
                        default_name = Some(name);
                    }
                    "named_imports" => {
                        // Named imports exist
                        has_named = true;
                        // Extract named import specifiers: { Foo as Bar, Baz }
                        let mut nc = child.walk();
                        for ni in child.children(&mut nc) {
                            if ni.kind() == "import_specifier" {
                                let mut sp = ni.walk();
                                let mut local: Option<String> = None;
                                // Prefer the aliased local name if present
                                for part in ni.children(&mut sp) {
                                    if part.kind() == "identifier" {
                                        local = Some(code[part.byte_range()].to_string());
                                    }
                                }
                                imports.push(Import {
                                    path: source_path.to_string(),
                                    alias: local,
                                    file_id,
                                    is_glob: false,
                                    is_type_only,
                                });
                            }
                        }
                    }
                    "namespace_import" => {
                        // * as name
                        has_namespace = true;
                        let mut ns_cursor = child.walk();
                        let children: Vec<_> = child.children(&mut ns_cursor).collect();
                        if let Some(identifier) =
                            children.iter().rev().find(|n| n.kind() == "identifier")
                        {
                            namespace_name = Some(code[identifier.byte_range()].to_string());
                        }
                    }
                    _ => {}
                }
            }

            // Add imports based on what we found
            // Following Rust pattern: one Import per module, with alias for default/namespace
            if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "  Summary: has_default={has_default}, has_named={has_named}, has_namespace={has_namespace}"
                );
                eprintln!("  default_name={default_name:?}, namespace_name={namespace_name:?}");
            }

            if has_namespace {
                // Namespace import: import * as utils from './utils'
                imports.push(Import {
                    path: source_path.to_string(),
                    alias: namespace_name,
                    file_id,
                    is_glob: true,
                    is_type_only,
                });
            } else if has_default && has_named {
                // Mixed import: import React, { Component } from 'react'
                // We create one import with the default as alias
                imports.push(Import {
                    path: source_path.to_string(),
                    alias: default_name,
                    file_id,
                    is_glob: false,
                    is_type_only,
                });
            } else if has_default {
                // Default only: import React from 'react'
                if crate::config::is_global_debug_enabled() {
                    eprintln!(
                        "  Adding default import: path='{source_path}', alias={default_name:?}, type_only={is_type_only}"
                    );
                }
                imports.push(Import {
                    path: source_path.to_string(),
                    alias: default_name,
                    file_id,
                    is_glob: false,
                    is_type_only,
                });
            } else if has_named {
                // Named-only already pushed per specifier above
            }
        } else {
            // Side-effect import (no import clause)
            imports.push(Import {
                path: source_path.to_string(),
                alias: None,
                file_id,
                is_glob: false,
                is_type_only: false, // Side-effect imports are never type-only
            });
        }
    }

    /// Process export statements (for re-exports)
    fn process_export_statement(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        // Get the source module
        let source_node = match node.child_by_field_name("source") {
            Some(n) => n,
            None => return,
        };

        let source_path = &code[source_node.byte_range()];
        let source_path = source_path.trim_matches(|c| c == '"' || c == '\'' || c == '`');

        // Check if it's a type-only export
        let node_text = &code[node.byte_range()];
        let is_type_only = node_text.starts_with("export type");

        // Check what's being exported
        if node_text.contains("* from") {
            // export * from './module'
            imports.push(Import {
                path: source_path.to_string(),
                alias: None,
                file_id,
                is_glob: true,
                is_type_only,
            });
        } else {
            // Named re-exports - just track the module being imported from
            imports.push(Import {
                path: source_path.to_string(),
                alias: None,
                file_id,
                is_glob: false,
                is_type_only,
            });
        }
    }

    // Helper methods for find_calls()
    #[allow(clippy::only_used_in_recursion)]
    fn extract_calls_recursive<'a>(
        &self,
        node: &tree_sitter::Node,
        code: &'a str,
        current_function: Option<&'a str>,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Handle export wrappers that contain a function declaration. This helps
        // when the tree is fragmented under an ERROR root and field labeling is unreliable.
        if node.kind() == "export_statement" {
            let mut w = node.walk();
            for child in node.children(&mut w) {
                if child.kind() == "function_declaration" {
                    // Try to get function name
                    let func_name = child
                        .child_by_field_name("name")
                        .or_else(|| {
                            let mut cw = child.walk();
                            child.children(&mut cw).find(|n| n.kind() == "identifier")
                        })
                        .map(|n| &code[n.byte_range()]);
                    // Recurse into the function with proper context
                    self.extract_calls_recursive(&child, code, func_name, calls);
                    // Continue scanning other children as well
                }
            }
        }
        // Handle function context - track which function we're inside
        // CRITICAL: Only set NEW context when entering a function, otherwise INHERIT current context
        let function_context = if node.kind() == "function_declaration"
            || node.kind() == "method_definition"
            || node.kind() == "arrow_function"
            || node.kind() == "function_expression"
        {
            // We're entering a NEW function scope - extract its name
            if let Some(name_node) = node.child_by_field_name("name").or_else(|| {
                // Fallback: some fragmented/ERROR-wrapped trees may not label fields
                let mut w = node.walk();
                node.children(&mut w).find(|n| n.kind() == "identifier")
            }) {
                let name = &code[name_node.byte_range()];
                eprintln!(
                    "DEBUG: Entering {} '{}' at line {}",
                    node.kind(),
                    name,
                    node.start_position().row + 1
                );
                Some(name)
            } else {
                // Arrow functions might not have a name, check parent for variable declaration
                // Handle case: const ComponentName = () => { ... }
                if node.kind() == "arrow_function" {
                    if let Some(parent) = node.parent() {
                        if parent.kind() == "variable_declarator" {
                            // Get the name from the variable declarator
                            if let Some(name_node) = parent.child_by_field_name("name") {
                                Some(&code[name_node.byte_range()])
                            } else {
                                current_function
                            }
                        } else {
                            current_function
                        }
                    } else {
                        current_function
                    }
                } else {
                    current_function
                }
            }
        } else if node.kind() == "identifier" && current_function.is_none() {
            // ONLY check for fragmented functions if we're NOT already in a function
            // Fragmented function detection only at top level error/program contexts.
            if let Some(parent) = node.parent() {
                if parent.kind() == "ERROR" || parent.kind() == "program" {
                    if let Some(next_sibling) = node.next_sibling() {
                        if next_sibling.kind() == "formal_parameters" {
                            // This is a fragmented function (e.g., due to "use client" causing ERROR root)
                            Some(&code[node.byte_range()])
                        } else {
                            current_function
                        }
                    } else {
                        current_function
                    }
                } else {
                    current_function
                }
            } else {
                current_function
            }
        } else if node.kind() == "variable_declarator" && current_function.is_none() {
            // ONLY check variable declarators at top level, not inside functions
            // Check if this variable contains an arrow function or function expression
            if let Some(init) = node.child_by_field_name("value") {
                if init.kind() == "arrow_function" || init.kind() == "function_expression" {
                    // Get the variable name to use as function context
                    if let Some(name_node) = node.child_by_field_name("name") {
                        Some(&code[name_node.byte_range()])
                    } else {
                        current_function
                    }
                } else {
                    current_function
                }
            } else {
                current_function
            }
        } else {
            // Not a function declaration - INHERIT the current context
            current_function
        };

        // Check if this is a call expression
        if node.kind() == "call_expression" {
            // Try to obtain the callee node robustly: prefer 'function' field,
            // but fall back to the first child if fields are missing under ERROR nodes.
            let function_node = node.child_by_field_name("function").or_else(|| {
                let mut w = node.walk();
                node.children(&mut w).next()
            });

            if let Some(function_node) = function_node {
                // Extract function name for all types of calls (including member expressions like console.log)
                if let Some(fn_name) = Self::extract_function_name(&function_node, code) {
                    eprintln!(
                        "DEBUG: Found call to {} at line {}, context = {:?}",
                        fn_name,
                        node.start_position().row + 1,
                        function_context
                    );
                    // If we don't have a function context yet, try to infer it from ancestors
                    let inferred_context = if function_context.is_none() {
                        let mut anc = node.parent();
                        let mut ctx: Option<&'a str> = None;
                        while let Some(a) = anc {
                            match a.kind() {
                                "function_declaration" => {
                                    if let Some(name_node) =
                                        a.child_by_field_name("name").or_else(|| {
                                            let mut w = a.walk();
                                            a.children(&mut w).find(|n| n.kind() == "identifier")
                                        })
                                    {
                                        ctx = Some(&code[name_node.byte_range()]);
                                        break;
                                    }
                                }
                                "arrow_function" | "function_expression" => {
                                    if let Some(p) = a.parent() {
                                        if p.kind() == "variable_declarator" {
                                            if let Some(name_node) = p.child_by_field_name("name") {
                                                ctx = Some(&code[name_node.byte_range()]);
                                                break;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                            anc = a.parent();
                        }
                        ctx
                    } else {
                        None
                    };

                    if let Some(context) = function_context.or(inferred_context) {
                        let range = Range {
                            start_line: (node.start_position().row + 1) as u32,
                            start_column: node.start_position().column as u16,
                            end_line: (node.end_position().row + 1) as u32,
                            end_column: node.end_position().column as u16,
                        };
                        calls.push((context, fn_name, range));
                        eprintln!("DEBUG: Added call {context} -> {fn_name}");
                    } else {
                        eprintln!("DEBUG: Skipping call to {fn_name} - no function context");
                    }
                }
            }
        }

        // Special handling for fragmented functions
        // If this is an identifier followed by formal_parameters, we need to process
        // the following siblings with this function's context
        if node.kind() == "identifier" {
            if let Some(parent) = node.parent() {
                if parent.kind() == "ERROR" || parent.kind() == "program" {
                    if let Some(next_sibling) = node.next_sibling() {
                        if next_sibling.kind() == "formal_parameters" {
                            // Process subsequent siblings with this function's context
                            let mut current = next_sibling.next_sibling();
                            while let Some(sibling) = current {
                                // Heuristic boundary: stop if we hit another top-level declaration
                                let k = sibling.kind();
                                if k == "function_declaration"
                                    || k == "class_declaration"
                                    || k == "abstract_class_declaration"
                                    || k == "export_statement"
                                {
                                    break;
                                }
                                self.extract_calls_recursive(
                                    &sibling,
                                    code,
                                    function_context,
                                    calls,
                                );
                                current = sibling.next_sibling();
                            }
                            // Don't process children since we handled siblings
                            return;
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
            // Function declarations with parameters and return types
            "function_declaration"
            | "function_signature"
            | "method_definition"
            | "method_signature" => {
                let context_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                // Check parameters
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.extract_parameter_types(params, code, context_name, uses);
                }

                // Check return type - try both "type" and "return_type" fields
                if let Some(return_type) = node.child_by_field_name("type") {
                    self.extract_type_from_annotation(&return_type, code, context_name, uses);
                } else if let Some(return_type) = node.child_by_field_name("return_type") {
                    self.extract_type_from_annotation(&return_type, code, context_name, uses);
                } else {
                    // Also look for type_annotation as a direct child (not a field)
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "type_annotation" {
                            // Make sure it's the return type (comes after parameters)
                            if child.start_position().column > 30 {
                                // Heuristic: return types are usually after column 30
                                self.extract_type_from_annotation(&child, code, context_name, uses);
                            }
                        }
                    }
                }
            }

            // Class declarations with fields
            "class_declaration" | "abstract_class_declaration" => {
                let class_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                // Check for class_heritage which contains extends and implements
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "class_heritage" {
                        // Look for implements_clause and extends_clause within heritage
                        let mut heritage_cursor = child.walk();
                        for heritage_child in child.children(&mut heritage_cursor) {
                            if heritage_child.kind() == "implements_clause" {
                                self.extract_implements_types(
                                    &heritage_child,
                                    code,
                                    class_name,
                                    uses,
                                );
                            } else if heritage_child.kind() == "extends_clause" {
                                self.extract_extends_types(&heritage_child, code, class_name, uses);
                            }
                        }
                    }
                }

                // Check class body for field types
                if let Some(body) = node.child_by_field_name("body") {
                    self.extract_class_field_types(&body, code, class_name, uses);
                }
            }

            // Variable declarations with type annotations
            "variable_declarator" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let var_name = &code[name_node.byte_range()];

                    // Look for type annotation
                    if let Some(type_ann) = node.child_by_field_name("type") {
                        self.extract_type_from_annotation(&type_ann, code, var_name, uses);
                    }
                }
            }

            // Interface declarations with extends
            "interface_declaration" => {
                let interface_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                // Check extends clause - look through all children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "extends_clause" || child.kind() == "extends_type_clause" {
                        self.extract_extends_types(&child, code, interface_name, uses);
                    }
                }
            }

            // NEW: Handle constructor calls with generic type arguments
            // Example: new Map<string, Session>()
            "new_expression" => {
                // Get the context for the variable this is assigned to
                // We need to traverse up to find the variable_declarator
                let context_name = if let Some(parent) = node.parent() {
                    if parent.kind() == "variable_declarator" {
                        parent
                            .child_by_field_name("name")
                            .map(|n| &code[n.byte_range()])
                            .unwrap_or("anonymous")
                    } else {
                        "anonymous"
                    }
                } else {
                    "anonymous"
                };

                // Check for type_arguments field
                if let Some(type_args) = node.child_by_field_name("type_arguments") {
                    self.extract_types_from_type_arguments(&type_args, code, context_name, uses);
                }
            }

            // NEW: Handle function calls with generic type arguments
            // Example: useState<Session>(null)
            "call_expression" => {
                // Get the function being called for context
                let func_name = node
                    .child_by_field_name("function")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                // Check for type_arguments field
                if let Some(type_args) = node.child_by_field_name("type_arguments") {
                    self.extract_types_from_type_arguments(&type_args, code, func_name, uses);
                }
            }

            _ => {}
        }

        // Recurse to children
        for child in node.children(&mut node.walk()) {
            self.extract_type_uses_recursive(&child, code, uses);
        }
    }

    fn extract_parameter_types<'a>(
        &self,
        params_node: tree_sitter::Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for param in params_node.children(&mut params_node.walk()) {
            if matches!(
                param.kind(),
                "required_parameter" | "optional_parameter" | "rest_parameter"
            ) {
                if let Some(type_ann) = param.child_by_field_name("type") {
                    self.extract_type_from_annotation(&type_ann, code, context_name, uses);
                }
            }
        }
    }

    fn extract_type_from_annotation<'a>(
        &self,
        type_node: &tree_sitter::Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Find the actual type identifier
        if let Some(type_name) = self.extract_simple_type_name(type_node, code) {
            // Filter out TS primitive/predefined types to avoid noisy unresolved relationships
            if matches!(
                type_name,
                "string"
                    | "number"
                    | "boolean"
                    | "any"
                    | "void"
                    | "unknown"
                    | "never"
                    | "null"
                    | "undefined"
                    | "object"
                    | "bigint"
                    | "symbol"
            ) {
                return;
            }
            let range = Range::new(
                type_node.start_position().row as u32,
                type_node.start_position().column as u16,
                type_node.end_position().row as u32,
                type_node.end_position().column as u16,
            );
            uses.push((context_name, type_name, range));
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn extract_simple_type_name<'a>(
        &self,
        node: &tree_sitter::Node,
        code: &'a str,
    ) -> Option<&'a str> {
        match node.kind() {
            "type_identifier" => Some(&code[node.byte_range()]),
            "predefined_type" => Some(&code[node.byte_range()]),
            "generic_type" => {
                // For generic types like Array<User>, extract the base type
                if let Some(name) = node.child_by_field_name("name") {
                    return Some(&code[name.byte_range()]);
                }
                None
            }
            _ => {
                // Try to find a type_identifier child
                for child in node.children(&mut node.walk()) {
                    if let Some(name) = self.extract_simple_type_name(&child, code) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn extract_class_field_types<'a>(
        &self,
        body_node: &tree_sitter::Node,
        code: &'a str,
        class_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in body_node.children(&mut body_node.walk()) {
            if matches!(
                child.kind(),
                "public_field_definition" | "property_declaration"
            ) {
                if let Some(type_ann) = child.child_by_field_name("type") {
                    self.extract_type_from_annotation(&type_ann, code, class_name, uses);
                }
            }
        }
    }

    fn extract_implements_types<'a>(
        &self,
        implements_node: &tree_sitter::Node,
        code: &'a str,
        class_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in implements_node.children(&mut implements_node.walk()) {
            if matches!(child.kind(), "type_identifier" | "generic_type") {
                if let Some(type_name) = self.extract_simple_type_name(&child, code) {
                    let range = Range::new(
                        child.start_position().row as u32,
                        child.start_position().column as u16,
                        child.end_position().row as u32,
                        child.end_position().column as u16,
                    );
                    uses.push((class_name, type_name, range));
                }
            }
        }
    }

    fn extract_extends_types<'a>(
        &self,
        extends_node: &tree_sitter::Node,
        code: &'a str,
        interface_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in extends_node.children(&mut extends_node.walk()) {
            if matches!(child.kind(), "type_identifier" | "generic_type") {
                if let Some(type_name) = self.extract_simple_type_name(&child, code) {
                    let range = Range::new(
                        child.start_position().row as u32,
                        child.start_position().column as u16,
                        child.end_position().row as u32,
                        child.end_position().column as u16,
                    );
                    uses.push((interface_name, type_name, range));
                }
            }
        }
    }

    /// Extract type references from type_arguments node
    /// Handles cases like: <string, Session> or <Map<string, User>>
    #[allow(clippy::only_used_in_recursion)]
    fn extract_types_from_type_arguments<'a>(
        &self,
        type_args_node: &tree_sitter::Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        for child in type_args_node.children(&mut type_args_node.walk()) {
            match child.kind() {
                // Simple type identifiers like Session, User
                "type_identifier" => {
                    let type_name = &code[child.byte_range()];
                    let range = Range::new(
                        child.start_position().row as u32,
                        child.start_position().column as u16,
                        child.end_position().row as u32,
                        child.end_position().column as u16,
                    );
                    uses.push((context_name, type_name, range));
                }
                // Generic types like Map<string, User>
                "generic_type" => {
                    // Extract the base type (e.g., Map)
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let type_name = &code[name_node.byte_range()];
                        let range = Range::new(
                            name_node.start_position().row as u32,
                            name_node.start_position().column as u16,
                            name_node.end_position().row as u32,
                            name_node.end_position().column as u16,
                        );
                        uses.push((context_name, type_name, range));
                    }
                    // Recursively extract nested type arguments
                    if let Some(nested_args) = child.child_by_field_name("type_arguments") {
                        self.extract_types_from_type_arguments(
                            &nested_args,
                            code,
                            context_name,
                            uses,
                        );
                    }
                }
                // Skip punctuation and predefined types
                "<" | ">" | "," | "predefined_type" => {}
                _ => {}
            }
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
            // Interface method signatures
            "interface_declaration" => {
                let interface_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        if child.kind() == "method_signature" {
                            if let Some(name_node) = child.child_by_field_name("name") {
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
            }

            // Class method definitions
            "class_declaration" | "abstract_class_declaration" => {
                let class_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        if matches!(
                            child.kind(),
                            "method_definition" | "abstract_method_signature"
                        ) {
                            if let Some(name_node) = child.child_by_field_name("name") {
                                let method_name = &code[name_node.byte_range()];
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

            // Type aliases with object types (method signatures in type literals)
            "type_alias_declaration" => {
                let type_name = node
                    .child_by_field_name("name")
                    .map(|n| &code[n.byte_range()])
                    .unwrap_or("anonymous");

                if let Some(value) = node.child_by_field_name("value") {
                    if value.kind() == "object_type" {
                        for child in value.children(&mut value.walk()) {
                            if child.kind() == "method_signature" {
                                if let Some(name_node) = child.child_by_field_name("name") {
                                    let method_name = &code[name_node.byte_range()];
                                    let range = Range::new(
                                        child.start_position().row as u32,
                                        child.start_position().column as u16,
                                        child.end_position().row as u32,
                                        child.end_position().column as u16,
                                    );
                                    defines.push((type_name, method_name, range));
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
        // Track function context - SAME FIX as extract_calls_recursive
        // Only set NEW context when entering a function, otherwise INHERIT
        let function_context = if node.kind() == "function_declaration"
            || node.kind() == "method_definition"
            || node.kind() == "arrow_function"
            || node.kind() == "function_expression"
        {
            // We're entering a NEW function - extract its name
            if let Some(name_node) = node.child_by_field_name("name") {
                Some(&code[name_node.byte_range()])
            } else if node.kind() == "arrow_function" {
                // Check parent for variable declarator name
                if let Some(parent) = node.parent() {
                    if parent.kind() == "variable_declarator" {
                        if let Some(name_node) = parent.child_by_field_name("name") {
                            Some(&code[name_node.byte_range()])
                        } else {
                            current_function // Anonymous, inherit context
                        }
                    } else {
                        current_function // Anonymous, inherit context
                    }
                } else {
                    current_function // Anonymous, inherit context
                }
            } else {
                current_function // Anonymous function, inherit context
            }
        } else if node.kind() == "identifier" && current_function.is_none() {
            // Check for fragmented functions only at top level
            if let Some(parent) = node.parent() {
                if parent.kind() == "ERROR" || parent.kind() == "program" {
                    if let Some(next_sibling) = node.next_sibling() {
                        if next_sibling.kind() == "formal_parameters" {
                            Some(&code[node.byte_range()])
                        } else {
                            current_function
                        }
                    } else {
                        current_function
                    }
                } else {
                    current_function
                }
            } else {
                current_function
            }
        } else {
            // Not a function declaration - INHERIT the current context
            current_function
        };

        // Check for method calls
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "member_expression" {
                    // It's a method call!
                    if let Some((receiver, method_name, is_static)) =
                        self.extract_method_signature(&function_node, code)
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

    fn extract_method_signature<'a>(
        &self,
        member_expr: &tree_sitter::Node,
        code: &'a str,
    ) -> Option<(Option<&'a str>, &'a str, bool)> {
        // member_expression has 'object' and 'property' fields
        let object = member_expr.child_by_field_name("object");
        let property = member_expr.child_by_field_name("property");

        match (object, property) {
            (Some(obj), Some(prop)) => {
                let receiver = &code[obj.byte_range()];
                let method_name = &code[prop.byte_range()];

                // Check if it's a static call (TypeScript doesn't have :: but uses .)
                // We can't easily distinguish static from instance in TypeScript
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
            "member_expression" => {
                // For member expressions like console.log, return the full dotted name
                Some(&code[node.byte_range()])
            }
            "await_expression" => {
                // Handle await foo()
                if let Some(expr) = node.child_by_field_name("expression") {
                    Self::extract_function_name(&expr, code)
                } else {
                    // Sometimes await_expression has the identifier as a direct child
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if let Some(name) = Self::extract_function_name(&child, code) {
                            return Some(name);
                        }
                    }
                    None
                }
            }
            _ => None,
        }
    }

    /// Recursively register all nodes for audit tracking
    /// This is separate from symbol extraction - it just ensures all nodes are counted
    fn register_node_recursively(&mut self, node: Node) {
        self.register_handled_node(node.kind(), node.kind_id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.register_node_recursively(child);
        }
    }
}

impl NodeTracker for TypeScriptParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<crate::parsing::HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for TypeScriptParser {
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
        // Look for JSDoc/TSDoc comments (/** ... */)

        // First, check if this node is inside an export_statement
        // If so, we need to check the export_statement's previous sibling for the comment
        let comment_node = if let Some(parent) = node.parent() {
            if parent.kind() == "export_statement" {
                // For exported functions, check the export statement's previous sibling
                parent.prev_sibling()
            } else {
                // For non-exported functions, check the node's previous sibling
                node.prev_sibling()
            }
        } else {
            // No parent, check the node's previous sibling
            node.prev_sibling()
        };

        if let Some(prev) = comment_node {
            if prev.kind() == "comment" {
                let comment = &code[prev.byte_range()];
                if comment.starts_with("/**") {
                    // Clean up the comment
                    let cleaned = comment
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim_start_matches(" * ").trim_start_matches(" *"))
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string();

                    return Some(cleaned);
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

        let root = tree.root_node();
        let mut calls = Vec::new();

        // Track current function context
        self.extract_calls_recursive(&root, code, None, &mut calls);

        calls
    }

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

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut implementations = Vec::new();

        if let Some(tree) = self.parser.parse(code, None) {
            self.find_implementations_in_node(tree.root_node(), code, &mut implementations, false);
        }

        implementations
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut extends = Vec::new();

        if let Some(tree) = self.parser.parse(code, None) {
            self.find_implementations_in_node(tree.root_node(), code, &mut extends, true);
        }

        extends
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let mut imports = Vec::new();

        if let Some(tree) = self.parser.parse(code, None) {
            let root = tree.root_node();
            self.extract_imports_from_node(root, code, file_id, &mut imports);
        }

        imports
    }

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
        crate::parsing::Language::TypeScript
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Basic TS variable type inference for `const/let/var x = new Type()` patterns
        let mut bindings = Vec::new();
        if let Some(tree) = self.parser.parse(code, None) {
            let root = tree.root_node();

            fn walk<'a>(
                node: &tree_sitter::Node,
                code: &'a str,
                out: &mut Vec<(&'a str, &'a str, Range)>,
            ) {
                // Look for lexical_declaration -> variable_declarator with new_expression initializer
                if node.kind() == "lexical_declaration" || node.kind() == "variable_declaration" {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "variable_declarator" {
                            let name = child.child_by_field_name("name").and_then(|n| {
                                if n.kind() == "identifier" {
                                    Some(&code[n.byte_range()])
                                } else {
                                    None
                                }
                            });
                            let init = child.child_by_field_name("value");
                            if let (Some(var), Some(init_node)) = (name, init) {
                                if init_node.kind() == "new_expression" {
                                    // Extract constructor type: new TypeName(...)
                                    if let Some(constructor) =
                                        init_node.child_by_field_name("constructor")
                                    {
                                        // constructor might be an identifier or qualified name
                                        // We take the last identifier as the type name
                                        let type_name = if constructor.kind() == "identifier" {
                                            Some(&code[constructor.byte_range()])
                                        } else {
                                            // Fallback: try to find a trailing identifier
                                            let mut last_ident: Option<&str> = None;
                                            let mut c2 = constructor.walk();
                                            for part in constructor.children(&mut c2) {
                                                if part.kind() == "identifier" {
                                                    last_ident = Some(&code[part.byte_range()]);
                                                }
                                            }
                                            last_ident
                                        };
                                        if let Some(typ) = type_name {
                                            let range = Range::new(
                                                child.start_position().row as u32,
                                                child.start_position().column as u16,
                                                child.end_position().row as u32,
                                                child.end_position().column as u16,
                                            );
                                            out.push((var, typ, range));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Recurse
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    walk(&child, code, out);
                }
            }

            walk(&root, code, &mut bindings);
        }

        bindings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FileId;

    #[test]
    fn test_typescript_import_extraction() {
        println!("\n=== TypeScript Import Extraction Test ===\n");

        let mut parser = TypeScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
import { Component, useState } from 'react';
import React from 'react';
import * as utils from './utils';
import type { Props } from './types';
import './styles.css';
export { Button } from './Button';
export * from './common';
"#;

        println!("Test code:\n{code}");

        let imports = parser.find_imports(code, file_id);

        println!("\nExtracted {} imports:", imports.len());
        for (i, import) in imports.iter().enumerate() {
            println!(
                "  {}. {} -> {:?} (glob: {})",
                i + 1,
                import.path,
                import.alias,
                import.is_glob
            );
        }

        // Verify counts (per-specifier imports now included)
        assert_eq!(imports.len(), 8, "Should extract 8 imports");

        // Verify specific imports
        // Named imports create one Import per specifier with local alias
        assert!(
            imports
                .iter()
                .any(|i| i.path == "react" && i.alias == Some("Component".to_string()))
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "react" && i.alias == Some("useState".to_string()))
        );
        // Default import has alias
        assert!(
            imports
                .iter()
                .any(|i| i.path == "react" && i.alias == Some("React".to_string()))
        );
        // Namespace import has alias and is_glob
        assert!(
            imports
                .iter()
                .any(|i| i.path == "./utils" && i.alias == Some("utils".to_string()) && i.is_glob)
        );
        // Type import (named) captured as import with alias
        assert!(
            imports
                .iter()
                .any(|i| i.path == "./types" && i.alias == Some("Props".to_string()))
        );
        // Side-effect import
        assert!(
            imports
                .iter()
                .any(|i| i.path == "./styles.css" && i.alias.is_none())
        );
        // Re-export
        assert!(imports.iter().any(|i| i.path == "./Button"));
        // Re-export all
        assert!(imports.iter().any(|i| i.path == "./common" && i.is_glob));

        println!("\n✅ Import extraction test passed");
    }

    #[test]
    fn test_generic_type_extraction_in_constructors() {
        println!("\n=== TypeScript Generic Type Extraction in Constructors Test ===\n");

        let mut parser = TypeScriptParser::new().unwrap();

        let code = r#"
interface Session {
    id: string;
}

interface User {
    name: string;
}

const sessions = new Map<string, Session>();
const users = new Set<User>();
const nested = new Array<Map<string, User>>();
const simple = new Map();
const typed: Map<string, Session> = new Map();
const hook = useState<Session>(null);
"#;

        println!("Test code:\n{code}");

        // Extract type uses
        let uses = parser.find_uses(code);

        println!("\nExtracted {} type uses:", uses.len());
        for (i, (context, type_name, _range)) in uses.iter().enumerate() {
            println!("  {}. {} uses {}", i + 1, context, type_name);
        }

        // Verify that generic type parameters are captured
        assert!(
            uses.iter()
                .any(|(ctx, typ, _)| ctx == &"sessions" && typ == &"Session"),
            "Should capture: sessions uses Session"
        );

        assert!(
            uses.iter()
                .any(|(ctx, typ, _)| ctx == &"users" && typ == &"User"),
            "Should capture: users uses User"
        );

        assert!(
            uses.iter()
                .any(|(ctx, typ, _)| ctx == &"nested" && typ == &"User"),
            "Should capture: nested uses User (from Map<string, User>)"
        );

        // Note: Type annotations are already handled by the existing code
        // in the "variable_declarator" case, which extracts from node.child_by_field_name("type")
        assert!(
            uses.iter()
                .any(|(ctx, typ, _)| ctx == &"typed" && (typ == &"Session" || typ == &"Map")),
            "Should capture type annotation (either Map or Session)"
        );

        assert!(
            uses.iter()
                .any(|(ctx, typ, _)| ctx == &"useState" && typ == &"Session"),
            "Should capture: useState uses Session (from function call)"
        );

        println!("\n✅ Generic type extraction test passed");
    }

    #[test]
    fn test_extends_vs_implements_separation() {
        println!("\n=== TypeScript Extends vs Implements Separation Test ===\n");

        let mut parser = TypeScriptParser::new().unwrap();

        let code = r#"
interface Serializable {
    serialize(): string;
}

interface Comparable<T> {
    compareTo(other: T): number;
}

class BaseEntity {
    id: number;
}

// Class extends another class
class User extends BaseEntity implements Serializable, Comparable<User> {
    name: string;

    serialize(): string {
        return JSON.stringify(this);
    }

    compareTo(other: User): number {
        return this.id - other.id;
    }
}

// Class extends a class (inheritance chain)
class Admin extends User {
    permissions: string[];
}

// Interface extends another interface
interface AdvancedSerializable extends Serializable {
    deserialize(data: string): void;
}
"#;

        println!("Test code:\n{code}");

        // Get extends relationships
        let extends = parser.find_extends(code);
        println!("\nExtends relationships ({}):", extends.len());
        for (child, parent, _) in &extends {
            println!("  {child} extends {parent}");
        }

        // Get implements relationships
        let implements = parser.find_implementations(code);
        println!("\nImplements relationships ({}):", implements.len());
        for (implementor, interface, _) in &implements {
            println!("  {implementor} implements {interface}");
        }

        // Verify extends relationships
        assert!(
            extends
                .iter()
                .any(|(c, p, _)| c == &"User" && p == &"BaseEntity"),
            "User should extend BaseEntity"
        );
        assert!(
            extends
                .iter()
                .any(|(c, p, _)| c == &"Admin" && p == &"User"),
            "Admin should extend User"
        );
        assert!(
            extends
                .iter()
                .any(|(c, p, _)| c == &"AdvancedSerializable" && p == &"Serializable"),
            "AdvancedSerializable should extend Serializable"
        );

        // Verify implements relationships
        assert!(
            implements
                .iter()
                .any(|(c, i, _)| c == &"User" && i == &"Serializable"),
            "User should implement Serializable"
        );
        assert!(
            implements
                .iter()
                .any(|(c, i, _)| c == &"User" && i == &"Comparable"),
            "User should implement Comparable"
        );

        // Verify separation - extends should NOT be in implements
        assert!(
            !implements
                .iter()
                .any(|(c, p, _)| c == &"User" && p == &"BaseEntity"),
            "User extends BaseEntity should NOT be in implements"
        );
        assert!(
            !implements
                .iter()
                .any(|(c, p, _)| c == &"Admin" && p == &"User"),
            "Admin extends User should NOT be in implements"
        );

        println!("\n✅ Extends vs Implements separation test passed");
    }

    #[test]
    fn test_complex_import_patterns() {
        println!("\n=== Complex Import Patterns Test ===\n");

        let mut parser = TypeScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
// Mixed default and named
import React, { Component, useState as useStateHook } from 'react';

// Type mixed with value imports
import { type Config, createConfig } from './config';

// Aliased imports
import { Helper as H } from './helper';

// Re-export with rename
export { default as MyButton } from './Button';
"#;

        let imports = parser.find_imports(code, file_id);

        println!("Found {} imports in complex patterns", imports.len());
        for import in &imports {
            println!(
                "  - {} -> {:?} (glob: {})",
                import.path, import.alias, import.is_glob
            );
        }

        // Should have 7 imports (per-specifier named imports)
        assert_eq!(imports.len(), 7, "Should have 7 imports");

        // Check for React default import
        let react_default = imports
            .iter()
            .find(|i| i.path == "react" && i.alias == Some("React".to_string()));
        assert!(react_default.is_some(), "Should find React default import");

        // Check for per-specifier config imports (Config type and createConfig function)
        assert!(
            imports
                .iter()
                .any(|i| i.path == "./config" && i.alias == Some("Config".to_string()))
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "./config" && i.alias == Some("createConfig".to_string()))
        );

        // Check for aliased helper import
        assert!(
            imports
                .iter()
                .any(|i| i.path == "./helper" && i.alias == Some("H".to_string()))
        );

        println!("✅ Complex patterns handled correctly");
    }

    #[test]
    fn test_typescript_export_visibility_is_public() {
        let mut parser = TypeScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();
        let code = r#"export function createChat() { return 'ok'; }"#;

        let mut counter = SymbolCounter::new();
        // Use internal parse path by calling parse directly via trait impl in a minimal way
        let symbols = parser.parse(code, file_id, &mut counter);
        // Should produce exactly one function symbol named createChat with Public visibility
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "createChat"
                    && matches!(s.visibility, Visibility::Public))
        );
    }

    #[test]
    fn test_typescript_find_variable_types_new_expression() {
        let mut parser = TypeScriptParser::new().unwrap();
        let code = r#"
            class ChatSDK { createChat(): string { return 'x'; } }
            function start() {
                const sdk = new ChatSDK();
                sdk.createChat();
            }
        "#;
        let bindings = parser.find_variable_types(code);
        // Expect a binding for sdk -> ChatSDK
        assert!(
            bindings
                .iter()
                .any(|(var, typ, _)| *var == "sdk" && *typ == "ChatSDK")
        );
    }

    #[test]
    fn test_typescript_find_method_calls_extraction() {
        let mut parser = TypeScriptParser::new().unwrap();
        let code = r#"
            class ChatSDK { createChat(): string { return 'x'; } }
            function startVoiceConversation() {
                const sdk = new ChatSDK();
                sdk.createChat();
            }
        "#;
        let calls = parser.find_method_calls(code);
        // Check that we have at least one call to createChat with receiver sdk
        assert!(calls.iter().any(|c| c.caller == "startVoiceConversation"
            && c.method_name == "createChat"
            && c.receiver.as_deref() == Some("sdk")));
    }

    #[test]
    fn test_typescript_filter_primitive_uses() {
        let mut parser = TypeScriptParser::new().unwrap();
        let code = r#"
            function f(): string { return '' }
            function g(): number { return 1 }
        "#;
        let uses = parser.find_uses(code);
        // No primitive types should be reported
        assert!(
            uses.is_empty(),
            "primitive type uses should be filtered out"
        );
    }

    #[test]
    fn test_import_path_formats() {
        println!("\n=== Import Path Formats Test ===\n");

        let mut parser = TypeScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
// Relative paths
import { a } from './sibling';
import { b } from '../parent';
import { c } from '../../grandparent';

// Node modules
import express from 'express';
import { Request } from '@types/express';

// Path aliases (would need tsconfig to resolve)
import { service } from '@app/services';

// Index imports
import utils from './utils';  // implies ./utils/index
"#;

        let imports = parser.find_imports(code, file_id);

        println!("Path formats found:");
        for import in &imports {
            println!("  - {}", import.path);
        }

        assert!(imports.iter().any(|i| i.path == "./sibling"));
        assert!(imports.iter().any(|i| i.path == "../parent"));
        assert!(imports.iter().any(|i| i.path == "express"));
        assert!(imports.iter().any(|i| i.path.starts_with("@")));

        println!("✅ Various path formats extracted correctly");
    }

    #[test]
    fn test_export_variations() {
        println!("\n=== Export Variations Test ===\n");

        let mut parser = TypeScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
// Re-exports
export { Component } from 'react';
export { Helper as PublicHelper } from './helper';
export * from './utils';

// Type re-exports
export type { Props } from './types';
"#;

        let imports = parser.find_imports(code, file_id);

        println!("Export-based imports found:");
        for import in &imports {
            println!(
                "  - {} -> {:?} (glob: {})",
                import.path, import.alias, import.is_glob
            );
        }

        assert!(
            imports
                .iter()
                .any(|i| i.path == "react" && i.alias.is_none())
        );
        assert!(imports.iter().any(|i| i.path == "./helper"));
        assert!(imports.iter().any(|i| i.path == "./utils" && i.is_glob));

        println!("✅ Export variations handled correctly");
    }
}
