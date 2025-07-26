//! A sample code analyzer module for testing embeddings

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Function,
    Struct,
    Trait,
    Impl,
    Const,
    Type,
}

pub struct CodeAnalyzer {
    symbols: HashMap<String, Vec<Symbol>>,
}

impl CodeAnalyzer {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }
    
    pub fn analyze_function(&mut self, name: &str, line: usize, column: usize) {
        let symbol = Symbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            line,
            column,
        };
        
        self.symbols
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(symbol);
    }
    
    pub fn analyze_struct(&mut self, name: &str, line: usize, column: usize) {
        let symbol = Symbol {
            name: name.to_string(),
            kind: SymbolKind::Struct,
            line,
            column,
        };
        
        self.symbols
            .entry(name.to_string())
            .or_insert_with(Vec::new)
            .push(symbol);
    }
    
    pub fn find_symbol(&self, name: &str) -> Option<&Vec<Symbol>> {
        self.symbols.get(name)
    }
    
    pub fn get_all_symbols(&self) -> Vec<&Symbol> {
        self.symbols.values().flatten().collect()
    }
    
    pub fn count_by_kind(&self, kind: SymbolKind) -> usize {
        self.symbols
            .values()
            .flatten()
            .filter(|s| s.kind == kind)
            .count()
    }
}

impl Default for CodeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn analyze_code(code: &str) -> CodeAnalyzer {
    let mut analyzer = CodeAnalyzer::new();
    
    // Simple heuristic analysis
    for (line_num, line) in code.lines().enumerate() {
        let trimmed = line.trim();
        
        if trimmed.starts_with("fn ") {
            if let Some(name) = extract_function_name(trimmed) {
                analyzer.analyze_function(name, line_num + 1, 0);
            }
        } else if trimmed.starts_with("struct ") {
            if let Some(name) = extract_struct_name(trimmed) {
                analyzer.analyze_struct(name, line_num + 1, 0);
            }
        }
    }
    
    analyzer
}

fn extract_function_name(line: &str) -> Option<&str> {
    line.strip_prefix("fn ")
        .and_then(|s| s.split('(').next())
        .map(|s| s.trim())
}

fn extract_struct_name(line: &str) -> Option<&str> {
    line.strip_prefix("struct ")
        .and_then(|s| s.split(|c: char| c == ' ' || c == '{').next())
        .map(|s| s.trim())
}