//! C# language parser implementation

use crate::indexing::Import;
use crate::parsing::method_call::MethodCall;
use crate::parsing::{Language, LanguageParser};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind};
use std::any::Any;
use tree_sitter::{Node, Parser};

/// Error type for C# parsing operations
#[derive(Debug, thiserror::Error)]
pub enum CSharpParseError {
    #[error("Failed to set C# language: {0}")]
    LanguageSetup(String),
    #[error("Failed to parse code")]
    ParseFailed,
}

pub struct CSharpParser {
    parser: Parser,
    #[allow(dead_code)] // Will be used for debug output in future implementations
    debug: bool,
}

impl std::fmt::Debug for CSharpParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CSharpParser")
            .field("language", &"C#")
            .finish()
    }
}

impl CSharpParser {
    pub fn new() -> Result<Self, CSharpParseError> {
        Self::with_debug(false)
    }

    pub fn with_debug(debug: bool) -> Result<Self, CSharpParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .map_err(|e| CSharpParseError::LanguageSetup(e.to_string()))?;

        Ok(Self { parser, debug })
    }

    /// Extract symbols from an AST node recursively
    fn extract_symbols_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        match node.kind() {
            "namespace_declaration" | "file_scoped_namespace_declaration" => {
                if let Some(symbol) = self.process_namespace(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "class_declaration" => {
                if let Some(symbol) = self.process_class(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "interface_declaration" => {
                if let Some(symbol) = self.process_interface(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "struct_declaration" => {
                if let Some(symbol) = self.process_struct(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "enum_declaration" => {
                if let Some(symbol) = self.process_enum(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "record_declaration" | "record_struct_declaration" => {
                if let Some(symbol) = self.process_record(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "delegate_declaration" => {
                if let Some(symbol) = self.process_delegate(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "method_declaration" => {
                if let Some(symbol) = self.process_method(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "constructor_declaration" => {
                if let Some(symbol) = self.process_constructor(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
                self.process_children(node, code, file_id, symbols, counter);
            }
            "property_declaration" => {
                if let Some(symbol) = self.process_property(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "field_declaration" => {
                for symbol in self.process_field(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "event_declaration" | "event_field_declaration" => {
                // Events can have multiple declarators like fields
                for symbol in self.process_events(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "indexer_declaration" => {
                if let Some(symbol) = self.process_indexer(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            "operator_declaration" => {
                if let Some(symbol) = self.process_operator(node, code, file_id, counter) {
                    symbols.push(symbol);
                }
            }
            _ => {
                self.process_children(node, code, file_id, symbols, counter);
            }
        }
    }

    fn process_namespace(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = self.extract_namespace_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Module, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_class(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Class, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_class_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_interface(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Interface, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_interface_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_struct(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Struct, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_enum(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Enum, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_record(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Records are like classes in C#
        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Class, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_record_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_delegate(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Delegates are like type aliases for functions
        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::TypeAlias, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_delegate_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_method(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let kind = if self.is_inside_interface(node) {
            SymbolKind::Function
        } else {
            SymbolKind::Method
        };

        let mut symbol = Symbol::new(symbol_id, name, kind, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_method_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_constructor(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Get parent class name for constructor
        let name = self.get_parent_type_name(node, code)?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Method, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_constructor_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_property(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        let name = node
            .child_by_field_name("name")
            .map(|n| &code[n.byte_range()])?;
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        // Properties are like fields but with accessors
        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Field, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_property_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_field(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();

        // Look for variable_declaration child which contains the declarators
        for child in node.children(&mut node.walk()) {
            if child.kind() == "variable_declaration" {
                // Variable declaration contains variable_declarator nodes
                for declarator in child.children(&mut child.walk()) {
                    if declarator.kind() == "variable_declarator" {
                        if let Some(name_node) = declarator.child_by_field_name("name") {
                            let name = &code[name_node.byte_range()];
                            let range = self.node_to_range(declarator);
                            let symbol_id = counter.next_id();
                            let mut symbol =
                                Symbol::new(symbol_id, name, SymbolKind::Field, file_id, range);
                            symbol.doc_comment =
                                self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
                            symbols.push(symbol);
                        }
                    }
                }
            }
        }

        symbols
    }

    fn process_events(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();

        // For event_field_declaration, we need to look at the variable_declaration child
        if node.kind() == "event_field_declaration" {
            // Similar structure to field_declaration
            for child in node.children(&mut node.walk()) {
                if child.kind() == "variable_declaration" {
                    for declarator in child.children(&mut child.walk()) {
                        if declarator.kind() == "variable_declarator" {
                            if let Some(name_node) = declarator.child_by_field_name("name") {
                                let name = &code[name_node.byte_range()];
                                let range = self.node_to_range(declarator);
                                let symbol_id = counter.next_id();
                                let mut symbol =
                                    Symbol::new(symbol_id, name, SymbolKind::Field, file_id, range);
                                symbol.doc_comment =
                                    self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
                                symbols.push(symbol);
                            }
                        }
                    }
                }
            }
        } else {
            // For event_declaration (property-like events)
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = &code[name_node.byte_range()];
                let range = self.node_to_range(node);
                let symbol_id = counter.next_id();
                let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Field, file_id, range);
                symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
                symbols.push(symbol);
            }
        }

        symbols
    }

    fn process_indexer(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Indexers don't have names, use "this[]"
        let name = "this[]";
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name, SymbolKind::Method, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_indexer_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_operator(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        counter: &mut SymbolCounter,
    ) -> Option<Symbol> {
        // Extract operator token
        let operator = node
            .child_by_field_name("operator")
            .map(|n| &code[n.byte_range()])
            .unwrap_or("operator");
        let name = format!("operator {operator}");
        let range = self.node_to_range(node);
        let symbol_id = counter.next_id();

        let mut symbol = Symbol::new(symbol_id, name.as_str(), SymbolKind::Method, file_id, range);
        symbol.doc_comment = self.extract_xml_doc(node, code).map(|s| s.into_boxed_str());
        symbol.signature = self
            .build_operator_signature(node, code)
            .map(|s| s.into_boxed_str());
        Some(symbol)
    }

    fn process_children(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        for child in node.children(&mut node.walk()) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter);
        }
    }

    fn extract_namespace_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        node.child_by_field_name("name")
            .map(|name_node| &code[name_node.byte_range()])
    }

    fn extract_xml_doc(&self, node: Node, code: &str) -> Option<String> {
        // Look for XML documentation comments preceding the node
        // Check multiple previous siblings as there might be multiple comment lines
        let mut doc_lines = Vec::new();
        let mut current = node.prev_sibling();

        // Collect all consecutive XML doc comments
        while let Some(prev) = current {
            if prev.kind() == "xml_doc_comment" || prev.kind() == "comment" {
                let doc = &code[prev.byte_range()];
                // Only process if it starts with /// (XML doc comment)
                if doc.trim().starts_with("///") {
                    // Insert at beginning since we're going backwards
                    doc_lines.insert(0, doc);
                }
                current = prev.prev_sibling();
            } else if prev.kind() == "modifier" {
                // Skip modifiers (public, private, etc.) and continue looking
                current = prev.prev_sibling();
            } else {
                break;
            }
        }

        if !doc_lines.is_empty() {
            // Clean up XML doc comment markers
            let cleaned = doc_lines
                .iter()
                .flat_map(|doc| doc.lines())
                .map(|line| line.trim_start_matches("///").trim())
                .collect::<Vec<_>>()
                .join("\n");
            return Some(cleaned);
        }

        None
    }

    fn build_class_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut parts = Vec::new();

        // Add modifiers
        for child in node.children(&mut node.walk()) {
            if child.kind() == "modifier" {
                parts.push(&code[child.byte_range()]);
            }
        }

        parts.push("class");

        if let Some(name) = node.child_by_field_name("name") {
            parts.push(&code[name.byte_range()]);
        }

        // Add base list if present
        if let Some(bases) = node.child_by_field_name("bases") {
            let base_str = &code[bases.byte_range()];
            parts.push(base_str);
        }

        Some(parts.join(" "))
    }

    fn build_interface_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut parts = Vec::new();

        for child in node.children(&mut node.walk()) {
            if child.kind() == "modifier" {
                parts.push(&code[child.byte_range()]);
            }
        }

        parts.push("interface");

        if let Some(name) = node.child_by_field_name("name") {
            parts.push(&code[name.byte_range()]);
        }

        if let Some(bases) = node.child_by_field_name("bases") {
            let base_str = &code[bases.byte_range()];
            parts.push(base_str);
        }

        Some(parts.join(" "))
    }

    fn build_record_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut parts = Vec::new();

        for child in node.children(&mut node.walk()) {
            if child.kind() == "modifier" {
                parts.push(&code[child.byte_range()]);
            }
        }

        parts.push("record");

        if let Some(name) = node.child_by_field_name("name") {
            parts.push(&code[name.byte_range()]);
        }

        // Add parameters if present (for positional records)
        if let Some(params) = node.child_by_field_name("parameters") {
            let param_str = &code[params.byte_range()];
            parts.push(param_str);
        }

        Some(parts.join(" "))
    }

    fn build_delegate_signature(&self, node: Node, code: &str) -> Option<String> {
        let start = node.start_byte();
        let end = node.end_byte();
        Some(code[start..end].to_string())
    }

    fn build_method_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut parts = Vec::new();

        // Add return type
        if let Some(return_type) = node.child_by_field_name("type") {
            parts.push(&code[return_type.byte_range()]);
        }

        // Add method name
        if let Some(name) = node.child_by_field_name("name") {
            parts.push(&code[name.byte_range()]);
        }

        // Add parameters
        if let Some(params) = node.child_by_field_name("parameters") {
            let param_str = &code[params.byte_range()];
            parts.push(param_str);
        }

        Some(parts.join(" "))
    }

    fn build_constructor_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut parts = Vec::new();

        // Add constructor name (class name)
        if let Some(name) = self.get_parent_type_name(node, code) {
            parts.push(name);
        }

        // Add parameters
        if let Some(params) = node.child_by_field_name("parameters") {
            let param_str = &code[params.byte_range()];
            parts.push(param_str);
        }

        Some(parts.join(" "))
    }

    fn build_property_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut result = String::new();

        // Add type
        if let Some(type_node) = node.child_by_field_name("type") {
            result.push_str(&code[type_node.byte_range()]);
            result.push(' ');
        }

        // Add name
        if let Some(name) = node.child_by_field_name("name") {
            result.push_str(&code[name.byte_range()]);
        }

        // Check for accessors
        let mut accessors = Vec::new();
        for child in node.children(&mut node.walk()) {
            if child.kind() == "accessor_list" {
                for accessor in child.children(&mut child.walk()) {
                    match accessor.kind() {
                        "get_accessor" => accessors.push("get"),
                        "set_accessor" => accessors.push("set"),
                        "init_accessor" => accessors.push("init"),
                        _ => {}
                    }
                }
            }
        }

        if !accessors.is_empty() {
            result.push_str(" { ");
            result.push_str(&accessors.join("; "));
            result.push_str(" }");
        }

        Some(result)
    }

    fn build_indexer_signature(&self, node: Node, code: &str) -> Option<String> {
        let mut parts = Vec::new();

        // Add return type
        if let Some(type_node) = node.child_by_field_name("type") {
            parts.push(&code[type_node.byte_range()]);
        }

        // Add "this"
        parts.push("this");

        // Add parameters
        if let Some(params) = node.child_by_field_name("parameters") {
            let param_str = &code[params.byte_range()];
            parts.push(param_str);
        }

        Some(parts.join(" "))
    }

    fn build_operator_signature(&self, node: Node, code: &str) -> Option<String> {
        let start = node.start_byte();
        let end = node.end_byte();
        Some(code[start..end].to_string())
    }

    fn is_inside_interface(&self, node: Node) -> bool {
        let mut parent = node.parent();
        while let Some(p) = parent {
            if p.kind() == "interface_declaration" {
                return true;
            }
            parent = p.parent();
        }
        false
    }

    fn get_parent_type_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        let mut parent = node.parent();
        while let Some(p) = parent {
            match p.kind() {
                "class_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "interface_declaration" => {
                    return p.child_by_field_name("name").map(|n| &code[n.byte_range()]);
                }
                _ => parent = p.parent(),
            }
        }
        None
    }

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

    // Relationship extraction methods

    fn extract_calls_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "invocation_expression" => {
                if let Some(function_node) = node.child_by_field_name("function") {
                    // Handle simple calls like Method() or Class.Method()
                    let caller = self.get_enclosing_method_name(node, code).unwrap_or("");
                    let callee = match function_node.kind() {
                        "identifier" => &code[function_node.byte_range()],
                        "member_access_expression" => {
                            // Extract the member name from qualified calls
                            if let Some(name_node) = function_node.child_by_field_name("name") {
                                &code[name_node.byte_range()]
                            } else {
                                &code[function_node.byte_range()]
                            }
                        }
                        _ => &code[function_node.byte_range()],
                    };
                    let range = self.node_to_range(node);
                    calls.push((caller, callee, range));
                }
            }
            "object_creation_expression" => {
                // Handle constructor calls like new MyClass()
                if let Some(type_node) = node.child_by_field_name("type") {
                    let caller = self.get_enclosing_method_name(node, code).unwrap_or("");
                    let type_name = &code[type_node.byte_range()];
                    let range = self.node_to_range(node);
                    calls.push((caller, type_name, range));
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            self.extract_calls_from_node(child, code, calls);
        }
    }

    fn extract_method_calls_from_node(&self, node: Node, code: &str, calls: &mut Vec<MethodCall>) {
        if node.kind() == "invocation_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                match function_node.kind() {
                    "member_access_expression" => {
                        // Extract receiver and method name
                        if let Some(object_node) = function_node.child_by_field_name("expression") {
                            if let Some(name_node) = function_node.child_by_field_name("name") {
                                let receiver = &code[object_node.byte_range()];
                                let method = &code[name_node.byte_range()];
                                let range = self.node_to_range(node);
                                calls.push(
                                    MethodCall::new(
                                        self.get_enclosing_method_name(node, code).unwrap_or(""),
                                        method,
                                        range,
                                    )
                                    .with_receiver(receiver),
                                );
                            }
                        }
                    }
                    "identifier" => {
                        // Simple method call without explicit receiver
                        let method = &code[function_node.byte_range()];
                        let range = self.node_to_range(node);
                        calls.push(MethodCall::new(
                            self.get_enclosing_method_name(node, code).unwrap_or(""),
                            method,
                            range,
                        ));
                    }
                    _ => {}
                }
            }
        }

        for child in node.children(&mut node.walk()) {
            self.extract_method_calls_from_node(child, code, calls);
        }
    }

    fn extract_implementations_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "class_declaration" | "struct_declaration" | "record_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let class_name = &code[name_node.byte_range()];

                    // Look for base_list (inheritance and interfaces)
                    for child in node.children(&mut node.walk()) {
                        if child.kind() == "base_list" {
                            for base_child in child.children(&mut child.walk()) {
                                // In the base_list, identifiers and qualified_names directly represent base types
                                if base_child.kind() == "identifier"
                                    || base_child.kind() == "qualified_name"
                                {
                                    let base_name = &code[base_child.byte_range()];
                                    let range = self.node_to_range(base_child);
                                    implementations.push((class_name, base_name, range));
                                } else if base_child.kind() == "generic_name" {
                                    // Handle generic base types like IList<T>
                                    if let Some(id_node) = base_child.child_by_field_name("name") {
                                        let base_name = &code[id_node.byte_range()];
                                        let range = self.node_to_range(id_node);
                                        implementations.push((class_name, base_name, range));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "interface_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let interface_name = &code[name_node.byte_range()];

                    // Interfaces can extend other interfaces
                    for child in node.children(&mut node.walk()) {
                        if child.kind() == "base_list" {
                            for base_child in child.children(&mut child.walk()) {
                                if base_child.kind() == "identifier"
                                    || base_child.kind() == "qualified_name"
                                {
                                    let base_name = &code[base_child.byte_range()];
                                    let range = self.node_to_range(base_child);
                                    implementations.push((interface_name, base_name, range));
                                } else if base_child.kind() == "generic_name" {
                                    if let Some(id_node) = base_child.child_by_field_name("name") {
                                        let base_name = &code[id_node.byte_range()];
                                        let range = self.node_to_range(id_node);
                                        implementations.push((interface_name, base_name, range));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            self.extract_implementations_from_node(child, code, implementations);
        }
    }

    fn extract_type_uses_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "variable_declaration" => {
                // Extract variable type usage
                if let Some(type_node) = node.child_by_field_name("type") {
                    let type_name = &code[type_node.byte_range()];
                    let context = self.get_enclosing_context_name(node, code).unwrap_or("");
                    let range = self.node_to_range(type_node);
                    uses.push((context, type_name, range));
                }
            }
            "parameter" => {
                // Extract parameter type usage
                if let Some(type_node) = node.child_by_field_name("type") {
                    let type_name = &code[type_node.byte_range()];
                    let context = self.get_enclosing_method_name(node, code).unwrap_or("");
                    let range = self.node_to_range(type_node);
                    uses.push((context, type_name, range));
                }
            }
            "cast_expression" | "as_expression" | "is_expression" | "is_pattern_expression" => {
                // Extract type in casting/checking expressions
                // For as_expression and is_expression, look for right or pattern fields
                if let Some(type_node) = node.child_by_field_name("type") {
                    let type_name = &code[type_node.byte_range()];
                    let context = self.get_enclosing_method_name(node, code).unwrap_or("");
                    let range = self.node_to_range(type_node);
                    uses.push((context, type_name, range));
                } else if let Some(right_node) = node.child_by_field_name("right") {
                    // For "as" expressions, the type is on the right
                    let type_name = &code[right_node.byte_range()];
                    let context = self.get_enclosing_method_name(node, code).unwrap_or("");
                    let range = self.node_to_range(right_node);
                    uses.push((context, type_name, range));
                } else if let Some(pattern_node) = node.child_by_field_name("pattern") {
                    // For "is" expressions with patterns
                    // Look for type pattern like "string str"
                    for child in pattern_node.children(&mut pattern_node.walk()) {
                        if child.kind() == "declaration_pattern" {
                            if let Some(type_node) = child.child_by_field_name("type") {
                                let type_name = &code[type_node.byte_range()];
                                let context =
                                    self.get_enclosing_method_name(node, code).unwrap_or("");
                                let range = self.node_to_range(type_node);
                                uses.push((context, type_name, range));
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            self.extract_type_uses_from_node(child, code, uses);
        }
    }

    fn extract_method_defines_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = &code[name_node.byte_range()];
                    let class_name = self.get_parent_type_name(node, code).unwrap_or("");
                    let range = self.node_to_range(name_node);
                    defines.push((class_name, method_name, range));
                }
            }
            "constructor_declaration" => {
                let class_name = self.get_parent_type_name(node, code).unwrap_or("");
                let range = self.node_to_range(node);
                defines.push((class_name, class_name, range)); // Constructor has same name as class
            }
            "property_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let property_name = &code[name_node.byte_range()];
                    let class_name = self.get_parent_type_name(node, code).unwrap_or("");
                    let range = self.node_to_range(name_node);
                    defines.push((class_name, property_name, range));
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            self.extract_method_defines_from_node(child, code, defines);
        }
    }

    fn extract_imports_from_node(
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "using_directive" => {
                // Check if this is an alias using: using Alias = Namespace.Type;
                let mut alias: Option<String> = None;
                let mut path: Option<String> = None;
                let mut found_equals = false;
                let mut is_global = false;
                let mut is_static = false;

                for child in node.children(&mut node.walk()) {
                    match child.kind() {
                        "global" => {
                            is_global = true;
                        }
                        "static" => {
                            is_static = true;
                        }
                        "=" => {
                            // This indicates an alias declaration
                            // Move path to alias since what we thought was path is actually the alias
                            found_equals = true;
                            if path.is_some() && alias.is_none() {
                                alias = path.take();
                            }
                        }
                        "identifier" => {
                            // This could be alias (before =) or namespace (after = or standalone)
                            if path.is_none() {
                                path = Some(code[child.byte_range()].to_string());
                            }
                        }
                        "qualified_name" => {
                            // This is always the namespace/type being imported
                            if found_equals || path.is_none() {
                                path = Some(code[child.byte_range()].to_string());
                            }
                        }
                        _ => {}
                    }
                }

                // Add the import with appropriate prefix
                if let Some(import_path) = path {
                    let final_path = if is_global {
                        format!("global::{import_path}")
                    } else if is_static {
                        format!("static::{import_path}")
                    } else {
                        import_path
                    };

                    imports.push(Import {
                        path: final_path,
                        alias,
                        file_id,
                        is_glob: false,
                    });
                }
            }
            "global_using_directive" => {
                // Handle global using directives (C# 10+) - kept for compatibility
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "qualified_name" || child.kind() == "identifier" {
                        let path = format!("global::{}", &code[child.byte_range()]);
                        imports.push(Import {
                            path,
                            alias: None,
                            file_id,
                            is_glob: false,
                        });
                    }
                }
            }
            "using_static_directive" => {
                // Handle using static directives
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "qualified_name" || child.kind() == "identifier" {
                        let path = format!("static::{}", &code[child.byte_range()]);
                        imports.push(Import {
                            path,
                            alias: None,
                            file_id,
                            is_glob: false,
                        });
                    }
                }
            }
            _ => {}
        }

        for child in node.children(&mut node.walk()) {
            Self::extract_imports_from_node(child, code, file_id, imports);
        }
    }

    fn get_enclosing_method_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        let mut parent = node.parent();
        while let Some(p) = parent {
            match p.kind() {
                "method_declaration" => {
                    return p.child_by_field_name("name").map(|n| &code[n.byte_range()]);
                }
                "constructor_declaration" => {
                    return self.get_parent_type_name(p, code);
                }
                "property_declaration" => {
                    return p.child_by_field_name("name").map(|n| &code[n.byte_range()]);
                }
                _ => parent = p.parent(),
            }
        }
        None
    }

    fn get_enclosing_context_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        // First try to get method name, then class name
        self.get_enclosing_method_name(node, code)
            .or_else(|| self.get_parent_type_name(node, code))
    }

    fn find_inherent_methods_in_node(
        &self,
        node: Node,
        code: &str,
        methods: &mut Vec<(String, String, Range)>,
    ) {
        if node.kind() == "method_declaration" {
            // Check if it's an extension method first
            if self.is_extension_method(node, code) {
                // Extension methods are treated as inherent methods on the extended type
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = code[name_node.byte_range()].to_string();

                    // Get the extended type from the first parameter
                    if let Some(extended_type) = self.get_extension_method_type(node, code) {
                        let range = self.node_to_range(name_node);
                        methods.push((extended_type, method_name, range));
                    }
                }
            } else {
                // Regular method
                if let Some(name_node) = node.child_by_field_name("name") {
                    let method_name = code[name_node.byte_range()].to_string();

                    // Get the parent type name (class, struct, record, or interface)
                    if let Some(type_name) = self.get_parent_type_name(node, code) {
                        let range = self.node_to_range(name_node);
                        methods.push((type_name.to_string(), method_name, range));
                    }
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_inherent_methods_in_node(child, code, methods);
        }
    }

    fn is_extension_method(&self, node: Node, code: &str) -> bool {
        // Check if this is a static method with 'this' as first parameter modifier
        if node.kind() != "method_declaration" {
            return false;
        }

        // Check for static modifier directly as child of method_declaration
        let mut has_static = false;
        for child in node.children(&mut node.walk()) {
            if child.kind() == "modifier" {
                let modifier_text = &code[child.byte_range()];
                if modifier_text == "static" {
                    has_static = true;
                    break;
                }
            }
        }

        if !has_static {
            return false;
        }

        // Check for 'this' in first parameter - look for parameter_list
        for child in node.children(&mut node.walk()) {
            if child.kind() == "parameter_list" {
                // Check the parameter list text for 'this' keyword
                let param_list_text = &code[child.byte_range()];
                if param_list_text.contains("this ") {
                    return true;
                }
            }
        }

        false
    }

    fn get_extension_method_type(&self, node: Node, code: &str) -> Option<String> {
        // Get the type from the first parameter with 'this' modifier
        for child in node.children(&mut node.walk()) {
            if child.kind() == "parameter_list" {
                for param in child.children(&mut child.walk()) {
                    if param.kind() == "parameter" {
                        // Get the type of the first parameter
                        if let Some(type_node) = param.child_by_field_name("type") {
                            return Some(code[type_node.byte_range()].to_string());
                        }
                        break;
                    }
                }
            }
        }
        None
    }
}

impl LanguageParser for CSharpParser {
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

        let root_node = tree.root_node();
        let mut symbols = Vec::new();

        self.extract_symbols_from_node(root_node, code, file_id, &mut symbols, symbol_counter);

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.extract_xml_doc(*node, code)
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();
        self.extract_calls_from_node(root_node, code, &mut calls);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();
        self.extract_method_calls_from_node(root_node, code, &mut calls);
        calls
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut implementations = Vec::new();
        self.extract_implementations_from_node(root_node, code, &mut implementations);
        implementations
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut uses = Vec::new();
        self.extract_type_uses_from_node(root_node, code, &mut uses);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut defines = Vec::new();
        self.extract_method_defines_from_node(root_node, code, &mut defines);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut imports = Vec::new();
        Self::extract_imports_from_node(root_node, code, file_id, &mut imports);
        imports
    }

    fn language(&self) -> Language {
        Language::CSharp
    }

    fn find_variable_types<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Not yet implemented: C# variable type extraction
        unimplemented!("CSharpParser::find_variable_types is not yet implemented");
    }

    fn find_inherent_methods(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut methods = Vec::new();
        self.find_inherent_methods_in_node(root_node, code, &mut methods);
        methods
    }
}

impl Default for CSharpParser {
    fn default() -> Self {
        Self::new().expect("Failed to create C# parser")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let parser = CSharpParser::new();
        assert!(parser.is_ok());
    }

    #[test]
    fn test_language() {
        let parser = CSharpParser::new().unwrap();
        assert_eq!(parser.language(), Language::CSharp);
    }

    #[test]
    fn test_extract_namespace() {
        let code = r#"
namespace MyApp.Services
{
    public class UserService { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "MyApp.Services" && s.kind == SymbolKind::Module)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "UserService" && s.kind == SymbolKind::Class)
        );
    }

    #[test]
    fn test_extract_class_with_inheritance() {
        let code = r#"
public class UserService : BaseService, IUserService
{
    public void GetUser() { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "UserService" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "GetUser" && s.kind == SymbolKind::Method)
        );

        // Check implementations
        let implementations = parser.find_implementations(code);
        assert_eq!(implementations.len(), 2);
        assert!(
            implementations
                .iter()
                .any(|(class, base, _)| *class == "UserService" && *base == "BaseService")
        );
        assert!(
            implementations.iter().any(
                |(class, interface, _)| *class == "UserService" && *interface == "IUserService"
            )
        );
    }

    #[test]
    fn test_extract_interface() {
        let code = r#"
public interface IUserService : IService
{
    Task<User> GetUserAsync(int id);
    void UpdateUser(User user);
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "IUserService" && s.kind == SymbolKind::Interface)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "GetUserAsync" && s.kind == SymbolKind::Function)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "UpdateUser" && s.kind == SymbolKind::Function)
        );
    }

    #[test]
    fn test_extract_struct() {
        let code = r#"
public struct Point
{
    public int X { get; set; }
    public int Y { get; set; }
    
    public Point(int x, int y)
    {
        X = x;
        Y = y;
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Point" && s.kind == SymbolKind::Struct)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "X" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Y" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Point" && s.kind == SymbolKind::Method)
        ); // Constructor
    }

    #[test]
    fn test_extract_enum() {
        let code = r#"
public enum Status
{
    Active,
    Inactive,
    Pending
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Status" && s.kind == SymbolKind::Enum)
        );
    }

    #[test]
    fn test_extract_record() {
        let code = r#"
public record Person(string FirstName, string LastName);

public record class Employee : Person
{
    public int Id { get; init; }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Person" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Employee" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Id" && s.kind == SymbolKind::Field)
        );
    }

    #[test]
    fn test_extract_delegate() {
        let code = r#"
public delegate void EventHandler(object sender, EventArgs e);
public delegate Task<T> AsyncFunc<T>(string input);
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "EventHandler" && s.kind == SymbolKind::TypeAlias)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "AsyncFunc" && s.kind == SymbolKind::TypeAlias)
        );
    }

    #[test]
    fn test_extract_methods() {
        let code = r#"
public class Calculator
{
    public int Add(int a, int b) => a + b;
    
    private async Task<string> ProcessAsync()
    {
        return await Task.FromResult("done");
    }
    
    public static void Main(string[] args) { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Add" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "ProcessAsync" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Main" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_extract_properties() {
        let code = r#"
public class User
{
    public string Name { get; set; }
    public int Age { get; private set; }
    public string Email { get; init; }
    private string _id;
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Name" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Age" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Email" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "_id" && s.kind == SymbolKind::Field)
        );
    }

    #[test]
    fn test_extract_events() {
        let code = r#"
public class Button
{
    public event EventHandler Click;
    public event Action<string> TextChanged;
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Click" && s.kind == SymbolKind::Field)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "TextChanged" && s.kind == SymbolKind::Field)
        );
    }

    #[test]
    fn test_extract_indexer() {
        let code = r#"
public class MyList
{
    public int this[int index]
    {
        get { return 0; }
        set { }
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "this[]" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_extract_operator() {
        let code = r#"
public class Complex
{
    public static Complex operator +(Complex a, Complex b)
    {
        return new Complex();
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name().starts_with("operator") && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_extract_xml_documentation() {
        let code = r#"
/// <summary>
/// This is a user service class
/// </summary>
public class UserService
{
    /// <summary>
    /// Gets a user by ID
    /// </summary>
    /// <param name="id">The user ID</param>
    /// <returns>The user object</returns>
    public User GetUser(int id) { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        let class_symbol = symbols
            .iter()
            .find(|s| s.as_name() == "UserService")
            .unwrap();
        assert!(class_symbol.doc_comment.is_some());
        assert!(
            class_symbol
                .doc_comment
                .as_ref()
                .unwrap()
                .contains("user service class")
        );

        let method_symbol = symbols.iter().find(|s| s.as_name() == "GetUser").unwrap();
        assert!(method_symbol.doc_comment.is_some());
        assert!(
            method_symbol
                .doc_comment
                .as_ref()
                .unwrap()
                .contains("Gets a user by ID")
        );
    }

    #[test]
    fn test_extract_method_calls() {
        let code = r#"
public class Service
{
    public void Process()
    {
        Console.WriteLine("Processing");
        var result = Calculate();
        helper.DoWork();
        new MyClass().Initialize();
    }
    
    private int Calculate() => 42;
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let calls = parser.find_method_calls(code);

        assert!(
            calls
                .iter()
                .any(|c| c.method_name == "WriteLine" && c.receiver == Some("Console".to_string()))
        );
        assert!(
            calls
                .iter()
                .any(|c| c.method_name == "Calculate" && c.receiver.is_none())
        );
        assert!(
            calls
                .iter()
                .any(|c| c.method_name == "DoWork" && c.receiver == Some("helper".to_string()))
        );
        assert!(calls.iter().any(|c| c.method_name == "Initialize"));
    }

    #[test]
    fn test_extract_imports() {
        let code = r#"
using System;
using System.Collections.Generic;
using System.Linq;
using MyAlias = Some.Long.Namespace.Type;
global using System.Threading.Tasks;
using static System.Console;
"#;
        let mut parser = CSharpParser::new().unwrap();
        let imports = parser.find_imports(code, FileId::new(1).unwrap());

        assert!(imports.iter().any(|i| i.path == "System"));
        assert!(
            imports
                .iter()
                .any(|i| i.path == "System.Collections.Generic")
        );
        assert!(imports.iter().any(|i| i.path == "System.Linq"));
        assert!(imports.iter().any(
            |i| i.path == "Some.Long.Namespace.Type" && i.alias == Some("MyAlias".to_string())
        ));
        assert!(imports.iter().any(|i| i.path.starts_with("global::")));
        assert!(imports.iter().any(|i| i.path.starts_with("static::")));
    }

    #[test]
    fn test_file_scoped_namespace() {
        let code = r#"
namespace MyApp.Services;

public class UserService
{
    public void Process() { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "MyApp.Services" && s.kind == SymbolKind::Module)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "UserService" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Process" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_nested_types() {
        let code = r#"
public class Outer
{
    public class Inner
    {
        public void InnerMethod() { }
    }
    
    public interface IInner { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Outer" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Inner" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "InnerMethod" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "IInner" && s.kind == SymbolKind::Interface)
        );
    }

    #[test]
    fn test_generic_types() {
        let code = r#"
public class Repository<T> where T : class
{
    public T Get(int id) { }
    public List<T> GetAll() { }
}

public interface IService<TRequest, TResponse> { }
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Repository" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Get" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "GetAll" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "IService" && s.kind == SymbolKind::Interface)
        );
    }

    #[test]
    fn test_partial_class() {
        let code = r#"
public partial class MyClass
{
    public void Method1() { }
}

public partial class MyClass
{
    public void Method2() { }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        // Should extract both occurrences of the partial class
        let class_symbols: Vec<_> = symbols
            .iter()
            .filter(|s| s.as_name() == "MyClass" && s.kind == SymbolKind::Class)
            .collect();
        assert_eq!(class_symbols.len(), 2);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Method1" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Method2" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_async_methods() {
        let code = r#"
public class AsyncService
{
    public async Task<string> GetDataAsync()
    {
        return await Task.FromResult("data");
    }
    
    public async void FireAndForget() { }
    
    public async ValueTask<int> GetCountAsync() => 42;
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "GetDataAsync" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "FireAndForget" && s.kind == SymbolKind::Method)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "GetCountAsync" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_extract_type_uses() {
        let code = r#"
public class Service
{
    private readonly ILogger logger;
    
    public void Process(string input)
    {
        List<int> numbers = new List<int>();
        var result = input as object;
        if (result is string str) { }
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let uses = parser.find_uses(code);

        assert!(uses.iter().any(|(_, type_name, _)| *type_name == "ILogger"));
        assert!(uses.iter().any(|(_, type_name, _)| *type_name == "string"));
        assert!(
            uses.iter()
                .any(|(_, type_name, _)| *type_name == "List<int>")
        );
        assert!(uses.iter().any(|(_, type_name, _)| *type_name == "object"));
    }

    #[test]
    fn test_primary_constructor() {
        let code = r#"
public class Person(string firstName, string lastName)
{
    public string FullName => $"{firstName} {lastName}";
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "Person" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "FullName" && s.kind == SymbolKind::Field)
        );
    }

    #[test]
    fn test_extension_methods() {
        let code = r#"
public static class StringExtensions
{
    public static bool IsNullOrEmpty(this string str)
    {
        return string.IsNullOrEmpty(str);
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, FileId::new(1).unwrap(), &mut counter);

        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "StringExtensions" && s.kind == SymbolKind::Class)
        );
        assert!(
            symbols
                .iter()
                .any(|s| s.as_name() == "IsNullOrEmpty" && s.kind == SymbolKind::Method)
        );
    }

    #[test]
    fn test_extension_method_detection() {
        let code = r#"
public static class StringExtensions
{
    public static bool IsNullOrEmpty(this string str)
    {
        return string.IsNullOrEmpty(str);
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();

        // Parse the tree and inspect it
        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        // Walk through the tree and find method_declaration nodes
        fn walk_tree(node: tree_sitter::Node, code: &str, depth: usize) {
            if node.kind() == "method_declaration" {
                println!(
                    "{}Found method_declaration at depth {}",
                    "  ".repeat(depth),
                    depth
                );
                for child in node.children(&mut node.walk()) {
                    println!(
                        "{}  Child: {} - '{}'",
                        "  ".repeat(depth),
                        child.kind(),
                        if child.byte_range().len() < 50 {
                            &code[child.byte_range()]
                        } else {
                            "<too long>"
                        }
                    );
                }
            }

            for child in node.children(&mut node.walk()) {
                walk_tree(child, code, depth + 1);
            }
        }

        walk_tree(root, code, 0);

        // Now test if we can detect it as an extension method
        let methods = parser.find_inherent_methods(code);
        println!(
            "\nFound methods: {:?}",
            methods
                .iter()
                .map(|(t, m, _)| format!("{t}.{m}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_find_inherent_methods() {
        let code = r#"
public class Calculator
{
    public int Add(int a, int b) => a + b;
    
    private async Task<string> ProcessAsync()
    {
        return await Task.FromResult("done");
    }
    
    public static void Main(string[] args) { }
}

public struct Point
{
    public double Distance() => Math.Sqrt(X * X + Y * Y);
}

public interface IService
{
    void Execute();
    Task<bool> ValidateAsync(string input);
}

public static class StringExtensions
{
    public static bool IsNullOrEmpty(this string str)
    {
        return string.IsNullOrEmpty(str);
    }
    
    public static string Reverse(this string str)
    {
        return new string(str.ToCharArray().Reverse().ToArray());
    }
}
"#;
        let mut parser = CSharpParser::new().unwrap();
        let methods = parser.find_inherent_methods(code);

        // Should find methods from Calculator class
        assert!(
            methods.iter().any(
                |(type_name, method_name, _)| type_name == "Calculator" && method_name == "Add"
            )
        );
        assert!(
            methods
                .iter()
                .any(|(type_name, method_name, _)| type_name == "Calculator"
                    && method_name == "ProcessAsync")
        );
        assert!(
            methods
                .iter()
                .any(|(type_name, method_name, _)| type_name == "Calculator"
                    && method_name == "Main")
        );

        // Should find methods from Point struct
        assert!(
            methods.iter().any(
                |(type_name, method_name, _)| type_name == "Point" && method_name == "Distance"
            )
        );

        // Should find methods from IService interface
        assert!(
            methods
                .iter()
                .any(|(type_name, method_name, _)| type_name == "IService"
                    && method_name == "Execute")
        );
        assert!(
            methods
                .iter()
                .any(|(type_name, method_name, _)| type_name == "IService"
                    && method_name == "ValidateAsync")
        );

        // Should find extension methods mapped to string type
        assert!(
            methods
                .iter()
                .any(|(type_name, method_name, _)| type_name == "string"
                    && method_name == "IsNullOrEmpty")
        );
        assert!(
            methods.iter().any(
                |(type_name, method_name, _)| type_name == "string" && method_name == "Reverse"
            )
        );

        // Verify we found all expected methods
        let calculator_methods: Vec<_> = methods
            .iter()
            .filter(|(type_name, _, _)| type_name == "Calculator")
            .collect();
        assert_eq!(calculator_methods.len(), 3);

        let string_extensions: Vec<_> = methods
            .iter()
            .filter(|(type_name, _, _)| type_name == "string")
            .collect();
        assert_eq!(string_extensions.len(), 2);
    }
}
