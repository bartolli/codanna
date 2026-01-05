//! Java language parser implementation
//!
//! Provides symbol extraction for Java using tree-sitter.
//!
//! Scaffolding created based on Kotlin parser structure.
//! TODO: Implement methods after exploring actual Java AST with tree-sitter.

use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, MethodCall, NodeTracker, NodeTrackingState,
    ParserContext,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, Visibility};
use std::any::Any;
use std::collections::HashSet;
use std::sync::OnceLock;
use tree_sitter::{Node, Parser};

// Node type constants from tree-sitter-java grammar
const FILE_SCOPE: &str = "<file>";
const NODE_CLASS_DECLARATION: &str = "class_declaration";
const NODE_INTERFACE_DECLARATION: &str = "interface_declaration";
const NODE_ENUM_DECLARATION: &str = "enum_declaration";
const NODE_METHOD_DECLARATION: &str = "method_declaration";
const NODE_CONSTRUCTOR_DECLARATION: &str = "constructor_declaration";
const NODE_FIELD_DECLARATION: &str = "field_declaration";
const NODE_PACKAGE_DECLARATION: &str = "package_declaration";
const NODE_IMPORT_DECLARATION: &str = "import_declaration";
const NODE_MODIFIERS: &str = "modifiers";
const NODE_BLOCK_COMMENT: &str = "block_comment";
const NODE_LINE_COMMENT: &str = "line_comment";
const NODE_METHOD_INVOCATION: &str = "method_invocation";

// Lazy-initialized HashSet for primitive types
static JAVA_PRIMITIVE_TYPES: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn get_primitive_types() -> &'static HashSet<&'static str> {
    JAVA_PRIMITIVE_TYPES.get_or_init(|| {
        let mut set = HashSet::new();
        set.insert("int");
        set.insert("long");
        set.insert("short");
        set.insert("byte");
        set.insert("float");
        set.insert("double");
        set.insert("boolean");
        set.insert("char");
        set.insert("void");
        set
    })
}

/// Parser for Java source files
pub struct JavaParser {
    parser: Parser,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for JavaParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaParser")
            .field("language", &"Java")
            .finish()
    }
}

impl JavaParser {
    /// Create a new parser instance
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .map_err(|e| format!("Failed to initialize Java parser: {e}"))?;

        Ok(Self {
            parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    // =========================================================================
    // HELPER METHODS - Basic Utilities
    // =========================================================================

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

    #[allow(dead_code)]
    #[inline]
    fn trimmed_text<'a>(&self, code: &'a str, node: Node) -> &'a str {
        self.text_for_node(code, node).trim()
    }

    // =========================================================================
    // HELPER METHODS - Documentation Extraction
    // =========================================================================

    /// Extract documentation comments (/** */ or //)
    /// TODO: Implement after tree-sitter exploration
    fn doc_comment_for(&self, node: &Node, code: &str) -> Option<String> {
        let mut result = String::new();
        let mut has_comment = false;

        // Stack to collect comments in reverse order (traverse backwards)
        let mut comment_stack: [Option<&str>; 8] = [None; 8];
        let mut stack_len = 0;
        let mut current = node.prev_sibling();

        // Special case: if previous sibling is package_declaration, check its children
        if let Some(sibling) = current {
            if sibling.kind() == NODE_PACKAGE_DECLARATION {
                let mut cursor = sibling.walk();
                for child in sibling.named_children(&mut cursor) {
                    let child_kind = child.kind();
                    if child_kind == NODE_BLOCK_COMMENT || child_kind == NODE_LINE_COMMENT {
                        let raw = self.text_for_node(code, child);
                        if let Some(cleaned) = self.extract_comment_text(raw) {
                            if has_comment {
                                result.push('\n');
                            }
                            result.push_str(cleaned);
                            has_comment = true;
                        }
                    }
                }
                if has_comment {
                    return Some(result);
                }
            }
        }

        // Standard case: traverse backwards collecting comments
        current = node.prev_sibling();
        while let Some(sibling) = current {
            let sibling_kind = sibling.kind();
            if sibling_kind != NODE_BLOCK_COMMENT && sibling_kind != NODE_LINE_COMMENT {
                break;
            }

            let raw = self.text_for_node(code, sibling);
            if stack_len < comment_stack.len() {
                if let Some(cleaned) = self.extract_comment_text(raw) {
                    comment_stack[stack_len] = Some(cleaned);
                    stack_len += 1;
                    current = sibling.prev_sibling();
                    continue;
                }
            }
            break;
        }

        // Build result from stack in reverse order
        if stack_len > 0 {
            for i in (0..stack_len).rev() {
                if let Some(comment) = comment_stack[i] {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(comment);
                    has_comment = true;
                }
            }
        }

        if has_comment { Some(result) } else { None }
    }

    /// Extract text from comment, removing delimiters
    ///
    /// SAFETY: Uses strip_prefix/strip_suffix which work at character boundaries,
    /// making this unicode-safe (no panic on emoji or multi-byte chars)
    fn extract_comment_text<'a>(&self, raw: &'a str) -> Option<&'a str> {
        let trimmed = raw.trim();

        // JavaDoc comment: /** ... */
        if let Some(content) = trimmed
            .strip_prefix("/**")
            .and_then(|s| s.strip_suffix("*/"))
        {
            return Some(content.trim());
        }

        // Block comment: /* ... */
        if let Some(content) = trimmed
            .strip_prefix("/*")
            .and_then(|s| s.strip_suffix("*/"))
        {
            return Some(content.trim());
        }

        // Line comment: //
        if let Some(content) = trimmed.strip_prefix("//") {
            return Some(content.trim());
        }

        None
    }

    // =========================================================================
    // HELPER METHODS - Visibility & Modifiers
    // =========================================================================

    /// Determine visibility from modifiers
    fn determine_visibility(&self, node: Node, code: &str) -> Visibility {
        // Look for modifiers node among children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_MODIFIERS {
                let modifiers_text = self.text_for_node(code, child);

                if modifiers_text.contains("private") {
                    return Visibility::Private;
                } else if modifiers_text.contains("protected") {
                    return Visibility::Module; // Protected in Java
                } else if modifiers_text.contains("public") {
                    return Visibility::Public;
                }
                // If modifiers exist but no visibility keyword, it's package-private
                return Visibility::Crate;
            }
        }
        // No modifiers node means package-private (default in Java)
        Visibility::Crate
    }

    // =========================================================================
    // HELPER METHODS - Signature Extraction
    // =========================================================================

    /// Extract signature without body
    /// TODO: Implement after tree-sitter exploration
    fn extract_signature(&self, node: Node, code: &str) -> String {
        let mut signature = String::with_capacity(100);

        // Add modifiers if present
        if let Some(modifiers) = node.child_by_field_name("modifiers") {
            signature.push_str(self.text_for_node(code, modifiers));
            signature.push(' ');
        }

        // Add type parameters (generics) if present
        if let Some(type_params) = node.child_by_field_name("type_parameters") {
            signature.push_str(self.text_for_node(code, type_params));
            signature.push(' ');
        }

        // Add return type if present (methods have return type, constructors don't)
        if let Some(return_type) = node.child_by_field_name("type") {
            signature.push_str(self.text_for_node(code, return_type));
            signature.push(' ');
        }

        // Add name
        if let Some(name) = node.child_by_field_name("name") {
            signature.push_str(self.text_for_node(code, name));
        }

        // Add parameters
        if let Some(params) = node.child_by_field_name("parameters") {
            signature.push_str(self.text_for_node(code, params));
        }

        signature.trim().to_string()
    }

    /// Extract generic parameters (e.g., <T extends Foo, U>)
    /// TODO: Implement after tree-sitter exploration
    #[allow(dead_code)]
    fn extract_generic_parameters<'a>(&self, _node: Node, _code: &'a str) -> Vec<&'a str> {
        // TODO: Parse type_parameters node
        Vec::new()
    }

    /// Extract method parameter types
    /// TODO: Implement after tree-sitter exploration
    #[allow(dead_code)]
    fn extract_parameter_types<'a>(&self, _node: Node, _code: &'a str) -> Vec<&'a str> {
        // TODO: Parse formal_parameters node
        Vec::new()
    }

    /// Extract return type annotation
    /// TODO: Implement after tree-sitter exploration
    #[allow(dead_code)]
    fn extract_return_type<'a>(&self, _node: Node, _code: &'a str) -> Option<&'a str> {
        // TODO: Find return type from method_declaration
        None
    }

    // =========================================================================
    // SYMBOL EXTRACTION - Process Methods (convert node → Symbol)
    // =========================================================================

    /// Process class declaration node
    /// TODO: Implement after tree-sitter exploration
    fn handle_class_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        module_path: &str,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type_parameters, etc.)
        self.register_node_recursively(node);

        // Determine if this is a class, interface, or enum by checking node type
        let symbol_kind = match node.kind() {
            NODE_INTERFACE_DECLARATION => crate::SymbolKind::Interface,
            NODE_ENUM_DECLARATION => crate::SymbolKind::Enum,
            _ => crate::SymbolKind::Class,
        };

        // Extract class/interface/enum name - look for "identifier" child
        let mut class_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                class_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let class_name = match class_name {
            Some(name) => name,
            None => return,
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        let mut symbol = Symbol::new(symbol_id, class_name.as_str(), symbol_kind, file_id, range);
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }
        // Set scope context based on nesting
        // Nested class: ClassMember with parent class name
        // Top-level class: Module scope
        symbol.scope_context = Some(if let Some(parent_class) = context.current_class() {
            crate::symbol::ScopeContext::ClassMember {
                class_name: Some(parent_class.to_string().into()),
            }
        } else {
            crate::symbol::ScopeContext::Module
        });

        // Save parent context before entering new scope
        let saved_class = context.current_class().map(String::from);

        // Enter new scope
        context.enter_scope(crate::parsing::ScopeType::Class);
        context.set_current_class(Some(class_name.clone()));
        symbols.push(symbol);

        // Process class/interface/enum body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class_body"
                || child.kind() == "interface_body"
                || child.kind() == "enum_body"
            {
                let mut body_cursor = child.walk();
                for body_child in child.children(&mut body_cursor) {
                    self.extract_symbols_from_node(
                        body_child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        module_path,
                        depth + 1,
                    );
                }
                break;
            }
        }

        // Exit scope and restore context
        context.exit_scope();
        context.set_current_class(saved_class);
    }

    fn handle_method_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        _context: &ParserContext,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type_parameters, formal_parameters, etc.)
        self.register_node_recursively(node);

        // Extract method name - look for "identifier" child
        let mut method_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                method_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let method_name = match method_name {
            Some(name) => name,
            None => return,
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        let mut symbol = Symbol::new(
            symbol_id,
            method_name.as_str(),
            crate::SymbolKind::Function,
            file_id,
            range,
        );
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: _context.current_class().map(|name| name.to_string().into()),
        });

        symbols.push(symbol);
    }

    fn handle_constructor_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        _context: &ParserContext,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, formal_parameters, etc.)
        self.register_node_recursively(node);

        // Constructor name is same as class name - look for "identifier" child
        let mut constructor_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                constructor_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let constructor_name = match constructor_name {
            Some(name) => name,
            None => return,
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        let mut symbol = Symbol::new(
            symbol_id,
            constructor_name.as_str(),
            crate::SymbolKind::Function,
            file_id,
            range,
        );
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
            class_name: _context.current_class().map(|name| name.to_string().into()),
        });

        symbols.push(symbol);
    }

    fn handle_field_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        _context: &ParserContext,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type, variable_declarator, etc.)
        self.register_node_recursively(node);

        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        // Field declarations can have multiple variable_declarator children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                // Extract field name from variable_declarator
                let mut var_cursor = child.walk();
                for var_child in child.children(&mut var_cursor) {
                    if var_child.kind() == "identifier" {
                        let field_name = self.text_for_node(code, var_child).trim().to_string();
                        let symbol_id = counter.next_id();
                        let range = self.node_to_range(child);

                        let mut symbol = Symbol::new(
                            symbol_id,
                            field_name.as_str(),
                            crate::SymbolKind::Variable,
                            file_id,
                            range,
                        );
                        symbol.visibility = visibility;
                        if let Some(doc) = &doc_comment {
                            symbol.doc_comment = Some(doc.as_str().into());
                        }
                        symbol.scope_context = Some(crate::symbol::ScopeContext::ClassMember {
                            class_name: _context
                                .current_class()
                                .map(|name| name.to_string().into()),
                        });

                        symbols.push(symbol);
                        break;
                    }
                }
            }
        }
    }

    fn handle_annotation_type_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        module_path: &str,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        // Extract annotation type name - look for "identifier" child
        let mut annotation_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                annotation_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let annotation_name = match annotation_name {
            Some(name) => name,
            None => return,
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        let mut symbol = Symbol::new(
            symbol_id,
            annotation_name.as_str(),
            crate::SymbolKind::Interface, // Annotation types are similar to interfaces
            file_id,
            range,
        );
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }
        symbol.scope_context = Some(crate::symbol::ScopeContext::Module);

        // Save parent context before entering new scope
        let saved_class = context.current_class().map(String::from);

        // Enter new scope
        context.enter_scope(crate::parsing::ScopeType::Class);
        context.set_current_class(Some(annotation_name.clone()));
        symbols.push(symbol);

        // Process annotation type body (annotation_type_body)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "annotation_type_body" {
                let mut body_cursor = child.walk();
                for body_child in child.children(&mut body_cursor) {
                    self.extract_symbols_from_node(
                        body_child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        module_path,
                        depth + 1,
                    );
                }
                break;
            }
        }

        // Exit scope and restore context
        context.exit_scope();
        context.set_current_class(saved_class);
    }

    // =========================================================================
    // PACKAGE EXTRACTION
    // =========================================================================

    /// Extract package name from Java source file
    /// Returns the package in dot notation (e.g., "com.example.myapp")
    fn extract_package_name<'a>(&self, root: Node, code: &'a str) -> Option<&'a str> {
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == NODE_PACKAGE_DECLARATION {
                // Package declaration has scoped_identifier or identifier children
                let mut pkg_cursor = child.walk();
                for pkg_child in child.children(&mut pkg_cursor) {
                    match pkg_child.kind() {
                        "scoped_identifier" | "identifier" => {
                            return Some(self.text_for_node(code, pkg_child).trim());
                        }
                        _ => continue,
                    }
                }
            }
        }
        None
    }

    // =========================================================================
    // SYMBOL EXTRACTION - Main Recursive Walker
    // =========================================================================

    /// Extract symbols from AST recursively
    /// TODO: Implement after tree-sitter exploration - UPDATE match arms with actual node kinds
    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        module_path: &str,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            NODE_CLASS_DECLARATION | NODE_INTERFACE_DECLARATION | NODE_ENUM_DECLARATION => {
                self.handle_class_declaration(
                    node,
                    code,
                    file_id,
                    symbols,
                    counter,
                    context,
                    module_path,
                    depth,
                );
            }
            NODE_METHOD_DECLARATION => {
                self.handle_method_declaration(node, code, file_id, symbols, counter, context);
            }
            NODE_CONSTRUCTOR_DECLARATION => {
                self.handle_constructor_declaration(node, code, file_id, symbols, counter, context);
            }
            NODE_FIELD_DECLARATION => {
                self.handle_field_declaration(node, code, file_id, symbols, counter, context);
            }
            NODE_PACKAGE_DECLARATION | NODE_IMPORT_DECLARATION => {
                // Register recursively to track scoped_identifier chains
                self.register_node_recursively(node);
            }
            "annotation_type_declaration" => {
                // Handle @interface definitions (e.g., @interface CustomAnnotation {})
                self.handle_annotation_type_declaration(
                    node,
                    code,
                    file_id,
                    symbols,
                    counter,
                    context,
                    module_path,
                    depth,
                );
            }
            "ERROR" => {
                // Register error nodes but don't recurse (handled separately below)
                self.register_node(&node);
                // Try to extract symbols from ERROR node children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        module_path,
                        depth + 1,
                    );
                }
            }
            _ => {
                // Unhandled node - no registration needed
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        module_path,
                        depth + 1,
                    );
                }
            }
        }
    }

    // =========================================================================
    // RELATIONSHIP EXTRACTION - Recursive Collectors
    // =========================================================================

    /// Find import statements recursively
    fn find_imports_in_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if node.kind() == NODE_IMPORT_DECLARATION {
            let mut path = String::new();
            let mut is_glob = false;
            let mut is_static = false;
            let mut cursor = node.walk();

            // Build import path from children (scoped_identifier, identifier, asterisk)
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "scoped_identifier" | "identifier" => {
                        path = self.text_for_node(code, child).trim().to_string();
                    }
                    "asterisk" => {
                        is_glob = true;
                        if !path.is_empty() {
                            path.push_str(".*");
                        }
                    }
                    "static" => {
                        is_static = true;
                    }
                    _ => {} // Skip "import" keyword
                }
            }

            // TODO: Import struct doesn't have is_static field
            // For now, prepend "static " to path for static imports
            // Future: Add is_static field to Import struct
            if is_static && !path.is_empty() {
                path = format!("static {path}");
            }

            if !path.is_empty() {
                imports.push(Import {
                    file_id,
                    path,
                    alias: None,
                    is_glob,
                    is_type_only: false,
                });
            }
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.find_imports_in_node(child, code, file_id, imports);
        }
    }

    /// Collect function/method calls recursively
    fn collect_calls<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_method: Option<&'a str>,
    ) {
        if node.kind() == NODE_METHOD_INVOCATION {
            if let Some(name_node) = node.child_by_field_name("name") {
                let caller = current_method.unwrap_or(FILE_SCOPE);
                let callee = self.text_for_node(code, name_node).trim();
                if !callee.is_empty() {
                    calls.push((caller, callee, self.node_to_range(node)));
                }
            }
        }

        // Track current method context
        let new_method = if node.kind() == NODE_METHOD_DECLARATION
            || node.kind() == NODE_CONSTRUCTOR_DECLARATION
        {
            node.child_by_field_name("name")
                .map(|n| self.text_for_node(code, n).trim())
        } else {
            current_method
        };

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_calls(child, code, calls, new_method);
        }
    }

    /// Collect method calls with receiver information
    fn collect_method_calls(
        &self,
        node: Node,
        code: &str,
        method_calls: &mut Vec<MethodCall>,
        current_method: Option<&str>,
    ) {
        if node.kind() == NODE_METHOD_INVOCATION {
            if let Some(name_node) = node.child_by_field_name("name") {
                let method_name = self.text_for_node(code, name_node).trim().to_string();

                // Extract receiver if present (object field)
                let receiver = node
                    .child_by_field_name("object")
                    .map(|obj| self.text_for_node(code, obj).trim().to_string());

                if !method_name.is_empty() {
                    let caller = current_method.unwrap_or(FILE_SCOPE).to_string();
                    let range = self.node_to_range(node);

                    method_calls.push(MethodCall {
                        caller,
                        method_name,
                        receiver,
                        is_static: false, // TODO: detect static calls (Type.method vs instance.method)
                        range,
                        caller_range: None, // TODO: track caller definition range
                    });
                }
            }
        }

        // Track current method context
        let new_method = if node.kind() == NODE_METHOD_DECLARATION
            || node.kind() == NODE_CONSTRUCTOR_DECLARATION
        {
            node.child_by_field_name("name")
                .map(|n| self.text_for_node(code, n).trim())
        } else {
            current_method
        };

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_method_calls(child, code, method_calls, new_method);
        }
    }

    /// Collect implements relationships recursively
    fn collect_implements<'a>(
        &self,
        node: Node,
        code: &'a str,
        implements: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Check for class/enum with super_interfaces field
        if node.kind() == NODE_CLASS_DECLARATION || node.kind() == NODE_ENUM_DECLARATION {
            if let Some(name_node) = node.child_by_field_name("name") {
                let class_name = self.text_for_node(code, name_node).trim();

                // Get interfaces field (super_interfaces)
                if let Some(interfaces_node) = node.child_by_field_name("interfaces") {
                    // super_interfaces contains type_list with individual types
                    let mut cursor = interfaces_node.walk();
                    for child in interfaces_node.children(&mut cursor) {
                        if child.kind() == "type_list" {
                            let mut type_cursor = child.walk();
                            for type_node in child.children(&mut type_cursor) {
                                let interface_name = self.text_for_node(code, type_node).trim();
                                if !interface_name.is_empty() {
                                    implements.push((
                                        class_name,
                                        interface_name,
                                        self.node_to_range(type_node),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_implements(child, code, implements);
        }
    }

    /// Collect extends relationships recursively
    fn collect_extends<'a>(
        &self,
        node: Node,
        code: &'a str,
        extends: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Class extends (superclass field)
        if node.kind() == NODE_CLASS_DECLARATION {
            if let Some(name_node) = node.child_by_field_name("name") {
                let class_name = self.text_for_node(code, name_node).trim();

                if let Some(superclass_node) = node.child_by_field_name("superclass") {
                    // superclass node contains "extends Person", we need just "Person"
                    // Extract the type_identifier child
                    if let Some(type_node) = superclass_node
                        .child_by_field_name("type_identifier")
                        .or_else(|| superclass_node.named_child(0))
                    {
                        let parent_name = self.text_for_node(code, type_node).trim();
                        if !parent_name.is_empty() {
                            extends.push((class_name, parent_name, self.node_to_range(type_node)));
                        }
                    }
                }
            }
        }

        // Interface extends (extends_interfaces child)
        if node.kind() == NODE_INTERFACE_DECLARATION {
            if let Some(name_node) = node.child_by_field_name("name") {
                let interface_name = self.text_for_node(code, name_node).trim();

                // Look for extends_interfaces in children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "extends_interfaces" {
                        // extends_interfaces contains type_list
                        let mut ext_cursor = child.walk();
                        for ext_child in child.children(&mut ext_cursor) {
                            if ext_child.kind() == "type_list" {
                                let mut type_cursor = ext_child.walk();
                                for type_node in ext_child.children(&mut type_cursor) {
                                    let parent_interface =
                                        self.text_for_node(code, type_node).trim();
                                    if !parent_interface.is_empty() {
                                        extends.push((
                                            interface_name,
                                            parent_interface,
                                            self.node_to_range(type_node),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_extends(child, code, extends);
        }
    }

    /// Collect type uses recursively
    fn collect_type_uses<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        current_context: Option<&'a str>,
    ) {
        let primitives = get_primitive_types();

        // Track context: class name or method name
        let new_context = match node.kind() {
            NODE_CLASS_DECLARATION | NODE_INTERFACE_DECLARATION | NODE_ENUM_DECLARATION => {
                // Extract class name
                node.child_by_field_name("name")
                    .map(|n| self.text_for_node(code, n).trim())
            }
            NODE_METHOD_DECLARATION | NODE_CONSTRUCTOR_DECLARATION => {
                // Extract method/constructor name
                node.child_by_field_name("name")
                    .map(|n| self.text_for_node(code, n).trim())
            }
            _ => None,
        };

        let context = new_context.or(current_context);

        // Collect type references from field declarations
        if node.kind() == NODE_FIELD_DECLARATION {
            if let Some(ctx) = context {
                if let Some(type_node) = node.child_by_field_name("type") {
                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                        if !primitives.contains(type_name) {
                            uses.push((ctx, type_name, self.node_to_range(type_node)));
                        }
                    }
                }
            }
        }

        // Collect type references from method return types
        if node.kind() == NODE_METHOD_DECLARATION {
            if let Some(ctx) = context {
                if let Some(type_node) = node.child_by_field_name("type") {
                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                        if !primitives.contains(type_name) {
                            uses.push((ctx, type_name, self.node_to_range(type_node)));
                        }
                    }
                }
            }
        }

        // Collect type references from method/constructor parameters
        if node.kind() == NODE_METHOD_DECLARATION || node.kind() == NODE_CONSTRUCTOR_DECLARATION {
            if let Some(ctx) = context {
                if let Some(params_node) = node.child_by_field_name("parameters") {
                    self.collect_parameter_types(params_node, code, uses, ctx, primitives);
                }
            }
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_type_uses(child, code, uses, context);
        }
    }

    /// Helper to collect types from formal_parameters node
    fn collect_parameter_types<'a>(
        &self,
        params_node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        context: &'a str,
        primitives: &HashSet<&'static str>,
    ) {
        let mut cursor = params_node.walk();
        for param in params_node.children(&mut cursor) {
            if param.kind() == "formal_parameter" {
                if let Some(type_node) = param.child_by_field_name("type") {
                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                        if !primitives.contains(type_name) {
                            uses.push((context, type_name, self.node_to_range(type_node)));
                        }
                    }
                }
            }
        }
    }

    /// Extract type name from type node, filtering primitives
    fn extract_type_name<'a>(&self, type_node: Node, code: &'a str) -> Option<&'a str> {
        // Handle different type node kinds
        match type_node.kind() {
            "type_identifier" | "identifier" => Some(self.text_for_node(code, type_node).trim()),
            "generic_type" => {
                // Extract base type from generic (e.g., "List" from "List<String>")
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    if child.kind() == "type_identifier" {
                        return Some(self.text_for_node(code, child).trim());
                    }
                }
                None
            }
            "scoped_type_identifier" => {
                // Get the rightmost identifier (e.g., "Map" from "java.util.Map")
                let text = self.text_for_node(code, type_node).trim();
                text.rsplit('.').next()
            }
            "array_type" => {
                // Extract element type from array (e.g., "String" from "String[]")
                if let Some(element_type) = type_node.child_by_field_name("element") {
                    return self.extract_type_name(element_type, code);
                }
                None
            }
            _ => None,
        }
    }

    /// Collect method definitions recursively
    /// TODO: Implement after tree-sitter exploration
    fn collect_method_defines<'a>(
        &self,
        _node: Node,
        _code: &'a str,
        _defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // TODO: Find methods in classes/interfaces
    }

    /// Collect variable type declarations
    /// TODO: Implement after tree-sitter exploration
    fn collect_variable_types<'a>(
        &self,
        _node: Node,
        _code: &'a str,
        _var_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // TODO: Find local_variable_declaration
        // TODO: Extract variable name and type
    }

    /// Register a node and all its children recursively for audit tracking
    /// This ensures nested nodes (modifiers, type_parameters, formal_parameters, etc.) are tracked
    fn register_node_recursively(&mut self, node: Node) {
        self.register_handled_node(node.kind(), node.kind_id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.register_node_recursively(child);
        }
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl NodeTracker for JavaParser {
    fn get_handled_nodes(&self) -> &HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for JavaParser {
    fn parse(&mut self, code: &str, file_id: FileId, counter: &mut SymbolCounter) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut symbols = Vec::new();
        let mut context = ParserContext::new();

        // Extract package declaration from source file
        let module_path = self
            .extract_package_name(tree.root_node(), code)
            .unwrap_or(FILE_SCOPE);

        self.extract_symbols_from_node(
            tree.root_node(),
            code,
            file_id,
            &mut symbols,
            counter,
            &mut context,
            module_path,
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
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut calls = Vec::new();
        self.collect_calls(tree.root_node(), code, &mut calls, None);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut method_calls = Vec::new();
        self.collect_method_calls(tree.root_node(), code, &mut method_calls, None);
        method_calls
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut implements = Vec::new();
        self.collect_implements(tree.root_node(), code, &mut implements);
        implements
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut extends = Vec::new();
        self.collect_extends(tree.root_node(), code, &mut extends);
        extends
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut uses = Vec::new();
        self.collect_type_uses(tree.root_node(), code, &mut uses, None);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut defines = Vec::new();
        self.collect_method_defines(tree.root_node(), code, &mut defines);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut imports = Vec::new();
        self.find_imports_in_node(tree.root_node(), code, file_id, &mut imports);
        imports
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut var_types = Vec::new();
        self.collect_variable_types(tree.root_node(), code, &mut var_types);
        var_types
    }

    fn language(&self) -> Language {
        Language::Java
    }
}
