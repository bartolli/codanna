//! C language parser implementation

use crate::parsing::method_call::MethodCall;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Import, Language, LanguageParser, NodeTracker, NodeTrackingState, ParserContext,
    ScopeType,
};
use crate::types::{Range, SymbolCounter};
use crate::{FileId, Symbol, SymbolKind};
use std::any::Any;
use tree_sitter::{Node, Parser};

pub struct CParser {
    parser: Parser,
    context: ParserContext,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for CParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CParser").field("language", &"C").finish()
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
            context: ParserContext::new(),
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Parse C code and extract symbols
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

    /// Create a symbol with C-specific handling
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

        let mut symbol = Symbol::new(symbol_id, name.to_string(), kind, file_id, range);

        // Set scope context based on parser's current scope
        symbol.scope_context = Some(self.context.current_scope_context());

        // C has simpler visibility - most symbols are public by default
        // Static storage class makes symbols private to the compilation unit
        if let Some(parent) = name_node.parent() {
            let mut is_static = false;
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "storage_class_specifier" {
                    let storage_text = &code[child.byte_range()];
                    if storage_text == "static" {
                        is_static = true;
                        break;
                    }
                }
            }

            if is_static {
                symbol = symbol.with_visibility(crate::Visibility::Private);
            } else {
                symbol = symbol.with_visibility(crate::Visibility::Public);
            }
        } else {
            symbol = symbol.with_visibility(crate::Visibility::Public);
        }

        Some(symbol)
    }

    /// Helper to find function name node in C's complex declarator structure
    fn find_function_name_node(declarator: Node) -> Option<Node> {
        // C function declarators can be nested: function_declarator -> declarator -> identifier
        match declarator.kind() {
            "function_declarator" => {
                if let Some(inner) = declarator.child_by_field_name("declarator") {
                    Self::find_function_name_node(inner)
                } else {
                    None
                }
            }
            "identifier" => Some(declarator),
            "parenthesized_declarator" => {
                if let Some(inner) = declarator.child_by_field_name("declarator") {
                    Self::find_function_name_node(inner)
                } else {
                    None
                }
            }
            _ => {
                // Search children for identifier
                for child in declarator.children(&mut declarator.walk()) {
                    if child.kind() == "identifier" {
                        return Some(child);
                    }
                    if let Some(found) = Self::find_function_name_node(child) {
                        return Some(found);
                    }
                }
                None
            }
        }
    }

    /// Helper to find declarator name for variables and parameters
    fn find_declarator_name(node: Node) -> Option<Node> {
        match node.kind() {
            "identifier" => Some(node),
            "init_declarator" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    Self::find_declarator_name(declarator)
                } else {
                    None
                }
            }
            "parameter_declaration" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    Self::find_declarator_name(declarator)
                } else {
                    None
                }
            }
            "pointer_declarator" | "array_declarator" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    Self::find_declarator_name(declarator)
                } else {
                    None
                }
            }
            _ => {
                // Search children for identifier
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "identifier" {
                        return Some(child);
                    }
                    if let Some(found) = Self::find_declarator_name(child) {
                        return Some(found);
                    }
                }
                None
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
            "translation_unit" => {
                self.register_handled_node("translation_unit", node.kind_id());
                // Root node - establish file-level scope context
                // This doesn't create symbols but provides the top-level context for all other nodes
                self.context.enter_scope(ScopeType::Module);

                // Process all top-level declarations
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

                self.context.exit_scope();
                return; // Skip default traversal
            }
            "function_definition" => {
                self.register_handled_node("function_definition", node.kind_id());
                // C function names are nested in declarator structure
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    if let Some(name_node) = Self::find_function_name_node(declarator) {
                        if let Some(symbol) = self.create_symbol(
                            counter,
                            node,
                            name_node,
                            SymbolKind::Function,
                            file_id,
                            code,
                        ) {
                            symbols.push(symbol);
                        }
                    }
                }

                // Enter function scope for nested declarations
                self.context
                    .enter_scope(ScopeType::Function { hoisting: false });

                // Process function body for nested symbols
                for child in node.children(&mut node.walk()) {
                    // Skip declarator (already processed) and parameter lists
                    if child.kind() != "function_declarator" && child.kind() != "parameter_list" {
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

                self.context.exit_scope();
                return; // Skip default traversal
            }
            "struct_specifier" => {
                self.register_handled_node("struct_specifier", node.kind_id());
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Struct,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }

                // Process struct fields
                if let Some(body) = node.child_by_field_name("body") {
                    self.context.enter_scope(ScopeType::Class);
                    for child in body.children(&mut body.walk()) {
                        self.extract_symbols_from_node(
                            child,
                            code,
                            file_id,
                            symbols,
                            counter,
                            depth + 1,
                        );
                    }
                    self.context.exit_scope();
                }
            }
            "union_specifier" => {
                self.register_handled_node("union_specifier", node.kind_id());
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Struct,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }

                // Process union fields
                if let Some(body) = node.child_by_field_name("body") {
                    self.context.enter_scope(ScopeType::Class);
                    for child in body.children(&mut body.walk()) {
                        self.extract_symbols_from_node(
                            child,
                            code,
                            file_id,
                            symbols,
                            counter,
                            depth + 1,
                        );
                    }
                    self.context.exit_scope();
                }
            }
            "enum_specifier" => {
                self.register_handled_node("enum_specifier", node.kind_id());
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Enum,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }

                // Process enum values
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        if child.kind() == "enumerator" {
                            self.register_handled_node("enumerator", child.kind_id());
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
            "declaration" => {
                self.register_handled_node("declaration", node.kind_id());
                // Handle variable declarations
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "init_declarator" {
                        if let Some(name_node) = Self::find_declarator_name(child) {
                            if let Some(symbol) = self.create_symbol(
                                counter,
                                child,
                                name_node,
                                SymbolKind::Variable,
                                file_id,
                                code,
                            ) {
                                symbols.push(symbol);
                            }
                        }
                    }
                }
            }
            "init_declarator" => {
                self.register_handled_node("init_declarator", node.kind_id());
                // Handle variable initialization (int x = 5, Rectangle *rect = malloc(...), etc.)
                if let Some(name_node) = Self::find_declarator_name(node) {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Variable,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "compound_statement" => {
                self.register_handled_node("compound_statement", node.kind_id());
                // Handle block statements { ... } - establish block scope for nested declarations
                self.context.enter_scope(ScopeType::Block);

                // Process all statements and declarations within the block
                for child in node.children(&mut node.walk()) {
                    // Skip braces, process the contents
                    if child.kind() != "{" && child.kind() != "}" {
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

                self.context.exit_scope();
                return; // Skip default traversal
            }
            "parameter_declaration" => {
                self.register_handled_node("parameter_declaration", node.kind_id());
                // Handle function parameters
                if let Some(name_node) = Self::find_declarator_name(node) {
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Parameter,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "field_declaration" => {
                self.register_handled_node("field_declaration", node.kind_id());
                // Handle struct/union field declarations
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "field_declarator" {
                        if let Some(name_node) = child.child(0) {
                            if name_node.kind() == "field_identifier" {
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
            }
            "preproc_include" => {
                self.register_handled_node("preproc_include", node.kind_id());
                // Track preprocessor includes for dependency resolution
                // This helps with cross-file symbol resolution and dependency analysis
                if let Some(path_node) = node.child_by_field_name("path") {
                    // Create an import symbol for the included file
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        path_node,
                        SymbolKind::Macro,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "preproc_def" => {
                self.register_handled_node("preproc_def", node.kind_id());
                // Track preprocessor macro definitions for symbol resolution
                // This helps with macro expansion and cross-file symbol analysis
                if let Some(name_node) = node.child_by_field_name("name") {
                    // Create a macro symbol for the definition
                    if let Some(symbol) = self.create_symbol(
                        counter,
                        node,
                        name_node,
                        SymbolKind::Macro,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            "if_statement" => {
                self.register_handled_node("if_statement", node.kind_id());
                // Control flow statement - important for scope and flow analysis
                // Enter block scope for the if statement body
                self.context.enter_scope(ScopeType::Block);

                // Process all children (condition, then clause, else clause)
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

                self.context.exit_scope();
                return; // Skip default traversal since we handled children
            }
            "while_statement" => {
                self.register_handled_node("while_statement", node.kind_id());
                // Control flow statement - important for scope and flow analysis
                // Enter block scope for the while loop body
                self.context.enter_scope(ScopeType::Block);

                // Process all children (condition, body)
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

                self.context.exit_scope();
                return; // Skip default traversal since we handled children
            }
            "for_statement" => {
                self.register_handled_node("for_statement", node.kind_id());
                // Control flow statement - important for scope and flow analysis
                // Enter block scope for the for loop body
                self.context.enter_scope(ScopeType::Block);

                // Process all children (initialization, condition, update, body)
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

                self.context.exit_scope();
                return; // Skip default traversal since we handled children
            }
            "do_statement" => {
                self.register_handled_node("do_statement", node.kind_id());
                // Control flow statement - important for scope and flow analysis
                // Enter block scope for the do-while loop body
                self.context.enter_scope(ScopeType::Block);

                // Process all children (body, condition)
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

                self.context.exit_scope();
                return; // Skip default traversal since we handled children
            }
            "switch_statement" => {
                self.register_handled_node("switch_statement", node.kind_id());
                // Control flow statement - important for scope and flow analysis
                // Enter block scope for the switch statement body
                self.context.enter_scope(ScopeType::Block);

                // Process all children (expression, case statements)
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

                self.context.exit_scope();
                return; // Skip default traversal since we handled children
            }
            "case_statement" => {
                self.register_handled_node("case_statement", node.kind_id());
                // Control flow statement - case labels within switch statements
                // These don't create new scopes but are important for control flow analysis
                // Process all children (label, statements)
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
                return; // Skip default traversal since we handled children
            }
            "expression_statement" => {
                self.register_handled_node("expression_statement", node.kind_id());
                // Expression statements - important for tracking expressions that might contain symbols
                // These typically don't create symbols but are part of comprehensive AST coverage
                // Process all children
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
                return; // Skip default traversal since we handled children
            }
            "continue_statement" => {
                self.register_handled_node("continue_statement", node.kind_id());
                // Continue statement - control flow jump statement
                // These don't create symbols but are important for control flow analysis
                // No children to process, just mark as handled
            }
            "compound_literal_expression" => {
                self.register_handled_node("compound_literal_expression", node.kind_id());
                // Compound literals like (struct Point){.x=1, .y=2}
                // These may contain initializer_pair children that we want to track
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
                return; // Skip default traversal since we handled children
            }
            "initializer_pair" => {
                self.register_handled_node("initializer_pair", node.kind_id());
                // Designated initializers like .field = value or [index] = value
                // These don't create symbols but are important for initialization patterns
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
                return; // Skip default traversal since we handled children
            }
            "linkage_specification" => {
                self.register_handled_node("linkage_specification", node.kind_id());
                // extern "C" blocks and other linkage specifications
                // Important for cross-language compatibility analysis
                self.context.enter_scope(ScopeType::Block);

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

                self.context.exit_scope();
                return; // Skip default traversal since we handled children
            }
            "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_else" => {
                self.register_handled_node(node.kind(), node.kind_id());
                // Conditional preprocessing directives - important for build-time logic
                // These control compilation and symbol visibility
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
                return; // Skip default traversal since we handled children
            }
            "preproc_call" => {
                self.register_handled_node("preproc_call", node.kind_id());
                // Function-like macro invocations
                // These are important for macro expansion analysis
                if let Some(name_node) = node.child(0) {
                    if name_node.kind() == "identifier" {
                        // Track macro calls as macro symbols for analysis
                        if let Some(symbol) = self.create_symbol(
                            counter,
                            node,
                            name_node,
                            SymbolKind::Macro,
                            file_id,
                            code,
                        ) {
                            symbols.push(symbol);
                        }
                    }
                }

                // Process remaining children for arguments
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
                return; // Skip default traversal since we handled children
            }
            "attribute_declaration" => {
                self.register_handled_node("attribute_declaration", node.kind_id());
                // __attribute__ declarations for compiler directives
                // Important for understanding code structure and optimization hints
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
                return; // Skip default traversal since we handled children
            }
            _ => {}
        }

        // Process children
        for child in node.children(&mut node.walk()) {
            self.extract_symbols_from_node(child, code, file_id, symbols, counter, depth + 1);
        }
    }

    fn extract_calls_from_node(node: Node, code: &str, calls: &mut Vec<MethodCall>) {
        if node.kind() == "call_expression" {
            if let Some(function_node) = node.child_by_field_name("function") {
                let function_name = &code[function_node.byte_range()];
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

    /// Find function calls in AST node recursively
    fn find_calls_in_node<'a>(
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
            if let Some(child) = node.child(i as u32) {
                Self::find_calls_in_node(child, code, calls);
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
}

impl NodeTracker for CParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id)
    }
}

impl LanguageParser for CParser {
    fn parse(
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

        // Start recursion at depth 0
        self.extract_symbols_from_node(root_node, code, file_id, &mut symbols, symbol_counter, 0);

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

        Self::find_calls_in_node(root_node, code, &mut calls);
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
        Language::C
    }
}
