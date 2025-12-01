//! Swift language parser implementation
//!
//! Provides symbol extraction for Swift using tree-sitter.

use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Import, Language, LanguageParser, NodeTracker, NodeTrackingState, ParserContext,
    ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use std::collections::HashSet;
use tree_sitter::{Node, Parser};

// Node type constants for Swift
const NODE_CLASS_DECLARATION: &str = "class_declaration";
const NODE_PROTOCOL_DECLARATION: &str = "protocol_declaration";
const NODE_FUNCTION_DECLARATION: &str = "function_declaration";
const NODE_INIT_DECLARATION: &str = "init_declaration";
const NODE_DEINIT_DECLARATION: &str = "deinit_declaration";
const NODE_PROPERTY_DECLARATION: &str = "property_declaration";
const NODE_TYPEALIAS_DECLARATION: &str = "typealias_declaration";
const NODE_SUBSCRIPT_DECLARATION: &str = "subscript_declaration";
const NODE_IMPORT_DECLARATION: &str = "import_declaration";
const NODE_ENUM_ENTRY: &str = "enum_entry";
const NODE_MODIFIERS: &str = "modifiers";
const NODE_VISIBILITY_MODIFIER: &str = "visibility_modifier";
const NODE_SIMPLE_IDENTIFIER: &str = "simple_identifier";
const NODE_COMMENT: &str = "comment";
const NODE_MULTILINE_COMMENT: &str = "multiline_comment";
const NODE_CALL_EXPRESSION: &str = "call_expression";
const NODE_NAVIGATION_EXPRESSION: &str = "navigation_expression";
const NODE_TYPE_IDENTIFIER: &str = "type_identifier";
const NODE_INHERITANCE_SPECIFIER: &str = "inheritance_specifier";
const NODE_USER_TYPE: &str = "user_type";
const NODE_CLASS_BODY: &str = "class_body";
const NODE_PROTOCOL_BODY: &str = "protocol_body";
const NODE_TYPE_ANNOTATION: &str = "type_annotation";
const NODE_PARAMETER: &str = "parameter";
const NODE_ERROR: &str = "ERROR";
const NODE_ATTRIBUTE: &str = "attribute";

/// Placeholder for file-level scope in relationships
const FILE_SCOPE: &str = "<file>";

/// Swift built-in types to filter from type usage tracking
const SWIFT_BUILTIN_TYPES: &[&str] = &[
    "Int",
    "Int8",
    "Int16",
    "Int32",
    "Int64",
    "UInt",
    "UInt8",
    "UInt16",
    "UInt32",
    "UInt64",
    "Float",
    "Double",
    "Bool",
    "String",
    "Character",
    "Array",
    "Dictionary",
    "Set",
    "Optional",
    "Any",
    "AnyObject",
    "AnyClass",
    "Void",
    "Never",
    "Error",
    "Result",
    "Data",
    "Date",
    "URL",
];

/// Parser for Swift source files
pub struct SwiftParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for SwiftParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwiftParser")
            .field("language", &"Swift")
            .finish()
    }
}

impl SwiftParser {
    /// Create a new parser instance
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        let language: tree_sitter::Language = tree_sitter_swift::LANGUAGE.into();
        parser
            .set_language(&language)
            .map_err(|e| format!("Failed to initialize Swift parser: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
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

    /// Recursively register a node and all its children for audit tracking
    ///
    /// This ensures nested nodes (modifiers, type parameters, etc.) are tracked
    /// in the audit report, matching the pattern from Java/Kotlin/TypeScript parsers.
    fn register_node_recursively(&mut self, node: Node) {
        self.node_tracker
            .register_handled_node(node.kind(), node.kind_id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.register_node_recursively(child);
        }
    }

    /// Extract raw source text for a node
    fn text_for_node<'a>(&self, code: &'a str, node: Node) -> &'a str {
        &code[node.byte_range()]
    }

    /// Extract trimmed source text for a node
    fn trimmed_text<'a>(&self, code: &'a str, node: Node) -> &'a str {
        self.text_for_node(code, node).trim()
    }

    /// Extract documentation comments (/// or /** */)
    fn doc_comment_for(&self, node: &Node, code: &str) -> Option<String> {
        let mut result = String::new();
        let mut current = node.prev_sibling();

        while let Some(sibling) = current {
            let kind = sibling.kind();
            if kind != NODE_COMMENT && kind != NODE_MULTILINE_COMMENT {
                break;
            }

            let raw = self.text_for_node(code, sibling);
            if let Some(cleaned) = self.clean_comment(raw) {
                if !result.is_empty() {
                    result.insert(0, '\n');
                }
                result.insert_str(0, cleaned);
            }
            current = sibling.prev_sibling();
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Clean comment text by removing comment markers
    fn clean_comment<'a>(&self, raw: &'a str) -> Option<&'a str> {
        let trimmed = raw.trim();

        // Handle /** ... */ doc comments
        if let Some(content) = trimmed
            .strip_prefix("/**")
            .and_then(|s| s.strip_suffix("*/"))
        {
            let cleaned = content.trim();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }

        // Handle /// doc comments
        if let Some(content) = trimmed.strip_prefix("///") {
            let cleaned = content.trim();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }

        // Handle // comments
        if let Some(content) = trimmed.strip_prefix("//") {
            let cleaned = content.trim();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }

        None
    }

    /// Extract signature from a declaration node (excluding body)
    fn extract_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Exclude body if present
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Determine visibility from a node's modifiers
    fn determine_visibility(&self, node: Node, code: &str) -> Visibility {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_MODIFIERS {
                let mut mod_cursor = child.walk();
                for modifier in child.children(&mut mod_cursor) {
                    if modifier.kind() == NODE_VISIBILITY_MODIFIER {
                        let text = self.trimmed_text(code, modifier);
                        return match text {
                            "open" | "public" => Visibility::Public,
                            "private" | "fileprivate" => Visibility::Private,
                            "internal" => Visibility::Module,
                            _ => Visibility::Module,
                        };
                    }
                }
            }
        }
        // Swift default is internal
        Visibility::Module
    }

    /// Get the declaration_kind field from a class_declaration
    /// Returns: "class", "struct", "enum", "actor", or "extension"
    fn get_declaration_kind<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("declaration_kind")
            .map(|n| self.trimmed_text(code, n))
    }

    /// Extract symbols from the AST recursively
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

        self.register_node(&node);

        // Agnostic recovery: Before processing any node, check if it contains
        // ERROR children that might represent misparsed class declarations.
        // This handles patterns like function_declaration containing an ERROR
        // that's actually a class.
        if self.try_recover_class_from_error_children(node, code, file_id, symbols, counter, depth)
        {
            return; // Successfully recovered a class from ERROR child
        }

        match node.kind() {
            NODE_CLASS_DECLARATION => {
                self.process_class_declaration(node, code, file_id, symbols, counter, depth);
            }

            NODE_PROTOCOL_DECLARATION => {
                self.process_protocol_declaration(node, code, file_id, symbols, counter, depth);
            }

            NODE_FUNCTION_DECLARATION => {
                self.process_function_declaration(node, code, file_id, symbols, counter, depth);
            }

            NODE_INIT_DECLARATION => {
                self.process_init_declaration(node, code, file_id, symbols, counter);
            }

            NODE_DEINIT_DECLARATION => {
                self.process_deinit_declaration(node, code, file_id, symbols, counter);
            }

            NODE_PROPERTY_DECLARATION => {
                self.process_property_declaration(node, code, file_id, symbols, counter);
            }

            NODE_TYPEALIAS_DECLARATION => {
                self.process_typealias_declaration(node, code, file_id, symbols, counter);
            }

            NODE_ENUM_ENTRY => {
                self.process_enum_entry(node, code, file_id, symbols, counter);
            }

            NODE_SUBSCRIPT_DECLARATION => {
                self.process_subscript_declaration(node, code, file_id, symbols, counter);
            }

            // Try to recover class declarations from ERROR nodes
            // This handles cases like `open class Session: @unchecked Sendable`
            // where tree-sitter-swift fails to parse correctly
            NODE_ERROR => {
                if self.try_recover_class_from_error(node, code, file_id, symbols, counter, depth) {
                    return; // Successfully recovered, don't recurse
                }
                // Fall through to default recursion for unrecoverable errors
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        depth + 1,
                    );
                }
            }

            // Recurse into containers
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        depth + 1,
                    );
                }
            }
        }
    }

    /// Process class/struct/enum/actor/extension declarations
    fn process_class_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type_parameters, etc.)
        self.register_node_recursively(node);

        let declaration_kind = self.get_declaration_kind(node, code).unwrap_or("class");

        // Get the name
        let name = match node.child_by_field_name("name") {
            Some(n) => self.trimmed_text(code, n).to_string(),
            None => return,
        };

        // For extensions, we don't create a new type symbol
        // We just extract the methods/properties inside
        if declaration_kind == "extension" {
            // Save context
            let saved_class = self.context.current_class().map(|s| s.to_string());

            self.context.enter_scope(ScopeType::Class);
            self.context.set_current_class(Some(name.clone()));

            // Process body
            if let Some(body) = node.child_by_field_name("body") {
                self.extract_symbols_from_node(body, code, file_id, symbols, counter, depth + 1);
            }

            self.context.exit_scope();
            self.context.set_current_class(saved_class);
            return;
        }

        // Determine symbol kind based on declaration_kind
        let kind = match declaration_kind {
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "actor" => SymbolKind::Class, // Actor is similar to class
            _ => SymbolKind::Class,
        };

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(counter.next_id(), name.clone(), kind, file_id, range);
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);

        // Save context and enter class scope
        let saved_class = self.context.current_class().map(|s| s.to_string());

        self.context.enter_scope(ScopeType::Class);
        self.context.set_current_class(Some(name));

        // Process body
        if let Some(body) = node.child_by_field_name("body") {
            self.extract_symbols_from_node(body, code, file_id, symbols, counter, depth + 1);
        }

        self.context.exit_scope();
        self.context.set_current_class(saved_class);
    }

    /// Check if any ERROR children of this node represent a misparsed class
    ///
    /// This is the agnostic entry point that scans children for ERROR nodes
    /// and checks if they, combined with their siblings, form a class pattern.
    fn try_recover_class_from_error_children(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) -> bool {
        // Only check nodes that might contain misparsed classes
        // Skip if node is already a proper class or if it's at file scope
        if node.kind() == NODE_CLASS_DECLARATION
            || node.kind() == "source_file"
            || node.kind() == NODE_ERROR
        {
            return false;
        }

        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();

        // Check if any child is an ERROR node
        let error_children: Vec<_> = children
            .iter()
            .filter(|c| c.kind() == NODE_ERROR)
            .copied()
            .collect();

        if error_children.is_empty() {
            return false;
        }

        // Try to recover a class from each ERROR child using the full context
        for error_node in error_children {
            if self.try_recover_class_from_error(error_node, code, file_id, symbols, counter, depth)
            {
                return true;
            }
        }

        false
    }

    /// Try to recover a class declaration from an ERROR node
    ///
    /// The tree-sitter-swift grammar fails to parse class declarations like:
    ///   `open class Session: @unchecked Sendable { ... }`
    /// when there's no base class before the `@unchecked` attribute.
    ///
    /// This method uses an agnostic approach: gather class-like elements from
    /// the ERROR node AND its parent context (siblings), then check if the
    /// combined pattern represents a class declaration.
    fn try_recover_class_from_error(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) -> bool {
        // Collect all relevant nodes: ERROR children + parent's other children (siblings)
        let mut cursor = node.walk();
        let error_children: Vec<_> = node.children(&mut cursor).collect();

        // Also gather siblings from parent (for patterns where modifiers are at parent level)
        let parent_children: Vec<_> = if let Some(parent) = node.parent() {
            let mut pcursor = parent.walk();
            parent.children(&mut pcursor).collect()
        } else {
            Vec::new()
        };

        // Agnostic pattern detection: look for class signature elements anywhere
        // 1. Visibility modifier (in ERROR or sibling nodes)
        let has_visibility = self.find_visibility_in_nodes(&error_children, code)
            || self.find_visibility_in_nodes(&parent_children, code);

        if !has_visibility {
            return false;
        }

        // 2. Class name (simple_identifier in ERROR, not in nested declarations)
        let name = self.find_class_name_in_error(&error_children, code);
        let name = match name {
            Some(n) if !n.is_empty() => n,
            _ => return false,
        };

        // 3. Inheritance/attribute markers (somewhere in the tree)
        let has_inheritance = Self::find_inheritance_markers(&error_children)
            || Self::find_inheritance_markers(&parent_children);

        if !has_inheritance {
            return false;
        }

        // 4. Body content (declarations that belong to a class)
        let has_body = Self::find_body_declarations(&error_children);

        if !has_body {
            return false;
        }

        // We have a class pattern - extract it
        self.extract_recovered_class(node, &name, code, file_id, symbols, counter, depth)
    }

    /// Check if any nodes contain visibility modifiers
    fn find_visibility_in_nodes(&self, nodes: &[Node], code: &str) -> bool {
        for child in nodes {
            if child.kind() == NODE_MODIFIERS {
                let mut mod_cursor = child.walk();
                if child
                    .children(&mut mod_cursor)
                    .any(|m| m.kind() == NODE_VISIBILITY_MODIFIER)
                {
                    return true;
                }
            }
            // Check text for "class" keyword which indicates class context
            if child.kind() == NODE_SIMPLE_IDENTIFIER {
                let text = self.trimmed_text(code, *child);
                if text == "class" || text == "struct" || text == "enum" || text == "actor" {
                    return true;
                }
            }
        }
        false
    }

    /// Find class name from ERROR node children
    /// Look for simple_identifier that's not a keyword or nested in declarations
    fn find_class_name_in_error(&self, children: &[Node], code: &str) -> Option<String> {
        let keywords = [
            "class",
            "struct",
            "enum",
            "actor",
            "extension",
            "open",
            "public",
            "private",
            "internal",
            "final",
        ];

        // First pass: find identifier after "class" keyword
        let mut found_class_keyword = false;
        for child in children {
            if child.kind() == NODE_SIMPLE_IDENTIFIER {
                let text = self.trimmed_text(code, *child);
                if text == "class" || text == "struct" || text == "enum" || text == "actor" {
                    found_class_keyword = true;
                    continue;
                }
                if found_class_keyword && !keywords.contains(&text) {
                    return Some(text.to_string());
                }
            }
        }

        // Second pass: find any identifier that's not a keyword
        for child in children {
            if child.kind() == NODE_SIMPLE_IDENTIFIER {
                let text = self.trimmed_text(code, *child);
                if !keywords.contains(&text) && !text.starts_with('@') {
                    return Some(text.to_string());
                }
            }
        }

        None
    }

    /// Check for inheritance specifiers or attributes indicating protocol conformance
    fn find_inheritance_markers(nodes: &[Node]) -> bool {
        for child in nodes {
            if child.kind() == NODE_INHERITANCE_SPECIFIER || child.kind() == NODE_ATTRIBUTE {
                return true;
            }
            // Recursively check nested ERROR nodes
            if child.kind() == NODE_ERROR {
                let mut inner_cursor = child.walk();
                let inner_children: Vec<_> = child.children(&mut inner_cursor).collect();
                if Self::find_inheritance_markers(&inner_children) {
                    return true;
                }
            }
        }
        false
    }

    /// Check for class body declarations
    fn find_body_declarations(nodes: &[Node]) -> bool {
        for child in nodes {
            if matches!(
                child.kind(),
                "property_declaration"
                    | "function_declaration"
                    | "init_declaration"
                    | "deinit_declaration"
                    | "subscript_declaration"
                    | "typealias_declaration"
            ) {
                return true;
            }
            // Recursively check nested ERROR nodes
            if child.kind() == NODE_ERROR {
                let mut inner_cursor = child.walk();
                let inner_children: Vec<_> = child.children(&mut inner_cursor).collect();
                if Self::find_body_declarations(&inner_children) {
                    return true;
                }
            }
        }
        false
    }

    /// Extract the recovered class and its members
    fn extract_recovered_class(
        &mut self,
        node: Node,
        name: &str,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) -> bool {
        self.register_node_recursively(node);

        // Build signature from source text up to first '{'
        let node_text = self.text_for_node(code, node);
        let signature = if let Some(brace_pos) = node_text.find('{') {
            node_text[..brace_pos].trim().to_string()
        } else {
            format!("class {name}")
        };

        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(
            counter.next_id(),
            name.to_string(),
            SymbolKind::Class,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);

        // Enter class scope and process members
        let saved_class = self.context.current_class().map(|s| s.to_string());
        self.context.enter_scope(ScopeType::Class);
        self.context.set_current_class(Some(name.to_string()));

        self.process_error_children_for_members(node, code, file_id, symbols, counter, depth);

        self.context.exit_scope();
        self.context.set_current_class(saved_class);

        true
    }

    /// Recursively process ERROR node children for class members
    fn process_error_children_for_members(
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
            match child.kind() {
                NODE_PROPERTY_DECLARATION
                | NODE_FUNCTION_DECLARATION
                | NODE_INIT_DECLARATION
                | NODE_DEINIT_DECLARATION
                | NODE_SUBSCRIPT_DECLARATION
                | NODE_TYPEALIAS_DECLARATION
                | NODE_CLASS_DECLARATION => {
                    // Handle nested types (structs, enums, classes) inside ERROR-recovered classes
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        depth + 1,
                    );
                }
                NODE_ERROR => {
                    // Recurse into nested ERROR nodes
                    self.process_error_children_for_members(
                        child, code, file_id, symbols, counter, depth,
                    );
                }
                _ => {}
            }
        }
    }

    /// Process protocol declarations
    fn process_protocol_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        let name = match node.child_by_field_name("name") {
            Some(n) => self.trimmed_text(code, n).to_string(),
            None => return,
        };

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(
            counter.next_id(),
            name.clone(),
            SymbolKind::Interface,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);

        // Save context and enter protocol scope
        let saved_class = self.context.current_class().map(|s| s.to_string());

        self.context.enter_scope(ScopeType::Class);
        self.context.set_current_class(Some(name));

        // Process body
        if let Some(body) = node.child_by_field_name("body") {
            self.extract_symbols_from_node(body, code, file_id, symbols, counter, depth + 1);
        }

        self.context.exit_scope();
        self.context.set_current_class(saved_class);
    }

    /// Process function declarations
    fn process_function_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        let name = match node.child_by_field_name("name") {
            Some(n) => self.trimmed_text(code, n).to_string(),
            None => return,
        };

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        // Determine if this is a method or standalone function
        let kind = if self.context.current_class().is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let mut symbol = Symbol::new(counter.next_id(), name.clone(), kind, file_id, range);
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);

        // Process nested symbols in function body
        let saved_function = self.context.current_function().map(|s| s.to_string());

        self.context
            .enter_scope(ScopeType::Function { hoisting: false });
        self.context.set_current_function(Some(name));

        if let Some(body) = node.child_by_field_name("body") {
            self.extract_symbols_from_node(body, code, file_id, symbols, counter, depth + 1);
        }

        self.context.exit_scope();
        self.context.set_current_function(saved_function);
    }

    /// Process init declarations (constructors)
    fn process_init_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(
            counter.next_id(),
            "init",
            SymbolKind::Method,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);
    }

    /// Process deinit declarations (destructors)
    fn process_deinit_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(
            counter.next_id(),
            "deinit",
            SymbolKind::Function,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = Visibility::Private; // deinit is always private
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);
    }

    /// Process property declarations
    fn process_property_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        // Property name is in the "name" field which contains a pattern
        let name = node.child_by_field_name("name").map(|pattern| {
            // The pattern contains bound_identifier
            let mut cursor = pattern.walk();
            for child in pattern.children(&mut cursor) {
                if child.kind() == "bound_identifier" || child.kind() == NODE_SIMPLE_IDENTIFIER {
                    return self.trimmed_text(code, child).to_string();
                }
            }
            // Try direct text if no bound_identifier found
            self.trimmed_text(code, pattern).to_string()
        });

        let name = match name {
            Some(n) if !n.is_empty() => n,
            _ => return,
        };

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        // Determine if this is a field or a variable
        let kind = if self.context.current_class().is_some() {
            SymbolKind::Field
        } else {
            SymbolKind::Variable
        };

        let mut symbol = Symbol::new(counter.next_id(), name.clone(), kind, file_id, range);
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);
    }

    /// Process typealias declarations
    fn process_typealias_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        let name = match node.child_by_field_name("name") {
            Some(n) => self.trimmed_text(code, n).to_string(),
            None => return,
        };

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(
            counter.next_id(),
            name.clone(),
            SymbolKind::TypeAlias,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);
    }

    /// Process enum entries - handles multiple names per entry
    fn process_enum_entry(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        // enum_entry can have multiple "name" fields for comma-separated cases
        let mut cursor = node.walk();
        for child in node.children_by_field_name("name", &mut cursor) {
            let name = self.trimmed_text(code, child).to_string();
            if name.is_empty() {
                continue;
            }

            let range = self.node_to_range(child);

            let mut symbol = Symbol::new(
                counter.next_id(),
                name.clone(),
                SymbolKind::Constant,
                file_id,
                range,
            );
            symbol.visibility = Visibility::Public; // Enum cases are always public within the enum

            symbols.push(symbol);
        }
    }

    /// Process subscript declarations
    fn process_subscript_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Register ALL child nodes recursively for audit
        self.register_node_recursively(node);

        let signature = self.extract_signature(node, code);
        let visibility = self.determine_visibility(node, code);
        let doc_comment = self.doc_comment_for(&node, code);
        let range = self.node_to_range(node);

        let mut symbol = Symbol::new(
            counter.next_id(),
            "subscript",
            SymbolKind::Method,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = visibility;
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);
    }

    /// Extract import declarations
    fn extract_imports(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if node.kind() == NODE_IMPORT_DECLARATION {
            // Register ALL child nodes recursively for audit
            self.register_node_recursively(node);

            // Get the identifier (module name)
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    let path = self.trimmed_text(code, child).to_string();
                    imports.push(Import {
                        path,
                        alias: None,
                        file_id,
                        is_glob: false,
                        is_type_only: false,
                    });
                    break;
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_imports(child, code, file_id, imports);
        }
    }

    // =========================================================================
    // Relationship Extraction Methods
    // =========================================================================

    /// Collect inheritance relationships (class extends, protocol conformance)
    ///
    /// Returns tuples of (derived_type, base_type, range)
    fn collect_extends<'a>(
        &self,
        node: Node,
        code: &'a str,
        results: &mut Vec<(&'a str, &'a str, Range)>,
        current_class: Option<&'a str>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        // Track current class/struct/enum/protocol name
        let new_class = match node.kind() {
            NODE_CLASS_DECLARATION | NODE_PROTOCOL_DECLARATION => {
                // Find the type_identifier child (class/struct/enum/protocol name)
                let mut cursor = node.walk();
                let mut class_name = None;
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        class_name = Some(self.trimmed_text(code, child));
                        break;
                    }
                }
                class_name
            }
            _ => None,
        };

        let class_context = new_class.or(current_class);

        // Look for inheritance_specifier nodes (: SuperClass, Protocol)
        if node.kind() == NODE_INHERITANCE_SPECIFIER {
            if let Some(derived) = class_context {
                // Get the type from user_type -> type_identifier
                if let Some(user_type) = node.child(0) {
                    if user_type.kind() == NODE_USER_TYPE {
                        // Find type_identifier inside user_type
                        let mut cursor = user_type.walk();
                        for child in user_type.children(&mut cursor) {
                            if child.kind() == NODE_TYPE_IDENTIFIER {
                                let base = self.trimmed_text(code, child);
                                if !base.is_empty() {
                                    results.push((derived, base, self.node_to_range(node)));
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_extends(child, code, results, class_context, depth + 1);
        }
    }

    /// Collect type usage relationships (type references in properties, parameters, returns)
    ///
    /// Returns tuples of (context, type_name, range)
    fn collect_uses<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        current_context: Option<&'a str>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            NODE_CLASS_DECLARATION | NODE_PROTOCOL_DECLARATION => {
                // Find the type_identifier child (class name)
                let mut class_name = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        class_name = Some(self.trimmed_text(code, child));
                        break;
                    }
                }

                if let Some(class_name) = class_name {
                    // Process body with class context
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == NODE_CLASS_BODY || child.kind() == NODE_PROTOCOL_BODY {
                            let mut body_cursor = child.walk();
                            for body_child in child.children(&mut body_cursor) {
                                self.collect_uses(
                                    body_child,
                                    code,
                                    uses,
                                    Some(class_name),
                                    depth + 1,
                                );
                            }
                        }
                    }
                }
                return;
            }

            NODE_FUNCTION_DECLARATION | NODE_INIT_DECLARATION => {
                let func_name = node
                    .child_by_field_name("name")
                    .map(|n| self.trimmed_text(code, n))
                    .or(current_context);

                // Extract types from parameters and return type
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_type_references(child, code, uses, func_name, depth + 1);
                }
                return;
            }

            NODE_PROPERTY_DECLARATION => {
                let context = current_context.unwrap_or(FILE_SCOPE);

                // Extract type from type_annotation
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_ANNOTATION {
                        if let Some(type_name) = self.extract_type_name(child, code) {
                            if !self.is_builtin_type(type_name) {
                                uses.push((context, type_name, self.node_to_range(child)));
                            }
                        }
                    }
                }
            }

            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_uses(child, code, uses, current_context, depth + 1);
        }
    }

    /// Extract type references from parameters and return types
    fn extract_type_references<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        context: Option<&'a str>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        let ctx = context.unwrap_or(FILE_SCOPE);

        match node.kind() {
            NODE_PARAMETER => {
                // Extract type from parameter's type annotation
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_USER_TYPE || child.kind() == NODE_TYPE_IDENTIFIER {
                        if let Some(type_name) = self.extract_type_name(child, code) {
                            if !self.is_builtin_type(type_name) {
                                uses.push((ctx, type_name, self.node_to_range(child)));
                            }
                        }
                    }
                }
            }

            NODE_USER_TYPE | NODE_TYPE_IDENTIFIER => {
                // This could be a return type
                if let Some(type_name) = self.extract_type_name(node, code) {
                    if !self.is_builtin_type(type_name) {
                        uses.push((ctx, type_name, self.node_to_range(node)));
                    }
                }
            }

            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.extract_type_references(child, code, uses, context, depth + 1);
                }
            }
        }
    }

    /// Extract type name from a type node
    fn extract_type_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            NODE_TYPE_IDENTIFIER => Some(self.trimmed_text(code, node)),

            NODE_USER_TYPE => {
                // user_type contains type_identifier
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        return Some(self.trimmed_text(code, child));
                    }
                }
                None
            }

            NODE_TYPE_ANNOTATION => {
                // type_annotation contains user_type -> type_identifier
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_USER_TYPE {
                        return self.extract_type_name(child, code);
                    }
                }
                None
            }

            _ => None,
        }
    }

    /// Check if a type name is a Swift built-in type
    fn is_builtin_type(&self, type_name: &str) -> bool {
        SWIFT_BUILTIN_TYPES.contains(&type_name)
    }

    /// Collect method definitions (type -> method relationships)
    ///
    /// Returns tuples of (type_name, method_name, range)
    fn collect_defines<'a>(
        &self,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            NODE_CLASS_DECLARATION | NODE_PROTOCOL_DECLARATION => {
                // Find the type name - either type_identifier (class/struct/enum)
                // or user_type -> type_identifier (extension)
                let mut class_name = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        class_name = Some(self.trimmed_text(code, child));
                        break;
                    } else if child.kind() == NODE_USER_TYPE {
                        // Extension: `extension Int { ... }` has user_type -> type_identifier
                        class_name = self.extract_type_name(child, code);
                        break;
                    }
                }

                let class_name = class_name.unwrap_or("anonymous");

                // Extract methods from body
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_CLASS_BODY || child.kind() == NODE_PROTOCOL_BODY {
                        let mut body_cursor = child.walk();
                        for body_child in child.children(&mut body_cursor) {
                            match body_child.kind() {
                                NODE_FUNCTION_DECLARATION
                                | NODE_INIT_DECLARATION
                                | NODE_DEINIT_DECLARATION
                                | NODE_SUBSCRIPT_DECLARATION => {
                                    // Get method name
                                    let method_name = if body_child.kind() == NODE_INIT_DECLARATION
                                    {
                                        "init"
                                    } else if body_child.kind() == NODE_DEINIT_DECLARATION {
                                        "deinit"
                                    } else if body_child.kind() == NODE_SUBSCRIPT_DECLARATION {
                                        "subscript"
                                    } else {
                                        body_child
                                            .child_by_field_name("name")
                                            .map(|n| self.trimmed_text(code, n))
                                            .unwrap_or("unknown")
                                    };

                                    defines.push((
                                        class_name,
                                        method_name,
                                        self.node_to_range(body_child),
                                    ));
                                }
                                // Also handle protocol_function_declaration
                                "protocol_function_declaration" => {
                                    let mut method_cursor = body_child.walk();
                                    for method_child in body_child.children(&mut method_cursor) {
                                        if method_child.kind() == NODE_SIMPLE_IDENTIFIER {
                                            let method_name = self.trimmed_text(code, method_child);
                                            defines.push((
                                                class_name,
                                                method_name,
                                                self.node_to_range(body_child),
                                            ));
                                            break;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Recursively process children to handle nested types
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_defines(child, code, defines, depth + 1);
        }
    }

    /// Collect function/method calls recursively
    fn collect_calls<'a>(
        &mut self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: Option<&'a str>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        self.register_node(&node);

        match node.kind() {
            NODE_FUNCTION_DECLARATION => {
                let func_name = node
                    .child_by_field_name("name")
                    .map(|n| self.trimmed_text(code, n));

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_calls(child, code, calls, func_name, depth + 1);
                }
            }

            NODE_CALL_EXPRESSION => {
                if let Some(caller) = current_function {
                    // Get the callee name from the call expression
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == NODE_SIMPLE_IDENTIFIER {
                            let callee = self.trimmed_text(code, child);
                            calls.push((caller, callee, self.node_to_range(node)));
                            break;
                        } else if child.kind() == NODE_NAVIGATION_EXPRESSION {
                            // Get the last identifier in the navigation chain
                            if let Some(callee) = self.get_last_identifier(child, code) {
                                calls.push((caller, callee, self.node_to_range(node)));
                            }
                            break;
                        }
                    }
                }

                // Continue to collect nested calls
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_calls(child, code, calls, current_function, depth + 1);
                }
            }

            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.collect_calls(child, code, calls, current_function, depth + 1);
                }
            }
        }
    }

    /// Get the last identifier in a navigation expression
    fn get_last_identifier<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        let mut last = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_SIMPLE_IDENTIFIER {
                last = Some(self.trimmed_text(code, child));
            } else if child.kind() == "navigation_suffix" {
                let mut suffix_cursor = child.walk();
                for suffix_child in child.children(&mut suffix_cursor) {
                    if suffix_child.kind() == NODE_SIMPLE_IDENTIFIER {
                        last = Some(self.trimmed_text(code, suffix_child));
                    }
                }
            }
        }

        last
    }
}

impl LanguageParser for SwiftParser {
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

        // Create a file-level symbol
        let module_id = symbol_counter.next_id();
        let mut module_symbol = Symbol::new(
            module_id,
            "<file>",
            SymbolKind::Module,
            file_id,
            self.node_to_range(root),
        );
        module_symbol.scope_context = Some(crate::symbol::ScopeContext::Module);
        symbols.push(module_symbol);

        // Reset context for new file
        self.context = ParserContext::new();

        self.extract_symbols_from_node(root, code, file_id, &mut symbols, symbol_counter, 0);

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
        self.collect_calls(tree.root_node(), code, &mut calls, None, 0);
        calls
    }

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Swift protocol conformance is declared with class/struct, handled in find_extends
        Vec::new()
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut results = Vec::new();
        self.collect_extends(tree.root_node(), code, &mut results, None, 0);
        results
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut uses = Vec::new();
        self.collect_uses(tree.root_node(), code, &mut uses, None, 0);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut defines = Vec::new();
        self.collect_defines(tree.root_node(), code, &mut defines, 0);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut imports = Vec::new();
        self.extract_imports(tree.root_node(), code, file_id, &mut imports);
        imports
    }

    fn language(&self) -> Language {
        Language::Swift
    }
}

impl NodeTracker for SwiftParser {
    fn get_handled_nodes(&self) -> &HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}
