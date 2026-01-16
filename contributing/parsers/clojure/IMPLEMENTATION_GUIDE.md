# Clojure Language Support Implementation Guide

This document provides a complete specification for implementing Clojure language support in codanna. It follows the established patterns from existing language implementations.

## Overview

| Item | Value |
|------|-------|
| Language ID | `clojure` |
| Display Name | `Clojure` |
| Extensions | `.clj`, `.cljc`, `.cljs`, `.edn` |
| Tree-sitter Crate | `tree-sitter-clojure` |
| Module Separator | `.` (for namespaces) or `/` (for var references) |
| Default Enabled | `true` |

## Files to Create

```
src/parsing/clojure/
├── mod.rs          # Module exports and register() function
├── definition.rs   # LanguageDefinition implementation
├── parser.rs       # LanguageParser implementation
├── behavior.rs     # LanguageBehavior implementation
├── resolution.rs   # ClojureResolutionContext
└── audit.rs        # ABI-15 coverage tracking
```

---

## 1. Cargo.toml Addition

Add to `[dependencies]` section:

```toml
tree-sitter-clojure = "0.0.11"  # Check crates.io for latest version
```

---

## 2. mod.rs

```rust
//! Clojure language parser implementation

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::ClojureBehavior;
pub use definition::ClojureLanguage;
pub use parser::ClojureParser;
pub use resolution::ClojureResolutionContext;

// Re-export for registry registration
pub(crate) use definition::register;
```

---

## 3. definition.rs

```rust
//! Clojure language definition for the registry

use std::sync::Arc;

use super::{ClojureBehavior, ClojureParser};
use crate::parsing::{LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser};
use crate::{IndexResult, Settings};

/// Clojure language definition
pub struct ClojureLanguage;

impl ClojureLanguage {
    /// Language identifier constant
    pub const ID: LanguageId = LanguageId::new("clojure");
}

impl LanguageDefinition for ClojureLanguage {
    fn id(&self) -> LanguageId {
        Self::ID
    }

    fn name(&self) -> &'static str {
        "Clojure"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["clj", "cljc", "cljs", "edn"]
    }

    fn create_parser(&self, _settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let parser = ClojureParser::new()
            .map_err(|e| crate::IndexError::General(e.to_string()))?;
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(ClojureBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get(self.id().as_str())
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register Clojure language with the global registry
pub(crate) fn register(registry: &mut crate::parsing::LanguageRegistry) {
    registry.register(Arc::new(ClojureLanguage));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clojure_definition() {
        let clojure = ClojureLanguage;

        assert_eq!(clojure.id(), LanguageId::new("clojure"));
        assert_eq!(clojure.name(), "Clojure");
        assert!(clojure.extensions().contains(&"clj"));
        assert!(clojure.extensions().contains(&"cljs"));
        assert!(clojure.extensions().contains(&"cljc"));
        assert!(clojure.extensions().contains(&"edn"));
    }

    #[test]
    fn test_clojure_enabled_by_default() {
        let clojure = ClojureLanguage;
        let settings = Settings::default();

        // Should be enabled by default
        assert!(clojure.default_enabled());
    }
}
```

---

## 4. parser.rs - Symbol Extraction

### Clojure AST Node Types (Tree-sitter)

The tree-sitter-clojure grammar produces these key node types:

| Node Type | Clojure Form | Symbol Kind |
|-----------|--------------|-------------|
| `list` | `(defn ...)` | Container for definitions |
| `symbol` | `defn`, `my-fn` | Function/var names |
| `keyword` | `:keyword` | Keywords (not symbols) |
| `vector` | `[a b c]` | Parameter lists |
| `map` | `{:a 1}` | Maps |
| `string` | `"doc"` | Docstrings |
| `metadata` | `^:private` | Visibility metadata |

### Clojure Definition Forms to Extract

| Form | Symbol Kind | Example |
|------|-------------|---------|
| `defn` | Function | `(defn my-fn [x] ...)` |
| `defn-` | Function (private) | `(defn- private-fn [x] ...)` |
| `def` | Variable | `(def my-var 42)` |
| `defmacro` | Macro | `(defmacro when-let ...)` |
| `defprotocol` | Interface | `(defprotocol IMyProtocol ...)` |
| `defrecord` | Struct | `(defrecord MyRecord [a b])` |
| `deftype` | Struct | `(deftype MyType [a b])` |
| `defmulti` | Function | `(defmulti dispatch-fn ...)` |
| `defmethod` | Method | `(defmethod dispatch-fn :key ...)` |
| `ns` | Module | `(ns my.namespace ...)` |

### Parser Structure

```rust
//! Clojure language parser

use crate::parsing::parser::{
    check_recursion_depth, HandledNode, LanguageParser, NodeTracker, NodeTrackingState,
};
use crate::parsing::{Import, Language, ParserContext};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use std::collections::HashSet;
use tree_sitter::{Node, Parser, Tree};

pub struct ClojureParser {
    parser: Parser,
    tree: Option<Tree>,
    context: ParserContext,
    node_tracking: NodeTrackingState,
}

impl ClojureParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_clojure::LANGUAGE.into())
            .map_err(|e| format!("Failed to set Clojure language: {e}"))?;

        Ok(Self {
            parser,
            tree: None,
            context: ParserContext::new(),
            node_tracking: NodeTrackingState::new(),
        })
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
        if !check_recursion_depth(depth, node) {
            return;
        }

        self.register_handled_node(node.kind(), node.kind_id());

        match node.kind() {
            "list" => {
                // Check if this is a definition form
                if let Some(first_child) = node.child(0) {
                    if first_child.kind() == "symbol" {
                        let form_name = &code[first_child.byte_range()];
                        match form_name {
                            "defn" | "defn-" => {
                                self.process_defn(node, code, file_id, symbols, counter, form_name == "defn-");
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
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
            }
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

        let mut idx = 1; // Skip 'defn'
        while let Some(child) = node.child(idx) {
            match child.kind() {
                "symbol" if name_node.is_none() => {
                    name_node = Some(child);
                }
                "string" if doc_string.is_none() && name_node.is_some() => {
                    // Docstring comes after name
                    let raw = &code[child.byte_range()];
                    doc_string = Some(raw.trim_matches('"').to_string());
                }
                "vector" if params_node.is_none() => {
                    params_node = Some(child);
                }
                _ => {}
            }
            idx += 1;
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

            let symbol = Symbol {
                id: counter.next_id(),
                name: fn_name.into(),
                kind: SymbolKind::Function,
                file_id,
                range: Range::from_node(&node),
                signature: Some(signature.into()),
                doc_comment: doc_string.map(|s| s.into()),
                module_path: self.context.current_module().map(|s| s.into()),
                visibility,
                scope_context: None,
            };
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
        // Child 0: def
        // Child 1: name
        // Child 2: optional docstring or value
        // Child 3: value (if docstring present)

        if let Some(name_node) = node.child(1) {
            if name_node.kind() == "symbol" {
                let var_name = &code[name_node.byte_range()];
                let visibility = if var_name.starts_with('-') || var_name.starts_with("^:private") {
                    Visibility::Private
                } else {
                    Visibility::Public
                };

                // Check for docstring
                let doc_string = node.child(2).and_then(|c| {
                    if c.kind() == "string" {
                        Some(code[c.byte_range()].trim_matches('"').to_string())
                    } else {
                        None
                    }
                });

                let signature = format!("(def {var_name} ...)");

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: var_name.into(),
                    kind: SymbolKind::Variable,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(signature.into()),
                    doc_comment: doc_string.map(|s| s.into()),
                    module_path: self.context.current_module().map(|s| s.into()),
                    visibility,
                    scope_context: None,
                };
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
        // Similar to defn but SymbolKind::Macro
        if let Some(name_node) = node.child(1) {
            if name_node.kind() == "symbol" {
                let macro_name = &code[name_node.byte_range()];

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: macro_name.into(),
                    kind: SymbolKind::Macro,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(format!("(defmacro {macro_name} ...)").into()),
                    doc_comment: None,
                    module_path: self.context.current_module().map(|s| s.into()),
                    visibility: Visibility::Public,
                    scope_context: None,
                };
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
        if let Some(name_node) = node.child(1) {
            if name_node.kind() == "symbol" {
                let protocol_name = &code[name_node.byte_range()];

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: protocol_name.into(),
                    kind: SymbolKind::Interface,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(format!("(defprotocol {protocol_name} ...)").into()),
                    doc_comment: None,
                    module_path: self.context.current_module().map(|s| s.into()),
                    visibility: Visibility::Public,
                    scope_context: None,
                };
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
        if let Some(name_node) = node.child(1) {
            if name_node.kind() == "symbol" {
                let record_name = &code[name_node.byte_range()];

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: record_name.into(),
                    kind: SymbolKind::Struct,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(code[node.byte_range()].lines().next().unwrap_or("").into()),
                    doc_comment: None,
                    module_path: self.context.current_module().map(|s| s.into()),
                    visibility: Visibility::Public,
                    scope_context: None,
                };
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
        if let Some(name_node) = node.child(1) {
            if name_node.kind() == "symbol" {
                let multi_name = &code[name_node.byte_range()];

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: multi_name.into(),
                    kind: SymbolKind::Function,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(format!("(defmulti {multi_name} ...)").into()),
                    doc_comment: None,
                    module_path: self.context.current_module().map(|s| s.into()),
                    visibility: Visibility::Public,
                    scope_context: None,
                };
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
        // Child 0: defmethod
        // Child 1: multimethod name
        // Child 2: dispatch value
        if let (Some(name_node), Some(dispatch_node)) = (node.child(1), node.child(2)) {
            if name_node.kind() == "symbol" {
                let multi_name = &code[name_node.byte_range()];
                let dispatch_val = &code[dispatch_node.byte_range()];
                let method_name = format!("{multi_name} {dispatch_val}");

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: method_name.into(),
                    kind: SymbolKind::Method,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(format!("(defmethod {multi_name} {dispatch_val} ...)").into()),
                    doc_comment: None,
                    module_path: self.context.current_module().map(|s| s.into()),
                    visibility: Visibility::Public,
                    scope_context: None,
                };
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
        if let Some(name_node) = node.child(1) {
            if name_node.kind() == "symbol" {
                let ns_name = &code[name_node.byte_range()];
                self.context.set_current_module(ns_name);

                let symbol = Symbol {
                    id: counter.next_id(),
                    name: ns_name.into(),
                    kind: SymbolKind::Module,
                    file_id,
                    range: Range::from_node(&node),
                    signature: Some(format!("(ns {ns_name} ...)").into()),
                    doc_comment: None,
                    module_path: None, // ns IS the module path
                    visibility: Visibility::Public,
                    scope_context: None,
                };
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
        if node.kind() == "list" {
            // First child of a list is typically the function being called
            if let Some(first) = node.child(0) {
                if first.kind() == "symbol" {
                    let callee = &code[first.byte_range()];
                    // Skip special forms
                    if !matches!(callee, "defn" | "defn-" | "def" | "defmacro" | "defprotocol"
                                        | "defrecord" | "deftype" | "defmulti" | "defmethod"
                                        | "ns" | "if" | "let" | "do" | "fn" | "loop" | "recur"
                                        | "try" | "catch" | "finally" | "throw" | "quote"
                                        | "require" | "import" | "use") {
                        // Get caller from context (current function)
                        let caller = "<module>"; // Placeholder - real impl tracks context
                        calls.push((caller, callee, Range::from_node(&node)));
                    }
                }
            }
        }

        // Recurse
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.extract_calls_from_node(child, code, calls);
            }
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
        if node.kind() == "list" {
            if let Some(first) = node.child(0) {
                let form = &code[first.byte_range()];
                if form == ":require" || form == "require" {
                    // Parse require clauses
                    for i in 1..node.child_count() {
                        if let Some(child) = node.child(i) {
                            self.parse_require_clause(child, code, file_id, imports);
                        }
                    }
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                self.extract_imports_from_node(child, code, file_id, imports);
            }
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
            "symbol" => {
                // Simple require: [clojure.string]
                let ns = &code[node.byte_range()];
                imports.push(Import {
                    path: ns.to_string(),
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
            "vector" => {
                // Vector form: [clojure.string :as str] or [clojure.string :refer [join]]
                let mut ns_name = None;
                let mut alias = None;
                let mut is_refer_all = false;

                let mut i = 0;
                while let Some(child) = node.child(i) {
                    let text = &code[child.byte_range()];
                    match text {
                        ":as" => {
                            if let Some(alias_node) = node.child(i + 1) {
                                alias = Some(code[alias_node.byte_range()].to_string());
                            }
                            i += 1;
                        }
                        ":refer" => {
                            if let Some(refer_node) = node.child(i + 1) {
                                if &code[refer_node.byte_range()] == ":all" {
                                    is_refer_all = true;
                                }
                            }
                            i += 1;
                        }
                        _ if child.kind() == "symbol" && ns_name.is_none() => {
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
        self.context.reset();

        self.tree = self.parser.parse(code, None);

        let mut symbols = Vec::new();

        if let Some(tree) = &self.tree {
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
        Language::Clojure // Need to add this variant
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
```

---

## 5. behavior.rs

```rust
//! Clojure language behavior implementation

use crate::parsing::registry::LanguageId;
use crate::parsing::{LanguageBehavior, LanguageMetadata};
use crate::Visibility;
use std::path::Path;
use tree_sitter::Language;

pub struct ClojureBehavior;

impl ClojureBehavior {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClojureBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageBehavior for ClojureBehavior {
    fn language_id(&self) -> LanguageId {
        LanguageId::new("clojure")
    }

    fn format_module_path(&self, base_path: &str, symbol_name: &str) -> String {
        if base_path.is_empty() {
            symbol_name.to_string()
        } else {
            format!("{base_path}/{symbol_name}")
        }
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // Check for private indicators
        if signature.contains("defn-")
            || signature.contains("^:private")
            || signature.contains("^{:private true}") {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn module_separator(&self) -> &'static str {
        "."  // Clojure uses dots for namespaces
    }

    fn supports_traits(&self) -> bool {
        true  // Clojure has protocols
    }

    fn supports_inherent_methods(&self) -> bool {
        false  // Clojure doesn't have inherent methods
    }

    fn get_language(&self) -> Language {
        tree_sitter_clojure::LANGUAGE.into()
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Convert src/my/namespace/core.clj -> my.namespace.core
        let relative = file_path.strip_prefix(project_root).ok()?;

        // Remove src/ prefix if present
        let path_str = relative.to_string_lossy();
        let without_src = path_str
            .strip_prefix("src/")
            .or_else(|| path_str.strip_prefix("src\\"))
            .unwrap_or(&path_str);

        // Remove extension and convert path separators to dots
        let without_ext = without_src
            .strip_suffix(".clj")
            .or_else(|| without_src.strip_suffix(".cljc"))
            .or_else(|| without_src.strip_suffix(".cljs"))
            .unwrap_or(without_src);

        // Convert slashes to dots, underscores to hyphens
        let module_path = without_ext
            .replace('/', ".")
            .replace('\\', ".")
            .replace('_', "-");

        Some(module_path)
    }
}
```

---

## 6. resolution.rs

```rust
//! Clojure-specific symbol resolution

use crate::parsing::resolution::{
    GenericResolutionContext, ImportBinding, ImportOrigin, ResolutionScope, ScopeLevel,
};
use crate::parsing::{Import, ParserContext, ScopeType};
use crate::relationship::RelationKind;
use crate::storage::DocumentIndex;
use crate::{FileId, Symbol, SymbolId};

/// Clojure resolution context
///
/// Clojure has a simpler scoping model than OOP languages:
/// - Namespace-level vars (def, defn, defmacro)
/// - Local bindings (let, loop, fn params)
/// - Imported vars (require, use)
pub struct ClojureResolutionContext {
    inner: GenericResolutionContext,
    file_id: FileId,
}

impl ClojureResolutionContext {
    pub fn new(file_id: FileId) -> Self {
        Self {
            inner: GenericResolutionContext::new(),
            file_id,
        }
    }
}

impl ResolutionScope for ClojureResolutionContext {
    fn resolve(&self, name: &str) -> Option<SymbolId> {
        // Resolution order for Clojure:
        // 1. Local bindings (let, fn params)
        // 2. Namespace vars
        // 3. Imported/referred vars
        self.inner.resolve(name)
    }

    fn add_symbol(&mut self, name: String, symbol_id: SymbolId, scope_level: ScopeLevel) {
        self.inner.add_symbol(name, symbol_id, scope_level);
    }

    fn enter_scope(&mut self, scope_type: ScopeType) {
        self.inner.enter_scope(scope_type);
    }

    fn exit_scope(&mut self) {
        self.inner.exit_scope();
    }

    fn clear_local_scope(&mut self) {
        self.inner.clear_local_scope();
    }

    fn resolve_relationship(
        &self,
        target_name: &str,
        _context: &Symbol,
        _relation_kind: RelationKind,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Try local resolution first
        if let Some(id) = self.resolve(target_name) {
            return Some(id);
        }

        // Try qualified name lookup (namespace/var)
        if target_name.contains('/') {
            let parts: Vec<&str> = target_name.splitn(2, '/').collect();
            if parts.len() == 2 {
                let ns = parts[0];
                let var_name = parts[1];
                // Look up the fully qualified name
                return document_index
                    .find_by_name(var_name)
                    .into_iter()
                    .find(|sym| {
                        sym.module_path
                            .as_ref()
                            .map(|mp| mp.as_ref() == ns)
                            .unwrap_or(false)
                    })
                    .map(|sym| sym.id);
            }
        }

        // Fall back to global search
        document_index
            .find_by_name(target_name)
            .into_iter()
            .next()
            .map(|sym| sym.id)
    }

    fn populate_imports(&mut self, imports: &[Import]) {
        for import in imports {
            let binding = ImportBinding {
                local_name: import.alias.clone().unwrap_or_else(|| {
                    // Use last segment of path as default name
                    import.path.rsplit('.').next().unwrap_or(&import.path).to_string()
                }),
                imported_path: import.path.clone(),
                origin: ImportOrigin::Direct,
                symbol_id: None,
            };
            self.register_import_binding(binding);
        }
    }

    fn register_import_binding(&mut self, binding: ImportBinding) {
        self.inner.register_import_binding(binding);
    }
}
```

---

## 7. audit.rs

```rust
//! Clojure ABI-15 coverage tracking

use crate::parsing::parser::{HandledNode, NodeTracker};
use std::collections::HashSet;
use tree_sitter::Language;

/// Get all node types from the Clojure grammar
pub fn get_all_grammar_nodes() -> HashSet<HandledNode> {
    let language: Language = tree_sitter_clojure::LANGUAGE.into();
    let mut nodes = HashSet::new();

    // ABI-15 provides node count
    let node_count = language.node_kind_count();

    for id in 0..node_count {
        if let Some(name) = language.node_kind_for_id(id as u16) {
            // Skip anonymous nodes (operators, punctuation)
            if !language.node_kind_is_named(id as u16) {
                continue;
            }
            nodes.insert(HandledNode {
                name: name.to_string(),
                id: id as u16,
            });
        }
    }

    nodes
}

/// Generate audit report comparing handled vs available nodes
pub fn generate_audit_report(handled: &HashSet<HandledNode>) -> String {
    let all_nodes = get_all_grammar_nodes();

    let handled_names: HashSet<&str> = handled.iter().map(|n| n.name.as_str()).collect();
    let all_names: HashSet<&str> = all_nodes.iter().map(|n| n.name.as_str()).collect();

    let unhandled: Vec<_> = all_names.difference(&handled_names).collect();
    let coverage = (handled.len() as f64 / all_nodes.len() as f64) * 100.0;

    let mut report = String::new();
    report.push_str(&format!("# Clojure Parser Coverage Report\n\n"));
    report.push_str(&format!("Coverage: {:.1}% ({}/{})\n\n",
        coverage, handled.len(), all_nodes.len()));

    report.push_str("## Handled Nodes\n");
    let mut handled_list: Vec<_> = handled_names.iter().collect();
    handled_list.sort();
    for name in handled_list {
        report.push_str(&format!("- {name}\n"));
    }

    report.push_str("\n## Unhandled Nodes\n");
    let mut unhandled_sorted: Vec<_> = unhandled.iter().collect();
    unhandled_sorted.sort();
    for name in unhandled_sorted {
        report.push_str(&format!("- {name}\n"));
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::clojure::ClojureParser;

    #[test]
    fn audit_clojure_coverage() {
        let mut parser = ClojureParser::new().expect("Failed to create parser");

        // Parse comprehensive test code
        let code = r#"
(ns my.namespace
  (:require [clojure.string :as str]
            [clojure.set :refer [union]]))

(def my-var 42)

(def ^:private private-var "secret")

(defn public-fn
  "A public function with docstring"
  [x y]
  (+ x y))

(defn- private-fn [x]
  (* x 2))

(defmacro when-let+
  [bindings & body]
  `(when-let ~bindings ~@body))

(defprotocol IAnimal
  (speak [this])
  (move [this]))

(defrecord Dog [name breed]
  IAnimal
  (speak [this] "woof")
  (move [this] "run"))

(defmulti process-event :type)

(defmethod process-event :click [event]
  (println "Clicked!"))

(defmethod process-event :hover [event]
  (println "Hovered!"))
"#;

        let file_id = crate::FileId::new(1).unwrap();
        let mut counter = crate::types::SymbolCounter::new();
        let _symbols = parser.parse(code, file_id, &mut counter);

        let report = generate_audit_report(parser.get_handled_nodes());
        println!("{report}");
    }
}
```

---

## 8. Registration Updates

### src/parsing/mod.rs

Add:
```rust
pub mod clojure;

pub use clojure::{ClojureBehavior, ClojureParser};
```

### src/parsing/registry.rs

In `initialize_registry()` function, add:
```rust
super::clojure::register(registry);
```

### src/parsing/language.rs

Add to the `Language` enum:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Php,
    Go,
    C,
    Cpp,
    CSharp,
    Gdscript,
    Java,
    Kotlin,
    Swift,
    Clojure,  // Add this
}

impl Language {
    pub fn name(&self) -> &'static str {
        match self {
            // ... existing ...
            Language::Clojure => "Clojure",
        }
    }

    pub fn config_key(&self) -> &str {
        match self {
            // ... existing ...
            Language::Clojure => "clojure",
        }
    }

    pub fn to_language_id(&self) -> LanguageId {
        match self {
            // ... existing ...
            Language::Clojure => LanguageId::new("clojure"),
        }
    }
}
```

### src/parsing/factory.rs

Add Clojure to the match statements in both `create_parser()` and `create_parser_with_behavior()`:

```rust
Language::Clojure => {
    let parser = ClojureParser::new().map_err(|e| IndexError::General(e.to_string()))?;
    Ok(Box::new(parser))
}
```

And in `create_parser_with_behavior()`:
```rust
Language::Clojure => {
    let parser = ClojureParser::new().map_err(|e| IndexError::General(e.to_string()))?;
    ParserWithBehavior {
        parser: Box::new(parser),
        behavior: Box::new(ClojureBehavior::new()),
    }
}
```

---

## 9. Testing

### Test File: examples/clojure/comprehensive.clj

```clojure
(ns examples.clojure.comprehensive
  "A comprehensive example for parser testing"
  (:require [clojure.string :as str]
            [clojure.set :refer [union intersection]]
            [clojure.java.io :as io]))

;; Simple def
(def pi 3.14159)

;; Private def
(def ^:private secret-key "abc123")

;; Public function with docstring
(defn greet
  "Greets a person by name"
  [name]
  (str "Hello, " name "!"))

;; Private function
(defn- helper-fn
  [x]
  (* x 2))

;; Multi-arity function
(defn process
  "Process with optional config"
  ([data] (process data {}))
  ([data config]
   (let [result (transform data)]
     (if (:verbose config)
       (println "Result:" result)
       result))))

;; Macro
(defmacro with-timing
  "Executes body and prints elapsed time"
  [& body]
  `(let [start# (System/currentTimeMillis)]
     (try
       ~@body
       (finally
         (println "Elapsed:" (- (System/currentTimeMillis) start#) "ms")))))

;; Protocol
(defprotocol Serializable
  "Protocol for serialization"
  (serialize [this] "Convert to string")
  (deserialize [this data] "Parse from string"))

;; Record implementing protocol
(defrecord User [id name email]
  Serializable
  (serialize [this]
    (str (:id this) ":" (:name this) ":" (:email this)))
  (deserialize [this data]
    (let [[id name email] (str/split data #":")]
      (->User id name email))))

;; Deftype
(deftype Counter [^:volatile-mutable count]
  clojure.lang.IDeref
  (deref [this] count))

;; Multimethod
(defmulti area :shape)

(defmethod area :circle [{:keys [radius]}]
  (* pi radius radius))

(defmethod area :rectangle [{:keys [width height]}]
  (* width height))

(defmethod area :default [_]
  (throw (ex-info "Unknown shape" {})))

;; Higher-order function usage
(defn transform-all
  [items]
  (->> items
       (filter some?)
       (map str/trim)
       (remove str/blank?)))
```

### Run Tests

```bash
# Unit tests
cargo test clojure

# Coverage audit
cargo test audit_clojure -- --nocapture

# Parse example file
cargo run -- index examples/clojure/
cargo run -- mcp find_symbol greet
```

---

## 10. Clojure-Specific Considerations

### Symbol Kinds Mapping

| Clojure Form | SymbolKind |
|--------------|------------|
| `defn`, `defn-` | Function |
| `def` | Variable |
| `defmacro` | Macro |
| `defprotocol` | Interface |
| `defrecord`, `deftype` | Struct |
| `defmulti` | Function |
| `defmethod` | Method |
| `ns` | Module |

### Visibility Rules

1. `defn-` = Private
2. `^:private` metadata = Private
3. Names starting with `-` = Private (convention)
4. Everything else = Public

### Namespace Resolution

- Clojure uses `.` for namespace paths: `my.app.core`
- Var references use `/`: `clojure.string/join`
- Aliases work via `:as`: `[clojure.string :as str]` → `str/join`

### EDN Files

`.edn` files are data-only (no code), so parsing them yields no symbols unless they contain reader-tagged literals.

---

## Verification Checklist

- [ ] `tree-sitter-clojure` added to Cargo.toml
- [ ] All 6 files created in `src/parsing/clojure/`
- [ ] `Language::Clojure` variant added to enum
- [ ] Registry registration in `initialize_registry()`
- [ ] Module exports in `src/parsing/mod.rs`
- [ ] Factory methods updated
- [ ] Unit tests pass
- [ ] Audit coverage >50%
- [ ] Example file indexes correctly
- [ ] MCP tools work with Clojure symbols
