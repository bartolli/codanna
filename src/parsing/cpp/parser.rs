//! C++ language parser implementation

use crate::parsing::context::ParserContext;
use crate::parsing::method_call::MethodCall;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{Import, Language, LanguageParser, NodeTracker, NodeTrackingState};
use crate::types::{Range, SymbolCounter};
use crate::{FileId, Symbol, SymbolKind, Visibility};
use std::any::Any;
use tree_sitter::{Node, Parser};

pub struct CppParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for CppParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CppParser")
            .field("language", &"C++")
            .finish()
    }
}

impl CppParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .map_err(|e| format!("Failed to set C++ language: {e}"))?;

        Ok(Self {
            parser,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
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

    /// Parse C++ code and extract symbols
    ///
    /// This is the main parsing method that can be called directly.
    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        // Delegate to the LanguageParser trait implementation
        <Self as LanguageParser>::parse(self, code, file_id, symbol_counter)
    }

    /// Extract import statements from the code
    fn extract_imports_from_node(
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if node.kind() == "preproc_include" {
            if let Some(path_node) = node.child_by_field_name("path") {
                let path_text = &code[path_node.byte_range()];
                // Remove quotes
                let clean_path = path_text.trim_matches(|c| c == '"' || c == '<' || c == '>');
                imports.push(Import {
                    path: clean_path.to_string(),
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
        }

        // Recursively process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::extract_imports_from_node(child, code, file_id, imports);
            }
        }
    }

    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        depth: usize,
    ) {
        // Guard against stack overflow
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            "function_definition" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    // Check for qualified_identifier (ClassName::methodName)
                    let mut is_method = self.context.current_class().is_some();
                    let mut method_name = String::new();

                    for i in 0..declarator.child_count() {
                        if let Some(child) = declarator.child(i as u32) {
                            if child.kind() == "qualified_identifier" {
                                // This is a method implementation (Class::method)
                                is_method = true;
                                // Extract method name from qualified_identifier
                                for j in 0..child.child_count() {
                                    if let Some(id_node) = child.child(j as u32) {
                                        if id_node.kind() == "identifier" {
                                            method_name = code[id_node.byte_range()].to_string();
                                            break;
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }

                    // Fallback: try to get declarator field
                    if method_name.is_empty() {
                        if let Some(name_node) = declarator.child_by_field_name("declarator") {
                            method_name = code[name_node.byte_range()].to_string();
                        }
                    }

                    if !method_name.is_empty() {
                        let symbol_id = counter.next_id();
                        let doc_comment = self.extract_doc_comment(&node, code);
                        let range = Range::new(
                            node.start_position().row as u32,
                            node.start_position().column as u16,
                            node.end_position().row as u32,
                            node.end_position().column as u16,
                        );

                        let kind = if is_method {
                            SymbolKind::Method
                        } else {
                            SymbolKind::Function
                        };

                        let symbol = self.create_symbol(
                            symbol_id,
                            method_name,
                            kind,
                            file_id,
                            range,
                            None, // signature
                            doc_comment,
                            "", // module_path
                            Visibility::Public,
                        );

                        symbols.push(symbol);
                    }
                }
            }
            "class_specifier" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(name_node) = node.child_by_field_name("name") {
                    let class_name = &code[name_node.byte_range()];
                    let symbol_id = counter.next_id();
                    let doc_comment = self.extract_doc_comment(&node, code);
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );

                    let symbol = self.create_symbol(
                        symbol_id,
                        class_name.to_string(),
                        SymbolKind::Class,
                        file_id,
                        range,
                        None, // signature
                        doc_comment,
                        "", // module_path
                        Visibility::Public,
                    );

                    symbols.push(symbol);

                    // Enter class scope to track methods
                    self.context
                        .enter_scope(crate::parsing::context::ScopeType::Class);

                    // Save current context
                    let saved_function = self.context.current_function().map(|s| s.to_string());
                    let saved_class = self.context.current_class().map(|s| s.to_string());

                    // Set current class for method tracking
                    self.context.set_current_class(Some(class_name.to_string()));

                    // Process children to extract methods
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i as u32) {
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

                    // Exit scope first
                    self.context.exit_scope();

                    // Restore previous context
                    self.context.set_current_function(saved_function);
                    self.context.set_current_class(saved_class);

                    // Return early since we already processed children
                    return;
                }
            }
            "struct_specifier" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    let symbol_id = counter.next_id();
                    let doc_comment = self.extract_doc_comment(&node, code);
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );

                    let symbol = self.create_symbol(
                        symbol_id,
                        name.to_string(),
                        SymbolKind::Struct,
                        file_id,
                        range,
                        None, // signature
                        doc_comment,
                        "", // module_path
                        Visibility::Public,
                    );

                    symbols.push(symbol);
                }
            }
            "enum_specifier" => {
                self.register_handled_node(node.kind(), node.kind_id());
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    let symbol_id = counter.next_id();
                    let doc_comment = self.extract_doc_comment(&node, code);
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );

                    let symbol = self.create_symbol(
                        symbol_id,
                        name.to_string(),
                        SymbolKind::Enum,
                        file_id,
                        range,
                        None, // signature
                        doc_comment,
                        "", // module_path
                        Visibility::Public,
                    );

                    symbols.push(symbol);
                }
            }
            "field_declaration" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Check if this is a method declaration (has function_declarator child)
                if self.context.current_class().is_some() {
                    for i in 0..node.child_count() {
                        if let Some(child) = node.child(i as u32) {
                            if child.kind() == "function_declarator" {
                                // This is a method declaration
                                // Look for field_identifier child
                                for j in 0..child.child_count() {
                                    if let Some(name_node) = child.child(j as u32) {
                                        if name_node.kind() == "field_identifier" {
                                            let method_name = &code[name_node.byte_range()];
                                            let symbol_id = counter.next_id();
                                            let doc_comment = self.extract_doc_comment(&node, code);
                                            let range = Range::new(
                                                node.start_position().row as u32,
                                                node.start_position().column as u16,
                                                node.end_position().row as u32,
                                                node.end_position().column as u16,
                                            );

                                            let symbol = self.create_symbol(
                                                symbol_id,
                                                method_name.to_string(),
                                                SymbolKind::Method,
                                                file_id,
                                                range,
                                                None, // signature
                                                doc_comment,
                                                "", // module_path
                                                Visibility::Public,
                                            );

                                            symbols.push(symbol);
                                            break;
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
            _ => {
                // Track all nodes we encounter, even if not extracting symbols
                self.register_handled_node(node.kind(), node.kind_id());
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
            }
        }
    }

    fn extract_calls_from_node(node: Node, code: &str, calls: &mut Vec<MethodCall>) {
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                // Extract the actual function name from different call patterns
                let function_name = match function_node.kind() {
                    // Member function call: obj->method() or obj.method()
                    "field_expression" => {
                        // Get the field identifier (the actual method name)
                        if let Some(field_node) = function_node.child_by_field_name("field") {
                            &code[field_node.byte_range()]
                        } else {
                            &code[function_node.byte_range()]
                        }
                    }
                    // Simple function call: function()
                    _ => &code[function_node.byte_range()],
                };

                calls.push(MethodCall::new(
                    "", // caller will be set by the indexer
                    function_name,
                    Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    ),
                ));
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::extract_calls_from_node(child, code, calls);
            }
        }
    }

    /// Find method implementations in AST nodes recursively
    fn find_implementations_in_node<'a>(
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // In C++, method implementations often have the form Class::method
        if node.kind() == "function_definition" {
            if let Some(declarator) = node.child_by_field_name("declarator") {
                // Check if this is a method implementation (has :: in the name)
                let declarator_text = &code[declarator.byte_range()];
                if declarator_text.contains("::") {
                    // This is likely a method implementation
                    // Extract class name and method name
                    if let Some(separator_pos) = declarator_text.find("::") {
                        let class_name = &declarator_text[..separator_pos];
                        let method_name = &declarator_text[separator_pos + 2..];
                        let range = Range::new(
                            node.start_position().row as u32,
                            node.start_position().column as u16,
                            node.end_position().row as u32,
                            node.end_position().column as u16,
                        );
                        implementations.push((class_name, method_name, range));
                    }
                }
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::find_implementations_in_node(child, code, implementations);
            }
        }
    }

    /// Find inheritance relationships in AST nodes recursively
    fn find_extends_in_node<'a>(
        node: Node,
        code: &'a str,
        extends: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // In C++, inheritance is specified with : public BaseClass, : protected BaseClass, etc.
        if node.kind() == "class_specifier" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let derived_class = &code[name_node.byte_range()];

                // Look for base class specifiers
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        if child.kind() == "base_class_clause" {
                            // Extract base class names
                            Self::extract_base_classes_in_node(child, code, derived_class, extends);
                        }
                    }
                }
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::find_extends_in_node(child, code, extends);
            }
        }
    }

    /// Extract base classes from a base_class_clause node
    fn extract_base_classes_in_node<'a>(
        node: Node,
        code: &'a str,
        derived_class: &'a str,
        extends: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Process children to find base class names
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "type_identifier" {
                    let base_class = &code[child.byte_range()];
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    extends.push((derived_class, base_class, range));
                } else {
                    // Recursively process children
                    Self::extract_base_classes_in_node(child, code, derived_class, extends);
                }
            }
        }
    }

    /// Find variable and function uses in AST nodes recursively
    fn find_uses_in_node<'a>(node: Node, code: &'a str, uses: &mut Vec<(&'a str, &'a str, Range)>) {
        // Identifier nodes represent variable/function uses
        if node.kind() == "identifier" {
            // We need context to determine what this identifier is used in
            // For now, we'll just track the identifier name and its location
            let identifier_name = &code[node.byte_range()];
            let range = Range::new(
                node.start_position().row as u32,
                node.start_position().column as u16,
                node.end_position().row as u32,
                node.end_position().column as u16,
            );
            // Use empty string for context for now
            uses.push(("", identifier_name, range));
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::find_uses_in_node(child, code, uses);
            }
        }
    }

    /// Find variable and macro definitions in AST nodes recursively
    fn find_defines_in_node<'a>(
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Variable declarations
        if node.kind() == "declaration" {
            if let Some(declarator) = node.child_by_field_name("declarator") {
                let declarator_text = &code[declarator.byte_range()];
                // Extract variable name (before = if present)
                if let Some(equals_pos) = declarator_text.find('=') {
                    let var_name = declarator_text[..equals_pos].trim();
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    defines.push((var_name, "variable", range));
                } else {
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    defines.push((declarator_text.trim(), "variable", range));
                }
            }
        }
        // Preprocessor definitions
        else if node.kind() == "preproc_def" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let macro_name = &code[name_node.byte_range()];
                let range = Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                );
                defines.push((macro_name, "macro", range));
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::find_defines_in_node(child, code, defines);
            }
        }
    }

    /// Find variable type relationships in AST nodes recursively
    fn find_variable_types_in_node<'a>(
        node: Node,
        code: &'a str,
        variable_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Variable declarations with explicit types
        if node.kind() == "declaration" {
            if let Some(type_node) = node.child_by_field_name("type") {
                let type_name = &code[type_node.byte_range()];
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    let declarator_text = &code[declarator.byte_range()];
                    // Extract variable name (before = if present)
                    let var_name = if let Some(equals_pos) = declarator_text.find('=') {
                        declarator_text[..equals_pos].trim()
                    } else {
                        declarator_text.trim()
                    };
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    variable_types.push((var_name, type_name, range));
                }
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::find_variable_types_in_node(child, code, variable_types);
            }
        }
    }

    /// Find class method definitions in AST nodes recursively
    fn find_inherent_methods_in_node(
        node: Node,
        code: &str,
        inherent_methods: &mut Vec<(String, String, Range)>,
    ) {
        // Method definitions inside class specifiers
        if node.kind() == "class_specifier" {
            if let Some(class_name_node) = node.child_by_field_name("name") {
                let class_name = &code[class_name_node.byte_range()];

                // Look for method definitions inside the class body
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32) {
                        if child.kind() == "field_declaration_list" {
                            Self::extract_methods_from_class_body(
                                child,
                                code,
                                class_name,
                                inherent_methods,
                            );
                        }
                    }
                }
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::find_inherent_methods_in_node(child, code, inherent_methods);
            }
        }
    }

    /// Extract methods from class body
    fn extract_methods_from_class_body(
        node: Node,
        code: &str,
        class_name: &str,
        inherent_methods: &mut Vec<(String, String, Range)>,
    ) {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "declaration" || child.kind() == "function_definition" {
                    // Look for method names
                    if let Some(declarator) = child.child_by_field_name("declarator") {
                        let method_name = &code[declarator.byte_range()];
                        // Extract just the method name (before parameters)
                        if let Some(paren_pos) = method_name.find('(') {
                            let clean_method_name = method_name[..paren_pos].trim();
                            let range = Range::new(
                                child.start_position().row as u32,
                                child.start_position().column as u16,
                                child.end_position().row as u32,
                                child.end_position().column as u16,
                            );
                            inherent_methods.push((
                                class_name.to_string(),
                                clean_method_name.to_string(),
                                range,
                            ));
                        } else {
                            let range = Range::new(
                                child.start_position().row as u32,
                                child.start_position().column as u16,
                                child.end_position().row as u32,
                                child.end_position().column as u16,
                            );
                            inherent_methods.push((
                                class_name.to_string(),
                                method_name.trim().to_string(),
                                range,
                            ));
                        }
                    }
                } else {
                    // Recursively process children
                    Self::extract_methods_from_class_body(
                        child,
                        code,
                        class_name,
                        inherent_methods,
                    );
                }
            }
        }
    }
}

impl NodeTracker for CppParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<crate::parsing::HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}

impl LanguageParser for CppParser {
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

        self.extract_symbols_from_node(root_node, code, file_id, &mut symbols, symbol_counter, 0);

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        // Look for Doxygen/JavaDoc style comments (/** ... */ or ///)
        // Check previous sibling for doc comment
        let prev = node.prev_sibling()?;

        if prev.kind() == "comment" {
            let comment = &code[prev.byte_range()];

            // Handle block comments (/** ... */)
            if comment.starts_with("/**") {
                let cleaned = comment
                    .trim_start_matches("/**")
                    .trim_end_matches("*/")
                    .lines()
                    .map(|line| {
                        // Remove leading " * " or " *" from each line
                        line.trim_start_matches(" * ")
                            .trim_start_matches(" *")
                            .trim_start_matches("     * ") // Handle extra indentation
                            .trim_start_matches("     *")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();

                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
            // Handle line comments (///)
            else if comment.starts_with("///") {
                let cleaned = comment.trim_start_matches("///").trim().to_string();
                if !cleaned.is_empty() {
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

        let root_node = tree.root_node();
        let mut calls = Vec::new();

        // Use recursive method with context tracking
        Self::extract_calls_recursive(root_node, code, None, &mut calls);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();

        Self::extract_calls_from_node(root_node, code, &mut calls);

        calls
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut implementations = Vec::new();

        Self::find_implementations_in_node(root_node, code, &mut implementations);
        implementations
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut extends = Vec::new();

        Self::find_extends_in_node(root_node, code, &mut extends);
        extends
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut uses = Vec::new();

        Self::find_uses_in_node(root_node, code, &mut uses);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut defines = Vec::new();

        Self::find_defines_in_node(root_node, code, &mut defines);
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
        Language::Cpp
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut variable_types = Vec::new();

        Self::find_variable_types_in_node(root_node, code, &mut variable_types);
        variable_types
    }

    fn find_inherent_methods(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut inherent_methods = Vec::new();

        Self::find_inherent_methods_in_node(root_node, code, &mut inherent_methods);
        inherent_methods
    }
}

impl CppParser {
    /// Recursively extract function calls with context tracking
    fn extract_calls_recursive<'a>(
        node: Node,
        code: &'a str,
        current_function: Option<&'a str>,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Determine function context - track which function we're inside
        let function_context = if node.kind() == "function_definition" {
            // Extract function name
            if let Some(declarator) = node.child_by_field_name("declarator") {
                // Handle function_declarator (for both regular functions and methods)
                if declarator.kind() == "function_declarator" {
                    if let Some(inner_declarator) = declarator.child_by_field_name("declarator") {
                        match inner_declarator.kind() {
                            // Method: QWindow::setX
                            "qualified_identifier" => {
                                // Extract just the method name (after ::)
                                if let Some(name_node) =
                                    inner_declarator.child_by_field_name("name")
                                {
                                    Some(&code[name_node.byte_range()])
                                } else {
                                    Some(&code[inner_declarator.byte_range()])
                                }
                            }
                            // Regular function or simple identifier
                            _ => Some(&code[inner_declarator.byte_range()]),
                        }
                    } else {
                        current_function
                    }
                } else if let Some(name_node) = declarator.child_by_field_name("declarator") {
                    // Fallback for other declarator types
                    Some(&code[name_node.byte_range()])
                } else {
                    current_function
                }
            } else {
                current_function
            }
        } else {
            // Not a function - inherit current context
            current_function
        };

        // Check if this is a call expression
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                // Extract the actual function name from different call patterns
                let target_name = match function_node.kind() {
                    // Member function call: obj->method() or obj.method()
                    "field_expression" => {
                        // Get the field identifier (the actual method name)
                        if let Some(field_node) = function_node.child_by_field_name("field") {
                            &code[field_node.byte_range()]
                        } else {
                            &code[function_node.byte_range()]
                        }
                    }
                    // Simple function call: function()
                    _ => &code[function_node.byte_range()],
                };

                let range = Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                );

                // Only record call if we have a function context
                if let Some(context) = function_context {
                    calls.push((context, target_name, range));
                }
            }
        }

        // Recurse to children with current context
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::extract_calls_recursive(child, code, function_context, calls);
        }
    }
}
