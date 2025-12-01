//! Rust language parser implementation
//!
//! **Tree-sitter ABI Version**: ABI-15 (tree-sitter-rust 0.24.0)
//!
//! Note: This parser uses ABI-15 which includes enhanced metadata support.
//! When migrating or updating the parser, ensure compatibility with ABI-15 features.
//!
//! ## Design Trade-offs
//!
//! This parser implementation makes certain trade-offs between the project's
//! zero-cost abstraction guidelines and API stability:
//!
//! ### Current Limitations:
//! 1. **Methods return `Vec<T>`** - The LanguageParser trait requires Vec returns,
//!    preventing true iterator-based APIs
//! 2. **Recursive tree traversal** - Tree-sitter's recursive nature makes it
//!    challenging to implement lazy iterators without significant complexity
//! 3. **Mutable state** - Symbol counters and other mutable state prevent pure
//!    functional approaches
//!
//! ### Optimizations Applied:
//! 1. **Deferred string allocation** - Work with `&str` as long as possible
//! 2. **Minimal intermediate allocations** - Avoid temporary String objects
//! 3. **Iterator preparation** - Internal methods ready for future API migration
//!
//! ### Future Migration Path:
//! When the LanguageParser trait is updated to support iterators, this
//! implementation can be refactored to be fully zero-cost by:
//! - Returning `impl Iterator<Item = Symbol>` instead of `Vec<Symbol>`
//! - Using generator-based tree traversal or manual state machines
//! - Eliminating all intermediate allocations

use crate::parsing::Import;
use crate::parsing::method_call::MethodCall;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind};
use tree_sitter::{Node, Parser};

/// Debug print macro that respects the debug setting
macro_rules! debug_print {
    ($self:expr, $($arg:tt)*) => {
        if $self.debug {
            eprintln!("DEBUG: {}", format!($($arg)*));
        }
    };
}

// Helper enum for doc comment type classification
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocCommentType {
    OuterLine,     // ///
    OuterBlock,    // /**
    InnerLine,     // //!
    InnerBlock,    // /*!
    NotDocComment, // Regular comment
}

pub struct RustParser {
    parser: Parser,
    debug: bool,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for RustParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RustParser")
            .field("language", &"Rust")
            .finish()
    }
}

impl RustParser {
    pub fn new() -> Result<Self, String> {
        Self::with_debug(false)
    }

    pub fn with_debug(debug: bool) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| format!("Failed to set Rust language: {e}"))?;

        Ok(Self {
            parser,
            debug,
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
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
                // Extract the use path - look for the argument field which contains the import
                if let Some(arg_node) = node.child_by_field_name("argument") {
                    self.extract_import_from_node(arg_node, code, file_id, imports);
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

    fn extract_import_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "identifier" => {
                // Simple import like `use foo;`
                let path = code[node.byte_range()].to_string();
                imports.push(Import {
                    path,
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
            "scoped_identifier" => {
                // Import like `use foo::bar::baz;`
                let path = code[node.byte_range()].to_string();
                imports.push(Import {
                    path,
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
            "use_as_clause" => {
                // Import with alias like `use foo::bar as baz;`
                if let Some(path_node) = node.child_by_field_name("path") {
                    let path = code[path_node.byte_range()].to_string();
                    if let Some(alias_node) = node.child_by_field_name("alias") {
                        let alias = code[alias_node.byte_range()].to_string();
                        imports.push(Import {
                            path,
                            alias: Some(alias),
                            file_id,
                            is_glob: false,
                            is_type_only: false,
                        });
                    }
                }
            }
            "use_wildcard" => {
                // Glob import like `use foo::*;`
                // The wildcard node has a scoped_identifier child containing the path
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "scoped_identifier" {
                        let path = code[child.byte_range()].to_string();
                        imports.push(Import {
                            path,
                            alias: None,
                            file_id,
                            is_glob: true,
                            is_type_only: false,
                        });
                        break;
                    }
                }
            }
            "use_list" => {
                // Grouped imports like `use foo::{bar, baz};`
                if let Some(parent) = node.parent() {
                    let prefix = if parent.kind() == "scoped_use_list" {
                        if let Some(path_node) = parent.child_by_field_name("path") {
                            code[path_node.byte_range()].to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    // Process each item in the list
                    for child in node.children(&mut node.walk()) {
                        if child.kind() != "," && child.kind() != "{" && child.kind() != "}" {
                            self.extract_import_from_list_item(
                                child, code, file_id, &prefix, imports,
                            );
                        }
                    }
                }
            }
            "scoped_use_list" => {
                // Handle `use foo::{bar, baz}` pattern
                if let Some(list_node) = node.child_by_field_name("list") {
                    self.extract_import_from_node(list_node, code, file_id, imports);
                }
            }
            _ => {}
        }
    }

    fn extract_import_from_list_item(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        prefix: &str,
        imports: &mut Vec<Import>,
    ) {
        match node.kind() {
            "identifier" => {
                let name = code[node.byte_range()].to_string();
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}::{name}")
                };
                imports.push(Import {
                    path,
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                });
            }
            "use_as_clause" => {
                if let Some(path_node) = node.child_by_field_name("path") {
                    let name = code[path_node.byte_range()].to_string();
                    let path = if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}::{name}")
                    };
                    if let Some(alias_node) = node.child_by_field_name("alias") {
                        let alias = code[alias_node.byte_range()].to_string();
                        imports.push(Import {
                            path,
                            alias: Some(alias),
                            file_id,
                            is_glob: false,
                            is_type_only: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    pub fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        // Reset context for each file
        self.context = ParserContext::new();

        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut symbols = Vec::new();

        // Walk the tree manually to find symbols
        self.extract_symbols_from_node(root_node, code, file_id, &mut symbols, symbol_counter, 0);

        symbols
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

        // Debug: print node types that contain "type" or "const"
        if (node.kind().contains("type") || node.kind().contains("const")) && self.debug {
            eprintln!("DEBUG: Found node kind: {}", node.kind());
        }

        match node.kind() {
            "function_item" => {
                self.register_node_recursively(node);
                // Extract function name for parent tracking
                let func_name = node
                    .child_by_field_name("name")
                    .map(|n| code[n.byte_range()].to_string());

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
                    if let Some(mut symbol) =
                        self.create_symbol(counter, node, name_node, kind, file_id, code)
                    {
                        // Extract and add function signature
                        let signature = self.extract_signature(node, code);
                        symbol = symbol.with_signature(signature);
                        symbols.push(symbol);
                    }
                }

                // Enter function scope for nested items
                // Rust doesn't have hoisting like JS/TS
                self.context
                    .enter_scope(ScopeType::Function { hoisting: false });

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current function for parent tracking
                self.context.set_current_function(func_name.clone());

                // Process children for nested functions/types
                for child in node.children(&mut node.walk()) {
                    if child.kind() != "identifier" && child.kind() != "parameters" {
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

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);

                return; // Don't process children again
            }
            "struct_item" => {
                self.register_node_recursively(node);
                // Extract struct name for parent tracking
                let struct_name = node
                    .child_by_field_name("name")
                    .map(|n| code[n.byte_range()].to_string());

                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Struct,
                        file_id,
                        code,
                    );

                    if let Some(mut sym) = symbol {
                        // Extract and add struct signature
                        let signature = self.extract_struct_signature(node, code);
                        sym = sym.with_signature(signature);
                        symbols.push(sym);
                    }
                }

                // Structs can have nested items in Rust (though rare)
                // Enter struct scope for potential nested items
                self.context.enter_scope(ScopeType::Class);

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current class for parent tracking
                self.context.set_current_class(struct_name);

                // Process struct fields
                if let Some(field_list) = node.child_by_field_name("body") {
                    for child in field_list.children(&mut field_list.walk()) {
                        if child.kind() == "field_declaration" {
                            self.register_handled_node("field_declaration", child.kind_id());
                            if let Some(name_node) = child.child_by_field_name("name") {
                                if let Some(symbol) = self.create_symbol(
                                    counter,
                                    child,
                                    name_node,
                                    SymbolKind::Field,
                                    file_id,
                                    code,
                                ) {
                                    symbols.push(symbol);
                                }
                            }
                        }
                    }
                }

                // Process children for potential nested items
                for child in node.children(&mut node.walk()) {
                    if child.kind() != "identifier" && child.kind() != "field_declaration_list" {
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

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);
            }
            "enum_item" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Enum,
                        file_id,
                        code,
                    );

                    if let Some(mut sym) = symbol {
                        // Extract and add enum signature
                        let signature = self.extract_enum_signature(node, code);
                        sym = sym.with_signature(signature);
                        symbols.push(sym);
                    }
                }

                // Process enum variants
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        if child.kind() == "enum_variant" {
                            self.register_handled_node("enum_variant", child.kind_id());
                            if let Some(name_node) = child.child_by_field_name("name") {
                                if let Some(symbol) = self.create_symbol(
                                    counter,
                                    child,
                                    name_node,
                                    SymbolKind::Constant,
                                    file_id,
                                    code,
                                ) {
                                    symbols.push(symbol);
                                }
                            }
                        }
                    }
                }
            }
            "type_item" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::TypeAlias,
                        file_id,
                        code,
                    );

                    if let Some(mut sym) = symbol {
                        // Extract and add type alias signature
                        let signature = self.extract_type_alias_signature(node, code);
                        sym = sym.with_signature(signature);
                        symbols.push(sym);
                    }
                }
            }
            "const_item" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Constant,
                        file_id,
                        code,
                    );

                    if let Some(mut sym) = symbol {
                        // Extract and add constant signature
                        let signature = self.extract_const_signature(node, code);
                        sym = sym.with_signature(signature);
                        symbols.push(sym);
                    }
                }
            }
            "static_item" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Constant,
                        file_id,
                        code,
                    );

                    if let Some(mut sym) = symbol {
                        // Extract and add static signature (using const signature for statics)
                        let signature = self.extract_const_signature(node, code);
                        sym = sym.with_signature(signature);
                        symbols.push(sym);
                    }
                }
            }
            "trait_item" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    // For traits, we need the full node range, not just the name
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Trait,
                        file_id,
                        code,
                    );

                    if let Some(mut sym) = symbol {
                        // Extract and add trait signature
                        let signature = self.extract_trait_signature(node, code);
                        sym = sym.with_signature(signature);
                        symbols.push(sym);
                    }

                    // Enter trait scope for method signatures
                    self.context.enter_scope(ScopeType::Class); // Traits are like classes
                    // Also extract method signatures from the trait
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            if child.kind() == "function_signature_item"
                                || child.kind() == "function_item"
                            {
                                // Register the nested node types for audit tracking
                                self.register_handled_node(child.kind(), child.kind_id());
                                if let Some(method_name_node) = child.child_by_field_name("name") {
                                    if let Some(mut method_symbol) = self.create_symbol(
                                        counter,
                                        child,
                                        method_name_node,
                                        SymbolKind::Method,
                                        file_id,
                                        code,
                                    ) {
                                        // Extract and add method signature
                                        let signature = self.extract_signature(child, code);
                                        method_symbol = method_symbol.with_signature(signature);
                                        symbols.push(method_symbol);
                                    }
                                }
                            }
                        }
                    }
                    self.context.exit_scope();
                }
                // Don't recurse further - we've handled the trait methods
                return;
            }
            "impl_item" => {
                self.register_node_recursively(node);
                // Extract the type being implemented for parent tracking
                let impl_type_name = node
                    .child_by_field_name("type")
                    .and_then(|type_node| self.extract_type_name(type_node, code));

                // Enter impl block scope for methods
                self.context.enter_scope(ScopeType::Class); // Use Class scope for impl blocks

                // Save the current parent context before setting new one
                let saved_function = self.context.current_function().map(|s| s.to_string());
                let saved_class = self.context.current_class().map(|s| s.to_string());

                // Set current class to the impl type for parent tracking
                if let Some(type_name) = impl_type_name {
                    self.context.set_current_class(Some(type_name.to_string()));
                }

                // Process children
                for child in node.children(&mut node.walk()) {
                    self.extract_symbols_from_node(
                        child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        depth + 1,
                    );
                }

                // CRITICAL: Exit scope first (this clears the current context)
                self.context.exit_scope();

                // Then restore the previous parent context
                self.context.set_current_function(saved_function);
                self.context.set_current_class(saved_class);

                return; // Don't process children again
            }
            "mod_item" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Module,
                        file_id,
                        code,
                    );

                    if let Some(sym) = symbol {
                        symbols.push(sym);
                    }
                }

                // Process children for nested items within the module
                for child in node.children(&mut node.walk()) {
                    if child.kind() != "identifier" {
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
                return; // Skip default traversal since we handled children
            }
            "macro_definition" => {
                self.register_node_recursively(node);
                if let Some(name_node) = node.child_by_field_name("name") {
                    let symbol = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Macro,
                        file_id,
                        code,
                    );

                    if let Some(sym) = symbol {
                        symbols.push(sym);
                    }
                }
            }
            // Register use declarations and their children for audit tracking
            "use_declaration" => {
                self.register_node_recursively(node);
            }
            // Register closure expressions for audit tracking
            "closure_expression" => {
                self.register_node_recursively(node);
            }
            _ => {}
        }

        // Recurse into children (except for impl_item which returns early)
        for child in node.children(&mut node.walk()) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
        }
    }

    pub fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();

        self.find_calls_in_node(root_node, code, &mut calls);

        // Debug output commented out for cleaner benchmark display
        // TODO: Enable via settings.debug flag in future enhancement
        // if !calls.is_empty() {
        //     eprintln!("DEBUG [find_calls]: Found {} calls", calls.len());
        //     for (caller, target, range) in &calls {
        //         eprintln!("  - '{}' calls '{}' at line {}", caller, target, range.start_line);
        //     }
        // }

        calls
    }

    pub fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut implementations = Vec::new();

        self.find_implementations_in_node(root_node, code, &mut implementations);

        // Debug output commented out for cleaner benchmark display
        // TODO: Enable via settings.debug flag in future enhancement
        // if !implementations.is_empty() {
        //     eprintln!("DEBUG [find_implementations]: Found {} implementations", implementations.len());
        //     for (type_name, trait_name, range) in &implementations {
        //         eprintln!("  - '{}' implements '{}' at line {}", type_name, trait_name, range.start_line);
        //     }
        // }

        implementations
    }

    pub fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut uses = Vec::new();

        self.find_uses_in_node(root_node, code, &mut uses);

        // Debug output commented out for cleaner benchmark display
        // TODO: Enable via settings.debug flag in future enhancement
        // if !uses.is_empty() {
        //     eprintln!("DEBUG [find_uses]: Found {} type uses", uses.len());
        //     for (context, used_type, range) in &uses {
        //         eprintln!("  - '{}' uses type '{}' at line {}", context, used_type, range.start_line);
        //     }
        // }

        uses
    }

    pub fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut defines = Vec::new();

        self.find_defines_in_node(root_node, code, &mut defines);

        // Debug output commented out for cleaner benchmark display
        // TODO: Enable via settings.debug flag in future enhancement
        // if !defines.is_empty() {
        //     eprintln!("DEBUG [find_defines]: Found {} method definitions", defines.len());
        //     for (definer, method, range) in &defines {
        //         eprintln!("  - '{}' defines method '{}' at line {}", definer, method, range.start_line);
        //     }
        // }

        defines
    }

    /// Find inherent methods (methods in impl blocks without traits)
    /// Returns Vec<(type_name, method_name, range)>
    pub fn find_inherent_methods(&mut self, code: &str) -> Vec<(String, String, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut methods = Vec::new();

        self.find_inherent_methods_in_node(root_node, code, &mut methods);

        methods
    }

    fn find_calls_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        let containing_function = self.find_containing_function(node, code);

        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                // Debug output commented out for cleaner benchmark display
                // TODO: Enable via settings.debug flag in future enhancement
                // eprintln!("DEBUG [find_calls_in_node]: Found call_expression, function node kind: {}",
                //           function_node.kind());
                let mut target_name = None;

                // Handle direct function calls (e.g., `my_function()`)
                if function_node.kind() == "identifier" {
                    target_name = Some(&code[function_node.byte_range()]);
                }
                // Handle method calls (e.g., `variable.method()`)
                else if function_node.kind() == "field_expression" {
                    if let Some(field_node) = function_node.child_by_field_name("field") {
                        // For method calls, just return the method name
                        // The receiver info is better handled by find_method_calls()
                        target_name = Some(&code[field_node.byte_range()]);
                    }
                }
                // Handle associated functions (e.g., `String::new()`)
                else if function_node.kind() == "scoped_identifier" {
                    // Extract the full qualified path
                    target_name = Some(&code[function_node.byte_range()]);
                }

                if let (Some(target), Some(caller)) = (target_name, containing_function) {
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    // Debug output commented out for cleaner benchmark display
                    // TODO: Enable via settings.debug flag in future enhancement
                    // eprintln!("DEBUG [find_calls_in_node]: Adding call '{}' -> '{}' (node kind: {})",
                    //           caller, target, function_node.kind());
                    calls.push((caller, target, range));
                } else {
                    // Debug output commented out for cleaner benchmark display
                    // TODO: Enable via settings.debug flag in future enhancement
                    // eprintln!("DEBUG [find_calls_in_node]: Skipped - target: {:?}, caller: {:?}",
                    //           target_name, containing_function);
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_calls_in_node(child, code, calls);
        }
    }

    fn find_containing_function<'a>(&self, mut node: Node, code: &'a str) -> Option<&'a str> {
        loop {
            if node.kind() == "function_item" {
                if let Some(name_node) = node.child_by_field_name("name") {
                    return Some(&code[name_node.byte_range()]);
                }
            }

            match node.parent() {
                Some(parent) => node = parent,
                None => return None,
            }
        }
    }

    /// Recursively extracts method calls from AST nodes with enhanced receiver detection.
    ///
    /// Handles direct function calls, instance methods, and static method calls.
    /// Preserves caller context and receiver information for precise resolution.
    fn find_method_calls_in_node(
        &self,
        node: Node,
        code: &str,
        method_calls: &mut Vec<MethodCall>,
    ) {
        let containing_function = self.find_containing_function(node, code);

        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                // Handle direct function calls (e.g., `my_function()`)
                if function_node.kind() == "identifier" {
                    let method_name = code[function_node.byte_range()].to_string();
                    if let Some(caller) = containing_function {
                        let range = Range::new(
                            node.start_position().row as u32,
                            node.start_position().column as u16,
                            node.end_position().row as u32,
                            node.end_position().column as u16,
                        );
                        let method_call = MethodCall::new(caller, &method_name, range);
                        // Debug: Found function call (enable debug mode to see)
                        // eprintln!("DEBUG: Found function call: {caller} -> {method_name}");
                        method_calls.push(method_call);
                    }
                }
                // Handle method calls (e.g., `variable.method()`)
                else if function_node.kind() == "field_expression" {
                    if let Some(field_node) = function_node.child_by_field_name("field") {
                        let method_name = code[field_node.byte_range()].to_string();

                        // Extract receiver from field_expression
                        if let Some(value_node) = function_node.child_by_field_name("value") {
                            let receiver_text = code[value_node.byte_range()].to_string();

                            if let Some(caller) = containing_function {
                                let range = Range::new(
                                    node.start_position().row as u32,
                                    node.start_position().column as u16,
                                    node.end_position().row as u32,
                                    node.end_position().column as u16,
                                );

                                let method_call = match value_node.kind() {
                                    "self" => {
                                        // TODO: Add debug logging
                                        // eprintln!("DEBUG: Found self method call: {} -> self.{}", caller, method_name);
                                        MethodCall::new(caller, &method_name, range)
                                            .with_receiver("self")
                                    }
                                    "identifier" => {
                                        // TODO: Add debug logging
                                        // eprintln!("DEBUG: Found instance method call: {} -> {}.{}", caller, receiver_text, method_name);
                                        MethodCall::new(caller, &method_name, range)
                                            .with_receiver(&receiver_text)
                                    }
                                    "field_expression" => {
                                        // Chained calls like self.field.method()
                                        // TODO: Add debug logging
                                        // eprintln!("DEBUG: Found chained method call: {} -> {}.{}", caller, receiver_text, method_name);
                                        MethodCall::new(caller, &method_name, range)
                                            .with_receiver(&receiver_text)
                                    }
                                    _ => {
                                        // TODO: Add debug logging
                                        // eprintln!("DEBUG: Found method call with unknown receiver type: {} -> {}.{}", caller, receiver_text, method_name);
                                        MethodCall::new(caller, &method_name, range)
                                            .with_receiver(&receiver_text)
                                    }
                                };
                                method_calls.push(method_call);
                            }
                        }
                    }
                }
                // Handle static method calls (e.g., `String::new()`)
                else if function_node.kind() == "scoped_identifier" {
                    let full_path = code[function_node.byte_range()].to_string();

                    // Parse Type::method pattern
                    if let Some(scope_pos) = full_path.rfind("::") {
                        let type_name = &full_path[..scope_pos];
                        let method_name = &full_path[scope_pos + 2..];

                        if let Some(caller) = containing_function {
                            let range = Range::new(
                                node.start_position().row as u32,
                                node.start_position().column as u16,
                                node.end_position().row as u32,
                                node.end_position().column as u16,
                            );

                            let method_call = MethodCall::new(caller, method_name, range)
                                .with_receiver(type_name)
                                .static_method();

                            // TODO: Add debug logging
                            // eprintln!("DEBUG: Found static method call: {} -> {}::{}", caller, type_name, method_name);
                            method_calls.push(method_call);
                        }
                    }
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_method_calls_in_node(child, code, method_calls);
        }
    }

    fn find_implementations_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
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

    fn find_variable_types_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        bindings: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if node.kind() == "let_declaration" {
            debug_print!(
                self,
                "[find_variable_types_in_node]: Found let_declaration at line {}",
                node.start_position().row
            );

            // Extract variable name from pattern
            if let Some(pattern_node) = node.child_by_field_name("pattern") {
                // eprintln!("  - Pattern node kind: {}", pattern_node.kind());

                if let Some(var_name) = self.extract_variable_name(pattern_node, code) {
                    // eprintln!("  - Extracted variable name: '{}'", var_name);
                    // Extract type from value expression
                    if let Some(value_node) = node.child_by_field_name("value") {
                        // eprintln!("  - Value node kind: {}", value_node.kind());
                        // eprintln!("  - Value text: '{}'", &code[value_node.byte_range()]);

                        if let Some(type_name) = self.extract_value_type(value_node, code) {
                            // eprintln!("  - Extracted type name: '{}'", type_name);
                            let range = Range::new(
                                node.start_position().row as u32,
                                node.start_position().column as u16,
                                node.end_position().row as u32,
                                node.end_position().column as u16,
                            );
                            bindings.push((var_name, type_name, range));
                            // eprintln!("  ✓ Added binding: {} -> {}", var_name, type_name);
                        } else {
                            // eprintln!("  ✗ Could not extract type from value");
                        }
                    } else {
                        // eprintln!("  ✗ No value node found");
                    }
                } else {
                    // eprintln!("  ✗ Could not extract variable name");
                }
            } else {
                // eprintln!("  ✗ No pattern node found");
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_variable_types_in_node(child, code, bindings);
        }
    }

    fn extract_variable_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            "identifier" => Some(&code[node.byte_range()]),
            _ => None,
        }
    }

    fn extract_value_type<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        debug_print!(
            self,
            "    [extract_value_type] Node kind: '{}', text: '{}'",
            node.kind(),
            &code[node.byte_range()]
        );

        match node.kind() {
            // Direct struct construction: MyType { ... }
            "struct_expression" => {
                if let Some(type_node) = node.child_by_field_name("name") {
                    // eprintln!("    → struct_expression extracted type: {:?}", result);
                    self.extract_type_name(type_node, code)
                } else {
                    // eprintln!("    → struct_expression has no name field");
                    None
                }
            }
            // Reference: &expr - can't handle this without allocation
            "reference_expression" => {
                // For now, skip reference types as they require allocation
                // A full solution would need Cow<'a, str> or similar
                // eprintln!("    → reference_expression skipped (would require allocation)");
                None
            }
            // Variable reference: x = y
            "identifier" => {
                // Direct type name without prefix
                let result = &code[node.byte_range()];
                // eprintln!("    → identifier extracted: '{}'", result);
                Some(result)
            }
            // Call expressions like Type::new() - extract the type part
            "call_expression" => {
                if let Some(function_node) = node.child_by_field_name("function") {
                    // eprintln!("    → call_expression function kind: '{}'", function_node.kind());
                    if function_node.kind() == "scoped_identifier" {
                        // Extract type from Type::method pattern
                        let full_path = &code[function_node.byte_range()];
                        // eprintln!("    → scoped_identifier full path: '{}'", full_path);
                        if let Some(scope_pos) = full_path.find("::") {
                            let type_part = &full_path[..scope_pos];
                            // eprintln!("    → extracted type part: '{}'", type_part);
                            return Some(type_part);
                        }
                    }
                }
                // eprintln!("    → call_expression: no type extracted");
                None
            }
            _ => {
                // eprintln!("    → unhandled node kind: '{}'", node.kind());
                None
            }
        }
    }

    /// Extract the full type name including generic parameters
    /// Returns an owned String to support complex type construction
    #[allow(clippy::only_used_in_recursion)]
    fn extract_full_type_name(&self, node: Node, code: &str) -> String {
        match node.kind() {
            "type_identifier" => code[node.byte_range()].to_string(),
            "primitive_type" => code[node.byte_range()].to_string(),
            "scoped_type_identifier" => code[node.byte_range()].to_string(),
            "generic_type" => {
                // For generic types like Option<T>, Vec<T>, construct the full name
                let mut result = String::new();

                // Get the base type
                if let Some(type_node) = node.child_by_field_name("type") {
                    result.push_str(&self.extract_full_type_name(type_node, code));
                }

                // Get the generic arguments
                if let Some(args_node) = node.child_by_field_name("type_arguments") {
                    result.push('<');
                    let mut first = true;
                    for child in args_node.children(&mut args_node.walk()) {
                        if child.kind() != "," && child.kind() != "<" && child.kind() != ">" {
                            if !first {
                                result.push_str(", ");
                            }
                            result.push_str(&self.extract_full_type_name(child, code));
                            first = false;
                        }
                    }
                    result.push('>');
                }

                result
            }
            "reference_type" => {
                // Handle &T, &mut T
                let mut result = String::from("&");
                if node.child_by_field_name("mutable").is_some() {
                    result.push_str("mut ");
                }
                if let Some(type_node) = node.child_by_field_name("type") {
                    result.push_str(&self.extract_full_type_name(type_node, code));
                }
                result
            }
            "pointer_type" => {
                // Handle *const T, *mut T
                let mut result = String::from("*");
                if node.child_by_field_name("mutable").is_some() {
                    result.push_str("mut ");
                } else {
                    result.push_str("const ");
                }
                if let Some(type_node) = node.child_by_field_name("type") {
                    result.push_str(&self.extract_full_type_name(type_node, code));
                }
                result
            }
            _ => {
                // Default: use the text range of the node
                code[node.byte_range()].to_string()
            }
        }
    }

    /// Extract function/method signature from a node, excluding the body
    fn extract_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it (similar to TypeScript implementation)
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract struct signature including generics and visibility
    fn extract_struct_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract trait signature including generics and bounds
    fn extract_trait_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract enum signature including generics
    fn extract_enum_signature(&self, node: Node, code: &str) -> String {
        let start = node.start_byte();
        let mut end = node.end_byte();

        // Find the body and exclude it
        if let Some(body) = node.child_by_field_name("body") {
            end = body.start_byte();
        }

        code[start..end].trim().to_string()
    }

    /// Extract type alias signature
    fn extract_type_alias_signature(&self, node: Node, code: &str) -> String {
        // For type aliases, we want the entire definition including the assignment
        code[node.byte_range()].trim().to_string()
    }

    /// Extract constant signature
    fn extract_const_signature(&self, node: Node, code: &str) -> String {
        // For constants, we want the entire definition including the value
        code[node.byte_range()].trim().to_string()
    }

    /// Recursive type extraction from AST nodes requires &self for traversal context
    #[allow(clippy::only_used_in_recursion)]
    fn extract_type_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        match node.kind() {
            "type_identifier" => Some(&code[node.byte_range()]),
            "primitive_type" => Some(&code[node.byte_range()]), // Added for i32, f64, etc.
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
                Some(&code[node.byte_range()])
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

    fn find_uses_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let struct_name = &code[name_node.byte_range()];

                    // Find field list
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            if child.kind() == "field_declaration" {
                                if let Some(type_node) = child.child_by_field_name("type") {
                                    if let Some(type_name) = self.extract_type_name(type_node, code)
                                    {
                                        let range = Range::new(
                                            type_node.start_position().row as u32,
                                            type_node.start_position().column as u16,
                                            type_node.end_position().row as u32,
                                            type_node.end_position().column as u16,
                                        );
                                        uses.push((struct_name, type_name, range));
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

                    // For zero-cost, just use the function name directly
                    // The full qualified name would require allocation
                    let context_name = fn_name;

                    // Find parameters
                    if let Some(params) = node.child_by_field_name("parameters") {
                        for param in params.children(&mut params.walk()) {
                            if param.kind() == "parameter" {
                                if let Some(type_node) = param.child_by_field_name("type") {
                                    if let Some(type_name) = self.extract_type_name(type_node, code)
                                    {
                                        let range = Range::new(
                                            type_node.start_position().row as u32,
                                            type_node.start_position().column as u16,
                                            type_node.end_position().row as u32,
                                            type_node.end_position().column as u16,
                                        );
                                        uses.push((context_name, type_name, range));
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

    fn find_defines_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            "trait_item" => {
                if let Some(trait_name_node) = node.child_by_field_name("name") {
                    let trait_name = &code[trait_name_node.byte_range()];
                    // Find all methods defined in this trait
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            // Handle both function_signature_item and function_item
                            if child.kind() == "function_signature_item"
                                || child.kind() == "function_item"
                            {
                                if let Some(method_name_node) = child.child_by_field_name("name") {
                                    let method_name = &code[method_name_node.byte_range()];
                                    let range = Range::new(
                                        child.start_position().row as u32,
                                        child.start_position().column as u16,
                                        child.end_position().row as u32,
                                        child.end_position().column as u16,
                                    );
                                    defines.push((trait_name, method_name, range));
                                }
                            }
                        }
                    }
                }
            }
            "impl_item" => {
                // NOTE: This method extracts ALL impl methods (inherent + trait)
                // For trait-only methods, use find_implementations + trait method tracking
                // Get the type being implemented
                if let Some(type_node) = node.child_by_field_name("type") {
                    if let Some(type_name) = self.extract_type_name(type_node, code) {
                        // Find all methods defined in this impl block
                        if let Some(body) = node.child_by_field_name("body") {
                            for child in body.children(&mut body.walk()) {
                                if child.kind() == "function_item" {
                                    if let Some(method_name_node) =
                                        child.child_by_field_name("name")
                                    {
                                        let method_name = &code[method_name_node.byte_range()];
                                        let range = Range::new(
                                            child.start_position().row as u32,
                                            child.start_position().column as u16,
                                            child.end_position().row as u32,
                                            child.end_position().column as u16,
                                        );
                                        defines.push((type_name, method_name, range));
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

    fn find_inherent_methods_in_node(
        &self,
        node: Node,
        code: &str,
        methods: &mut Vec<(String, String, Range)>,
    ) {
        if node.kind() == "impl_item" {
            // Check if this is an inherent impl (no trait field)
            if node.child_by_field_name("trait").is_none() {
                if let Some(type_node) = node.child_by_field_name("type") {
                    // Extract the full type name including generics
                    let type_name = self.extract_full_type_name(type_node, code);

                    // Find method definitions in the impl body
                    if let Some(body_node) = node.child_by_field_name("body") {
                        for child in body_node.children(&mut body_node.walk()) {
                            if child.kind() == "function_item" {
                                if let Some(method_name_node) = child.child_by_field_name("name") {
                                    let method_name = &code[method_name_node.byte_range()];
                                    let range = Range::new(
                                        child.start_position().row as u32,
                                        child.start_position().column as u16,
                                        child.end_position().row as u32,
                                        child.end_position().column as u16,
                                    );
                                    methods.push((
                                        type_name.clone(),
                                        method_name.to_string(),
                                        range,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_inherent_methods_in_node(child, code, methods);
        }
    }

    fn create_symbol(
        &mut self,
        counter: &mut SymbolCounter,
        full_node: Node,
        name_node: Node,
        kind: SymbolKind,
        file_id: FileId,
        code: &str,
    ) -> Option<Symbol> {
        let name = &code[name_node.byte_range()];

        let symbol_id = counter.next_id();

        let range = Range::new(
            full_node.start_position().row as u32,
            full_node.start_position().column as u16,
            full_node.end_position().row as u32,
            full_node.end_position().column as u16,
        );

        // Find the parent node that might have doc comments
        let doc_node = name_node.parent()?;
        let doc_comment = self.extract_doc_comments(&doc_node, code);

        let mut symbol = Symbol::new(symbol_id, name, kind, file_id, range);

        // Set scope context based on parser's current scope
        symbol.scope_context = Some(self.context.current_scope_context());

        // Check for visibility modifiers
        if let Some(parent) = name_node.parent() {
            // Check if there's a visibility_modifier child
            let mut found_visibility = false;
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "visibility_modifier" {
                    symbol = symbol.with_visibility(crate::Visibility::Public);
                    found_visibility = true;
                    break;
                }
            }

            // Debug: print if we're looking at a function
            if self.debug && parent.kind() == "function_item" && name == "create_config" {
                eprintln!(
                    "DEBUG visibility check for create_config: found_visibility={found_visibility}"
                );
                eprintln!("  Parent kind: {}", parent.kind());
                eprintln!(
                    "  Children: {:?}",
                    parent
                        .children(&mut parent.walk())
                        .map(|c| c.kind())
                        .collect::<Vec<_>>()
                );
            }
        }

        if let Some(doc) = doc_comment {
            symbol = symbol.with_doc(doc);
        }

        Some(symbol)
    }

    /// Classify comment text into doc comment type with exact rules
    fn classify_doc_comment(&self, comment_text: &str) -> DocCommentType {
        if comment_text.starts_with("///") && !comment_text.starts_with("////") {
            DocCommentType::OuterLine
        } else if comment_text.starts_with("//!") {
            DocCommentType::InnerLine
        } else if comment_text.starts_with("/**")
            && !comment_text.starts_with("/***")
            && comment_text != "/**/"
        {
            DocCommentType::OuterBlock
        } else if comment_text.starts_with("/*!") && comment_text != "/*!" {
            DocCommentType::InnerBlock
        } else {
            DocCommentType::NotDocComment
        }
    }

    /// Check if comment is outer doc comment type
    fn is_outer_doc_comment(&self, comment_text: &str) -> bool {
        matches!(
            self.classify_doc_comment(comment_text),
            DocCommentType::OuterLine | DocCommentType::OuterBlock
        )
    }

    /// Check if comment is inner doc comment type  
    fn is_inner_doc_comment(&self, comment_text: &str) -> bool {
        matches!(
            self.classify_doc_comment(comment_text),
            DocCommentType::InnerLine | DocCommentType::InnerBlock
        )
    }

    /// Extract inner doc comments from inside a container node
    fn extract_inner_doc_comments(&self, node: &Node, code: &str) -> Option<String> {
        let mut inner_doc_parts = Vec::new();

        // Recursively scan for inner doc comments (they might be deep in the structure)
        self.collect_inner_doc_comments_recursive(node, code, &mut inner_doc_parts);

        if inner_doc_parts.is_empty() {
            None
        } else {
            // Single allocation - join all borrowed parts
            Some(inner_doc_parts.join("\n"))
        }
    }

    /// Recursively collect inner doc comments from node and its descendants
    fn collect_inner_doc_comments_recursive<'a>(
        &self,
        node: &Node,
        code: &'a str,
        parts: &mut Vec<&'a str>,
    ) {
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "line_comment" | "block_comment") {
                if let Ok(text) = child.utf8_text(code.as_bytes()) {
                    if self.is_inner_doc_comment(text) {
                        if text.starts_with("//!") {
                            let content = text.trim_start_matches("//!").trim();
                            if !content.is_empty() {
                                parts.push(content);
                            }
                        } else if text.starts_with("/*!") {
                            let content = text.trim_start_matches("/*!").trim_end_matches("*/");
                            // Process block content, work with borrowed strings
                            for line in content.lines() {
                                let cleaned = line.trim().trim_start_matches('*').trim();
                                if !cleaned.is_empty() {
                                    parts.push(cleaned);
                                }
                            }
                        }
                    }
                }
            } else {
                // Recursively scan children for inner doc comments
                self.collect_inner_doc_comments_recursive(&child, code, parts);
            }
        }
    }

    fn extract_doc_comments(&self, node: &Node, code: &str) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut current = node.prev_sibling();

        while let Some(sibling) = current {
            match sibling.kind() {
                "line_comment" | "block_comment" => {
                    if let Ok(text) = sibling.utf8_text(code.as_bytes()) {
                        if self.is_outer_doc_comment(text) {
                            let comment_type = self.classify_doc_comment(text);
                            let content = match comment_type {
                                DocCommentType::OuterLine => {
                                    text.trim_start_matches("///").trim().to_string()
                                }
                                DocCommentType::OuterBlock => text
                                    .trim_start_matches("/**")
                                    .trim_end_matches("*/")
                                    .trim()
                                    .to_string(),
                                _ => break, // Should not happen due to is_outer_doc_comment check
                            };
                            doc_lines.push(content);
                        } else {
                            break; // Non-outer-doc comment ends the sequence
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

    /// Recursively register all nodes for audit tracking
    /// This ensures child nodes (parameter, type_parameter, lifetime, etc.) are counted
    fn register_node_recursively(&mut self, node: Node) {
        self.register_handled_node(node.kind(), node.kind_id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.register_node_recursively(child);
        }
    }
}

impl LanguageParser for RustParser {
    fn parse(
        &mut self,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
    ) -> Vec<Symbol> {
        self.parse(code, file_id, symbol_counter)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        // Extract outer documentation (current functionality)
        let outer_docs = self.extract_doc_comments(node, code);

        // Extract inner documentation (NEW functionality)
        let inner_docs = self.extract_inner_doc_comments(node, code);

        // Combine with precedence (outer first, then inner)
        match (outer_docs, inner_docs) {
            (Some(outer), Some(inner)) => {
                // Combine with clear separation
                Some(format!("{outer}\n\n{inner}"))
            }
            (Some(outer), None) => Some(outer),
            (None, Some(inner)) => Some(inner),
            (None, None) => None,
        }
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        self.find_calls(code)
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        debug_print!(
            self,
            "RustParser::find_method_calls override called with enhanced AST detection"
        );

        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut method_calls = Vec::new();

        self.find_method_calls_in_node(root_node, code, &mut method_calls);

        debug_print!(
            self,
            "Enhanced method call detection found {} calls",
            method_calls.len()
        );
        method_calls
    }

    fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        self.find_implementations(code)
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        self.find_uses(code)
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        self.find_defines(code)
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        self.extract_imports(code, file_id)
    }

    fn language(&self) -> Language {
        Language::Rust
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut bindings = Vec::new();

        self.find_variable_types_in_node(root_node, code, &mut bindings);

        bindings
    }

    fn find_inherent_methods(&mut self, code: &str) -> Vec<(String, String, Range)> {
        self.find_inherent_methods(code)
    }
}

impl NodeTracker for RustParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id)
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

        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

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

        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].name.as_ref(), "Point");
        assert_eq!(symbols[0].kind, SymbolKind::Struct);
    }

    #[test]
    fn test_find_imports() {
        let mut parser = RustParser::new().unwrap();
        let file_id = FileId::new(1).unwrap();

        // Test simple import
        let code = "use std::vec::Vec;";
        let imports = parser.find_imports(code, file_id);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "std::vec::Vec");
        assert_eq!(imports[0].alias, None);
        assert!(!imports[0].is_glob);

        // Test aliased import
        let code = "use std::collections::HashMap as Map;";
        let imports = parser.find_imports(code, file_id);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "std::collections::HashMap");
        assert_eq!(imports[0].alias, Some("Map".to_string()));
        assert!(!imports[0].is_glob);

        // Test glob import
        let code = "use std::io::*;";
        let imports = parser.find_imports(code, file_id);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "std::io");
        assert_eq!(imports[0].alias, None);
        assert!(imports[0].is_glob);

        // Test grouped imports
        let code = "use std::collections::{HashMap, HashSet};";
        let imports = parser.find_imports(code, file_id);
        assert_eq!(imports.len(), 2);
        assert!(
            imports
                .iter()
                .any(|i| i.path == "std::collections::HashMap")
        );
        assert!(
            imports
                .iter()
                .any(|i| i.path == "std::collections::HashSet")
        );

        // Test multiple imports
        let code = r#"
            use std::vec::Vec;
            use std::io::{Read, Write};
            use super::module::Type;
        "#;
        let imports = parser.find_imports(code, file_id);
        assert_eq!(imports.len(), 4);
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

        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        // The parser correctly extracts 6 symbols including trait methods and struct field
        assert_eq!(symbols.len(), 6);

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_ref()).collect();
        assert!(names.contains(&"helper"));
        assert!(names.contains(&"Data"));
        assert!(names.contains(&"process"));
        assert!(names.contains(&"Operation"));
        assert!(names.contains(&"execute")); // Trait method is also extracted

        let functions: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(functions.len(), 2);

        let methods: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
        assert_eq!(methods.len(), 1); // The execute method
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
        let process_call = calls
            .iter()
            .find(|(caller, target, _)| *caller == "main" && *target == "process")
            .unwrap();
        assert_eq!(process_call.0, "main");
        assert_eq!(process_call.1, "process");

        // Check that process calls helper
        let helper_call = calls
            .iter()
            .find(|(caller, target, _)| *caller == "process" && *target == "helper")
            .unwrap();
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

        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(&code, file_id, &mut counter);

        // Should find: add, multiply, Point, Point::new, Point::distance,
        // Rectangle, Rectangle::width, Rectangle::height, Rectangle::area
        assert!(symbols.len() >= 4); // At least the top-level items

        let function_names: Vec<&str> = symbols
            .iter()
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
            println!("  {user} uses {used}");
        }

        // Rectangle uses Point (twice)
        let rect_uses: Vec<_> = uses
            .iter()
            .filter(|(user, _, _)| *user == "Rectangle")
            .collect();
        assert_eq!(rect_uses.len(), 2);
        assert!(rect_uses.iter().all(|(_, used, _)| *used == "Point"));

        // distance uses Point (twice for params) and f64 (once for return)
        let distance_uses: Vec<_> = uses
            .iter()
            .filter(|(user, _, _)| *user == "distance")
            .collect();

        // Check we have Point parameters and f64 return
        assert!(
            distance_uses
                .iter()
                .filter(|(_, used, _)| *used == "Point")
                .count()
                >= 2
        );
        assert!(
            distance_uses
                .iter()
                .filter(|(_, used, _)| *used == "f64")
                .count()
                >= 1
        );

        // get_center uses Rectangle and Point
        let center_uses: Vec<_> = uses
            .iter()
            .filter(|(user, _, _)| *user == "get_center")
            .collect();
        assert_eq!(center_uses.len(), 2);
        assert!(center_uses.iter().any(|(_, used, _)| *used == "Rectangle"));
        assert!(center_uses.iter().any(|(_, used, _)| *used == "Point"));
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
        let iterator_defines: Vec<_> = defines
            .iter()
            .filter(|(definer, _, _)| *definer == "Iterator")
            .collect();
        assert_eq!(iterator_defines.len(), 2); // next and size_hint
        assert!(
            iterator_defines
                .iter()
                .any(|(_, defined, _)| *defined == "next")
        );
        assert!(
            iterator_defines
                .iter()
                .any(|(_, defined, _)| *defined == "size_hint")
        );

        // Counter impl defines methods
        let counter_defines: Vec<_> = defines
            .iter()
            .filter(|(definer, _, _)| *definer == "Counter")
            .collect();
        assert_eq!(counter_defines.len(), 4); // new, increment, next, size_hint
        assert!(
            counter_defines
                .iter()
                .any(|(_, defined, _)| *defined == "new")
        );
        assert!(
            counter_defines
                .iter()
                .any(|(_, defined, _)| *defined == "increment")
        );
        assert!(
            counter_defines
                .iter()
                .any(|(_, defined, _)| *defined == "next")
        );
        assert!(
            counter_defines
                .iter()
                .any(|(_, defined, _)| *defined == "size_hint")
        );
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
        let display_impl = implementations
            .iter()
            .find(|(type_name, trait_name, _)| *type_name == "Point" && *trait_name == "Display")
            .expect("Should find Point implements Display");
        assert_eq!(display_impl.0, "Point");
        assert_eq!(display_impl.1, "Display");

        // Check Point implements std::fmt::Debug
        let debug_impl = implementations
            .iter()
            .find(|(type_name, trait_name, _)| {
                *type_name == "Point" && *trait_name == "std::fmt::Debug"
            })
            .expect("Should find Point implements std::fmt::Debug");
        assert_eq!(debug_impl.0, "Point");
        assert_eq!(debug_impl.1, "std::fmt::Debug");
    }

    #[test]
    fn test_find_inherent_methods() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            struct SimpleType {
                value: i32,
            }

            impl SimpleType {
                fn simple_method(&self) -> i32 {
                    self.value
                }

                fn another_method(&mut self) {
                    self.value += 1;
                }
            }

            impl Option<String> {
                fn option_method(&self) -> bool {
                    self.is_some()
                }
            }

            impl Vec<i32> {
                fn vec_method(&self) -> i32 {
                    self.iter().sum()
                }
            }

            // This has a trait, so not inherent
            impl Display for SimpleType {
                fn fmt(&self, f: &mut Formatter) -> Result {
                    write!(f, "{}", self.value)
                }
            }
        "#;

        let methods = parser.find_inherent_methods(code);

        // Should find 4 inherent methods
        assert_eq!(methods.len(), 4);

        // Check SimpleType methods
        assert!(
            methods
                .iter()
                .any(|(t, m, _)| t == "SimpleType" && m == "simple_method")
        );
        assert!(
            methods
                .iter()
                .any(|(t, m, _)| t == "SimpleType" && m == "another_method")
        );

        // Check complex type methods
        assert!(
            methods
                .iter()
                .any(|(t, m, _)| t == "Option<String>" && m == "option_method")
        );
        assert!(
            methods
                .iter()
                .any(|(t, m, _)| t == "Vec<i32>" && m == "vec_method")
        );

        // Should NOT find Display::fmt (it's a trait impl)
        assert!(!methods.iter().any(|(_, m, _)| m == "fmt"));
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
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        // Find documented_function
        let doc_fn = symbols
            .iter()
            .find(|s| s.name.as_ref() == "documented_function")
            .expect("Should find documented_function");
        assert!(doc_fn.doc_comment.is_some());
        let doc = doc_fn.doc_comment.as_ref().unwrap();
        assert!(doc.contains("well-documented function"));
        assert!(doc.contains("multiple lines"));

        // Find not_documented - should have no docs
        let no_doc_fn = symbols
            .iter()
            .find(|s| s.name.as_ref() == "not_documented")
            .expect("Should find not_documented");
        assert!(no_doc_fn.doc_comment.is_none());

        // Find DocumentedStruct with block comment
        let doc_struct = symbols
            .iter()
            .find(|s| s.name.as_ref() == "DocumentedStruct")
            .expect("Should find DocumentedStruct");
        assert!(doc_struct.doc_comment.is_some());
        let struct_doc = doc_struct.doc_comment.as_ref().unwrap();
        assert!(struct_doc.contains("block doc comment"));
        assert!(struct_doc.contains("block style"));

        // Find also_not_documented - should have no docs (3 asterisks)
        let also_no_doc = symbols
            .iter()
            .find(|s| s.name.as_ref() == "also_not_documented")
            .expect("Should find also_not_documented");
        assert!(also_no_doc.doc_comment.is_none());

        // Find edge_case - should have no docs (empty block)
        let edge = symbols
            .iter()
            .find(|s| s.name.as_ref() == "edge_case")
            .expect("Should find edge_case");
        assert!(edge.doc_comment.is_none());

        // Find simple_doc
        let simple = symbols
            .iter()
            .find(|s| s.name.as_ref() == "simple_doc")
            .expect("Should find simple_doc");
        assert!(simple.doc_comment.is_some());
        assert_eq!(
            simple.doc_comment.as_ref().unwrap().as_ref(),
            "Single line doc"
        );
    }

    #[test]
    fn test_visibility_detection() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
pub fn public_function() {}
fn private_function() {}
pub struct PublicStruct {}
struct PrivateStruct {}
        "#;

        let file_id = FileId::new(1).unwrap();
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        // Find public_function
        let pub_fn = symbols
            .iter()
            .find(|s| s.name.as_ref() == "public_function")
            .expect("Should find public_function");
        assert_eq!(pub_fn.visibility, crate::Visibility::Public);

        // Find private_function
        let priv_fn = symbols
            .iter()
            .find(|s| s.name.as_ref() == "private_function")
            .expect("Should find private_function");
        assert_eq!(priv_fn.visibility, crate::Visibility::Private);
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
        let mut counter = SymbolCounter::new();
        let symbols = parser.parse(code, file_id, &mut counter);

        // Test multi-line joining
        let multi = symbols
            .iter()
            .find(|s| s.name.as_ref() == "multi_line_doc")
            .unwrap();
        let doc = multi.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Line 1\nLine 2\nLine 3");

        // Test empty line preservation
        let empty = symbols
            .iter()
            .find(|s| s.name.as_ref() == "empty_line_doc")
            .unwrap();
        let doc = empty.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Empty doc\n\nAfter empty line");

        // Test compact lines
        let compact = symbols
            .iter()
            .find(|s| s.name.as_ref() == "compact_doc")
            .unwrap();
        let doc = compact.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Compact\nLines\nTogether");

        // Test trimming
        let trim = symbols
            .iter()
            .find(|s| s.name.as_ref() == "trim_test")
            .unwrap();
        let doc = trim.doc_comment.as_ref().unwrap();
        assert_eq!(doc.as_ref(), "Trim test");
    }

    #[test]
    fn test_find_variable_types() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
            fn main() {
                let config = Config::new();
                let server = Server { port: 8080 };
                let name = "test";
                let count = 42;
                let ref_config = &config;
                let opt = Some(config);
            }

            struct Config {
                host: String,
            }

            struct Server {
                port: u16,
            }
        "#;

        let bindings = parser.find_variable_types(code);

        // TODO: Add debug logging
        // eprintln!("DEBUG [find_variable_types]: Found {} variable bindings", bindings.len());
        // for (var_name, type_name, range) in &bindings {
        //     eprintln!("  - variable '{}' has type '{}' at line {}", var_name, type_name, range.start_line);
        // }

        // Should find direct type assignments
        assert!(
            bindings
                .iter()
                .any(|(var, typ, _)| *var == "config" && *typ == "Config")
        );
        assert!(
            bindings
                .iter()
                .any(|(var, typ, _)| *var == "server" && *typ == "Server")
        );

        // Literals don't have extractable type names
        assert!(!bindings.iter().any(|(var, _, _)| *var == "name"));
        assert!(!bindings.iter().any(|(var, _, _)| *var == "count"));

        // Reference types are skipped in our zero-cost implementation
        assert!(!bindings.iter().any(|(var, _, _)| *var == "ref_config"));

        // Complex types like Some(config) are not handled
        assert!(!bindings.iter().any(|(var, _, _)| *var == "opt"));

        // Verify we found exactly what we expected
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn test_find_variable_types_for_method_resolution() {
        let mut parser = RustParser::new().unwrap();
        // This tests the REAL use case: method resolution
        let code = r#"
            impl Display for Point {
                fn fmt(&self, f: &mut Formatter) -> Result {
                    write!(f, "({}, {})", self.x, self.y)
                }
            }

            fn process_data() {
                let point = Point::new(10, 20);
                let result = point.distance_to(&origin);

                let data = DataProcessor::default();
                data.process();

                let vec = Vec::new();
                vec.push(42);
            }
        "#;

        let bindings = parser.find_variable_types(code);

        // TODO: Add debug logging
        // eprintln!("DEBUG [method resolution test]: Found {} variable bindings", bindings.len());
        // for (var_name, type_name, range) in &bindings {
        //     eprintln!("  - variable '{}' has type '{}' at line {}", var_name, type_name, range.start_line);
        // }

        // These are the critical cases for method resolution:
        // 1. Constructor patterns (Type::new)
        assert!(
            bindings
                .iter()
                .any(|(var, typ, _)| *var == "point" && *typ == "Point")
        );
        assert!(
            bindings
                .iter()
                .any(|(var, typ, _)| *var == "data" && *typ == "DataProcessor")
        );
        assert!(
            bindings
                .iter()
                .any(|(var, typ, _)| *var == "vec" && *typ == "Vec")
        );

        // The indexer needs these type mappings to resolve:
        // - point.distance_to() -> looks up "point" -> finds "Point" -> resolves method
        // - data.process() -> looks up "data" -> finds "DataProcessor" -> resolves method
        // - vec.push() -> looks up "vec" -> finds "Vec" -> resolves method
    }

    #[test]
    fn test_find_method_calls_override() {
        use crate::parsing::LanguageParser;

        let mut parser = RustParser::new().unwrap();
        let code = r#"
            struct Data;
            impl Data {
                fn process(&self) {}
                fn new() -> Self { Data }
            }

            fn main() {
                let data = Data::new();  // Static call
                data.process();          // Instance call
                self.validate();         // Self call
            }
        "#;

        // Test the override method
        let method_calls = parser.find_method_calls(code);

        // Should find at least the same calls as legacy method
        let legacy_calls = parser.find_calls(code);
        assert_eq!(method_calls.len(), legacy_calls.len());

        // Verify we get MethodCall structs, not tuples
        for call in &method_calls {
            assert!(!call.caller.is_empty());
            assert!(!call.method_name.is_empty());
        }

        // Should have some method calls from the test code
        assert!(method_calls.len() >= 2); // At least Data::new and data.process
    }

    // Doc Comment extraction tests using examples/rust/doc_comments_comprehensive.rs patterns

    #[test]
    fn test_extract_doc_comment_basic_outer_line() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/// This is proper outer line documentation for documented_function
/// It spans multiple lines and should be collected together
/// Each line starts with exactly three slashes
pub fn documented_function() {
    // Regular comment inside function
}
        "#;

        // Parse code to get AST nodes
        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        // Find the function node
        let mut function_node = None;
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "function_item" {
                function_node = Some(child);
                break;
            }
        }

        let function_node = function_node.expect("Should find function node");

        // Test doc comment extraction
        let doc_comment = parser.extract_doc_comment(&function_node, code);
        let expected = "This is proper outer line documentation for documented_function\nIt spans multiple lines and should be collected together\nEach line starts with exactly three slashes";

        println!("=== BASELINE TEST: Basic Outer Line Doc Comments ===");
        println!("Expected: {expected}");
        println!("Actual:   {doc_comment:?}");

        assert!(doc_comment.is_some(), "Should extract doc comment");
        assert_eq!(
            doc_comment.unwrap(),
            expected,
            "Doc comment should match expected text"
        );
    }

    #[test]
    fn test_extract_doc_comment_exact_recognition() {
        let mut parser = RustParser::new().unwrap();

        // Test that 4+ slashes are NOT treated as doc comments
        let code = r#"
//// This is NOT a doc comment (4 slashes)
pub fn not_doc_commented_function() {}
        "#;

        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        let mut function_node = None;
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "function_item" {
                function_node = Some(child);
                break;
            }
        }

        let function_node = function_node.expect("Should find function node");
        let doc_comment = parser.extract_doc_comment(&function_node, code);

        println!("=== BASELINE TEST: Exact Recognition (4 slashes) ===");
        println!("Expected: None");
        println!("Actual:   {doc_comment:?}");

        assert!(
            doc_comment.is_none(),
            "4 slashes should NOT be treated as doc comment"
        );
    }

    #[test]
    fn test_extract_doc_comment_block_format() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/** 
 * This is proper outer block documentation for block_documented_function
 * It uses the standard block comment format
 * With asterisks for formatting
 */
pub fn block_documented_function() {
    /* Regular block comment inside function */
}
        "#;

        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        let mut function_node = None;
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "function_item" {
                function_node = Some(child);
                break;
            }
        }

        let function_node = function_node.expect("Should find function node");
        let doc_comment = parser.extract_doc_comment(&function_node, code);

        println!("=== BASELINE TEST: Block Doc Comments ===");
        println!("Expected: (block comment content)");
        println!("Actual:   {doc_comment:?}");

        assert!(doc_comment.is_some(), "Should extract block doc comment");
        // Note: We allow formatting differences in block comments
        let content = doc_comment.unwrap();
        assert!(
            content.contains("block documentation for block_documented_function"),
            "Should contain main text"
        );
    }

    #[test]
    fn test_extract_doc_comment_multiple_blocks() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/// First documentation block
/// Multiple lines in first block

/// Second documentation block  
/// This should also be collected

/** 
 * Third block in different format
 * Should be combined with the line comments above
 */
pub fn multiple_comment_blocks() {}
        "#;

        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        let mut function_node = None;
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "function_item" {
                function_node = Some(child);
                break;
            }
        }

        let function_node = function_node.expect("Should find function node");
        let doc_comment = parser.extract_doc_comment(&function_node, code);

        println!("=== BASELINE TEST: Multiple Comment Blocks ===");
        println!("Expected: All blocks combined");
        println!("Actual:   {doc_comment:?}");

        assert!(
            doc_comment.is_some(),
            "Should extract combined doc comments"
        );
        let content = doc_comment.unwrap();
        assert!(
            content.contains("First documentation block"),
            "Should contain first block"
        );
        assert!(
            content.contains("Second documentation block"),
            "Should contain second block"
        );
        assert!(
            content.contains("Third block in different format"),
            "Should contain third block"
        );
    }

    #[test]
    fn test_extract_doc_comment_inner_docs_now_supported() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/// Documentation for a public struct
/// This should be extracted as the doc comment
pub struct DocumentedStruct {
    //! Inner documentation for the struct itself
    //! This describes the struct from the inside
    
    /// Documentation for a field
    pub documented_field: String,
}
        "#;

        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        let mut struct_node = None;
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "struct_item" {
                struct_node = Some(child);
                break;
            }
        }

        let struct_node = struct_node.expect("Should find struct node");
        let doc_comment = parser.extract_doc_comment(&struct_node, code);

        println!("=== ENHANCED TEST: Inner Docs Now Supported! ===");
        println!("Expected: Outer docs + Inner docs combined");
        println!("Actual:   {doc_comment:?}");
        println!("SUCCESS: Inner docs are now included!");

        assert!(doc_comment.is_some(), "Should extract combined doc comment");
        let content = doc_comment.unwrap();
        assert!(
            content.contains("Documentation for a public struct"),
            "Should contain outer docs"
        );
        // NEW: Inner docs are now supported!
        assert!(
            content.contains("Inner documentation for the struct"),
            "Inner docs now supported!"
        );
    }

    #[test]
    fn test_classify_doc_comment_exact_rules() {
        let parser = RustParser::new().unwrap();

        println!("=== FOUNDATION TEST: Comment Type Classification ===");

        // Test outer line comments (exactly 3 slashes)
        let comment = "/// This is an outer line doc comment";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::OuterLine);

        // Test 4+ slashes should NOT be doc comments
        let comment = "//// This is NOT a doc comment";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::NotDocComment);

        // Test inner line comments
        let comment = "//! This is an inner line doc comment";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::InnerLine);

        // Test outer block comments (exactly 2 asterisks)
        let comment = "/** This is an outer block doc comment */";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::OuterBlock);

        // Test 3+ asterisks should NOT be doc comments
        let comment = "/*** This is NOT a doc comment */";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::NotDocComment);

        // Test empty block should NOT be doc comment
        let comment = "**/";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::NotDocComment);

        // Test inner block comments
        let comment = "/*! This is an inner block doc comment */";
        let result = parser.classify_doc_comment(comment);
        println!("Test: '{comment}' -> {result:?}");
        assert_eq!(result, DocCommentType::InnerBlock);

        println!("=== All comment type classifications correct ===");
    }

    #[test]
    fn test_inner_doc_comments_extraction() {
        let mut parser = RustParser::new().unwrap();
        let code = r#"
/// Outer documentation for DocumentedStruct
pub struct DocumentedStruct {
    //! Inner documentation for the struct itself
    //! This describes the struct from the inside
    
    /// Field documentation
    pub field: String,
}
        "#;

        // Parse code to get AST nodes
        let tree = parser.parser.parse(code, None).unwrap();
        let root = tree.root_node();

        // Find the struct node
        let mut struct_node = None;
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "struct_item" {
                struct_node = Some(child);
                break;
            }
        }

        let struct_node = struct_node.expect("Should find struct node");

        // Test enhanced doc comment extraction
        let doc_comment = parser.extract_doc_comment(&struct_node, code);

        println!("=== ENHANCED TEST: Inner Doc Comments ===");
        println!("Expected: Outer docs + Inner docs combined");
        println!("Actual:   {doc_comment:?}");

        assert!(doc_comment.is_some(), "Should extract combined doc comment");
        let content = doc_comment.unwrap();

        // Should contain both outer and inner docs
        assert!(
            content.contains("Outer documentation for DocumentedStruct"),
            "Should contain outer docs"
        );
        assert!(
            content.contains("Inner documentation for the struct itself"),
            "Should contain inner docs"
        );
        assert!(
            content.contains("This describes the struct from the inside"),
            "Should contain full inner docs"
        );

        println!("=== SUCCESS: Inner doc comments now supported! ===");
    }

    #[test]
    fn test_qualified_calls() {
        let code = r#"
pub fn init_config_file() {
    crate::init::init_global_dirs();
}

pub fn init_global_dirs() {
    println!("Initializing");
}
"#;

        let mut parser = RustParser::new().unwrap();
        let calls = parser.find_calls(code);

        println!("\n=== Testing qualified calls extraction ===");
        println!("Found {} calls:", calls.len());
        for (from, to, _) in &calls {
            println!("  '{from}' -> '{to}'");
        }

        // Check that we find the qualified call
        let has_qualified_call = calls
            .iter()
            .any(|(f, t, _)| *f == "init_config_file" && *t == "crate::init::init_global_dirs");

        assert!(
            has_qualified_call,
            "Should find call from init_config_file to crate::init::init_global_dirs\nFound calls: {calls:?}"
        );
    }
}
