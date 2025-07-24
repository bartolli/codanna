use crate::{Symbol, SymbolId, SymbolKind, FileId, Range};
use tree_sitter::{Parser, Node};

pub struct RustParser {
    parser: Parser,
}

impl RustParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| format!("Failed to set Rust language: {}", e))?;
        
        Ok(Self { parser })
    }
    
    pub fn parse(&mut self, code: &str, file_id: FileId) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut symbols = Vec::new();
        let mut symbol_id_counter = 1u32;
        
        // Walk the tree manually to find symbols
        self.extract_symbols_from_node(root_node, code, file_id, &mut symbols, &mut symbol_id_counter);
        
        symbols
    }
    
    fn extract_symbols_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut u32,
    ) {
        match node.kind() {
            "function_item" => {
                // Check if this function is inside an impl block
                let mut parent = node.parent();
                let mut is_method = false;
                
                // Walk up the tree to check for impl_item ancestor
                while let Some(p) = parent {
                    if p.kind() == "impl_item" {
                        is_method = true;
                        break;
                    }
                    parent = p.parent();
                }
                
                let kind = if is_method {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        name_node,
                        kind,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        name_node,
                        SymbolKind::Struct,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        name_node,
                        SymbolKind::Trait,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "impl_item" => {
                // Just recurse into impl blocks, functions will be handled with Method kind
            }
            _ => {}
        }
        
        // Recurse into children (except for impl_item which returns early)
        for child in node.children(&mut node.walk()) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter);
        }
    }
    
    pub fn find_calls(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut calls = Vec::new();
        
        self.find_calls_in_node(root_node, code, &mut calls);
        
        calls
    }
    
    fn find_calls_in_node(&self, node: Node, code: &str, calls: &mut Vec<(String, String, Range)>) {
        // Find the containing function
        let containing_function = self.find_containing_function(node, code);
        
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                if function_node.kind() == "identifier" {
                    let target_name = &code[function_node.byte_range()];
                    if let Some(ref caller) = containing_function {
                        let range = Range::new(
                            node.start_position().row as u32,
                            node.start_position().column as u16,
                            node.end_position().row as u32,
                            node.end_position().column as u16,
                        );
                        calls.push((caller.clone(), target_name.to_string(), range));
                    }
                }
            }
        }
        
        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_calls_in_node(child, code, calls);
        }
    }
    
    fn find_containing_function(&self, mut node: Node, code: &str) -> Option<String> {
        loop {
            if node.kind() == "function_item" {
                if let Some(name_node) = node.child_by_field_name("name") {
                    return Some(code[name_node.byte_range()].to_string());
                }
            }
            
            match node.parent() {
                Some(parent) => node = parent,
                None => return None,
            }
        }
    }
    
    fn create_symbol(
        &self,
        counter: &mut u32,
        name_node: Node,
        kind: SymbolKind,
        file_id: FileId,
        code: &str,
    ) -> Option<Symbol> {
        let name = &code[name_node.byte_range()];
        
        let symbol_id = SymbolId::new(*counter)?;
        *counter += 1;
        
        let range = Range::new(
            name_node.start_position().row as u32,
            name_node.start_position().column as u16,
            name_node.end_position().row as u32,
            name_node.end_position().column as u16,
        );
        
        Some(Symbol::new(
            symbol_id,
            name,
            kind,
            file_id,
            range,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_simple_function() {
        let mut parser = RustParser::new().unwrap();
        let code = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let file_id = FileId::new(1).unwrap();
        
        let symbols = parser.parse(code, file_id);
        
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name.as_ref(), "add");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }
    
    #[test]
    fn test_parse_struct() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            struct Point {
                x: f64,
                y: f64,
            }
        "#;
        let file_id = FileId::new(1).unwrap();
        
        let symbols = parser.parse(code, file_id);
        
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name.as_ref(), "Point");
        assert_eq!(symbols[0].kind, SymbolKind::Struct);
    }
    
    #[test]
    fn test_parse_multiple_items() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            fn helper() {}
            
            struct Data {
                value: i32,
            }
            
            fn process(d: Data) -> i32 {
                d.value
            }
            
            trait Operation {
                fn execute(&self);
            }
        "#;
        let file_id = FileId::new(1).unwrap();
        
        let symbols = parser.parse(code, file_id);
        
        assert_eq!(symbols.len(), 4);
        
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(names.contains(&"helper"));
        assert!(names.contains(&"Data"));
        assert!(names.contains(&"process"));
        assert!(names.contains(&"Operation"));
        
        let functions: Vec<_> = symbols.iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(functions.len(), 2);
    }
    
    #[test]
    fn test_find_function_calls() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            fn helper(x: i32) -> i32 {
                x * 2
            }
            
            fn process(x: i32) -> i32 {
                helper(x) + 1
            }
            
            fn main() {
                let result = process(42);
                let doubled = helper(result);
            }
        "#;
        
        let calls = parser.find_calls(code);
        
        // Should find: process->helper, main->process, main->helper
        assert!(calls.len() >= 3);
        
        // Check that main calls process
        let process_call = calls.iter().find(|(caller, target, _)| 
            caller == "main" && target == "process"
        ).unwrap();
        assert_eq!(process_call.0, "main");
        assert_eq!(process_call.1, "process");
        
        // Check that process calls helper
        let helper_call = calls.iter().find(|(caller, target, _)| 
            caller == "process" && target == "helper"
        ).unwrap();
        assert_eq!(helper_call.0, "process");
        assert_eq!(helper_call.1, "helper");
    }
    
    #[test]
    fn test_parse_test_fixture() {
        let mut parser = RustParser::new().unwrap();
        let code = std::fs::read_to_string("tests/fixtures/simple.rs").unwrap();
        let file_id = FileId::new(1).unwrap();
        
        let symbols = parser.parse(&code, file_id);
        
        // Should find: add, multiply, Point, Point::new, Point::distance, 
        // Rectangle, Rectangle::width, Rectangle::height, Rectangle::area
        assert!(symbols.len() >= 4); // At least the top-level items
        
        let function_names: Vec<&str> = symbols.iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .map(|s| s.name.as_ref())
            .collect();
        
        assert!(function_names.contains(&"add"));
        assert!(function_names.contains(&"multiply"));
    }
}