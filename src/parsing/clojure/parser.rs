//! Clojure language parser implementation
//!
//! This parser provides Clojure language support for the codebase intelligence system.
//! It extracts symbols, relationships, and documentation from Clojure source code using
//! tree-sitter for AST parsing.
//!
//! ## Supported Forms
//!
//! | Form | SymbolKind |
//! |------|------------|
//! | defn/defn- | Function |
//! | def | Variable |
//! | defmacro | Macro |
//! | defprotocol | Interface |
//! | defrecord/deftype | Struct |
//! | defmulti | Function |
//! | defmethod | Method |
//! | ns | Module |

use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Import, Language, LanguageParser, NodeTracker, NodeTrackingState, ParserContext,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use std::collections::HashSet;
use thiserror::Error;
use tree_sitter::{Node, Parser, Tree};

/// Clojure-specific parsing errors
#[derive(Error, Debug)]
pub enum ClojureParseError {
    #[error(
        "Failed to initialize Clojure parser: {reason}\nSuggestion: Ensure tree-sitter-clojure is properly installed"
    )]
    ParserInitFailed { reason: String },

    #[error("Failed to parse code")]
    ParseFailure,
}

/// Clojure language parser
pub struct ClojureParser {
    parser: Parser,
    tree: Option<Tree>,
    context: ParserContext,
    node_tracking: NodeTrackingState,
    /// Current namespace being parsed (Clojure-specific)
    current_namespace: Option<String>,
}

impl std::fmt::Debug for ClojureParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClojureParser")
            .field("language", &"Clojure")
            .finish()
    }
}

impl ClojureParser {
    /// Create a new Clojure parser instance
    pub fn new() -> Result<Self, ClojureParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_clojure_orchard::LANGUAGE.into())
            .map_err(|e| ClojureParseError::ParserInitFailed {
                reason: format!("tree-sitter error: {e}"),
            })?;

        Ok(Self {
            parser,
            tree: None,
            context: ParserContext::new(),
            node_tracking: NodeTrackingState::new(),
            current_namespace: None,
        })
    }

    /// Convert a tree-sitter Node to a Range
    fn range_from_node(node: &Node) -> Range {
        Range::new(
            node.start_position().row as u32,
            node.start_position().column as u16,
            node.end_position().row as u32,
            node.end_position().column as u16,
        )
    }

    /// Extract symbols from AST node recursively
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

        self.register_handled_node(node.kind(), node.kind_id());

        match node.kind() {
            "list_lit" => {
                // Check if this is a definition form
                if let Some(first_child) = node.named_child(0) {
                    if first_child.kind() == "sym_lit" {
                        let form_name = &code[first_child.byte_range()];
                        match form_name {
                            "defn" | "defn-" => {
                                self.process_defn(
                                    node,
                                    code,
                                    file_id,
                                    symbols,
                                    counter,
                                    form_name == "defn-",
                                );
                            }
                            "def" => {
                                self.process_def(node, code, file_id, symbols, counter);
                            }
                            "defmacro" => {
                                self.process_defmacro(node, code, file_id, symbols, counter);
                            }
                            "defprotocol" => {
                                self.process_defprotocol(node, code, file_id, symbols, counter);
                            }
                            "defrecord" | "deftype" => {
                                self.process_defrecord(node, code, file_id, symbols, counter);
                            }
                            "defmulti" => {
                                self.process_defmulti(node, code, file_id, symbols, counter);
                            }
                            "defmethod" => {
                                self.process_defmethod(node, code, file_id, symbols, counter);
                            }
                            "ns" => {
                                self.process_ns(node, code, file_id, symbols, counter);
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
        }
    }

    /// Process (defn name [params] body) or (defn- name [params] body)
    fn process_defn(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        is_private: bool,
    ) {
        // Structure: (defn name docstring? attr-map? [params] body*)
        // Child 0: defn symbol
        // Child 1: function name
        // Child 2+: docstring, metadata, params, body

        let mut name_node = None;
        let mut doc_string = None;
        let mut params_node = None;

        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        for (idx, child) in children.iter().enumerate() {
            match child.kind() {
                "sym_lit" if idx == 1 => {
                    name_node = Some(*child);
                }
                "str_lit" if name_node.is_some() && doc_string.is_none() => {
                    // Docstring comes after name
                    let raw = &code[child.byte_range()];
                    doc_string = Some(raw.trim_matches('"').to_string());
                }
                "vec_lit" if params_node.is_none() => {
                    params_node = Some(*child);
                }
                _ => {}
            }
        }

        if let Some(name) = name_node {
            let fn_name = &code[name.byte_range()];
            let visibility = if is_private || fn_name.starts_with('-') {
                Visibility::Private
            } else {
                Visibility::Public
            };

            // Build signature
            let signature = if let Some(params) = params_node {
                let params_str = &code[params.byte_range()];
                format!("(defn {fn_name} {params_str} ...)")
            } else {
                format!("(defn {fn_name} ...)")
            };

            let mut symbol = Symbol::new(
                counter.next_id(),
                fn_name.to_string(),
                SymbolKind::Function,
                file_id,
                Self::range_from_node(&node),
            );
            symbol.signature = Some(signature.into());
            symbol.doc_comment = doc_string.map(|s| s.into());
            symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
            symbol.visibility = visibility;

            symbols.push(symbol);
        }
    }

    /// Process (def name value) or (def name "doc" value)
    fn process_def(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        if children.len() < 2 {
            return;
        }

        let name_node = children.get(1);

        if let Some(name_node) = name_node {
            if name_node.kind() == "sym_lit" {
                let var_name = &code[name_node.byte_range()];
                let visibility = if var_name.starts_with('-') {
                    Visibility::Private
                } else {
                    Visibility::Public
                };

                // Check for docstring
                let doc_string = children.get(2).and_then(|c| {
                    if c.kind() == "str_lit" {
                        Some(code[c.byte_range()].trim_matches('"').to_string())
                    } else {
                        None
                    }
                });

                let signature = format!("(def {var_name} ...)");

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    var_name.to_string(),
                    SymbolKind::Variable,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature = Some(signature.into());
                symbol.doc_comment = doc_string.map(|s| s.into());
                symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
                symbol.visibility = visibility;

                symbols.push(symbol);
            }
        }
    }

    /// Process (defmacro name [params] body)
    fn process_defmacro(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        if let Some(name_node) = children.get(1) {
            if name_node.kind() == "sym_lit" {
                let macro_name = &code[name_node.byte_range()];

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    macro_name.to_string(),
                    SymbolKind::Macro,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature = Some(format!("(defmacro {macro_name} ...)").into());
                symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }
    }

    /// Process (defprotocol Name (method [args]) ...)
    fn process_defprotocol(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        if let Some(name_node) = children.get(1) {
            if name_node.kind() == "sym_lit" {
                let protocol_name = &code[name_node.byte_range()];

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    protocol_name.to_string(),
                    SymbolKind::Interface,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature = Some(format!("(defprotocol {protocol_name} ...)").into());
                symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }
    }

    /// Process (defrecord Name [fields]) or (deftype Name [fields])
    fn process_defrecord(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        if let Some(name_node) = children.get(1) {
            if name_node.kind() == "sym_lit" {
                let record_name = &code[name_node.byte_range()];

                let signature = code[node.byte_range()]
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    record_name.to_string(),
                    SymbolKind::Struct,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature = Some(signature.into());
                symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }
    }

    /// Process (defmulti name dispatch-fn)
    fn process_defmulti(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        if let Some(name_node) = children.get(1) {
            if name_node.kind() == "sym_lit" {
                let multi_name = &code[name_node.byte_range()];

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    multi_name.to_string(),
                    SymbolKind::Function,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature = Some(format!("(defmulti {multi_name} ...)").into());
                symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }
    }

    /// Process (defmethod multi-name dispatch-val [params] body)
    fn process_defmethod(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        // Child 0: defmethod
        // Child 1: multimethod name
        // Child 2: dispatch value
        if children.len() >= 3 {
            let name_node = &children[1];
            let dispatch_node = &children[2];

            if name_node.kind() == "sym_lit" {
                let multi_name = &code[name_node.byte_range()];
                let dispatch_val = &code[dispatch_node.byte_range()];
                let method_name = format!("{multi_name} {dispatch_val}");

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    method_name,
                    SymbolKind::Method,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature =
                    Some(format!("(defmethod {multi_name} {dispatch_val} ...)").into());
                symbol.module_path = self.current_namespace.as_ref().map(|s| s.clone().into());
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }
    }

    /// Process (ns namespace.name (:require ...) (:import ...))
    fn process_ns(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        if let Some(name_node) = children.get(1) {
            if name_node.kind() == "sym_lit" {
                let ns_name = &code[name_node.byte_range()];
                self.current_namespace = Some(ns_name.to_string());

                let mut symbol = Symbol::new(
                    counter.next_id(),
                    ns_name.to_string(),
                    SymbolKind::Module,
                    file_id,
                    Self::range_from_node(&node),
                );
                symbol.signature = Some(format!("(ns {ns_name} ...)").into());
                symbol.visibility = Visibility::Public;

                symbols.push(symbol);
            }
        }
    }

    /// Extract function calls from code
    fn extract_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let mut calls = Vec::new();

        if let Some(tree) = &self.tree {
            self.extract_calls_from_node(tree.root_node(), code, &mut calls);
        }

        calls
    }

    fn extract_calls_from_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if node.kind() == "list_lit" {
            // First child of a list is typically the function being called
            if let Some(first) = node.named_child(0) {
                if first.kind() == "sym_lit" {
                    let callee = &code[first.byte_range()];
                    // Skip special forms
                    if !matches!(
                        callee,
                        "defn" | "defn-"
                            | "def"
                            | "defmacro"
                            | "defprotocol"
                            | "defrecord"
                            | "deftype"
                            | "defmulti"
                            | "defmethod"
                            | "ns"
                            | "if"
                            | "let"
                            | "do"
                            | "fn"
                            | "loop"
                            | "recur"
                            | "try"
                            | "catch"
                            | "finally"
                            | "throw"
                            | "quote"
                            | "require"
                            | "import"
                            | "use"
                    ) {
                        // Get caller from context (current function)
                        let caller = "<module>"; // Placeholder - real impl tracks context
                        calls.push((caller, callee, Self::range_from_node(&node)));
                    }
                }
            }
        }

        // Recurse
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.extract_calls_from_node(child, code, calls);
        }
    }

    /// Extract require/use/import statements
    fn extract_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let mut imports = Vec::new();

        if let Some(tree) = &self.tree {
            self.extract_imports_from_node(tree.root_node(), code, file_id, &mut imports);
        }

        imports
    }

    fn extract_imports_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if node.kind() == "list_lit" {
            if let Some(first) = node.named_child(0) {
                if first.kind() == "kwd_lit" || first.kind() == "sym_lit" {
                    let form = &code[first.byte_range()];
                    if form == ":require" || form == "require" {
                        // Parse require clauses
                        let mut cursor = node.walk();
                        for child in node.named_children(&mut cursor).skip(1) {
                            self.parse_require_clause(child, code, file_id, imports);
                        }
                    }
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.extract_imports_from_node(child, code, file_id, imports);
        }
    }

    fn parse_require_clause(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "sym_lit" => {
                // Simple require: clojure.string
                let ns = &code[node.byte_range()];
                imports.push(Import {
                    path: ns.to_string(),
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
            "vec_lit" => {
                // Vector form: [clojure.string :as str] or [clojure.string :refer [join]]
                let mut ns_name = None;
                let mut alias = None;
                let mut is_refer_all = false;

                let mut cursor = node.walk();
                let children: Vec<_> = node.named_children(&mut cursor).collect();

                let mut i = 0;
                while i < children.len() {
                    let child = &children[i];
                    let text = &code[child.byte_range()];

                    match text {
                        ":as" => {
                            if let Some(alias_node) = children.get(i + 1) {
                                alias = Some(code[alias_node.byte_range()].to_string());
                            }
                            i += 1;
                        }
                        ":refer" => {
                            if let Some(refer_node) = children.get(i + 1) {
                                if &code[refer_node.byte_range()] == ":all" {
                                    is_refer_all = true;
                                }
                            }
                            i += 1;
                        }
                        _ if child.kind() == "sym_lit" && ns_name.is_none() => {
                            ns_name = Some(text.to_string());
                        }
                        _ => {}
                    }
                    i += 1;
                }

                if let Some(ns) = ns_name {
                    imports.push(Import {
                        path: ns,
                        alias,
                        file_id,
                        is_glob: is_refer_all,
                        is_type_only: false,
                    });
                }
            }
            _ => {}
        }
    }

}

impl LanguageParser for ClojureParser {
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        self.context = ParserContext::new();
        self.current_namespace = None;

        let tree = self.parser.parse(code, None);
        self.tree = tree.clone();

        let mut symbols = Vec::new();

        if let Some(tree) = tree {
            self.extract_symbols_from_node(
                tree.root_node(),
                code,
                file_id,
                &mut symbols,
                symbol_counter,
                0,
            );
        }

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        // In Clojure, docstrings are inside the form, not before
        // Check for comment nodes above
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let comment = &code[prev.byte_range()];
                return Some(comment.trim_start_matches(';').trim().to_string());
            }
        }
        None
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        self.tree = self.parser.parse(code, None);
        self.extract_calls(code)
    }

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Clojure protocols can have implementations via extend-type, extend-protocol
        // This would require more complex parsing
        Vec::new()
    }

    fn find_uses<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_defines<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        Vec::new()
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        self.tree = self.parser.parse(code, None);
        self.extract_imports(code, file_id)
    }

    fn language(&self) -> Language {
        Language::Clojure
    }
}

impl NodeTracker for ClojureParser {
    fn get_handled_nodes(&self) -> &HashSet<HandledNode> {
        self.node_tracking.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracking.register_handled_node(node_kind, node_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let parser = ClojureParser::new();
        assert!(parser.is_ok());
    }

    #[test]
    fn test_parse_defn() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(defn greet
  "Greets a person by name"
  [name]
  (str "Hello, " name "!"))
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(!symbols.is_empty());
        let func = symbols.iter().find(|s| s.name.as_ref() == "greet");
        assert!(func.is_some());
        let func = func.unwrap();
        assert_eq!(func.kind, SymbolKind::Function);
        assert_eq!(func.visibility, Visibility::Public);
    }

    #[test]
    fn test_parse_defn_private() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(defn- helper-fn [x] (* x 2))
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let func = symbols.iter().find(|s| s.name.as_ref() == "helper-fn");
        assert!(func.is_some());
        let func = func.unwrap();
        assert_eq!(func.kind, SymbolKind::Function);
        assert_eq!(func.visibility, Visibility::Private);
    }

    #[test]
    fn test_parse_def() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(def my-var 42)
(def pi 3.14159)
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(symbols.iter().any(|s| s.name.as_ref() == "my-var"));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "pi"));
    }

    #[test]
    fn test_parse_defmacro() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(defmacro when-let+
  [bindings & body]
  `(when-let ~bindings ~@body))
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let macro_sym = symbols.iter().find(|s| s.name.as_ref() == "when-let+");
        assert!(macro_sym.is_some());
        assert_eq!(macro_sym.unwrap().kind, SymbolKind::Macro);
    }

    #[test]
    fn test_parse_defprotocol() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(defprotocol IAnimal
  (speak [this])
  (move [this]))
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let protocol = symbols.iter().find(|s| s.name.as_ref() == "IAnimal");
        assert!(protocol.is_some());
        assert_eq!(protocol.unwrap().kind, SymbolKind::Interface);
    }

    #[test]
    fn test_parse_defrecord() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(defrecord User [id name email])
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let record = symbols.iter().find(|s| s.name.as_ref() == "User");
        assert!(record.is_some());
        assert_eq!(record.unwrap().kind, SymbolKind::Struct);
    }

    #[test]
    fn test_parse_defmulti_defmethod() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(defmulti area :shape)

(defmethod area :circle [{:keys [radius]}]
  (* 3.14159 radius radius))

(defmethod area :rectangle [{:keys [width height]}]
  (* width height))
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        // Check defmulti
        let multi = symbols.iter().find(|s| s.name.as_ref() == "area");
        assert!(multi.is_some());
        assert_eq!(multi.unwrap().kind, SymbolKind::Function);

        // Check defmethods
        let method_circle = symbols
            .iter()
            .find(|s| s.name.as_ref() == "area :circle");
        assert!(method_circle.is_some());
        assert_eq!(method_circle.unwrap().kind, SymbolKind::Method);
    }

    #[test]
    fn test_parse_ns() {
        let mut parser = ClojureParser::new().unwrap();
        let code = r#"
(ns my.app.core
  (:require [clojure.string :as str]))

(defn main [] (println "Hello"))
"#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        let ns = symbols.iter().find(|s| s.name.as_ref() == "my.app.core");
        assert!(ns.is_some());
        assert_eq!(ns.unwrap().kind, SymbolKind::Module);

        // The function should have the module path set
        let func = symbols.iter().find(|s| s.name.as_ref() == "main");
        assert!(func.is_some());
        assert_eq!(
            func.unwrap().module_path.as_ref().map(|s| s.as_ref()),
            Some("my.app.core")
        );
    }
}
