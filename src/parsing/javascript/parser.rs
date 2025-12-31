//! JavaScript parser implementation
//!
//! **Tree-sitter ABI Version**: ABI-14 (tree-sitter-javascript 0.23.0)
//!
//! Note: This parser uses ABI-14 with JSX support via the tree-sitter-javascript grammar.
//! JavaScript and TypeScript share very similar syntax, but JavaScript doesn't have
//! TypeScript-specific features like interfaces, type aliases, type annotations, abstract classes, etc.

use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    LanguageParser, MethodCall, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use tree_sitter::{Language, Node, Parser};

/// JavaScript language parser
pub struct JavaScriptParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
    /// Track symbols that are default exported (e.g., export default Container)
    default_exported_symbols: std::collections::HashSet<String>,
    /// Track symbols that are named exported (e.g., export { Card, CardHeader })
    named_exported_symbols: std::collections::HashSet<String>,
    /// Track JSX component usages (caller -> component name)
    component_usages: Vec<(String, String)>,
}

impl JavaScriptParser {
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

    /// Parse JavaScript source code and extract all symbols
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        // Reset context and exports for each file
        self.context = ParserContext::new();
        self.default_exported_symbols.clear();
        self.named_exported_symbols.clear();
        self.component_usages.clear();
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
                eprintln!("Failed to parse JavaScript file");
            }
        }

        // Update visibility for default exported symbols
        for symbol in &mut symbols {
            if self.default_exported_symbols.contains(symbol.name.as_ref()) {
                tracing::debug!(
                    "[javascript] marking '{}' as Public (default export)",
                    symbol.name
                );
                symbol.visibility = Visibility::Public;
            }
        }

        // Update visibility for named exported symbols
        for symbol in &mut symbols {
            if self.named_exported_symbols.contains(symbol.name.as_ref()) {
                symbol.visibility = Visibility::Public;
            }
        }

        symbols
    }

    /// Create a new JavaScript parser
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        // Use the JavaScript grammar which includes JSX support
        let language: Language = tree_sitter_javascript::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| format!("Failed to set JavaScript language: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
            default_exported_symbols: std::collections::HashSet::new(),
            named_exported_symbols: std::collections::HashSet::new(),
            component_usages: Vec::new(),
        })
    }

    /// Extract symbols from a JavaScript node
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
        // Guard against stack overflow
        if !check_recursion_depth(depth, node) {
            return;
        }
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                // Register ALL child nodes for audit (including parameters, etc.)
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
                // Note: In JavaScript, function declarations are hoisted
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
                        depth + 1,
                    );
                }

                // Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "class_declaration" => {
                // Register ALL child nodes for audit
                self.register_node_recursively(node);
                // Extract class name for parent tracking
                let class_name = node
                    .child_by_field_name("name")
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
                    self.extract_class_members(
                        node,
                        code,
                        file_id,
                        counter,
                        symbols,
                        module_path,
                        depth + 1,
                    );

                    // Exit scope first (this clears the current context)
                    self.context.exit_scope();

                    // Then restore the previous parent context
                    self.context.set_current_function(saved_function);
                    self.context.set_current_class(saved_class);
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
                    depth + 1,
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
                        depth + 1,
                    );
                    i += 1;
                }
            }
            "export_statement" => {
                // Check if this is a default export (export default SomeName)
                self.register_handled_node(node.kind(), node.kind_id());

                // Look for 'default' keyword followed by an identifier
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();

                // Check if this is "export default <identifier>"
                let mut found_default = false;
                for (i, child) in children.iter().enumerate() {
                    if child.kind() == "default" {
                        found_default = true;
                        // The next node should be the identifier being exported
                        if i + 1 < children.len() {
                            let next = &children[i + 1];
                            if next.kind() == "identifier" {
                                let symbol_name = &code[next.byte_range()];
                                self.default_exported_symbols
                                    .insert(symbol_name.to_string());
                                tracing::debug!(
                                    "[javascript] found default export of '{symbol_name}'"
                                );
                            }
                        }
                    }
                }

                // Check for named export lists (export { Card, CardHeader })
                for child in &children {
                    if child.kind() == "export_clause" {
                        // Process export specifiers within the export clause
                        let mut export_cursor = child.walk();
                        for export_child in child.children(&mut export_cursor) {
                            if export_child.kind() == "export_specifier" {
                                // Get the name being exported
                                if let Some(name_node) = export_child.child_by_field_name("name") {
                                    let symbol_name = &code[name_node.byte_range()];
                                    self.named_exported_symbols.insert(symbol_name.to_string());
                                }
                            }
                        }
                    }
                }

                // Still process children for nested declarations (e.g., export function foo())
                if !found_default {
                    for child in children {
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
            "jsx_element" | "jsx_self_closing_element" => {
                // Track JSX component usage as Uses relationship
                self.register_handled_node(node.kind(), node.kind_id());

                tracing::debug!(
                    "[javascript] found JSX node: {} at {}:{}, current function: {:?}",
                    node.kind(),
                    node.start_position().row,
                    node.start_position().column,
                    self.context.current_function()
                );

                self.track_jsx_component_usage(node, code);

                // Process children to find nested JSX elements
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
        // Get the class name from the name field
        let name_node = node.child_by_field_name("name")?;
        let name = &code[name_node.byte_range()];

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
        depth: usize,
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
                                depth + 1,
                            );

                            // Exit scope first (this clears the current context)
                            self.context.exit_scope();

                            // Then restore the previous parent context when exiting method
                            self.context.set_current_function(saved_function);
                        }
                    }
                    "field_definition" | "public_field_definition" => {
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

    /// Process variable declarations
    fn process_variable_declaration(
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
                                            depth + 1,
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
        let name_node = node.child_by_field_name("property")?;
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

    /// Extract class signature (with extends)
    fn extract_class_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the class body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Determine visibility based on export keywords
    fn determine_visibility(&self, node: Node, code: &str) -> Visibility {
        // 1) Ancestor check: many JS grammars wrap declarations in export_statement
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
        let prefix = crate::parsing::safe_substring_window(code, start, 10);
        if prefix.contains("export ") || prefix.contains("export\n") {
            return Visibility::Public;
        }

        // Default: not exported
        Visibility::Private
    }

    /// Determine method/property visibility
    fn determine_method_visibility(&self, node: Node, code: &str) -> Visibility {
        let signature = &code[node.byte_range()];

        // In JavaScript, private fields start with #
        if signature.starts_with("#") || signature.contains(" #") {
            Visibility::Private
        } else {
            Visibility::Public // Default for class members
        }
    }

    /// Find class extends relationships in JavaScript
    fn find_implementations_in_node<'a>(
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if node.kind() == "class_declaration" {
            // Get class name first
            let class_name = node
                .child_by_field_name("name")
                .map(|n| &code[n.byte_range()]);

            if let Some(class_name) = class_name {
                // Look for class_heritage child node (it's a child, not a field!)
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "class_heritage" {
                        // The heritage node directly contains the parent class identifier
                        // Structure: class_heritage -> extends keyword + identifier
                        let mut heritage_cursor = child.walk();
                        for heritage_child in child.children(&mut heritage_cursor) {
                            if heritage_child.kind() == "identifier"
                                || heritage_child.kind() == "member_expression"
                            {
                                let base_name = &code[heritage_child.byte_range()];
                                let range = Range::new(
                                    heritage_child.start_position().row as u32,
                                    heritage_child.start_position().column as u16,
                                    heritage_child.end_position().row as u32,
                                    heritage_child.end_position().column as u16,
                                );
                                implementations.push((class_name, base_name, range));
                            }
                        }
                    }
                }
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::find_implementations_in_node(child, code, implementations);
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
        tracing::debug!(
            "[javascript] process_import_statement, code: {}",
            &code[node.byte_range()]
        );

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
            tracing::debug!(
                "[javascript]   found import_clause: {}",
                &code[import_clause.byte_range()]
            );

            // Check for different import types
            let mut has_default = false;
            let mut has_named = false;
            let mut has_namespace = false;
            let mut default_name = None;
            let mut namespace_name = None;

            let mut cursor = import_clause.walk();
            for child in import_clause.children(&mut cursor) {
                tracing::debug!(
                    "[javascript]     child kind: {}, text: {}",
                    child.kind(),
                    &code[child.byte_range()]
                );
                match child.kind() {
                    "identifier" => {
                        // Default import
                        has_default = true;
                        let name = code[child.byte_range()].to_string();
                        tracing::debug!("[javascript]       setting default_name = {name}");
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
                                    is_type_only: false, // JavaScript doesn't have type-only imports
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
            tracing::debug!(
                "[javascript]   summary: has_default={has_default}, has_named={has_named}, has_namespace={has_namespace}, default_name={default_name:?}, namespace_name={namespace_name:?}"
            );

            if has_namespace {
                // Namespace import: import * as utils from './utils'
                imports.push(Import {
                    path: source_path.to_string(),
                    alias: namespace_name,
                    file_id,
                    is_glob: true,
                    is_type_only: false,
                });
            } else if has_default && has_named {
                // Mixed import: import React, { Component } from 'react'
                // We create one import with the default as alias
                imports.push(Import {
                    path: source_path.to_string(),
                    alias: default_name,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            } else if has_default {
                // Default only: import React from 'react'
                tracing::debug!(
                    "[javascript]   adding default import: path='{source_path}', alias={default_name:?}"
                );
                imports.push(Import {
                    path: source_path.to_string(),
                    alias: default_name,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
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
                is_type_only: false,
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

        // Check what's being exported
        let node_text = &code[node.byte_range()];
        if node_text.contains("* from") {
            // export * from './module'
            imports.push(Import {
                path: source_path.to_string(),
                alias: None,
                file_id,
                is_glob: true,
                is_type_only: false,
            });
        } else {
            // Named re-exports - just track the module being imported from
            imports.push(Import {
                path: source_path.to_string(),
                alias: None,
                file_id,
                is_glob: false,
                is_type_only: false,
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
        // Handle export wrappers that contain a function declaration
        if node.kind() == "export_statement" {
            let mut w = node.walk();
            for child in node.children(&mut w) {
                if child.kind() == "function_declaration"
                    || child.kind() == "generator_function_declaration"
                {
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
            || node.kind() == "generator_function_declaration"
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
                tracing::debug!(
                    "[javascript] entering {} '{}' at line {}",
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
                    tracing::debug!(
                        "[javascript] found call to {} at line {}, context = {:?}",
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
                                "function_declaration" | "generator_function_declaration" => {
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
                                        } else if p.kind() == "pair" {
                                            // Handle object property: { propertyName: () => { ... } }
                                            // Look for the parent object's variable name
                                            let mut obj_anc = p.parent();
                                            while let Some(oa) = obj_anc {
                                                if oa.kind() == "object" {
                                                    // Found the object, now find its variable declarator
                                                    if let Some(obj_parent) = oa.parent() {
                                                        if obj_parent.kind()
                                                            == "variable_declarator"
                                                        {
                                                            if let Some(name_node) = obj_parent
                                                                .child_by_field_name("name")
                                                            {
                                                                ctx = Some(
                                                                    &code[name_node.byte_range()],
                                                                );
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                obj_anc = oa.parent();
                                            }
                                            if ctx.is_some() {
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
                                    || k == "generator_function_declaration"
                                    || k == "class_declaration"
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
            || node.kind() == "generator_function_declaration"
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
                                caller_range: None, // TODO: track caller definition range
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

                // JavaScript doesn't have static method calls like :: but uses .
                // We can't easily distinguish static from instance without type information
                let is_static = false;

                Some((Some(receiver), method_name, is_static))
            }
            _ => None,
        }
    }

    /// Track JSX component usage relationships
    fn track_jsx_component_usage(&mut self, node: Node, code: &str) {
        let component_name = match node.kind() {
            "jsx_element" => {
                // For <Component>...</Component>, get name from opening element
                node.child_by_field_name("open_tag")
                    .and_then(|tag| tag.child_by_field_name("name"))
                    .map(|name| &code[name.byte_range()])
            }
            "jsx_self_closing_element" => {
                // For <Component />, get name directly
                node.child_by_field_name("name")
                    .map(|name| &code[name.byte_range()])
            }
            _ => None,
        };

        tracing::debug!("[javascript] JSX component_name extracted: {component_name:?}");

        if let Some(component_name) = component_name {
            // Filter out HTML elements (lowercase) - only track React components (uppercase)
            if component_name
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase())
            {
                // Track this as a component usage from current context
                if let Some(current_fn) = self.context.current_function() {
                    tracing::debug!(
                        "[javascript] tracking JSX usage: {current_fn} uses {component_name}"
                    );
                    self.component_usages
                        .push((current_fn.to_string(), component_name.to_string()));
                }
            } else {
                tracing::debug!("[javascript] skipping lowercase JSX element: {component_name}");
            }
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

    /// Extract JSX component usages recursively
    /// Tracks function context and collects JSX component uses
    fn extract_jsx_uses_recursive<'a>(
        node: &Node,
        code: &'a str,
        current_fn: Option<&'a str>,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) -> Option<&'a str> {
        // Track current function context
        let func_context = if node.kind() == "function_declaration"
            || node.kind() == "generator_function_declaration"
            || node.kind() == "arrow_function"
        {
            if let Some(name_node) = node.child_by_field_name("name") {
                Some(&code[name_node.byte_range()])
            } else {
                current_fn
            }
        } else {
            current_fn
        };

        // Extract JSX component usage
        if node.kind() == "jsx_element" || node.kind() == "jsx_self_closing_element" {
            let component_name = match node.kind() {
                "jsx_element" => node
                    .child_by_field_name("open_tag")
                    .and_then(|tag| tag.child_by_field_name("name"))
                    .map(|name| &code[name.byte_range()]),
                "jsx_self_closing_element" => node
                    .child_by_field_name("name")
                    .map(|name| &code[name.byte_range()]),
                _ => None,
            };

            if let Some(component_name) = component_name {
                // Only track uppercase components (React convention)
                if component_name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_uppercase())
                {
                    if let Some(fn_name) = func_context {
                        let range = Range {
                            start_line: node.start_position().row as u32,
                            start_column: node.start_position().column as u16,
                            end_line: node.end_position().row as u32,
                            end_column: node.end_position().column as u16,
                        };
                        uses.push((fn_name, component_name, range));
                    }
                }
            }
        }

        // Recurse to children with current context
        for child in node.children(&mut node.walk()) {
            Self::extract_jsx_uses_recursive(&child, code, func_context, uses);
        }

        func_context
    }
}

impl NodeTracker for JavaScriptParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<crate::parsing::HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for JavaScriptParser {
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
        // Look for JSDoc comments (/** ... */)

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
            Self::find_implementations_in_node(tree.root_node(), code, &mut implementations);
        }

        implementations
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // In JavaScript, extends is the only inheritance mechanism
        // So find_extends and find_implementations return the same thing
        self.find_implementations(code)
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

        // Extract JSX component usages during find_uses traversal
        Self::extract_jsx_uses_recursive(&root, code, None, &mut uses);

        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // JavaScript doesn't have interfaces or abstract methods
        // Class methods are already extracted as symbols, not as defines
        let _tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        Vec::new()
    }

    fn language(&self) -> crate::parsing::Language {
        crate::parsing::Language::JavaScript
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Basic JS variable type inference for `const/let/var x = new Type()` patterns
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
    fn test_javascript_import_extraction() {
        println!("\n=== JavaScript Import Extraction Test ===\n");

        let mut parser = JavaScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        let code = r#"
import { Component, useState } from 'react';
import React from 'react';
import * as utils from './utils';
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
        assert_eq!(imports.len(), 7, "Should extract 7 imports");

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
    fn test_javascript_export_visibility_is_public() {
        let mut parser = JavaScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();
        let code = r#"export function createChat() { return 'ok'; }"#;

        let mut counter = SymbolCounter::new();
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
    fn test_javascript_find_variable_types_new_expression() {
        let mut parser = JavaScriptParser::new().unwrap();
        let code = r#"
            class ChatSDK { createChat() { return 'x'; } }
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
    fn test_javascript_find_method_calls_extraction() {
        let mut parser = JavaScriptParser::new().unwrap();
        let code = r#"
            class ChatSDK { createChat() { return 'x'; } }
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
    fn test_jsx_component_usage_tracking() {
        let mut parser = JavaScriptParser::new().unwrap();
        let code = r#"
import React from 'react';
import { Button } from './components/ui/button';

export function MyPage() {
  return (
    <div>
      <Button>Click me</Button>
    </div>
  );
}

export function AnotherComponent() {
  return <Button>Another</Button>;
}
        "#;

        let uses = parser.find_uses(code);

        println!("\nJSX Uses found:");
        for (caller, component, _range) in &uses {
            println!("  {caller} uses {component}");
        }

        // Check that MyPage uses Button
        assert!(
            uses.iter()
                .any(|(caller, component, _)| *caller == "MyPage" && *component == "Button"),
            "MyPage should use Button component"
        );

        // Check that AnotherComponent uses Button
        assert!(
            uses.iter()
                .any(|(caller, component, _)| *caller == "AnotherComponent"
                    && *component == "Button"),
            "AnotherComponent should use Button component"
        );

        println!("✅ JSX component usage tracking working");
    }

    #[test]
    fn test_class_extends_extraction() {
        let mut parser = JavaScriptParser::new().unwrap();
        let code = r#"
class Animal {
    constructor(name) {
        this.name = name;
    }
}

class Dog extends Animal {
    constructor(name, breed) {
        super(name);
        this.breed = breed;
    }
}
        "#;

        let extends = parser.find_extends(code);

        println!("\nExtends relationships found:");
        for (child, parent, _) in &extends {
            println!("  {child} extends {parent}");
        }

        assert!(
            extends
                .iter()
                .any(|(child, parent, _)| *child == "Dog" && *parent == "Animal"),
            "Dog should extend Animal"
        );

        println!("✅ Class extends extraction working");
    }

    #[test]
    fn test_arrow_function_extraction() {
        let mut parser = JavaScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();

        let code = r#"
const myFunction = () => {
    console.log('Hello');
};

const add = (a, b) => a + b;
        "#;

        let symbols = parser.parse(code, file_id, &mut counter);

        println!("\nSymbols found:");
        for symbol in &symbols {
            println!("  {} ({:?})", symbol.name, symbol.kind);
        }

        // Should extract arrow functions as Function symbols
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "myFunction" && s.kind == SymbolKind::Function),
            "Should extract myFunction as Function"
        );

        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "add" && s.kind == SymbolKind::Function),
            "Should extract add as Function"
        );

        println!("✅ Arrow function extraction working");
    }

    #[test]
    fn test_const_vs_let_vs_var() {
        let mut parser = JavaScriptParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();

        let code = r#"
const myConst = 42;
let myLet = 'hello';
var myVar = true;
        "#;

        let symbols = parser.parse(code, file_id, &mut counter);

        println!("\nSymbols found:");
        for symbol in &symbols {
            println!("  {} ({:?})", symbol.name, symbol.kind);
        }

        // const should be Constant
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "myConst" && s.kind == SymbolKind::Constant),
            "const should be Constant"
        );

        // let should be Variable
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "myLet" && s.kind == SymbolKind::Variable),
            "let should be Variable"
        );

        // var should be Variable
        assert!(
            symbols
                .iter()
                .any(|s| s.name.as_ref() == "myVar" && s.kind == SymbolKind::Variable),
            "var should be Variable"
        );

        println!("✅ const/let/var extraction working");
    }
}
