use crate::{Symbol, SymbolId, SymbolKind, FileId, Range};
use crate::parsing::{Language, LanguageParser};
use crate::indexing::Import;
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
    
    /// Extract import statements from the code
    pub fn extract_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut imports = Vec::new();
        
        self.extract_imports_from_node(root_node, code, file_id, &mut imports);
        
        imports
    }
    
    fn extract_imports_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "use_declaration" => {
                // Extract the use path
                if let Some(use_tree) = node.children(&mut node.walk()).find(|n| n.kind() == "use_tree") {
                    self.extract_use_tree(use_tree, code, file_id, String::new(), imports);
                }
            }
            _ => {
                // Recursively check children
                for child in node.children(&mut node.walk()) {
                    self.extract_imports_from_node(child, code, file_id, imports);
                }
            }
        }
    }
    
    fn extract_use_tree(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        prefix: String,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "use_tree" => {
                // Handle different use patterns
                let mut path = prefix.clone();
                
                for child in node.children(&mut node.walk()) {
                    match child.kind() {
                        "identifier" | "scoped_identifier" => {
                            let segment = &code[child.byte_range()];
                            if !path.is_empty() {
                                path.push_str("::");
                            }
                            path.push_str(segment);
                        }
                        "use_as_clause" => {
                            // Handle "as" aliases
                            if let Some(alias_node) = child.child_by_field_name("alias") {
                                let alias = code[alias_node.byte_range()].to_string();
                                imports.push(Import {
                                    path: path.clone(),
                                    alias: Some(alias),
                                    file_id,
                                    is_glob: false,
                                });
                                return;
                            }
                        }
                        "use_wildcard" => {
                            // Handle glob imports (use foo::*)
                            imports.push(Import {
                                path: path.clone(),
                                alias: None,
                                file_id,
                                is_glob: true,
                            });
                            return;
                        }
                        "use_list" => {
                            // Handle grouped imports (use foo::{bar, baz})
                            for list_item in child.children(&mut child.walk()) {
                                if list_item.kind() == "use_tree" {
                                    self.extract_use_tree(list_item, code, file_id, path.clone(), imports);
                                }
                            }
                            return;
                        }
                        _ => {}
                    }
                }
                
                // Simple import without alias or glob
                if !path.is_empty() && path != prefix {
                    imports.push(Import {
                        path,
                        alias: None,
                        file_id,
                        is_glob: false,
                    });
                }
            }
            _ => {}
        }
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
    
    pub fn find_implementations(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut implementations = Vec::new();
        
        self.find_implementations_in_node(root_node, code, &mut implementations);
        
        implementations
    }
    
    pub fn find_uses(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut uses = Vec::new();
        
        self.find_uses_in_node(root_node, code, &mut uses);
        
        uses
    }
    
    pub fn find_defines(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut defines = Vec::new();
        
        self.find_defines_in_node(root_node, code, &mut defines);
        
        defines
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
    
    fn find_implementations_in_node(&self, node: Node, code: &str, implementations: &mut Vec<(String, String, Range)>) {
        if node.kind() == "impl_item" {
            // Check if this is a trait implementation (has trait field)
            if let Some(trait_node) = node.child_by_field_name("trait") {
                if let Some(type_node) = node.child_by_field_name("type") {
                    let trait_name = self.extract_type_name(trait_node, code);
                    let type_name = self.extract_type_name(type_node, code);
                    
                    if let (Some(trait_name), Some(type_name)) = (trait_name, type_name) {
                        let range = Range::new(
                            node.start_position().row as u32,
                            node.start_position().column as u16,
                            node.end_position().row as u32,
                            node.end_position().column as u16,
                        );
                        implementations.push((type_name, trait_name, range));
                    }
                }
            }
        }
        
        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_implementations_in_node(child, code, implementations);
        }
    }
    
    fn extract_type_name(&self, node: Node, code: &str) -> Option<String> {
        match node.kind() {
            "type_identifier" => Some(code[node.byte_range()].to_string()),
            "primitive_type" => Some(code[node.byte_range()].to_string()),  // Added for i32, f64, etc.
            "generic_type" => {
                // For generic types like Option<T>, extract the base type
                if let Some(type_node) = node.child_by_field_name("type") {
                    self.extract_type_name(type_node, code)
                } else {
                    None
                }
            }
            "scoped_type_identifier" => {
                // For types like std::fmt::Display, get the full path
                Some(code[node.byte_range()].to_string())
            }
            _ => {
                // Try to find a type_identifier child
                for child in node.children(&mut node.walk()) {
                    if let Some(name) = self.extract_type_name(child, code) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }
    
    fn find_uses_in_node(&self, node: Node, code: &str, uses: &mut Vec<(String, String, Range)>) {
        match node.kind() {
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let struct_name = &code[name_node.byte_range()];
                    
                    // Find field list
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            if child.kind() == "field_declaration" {
                                if let Some(type_node) = child.child_by_field_name("type") {
                                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                                        let range = Range::new(
                                            type_node.start_position().row as u32,
                                            type_node.start_position().column as u16,
                                            type_node.end_position().row as u32,
                                            type_node.end_position().column as u16,
                                        );
                                        uses.push((struct_name.to_string(), type_name, range));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let fn_name = &code[name_node.byte_range()];
                    
                    // Check if this is a method (inside impl block)
                    let context_name = if let Some(parent) = node.parent() {
                        if parent.kind() == "impl_item" {
                            // Get the type being implemented
                            if let Some(type_node) = parent.child_by_field_name("type") {
                                if let Some(type_name) = self.extract_type_name(type_node, code) {
                                    format!("{}::{}", type_name, fn_name)
                                } else {
                                    fn_name.to_string()
                                }
                            } else {
                                fn_name.to_string()
                            }
                        } else {
                            fn_name.to_string()
                        }
                    } else {
                        fn_name.to_string()
                    };
                    
                    // Find parameters
                    if let Some(params) = node.child_by_field_name("parameters") {
                        for param in params.children(&mut params.walk()) {
                            if param.kind() == "parameter" {
                                if let Some(type_node) = param.child_by_field_name("type") {
                                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                                        let range = Range::new(
                                            type_node.start_position().row as u32,
                                            type_node.start_position().column as u16,
                                            type_node.end_position().row as u32,
                                            type_node.end_position().column as u16,
                                        );
                                        uses.push((context_name.clone(), type_name, range));
                                    }
                                }
                            }
                        }
                    }
                    
                    // Find return type - check the return_type field
                    if let Some(return_type_node) = node.child_by_field_name("return_type") {
                        if let Some(type_name) = self.extract_type_name(return_type_node, code) {
                            let range = Range::new(
                                return_type_node.start_position().row as u32,
                                return_type_node.start_position().column as u16,
                                return_type_node.end_position().row as u32,
                                return_type_node.end_position().column as u16,
                            );
                            uses.push((context_name, type_name, range));
                        }
                    }
                }
            }
            _ => {}
        }
        
        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_uses_in_node(child, code, uses);
        }
    }
    
    fn find_defines_in_node(&self, node: Node, code: &str, defines: &mut Vec<(String, String, Range)>) {
        match node.kind() {
            "trait_item" => {
                if let Some(trait_name_node) = node.child_by_field_name("name") {
                    let trait_name = &code[trait_name_node.byte_range()];
                    
                    // Find all methods defined in this trait
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            if child.kind() == "function_signature_item" {
                                if let Some(method_name_node) = child.child_by_field_name("name") {
                                    let method_name = &code[method_name_node.byte_range()];
                                    let range = Range::new(
                                        child.start_position().row as u32,
                                        child.start_position().column as u16,
                                        child.end_position().row as u32,
                                        child.end_position().column as u16,
                                    );
                                    defines.push((trait_name.to_string(), method_name.to_string(), range));
                                }
                            }
                        }
                    }
                }
            }
            "impl_item" => {
                // Get the type being implemented
                if let Some(type_node) = node.child_by_field_name("type") {
                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                        // Find all methods defined in this impl block
                        if let Some(body) = node.child_by_field_name("body") {
                            for child in body.children(&mut body.walk()) {
                                if child.kind() == "function_item" {
                                    if let Some(method_name_node) = child.child_by_field_name("name") {
                                        let method_name = &code[method_name_node.byte_range()];
                                        let range = Range::new(
                                            child.start_position().row as u32,
                                            child.start_position().column as u16,
                                            child.end_position().row as u32,
                                            child.end_position().column as u16,
                                        );
                                        defines.push((type_name.clone(), method_name.to_string(), range));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        
        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_defines_in_node(child, code, defines);
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
        
        // Find the parent node that might have doc comments
        let doc_node = name_node.parent()?;
        let doc_comment = self.extract_doc_comments(&doc_node, code);
        
        let mut symbol = Symbol::new(
            symbol_id,
            name,
            kind,
            file_id,
            range,
        );
        
        if let Some(doc) = doc_comment {
            symbol = symbol.with_doc(doc);
        }
        
        Some(symbol)
    }
    
    fn extract_doc_comments(&self, node: &Node, code: &str) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut current = node.prev_sibling();
        
        while let Some(sibling) = current {
            match sibling.kind() {
                "line_comment" => {
                    if let Ok(text) = sibling.utf8_text(code.as_bytes()) {
                        // Check for exactly "///" not "////"
                        if text.starts_with("///") && !text.starts_with("////") {
                            let content = text.trim_start_matches("///").trim();
                            doc_lines.push(content.to_string());
                        } else {
                            break; // Non-doc comment ends the sequence
                        }
                    }
                }
                "block_comment" => {
                    if let Ok(text) = sibling.utf8_text(code.as_bytes()) {
                        // Check for exactly "/**" not "/***" and not "/**/"
                        if text.starts_with("/**") && !text.starts_with("/***") 
                           && text != "/**/" {
                            let content = text.trim_start_matches("/**")
                                             .trim_end_matches("*/")
                                             .trim();
                            doc_lines.push(content.to_string());
                        } else {
                            break; // Non-doc comment ends the sequence
                        }
                    }
                }
                _ => break, // Non-comment node ends the sequence
            }
            current = sibling.prev_sibling();
        }
        
        if doc_lines.is_empty() {
            None
        } else {
            doc_lines.reverse(); // Restore original order
            Some(doc_lines.join("\n"))
        }
    }
}

impl LanguageParser for RustParser {
    fn parse(&mut self, code: &str, file_id: FileId) -> Vec<Symbol> {
        self.parse(code, file_id)
    }
    
    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.extract_doc_comments(node, code)
    }
    
    fn find_calls(&mut self, code: &str) -> Vec<(String, String, Range)> {
        self.find_calls(code)
    }
    
    fn find_implementations(&mut self, code: &str) -> Vec<(String, String, Range)> {
        self.find_implementations(code)
    }
    
    fn find_uses(&mut self, code: &str) -> Vec<(String, String, Range)> {
        self.find_uses(code)
    }
    
    fn find_defines(&mut self, code: &str) -> Vec<(String, String, Range)> {
        self.find_defines(code)
    }
    
    fn language(&self) -> Language {
        Language::Rust
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
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let test_file = std::path::Path::new(manifest_dir).join("tests/fixtures/simple.rs");
        let code = std::fs::read_to_string(test_file).unwrap();
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
    
    #[test]
    fn test_find_uses() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            struct Point {
                x: f64,
                y: f64,
            }
            
            struct Rectangle {
                top_left: Point,
                bottom_right: Point,
            }
            
            fn distance(p1: Point, p2: Point) -> f64 {
                ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt()
            }
            
            fn get_center(rect: Rectangle) -> Point {
                Point {
                    x: (rect.top_left.x + rect.bottom_right.x) / 2.0,
                    y: (rect.top_left.y + rect.bottom_right.y) / 2.0,
                }
            }
        "#;
        
        let uses = parser.find_uses(code);
        
        // Debug print all uses
        println!("All uses found:");
        for (user, used, _) in &uses {
            println!("  {} uses {}", user, used);
        }
        
        // Rectangle uses Point (twice)
        let rect_uses: Vec<_> = uses.iter()
            .filter(|(user, _, _)| user == "Rectangle")
            .collect();
        assert_eq!(rect_uses.len(), 2);
        assert!(rect_uses.iter().all(|(_, used, _)| used == "Point"));
        
        // distance uses Point (twice for params) and f64 (once for return)
        let distance_uses: Vec<_> = uses.iter()
            .filter(|(user, _, _)| user == "distance")
            .collect();
        
        // Check we have Point parameters and f64 return
        assert!(distance_uses.iter().filter(|(_, used, _)| used == "Point").count() >= 2);
        assert!(distance_uses.iter().filter(|(_, used, _)| used == "f64").count() >= 1);
        
        // get_center uses Rectangle and Point
        let center_uses: Vec<_> = uses.iter()
            .filter(|(user, _, _)| user == "get_center")
            .collect();
        assert_eq!(center_uses.len(), 2);
        assert!(center_uses.iter().any(|(_, used, _)| used == "Rectangle"));
        assert!(center_uses.iter().any(|(_, used, _)| used == "Point"));
    }
    
    #[test]
    fn test_find_defines() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            trait Iterator {
                type Item;
                fn next(&mut self) -> Option<Self::Item>;
                fn size_hint(&self) -> (usize, Option<usize>);
            }
            
            struct Counter {
                count: u32,
            }
            
            impl Counter {
                fn new() -> Self {
                    Self { count: 0 }
                }
                
                fn increment(&mut self) {
                    self.count += 1;
                }
            }
            
            impl Iterator for Counter {
                type Item = u32;
                
                fn next(&mut self) -> Option<Self::Item> {
                    self.count += 1;
                    Some(self.count)
                }
                
                fn size_hint(&self) -> (usize, Option<usize>) {
                    (usize::MAX, None)
                }
            }
        "#;
        
        let defines = parser.find_defines(code);
        
        // Iterator trait defines methods
        let iterator_defines: Vec<_> = defines.iter()
            .filter(|(definer, _, _)| definer == "Iterator")
            .collect();
        assert_eq!(iterator_defines.len(), 2); // next and size_hint
        assert!(iterator_defines.iter().any(|(_, defined, _)| defined == "next"));
        assert!(iterator_defines.iter().any(|(_, defined, _)| defined == "size_hint"));
        
        // Counter impl defines methods
        let counter_defines: Vec<_> = defines.iter()
            .filter(|(definer, _, _)| definer == "Counter")
            .collect();
        assert_eq!(counter_defines.len(), 4); // new, increment, next, size_hint
        assert!(counter_defines.iter().any(|(_, defined, _)| defined == "new"));
        assert!(counter_defines.iter().any(|(_, defined, _)| defined == "increment"));
        assert!(counter_defines.iter().any(|(_, defined, _)| defined == "next"));
        assert!(counter_defines.iter().any(|(_, defined, _)| defined == "size_hint"));
    }
    
    #[test]
    fn test_find_implementations() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            trait Display {
                fn fmt(&self) -> String;
            }
            
            struct Point {
                x: i32,
                y: i32,
            }
            
            impl Display for Point {
                fn fmt(&self) -> String {
                    format!("({}, {})", self.x, self.y)
                }
            }
            
            impl std::fmt::Debug for Point {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "Point({}, {})", self.x, self.y)
                }
            }
        "#;
        
        let implementations = parser.find_implementations(code);
        
        // Should find two implementations
        assert_eq!(implementations.len(), 2);
        
        // Check Point implements Display
        let display_impl = implementations.iter()
            .find(|(type_name, trait_name, _)| type_name == "Point" && trait_name == "Display")
            .expect("Should find Point implements Display");
        assert_eq!(display_impl.0, "Point");
        assert_eq!(display_impl.1, "Display");
        
        // Check Point implements std::fmt::Debug
        let debug_impl = implementations.iter()
            .find(|(type_name, trait_name, _)| type_name == "Point" && trait_name == "std::fmt::Debug")
            .expect("Should find Point implements std::fmt::Debug");
        assert_eq!(debug_impl.0, "Point");
        assert_eq!(debug_impl.1, "std::fmt::Debug");
    }
    
    #[test]
    fn test_doc_comment_extraction() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/// This is a well-documented function.
/// 
/// It has multiple lines of documentation
/// explaining what it does.
pub fn documented_function() {}

//// This is NOT a doc comment (4 slashes)
fn not_documented() {}

/** This is a block doc comment.
 * 
 * It uses the block style.
 */
pub struct DocumentedStruct {
    field: i32,
}

/*** This is NOT a doc comment (3 asterisks) ***/
fn also_not_documented() {}

/**/ // Empty 2-asterisk block is NOT a doc comment
fn edge_case() {}

/// Single line doc
fn simple_doc() {}
        "#;
        
        let file_id = FileId::new(1).unwrap();
        let symbols = parser.parse(code, file_id);
        
        // Find documented_function
        let doc_fn = symbols.iter()
            .find(|s| s.name.as_ref() == "documented_function")
            .expect("Should find documented_function");
        assert!(doc_fn.doc_comment.is_some());
        let doc = doc_fn.doc_comment.as_ref().unwrap();
        assert!(doc.contains("well-documented function"));
        assert!(doc.contains("multiple lines"));
        
        // Find not_documented - should have no docs
        let no_doc_fn = symbols.iter()
            .find(|s| s.name.as_ref() == "not_documented")
            .expect("Should find not_documented");
        assert!(no_doc_fn.doc_comment.is_none());
        
        // Find DocumentedStruct with block comment
        let doc_struct = symbols.iter()
            .find(|s| s.name.as_ref() == "DocumentedStruct")
            .expect("Should find DocumentedStruct");
        assert!(doc_struct.doc_comment.is_some());
        let struct_doc = doc_struct.doc_comment.as_ref().unwrap();
        assert!(struct_doc.contains("block doc comment"));
        assert!(struct_doc.contains("block style"));
        
        // Find also_not_documented - should have no docs (3 asterisks)
        let also_no_doc = symbols.iter()
            .find(|s| s.name.as_ref() == "also_not_documented")
            .expect("Should find also_not_documented");
        assert!(also_no_doc.doc_comment.is_none());
        
        // Find edge_case - should have no docs (empty block)
        let edge = symbols.iter()
            .find(|s| s.name.as_ref() == "edge_case")
            .expect("Should find edge_case");
        assert!(edge.doc_comment.is_none());
        
        // Find simple_doc
        let simple = symbols.iter()
            .find(|s| s.name.as_ref() == "simple_doc")
            .expect("Should find simple_doc");
        assert!(simple.doc_comment.is_some());
        assert_eq!(simple.doc_comment.as_ref().unwrap().as_ref(), "Single line doc");
    }
    
    #[test]
    fn test_doc_comment_edge_cases() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/// Line 1
/// Line 2
/// Line 3
fn multi_line_doc() {}

///Empty doc
///
///After empty line
fn empty_line_doc() {}

///Compact
///Lines
///Together
fn compact_doc() {}

/// Trim test   
fn trim_test() {}
        "#;
        
        let file_id = FileId::new(1).unwrap();
        let symbols = parser.parse(code, file_id);
        
        // Test multi-line joining
        let multi = symbols.iter()
            .find(|s| s.name.as_ref() == "multi_line_doc")
            .unwrap();
        let doc = multi.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Line 1\nLine 2\nLine 3");
        
        // Test empty line preservation
        let empty = symbols.iter()
            .find(|s| s.name.as_ref() == "empty_line_doc")
            .unwrap();
        let doc = empty.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Empty doc\n\nAfter empty line");
        
        // Test compact lines
        let compact = symbols.iter()
            .find(|s| s.name.as_ref() == "compact_doc")
            .unwrap();
        let doc = compact.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Compact\nLines\nTogether");
        
        // Test trimming
        let trim = symbols.iter()
            .find(|s| s.name.as_ref() == "trim_test")
            .unwrap();
        let doc = trim.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Trim test");
    }
}