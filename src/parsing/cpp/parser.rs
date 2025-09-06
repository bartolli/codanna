//! C++ language parser implementation

use crate::parsing::method_call::MethodCall;
use crate::parsing::{Import, Language, LanguageParser};
use crate::types::{SymbolCounter, Range};
use crate::{FileId, Symbol, SymbolKind};
use std::any::Any;
use tree_sitter::{Node, Parser};

pub struct CppParser {
    parser: Parser,
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
        })
    }

    /// Extract import statements from the code
    fn extract_imports_from_node(
        &self,
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
            if let Some(child) = node.child(i) {
                self.extract_imports_from_node(child, code, file_id, imports);
            }
        }
    }

    fn extract_symbols_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        match node.kind() {
            "function_definition" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    if let Some(name_node) = declarator.child_by_field_name("declarator") {
                        let name = &code[name_node.byte_range()];
                        let symbol_id = counter.next_id();
                        symbols.push(Symbol::new(
                            symbol_id,
                            name.to_string(),
                            SymbolKind::Function,
                            file_id,
                            Range::new(node.start_position().row as u32, node.start_position().column as u16, 
                                      node.end_position().row as u32, node.end_position().column as u16),
                        ).with_visibility(crate::Visibility::Public));
                    }
                }
            }
            "class_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    let symbol_id = counter.next_id();
                    symbols.push(Symbol::new(
                        symbol_id,
                        name.to_string(),
                        SymbolKind::Class,
                        file_id,
                        Range::new(node.start_position().row as u32, node.start_position().column as u16, 
                                  node.end_position().row as u32, node.end_position().column as u16),
                    ).with_visibility(crate::Visibility::Public));
                }
            }
            "struct_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    let symbol_id = counter.next_id();
                    symbols.push(Symbol::new(
                        symbol_id,
                        name.to_string(),
                        SymbolKind::Struct,
                        file_id,
                        Range::new(node.start_position().row as u32, node.start_position().column as u16, 
                                  node.end_position().row as u32, node.end_position().column as u16),
                    ).with_visibility(crate::Visibility::Public));
                }
            }
            "enum_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &code[name_node.byte_range()];
                    let symbol_id = counter.next_id();
                    symbols.push(Symbol::new(
                        symbol_id,
                        name.to_string(),
                        SymbolKind::Enum,
                        file_id,
                        Range::new(node.start_position().row as u32, node.start_position().column as u16, 
                                  node.end_position().row as u32, node.end_position().column as u16),
                    ).with_visibility(crate::Visibility::Public));
                }
            }
            _ => {}
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.extract_symbols_from_node(child, code, file_id, symbols, counter);
            }
        }
    }

    fn extract_calls_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        calls: &mut Vec<MethodCall>,
    ) {
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                let function_name = &code[function_node.byte_range()];
                calls.push(MethodCall::new(
                    "", // caller will be set by the indexer
                    function_name,
                    Range::new(node.start_position().row as u32, node.start_position().column as u16, 
                              node.end_position().row as u32, node.end_position().column as u16),
                ));
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.extract_calls_from_node(child, code, file_id, calls);
            }
        }
    }

    /// Find method implementations in AST nodes recursively
    fn find_implementations_in_node<'a>(
        &self,
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
            if let Some(child) = node.child(i) {
                self.find_implementations_in_node(child, code, implementations);
            }
        }
    }

    /// Find inheritance relationships in AST nodes recursively
    fn find_extends_in_node<'a>(
        &self,
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
                    if let Some(child) = node.child(i) {
                        if child.kind() == "base_class_clause" {
                            // Extract base class names
                            self.extract_base_classes_in_node(child, code, derived_class, extends);
                        }
                    }
                }
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.find_extends_in_node(child, code, extends);
            }
        }
    }

    /// Extract base classes from a base_class_clause node
    fn extract_base_classes_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        derived_class: &'a str,
        extends: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Process children to find base class names
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
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
                    self.extract_base_classes_in_node(child, code, derived_class, extends);
                }
            }
        }
    }

    /// Find variable and function uses in AST nodes recursively
    fn find_uses_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
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
            if let Some(child) = node.child(i) {
                self.find_uses_in_node(child, code, uses);
            }
        }
    }

    /// Find variable and macro definitions in AST nodes recursively
    fn find_defines_in_node<'a>(
        &self,
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
            if let Some(child) = node.child(i) {
                self.find_defines_in_node(child, code, defines);
            }
        }
    }

    /// Find variable type relationships in AST nodes recursively
    fn find_variable_types_in_node<'a>(
        &self,
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
            if let Some(child) = node.child(i) {
                self.find_variable_types_in_node(child, code, variable_types);
            }
        }
    }

    /// Find class method definitions in AST nodes recursively
    fn find_inherent_methods_in_node(
        &self,
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
                    if let Some(child) = node.child(i) {
                        if child.kind() == "field_declaration_list" {
                            self.extract_methods_from_class_body(child, code, class_name, inherent_methods);
                        }
                    }
                }
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.find_inherent_methods_in_node(child, code, inherent_methods);
            }
        }
    }

    /// Extract methods from class body
    fn extract_methods_from_class_body(
        &self,
        node: Node,
        code: &str,
        class_name: &str,
        inherent_methods: &mut Vec<(String, String, Range)>,
    ) {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
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
                            inherent_methods.push((class_name.to_string(), clean_method_name.to_string(), range));
                        } else {
                            let range = Range::new(
                                child.start_position().row as u32,
                                child.start_position().column as u16,
                                child.end_position().row as u32,
                                child.end_position().column as u16,
                            );
                            inherent_methods.push((class_name.to_string(), method_name.trim().to_string(), range));
                        }
                    }
                } else {
                    // Recursively process children
                    self.extract_methods_from_class_body(child, code, class_name, inherent_methods);
                }
            }
        }
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

        self.extract_symbols_from_node(root_node, code, file_id, &mut symbols, symbol_counter);

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, _node: &Node, _code: &str) -> Option<String> {
        // C++ doesn't have standardized doc comments
        None
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();

        // Simple implementation that doesn't track containing functions
        // In a more sophisticated implementation, we would track the containing function
        self.find_calls_in_node(root_node, code, &mut calls);
        calls
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();

        self.extract_calls_from_node(root_node, code, FileId::new(1).unwrap(), &mut calls);

        calls
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

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut extends = Vec::new();

        self.find_extends_in_node(root_node, code, &mut extends);
        extends
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut uses = Vec::new();

        self.find_uses_in_node(root_node, code, &mut uses);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut defines = Vec::new();

        self.find_defines_in_node(root_node, code, &mut defines);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut imports = Vec::new();

        self.extract_imports_from_node(root_node, code, file_id, &mut imports);

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

        self.find_variable_types_in_node(root_node, code, &mut variable_types);
        variable_types
    }

    fn find_inherent_methods(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut inherent_methods = Vec::new();

        self.find_inherent_methods_in_node(root_node, code, &mut inherent_methods);
        inherent_methods
    }
}

impl CppParser {
    /// Find function calls in AST node recursively
    fn find_calls_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Simple implementation that doesn't track containing functions
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                let target_name = &code[function_node.byte_range()];
                // We don't have caller information in this simple implementation
                let range = Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                );
                // Use empty string for caller as we don't track it in this simple implementation
                calls.push(("", target_name, range));
            }
        }

        // Process children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.find_calls_in_node(child, code, calls);
            }
        }
    }
}