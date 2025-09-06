//! C language parser implementation

use crate::parsing::method_call::MethodCall;
use crate::parsing::{Import, Language, LanguageParser};
use crate::types::{SymbolCounter, Range};
use crate::{FileId, Symbol, SymbolKind};
use std::any::Any;
use tree_sitter::{Node, Parser};

pub struct CParser {
    parser: Parser,
}

impl std::fmt::Debug for CParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CParser")
            .field("language", &"C")
            .finish()
    }
}

impl CParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .map_err(|e| format!("Failed to set C language: {e}"))?;

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
}

impl LanguageParser for CParser {
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
        // C doesn't have standardized doc comments
        None
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();

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

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // C doesn't have interfaces or traits
        Vec::new()
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
        Language::C
    }
}