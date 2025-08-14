//! Swift language parser implementation
//!
//! This parser handles Swift source code analysis using tree-sitter-swift.
//! It extracts symbols, relationships, and documentation from Swift code.

use crate::indexing::Import;
use crate::parsing::method_call::MethodCall;
use crate::parsing::{Language, LanguageParser};
use crate::types::{FileId, Range, SymbolCounter, SymbolKind};
use crate::{Symbol, Visibility};
use tree_sitter::{Node, Parser};

pub struct SwiftParser {
    parser: Parser,
    #[allow(dead_code)]
    debug: bool,
}

impl std::fmt::Debug for SwiftParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwiftParser")
            .field("language", &"Swift")
            .finish()
    }
}

impl SwiftParser {
    pub fn new() -> Result<Self, String> {
        Self::with_debug(false)
    }

    pub fn with_debug(debug: bool) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_swift::LANGUAGE.into())
            .map_err(|e| format!("Failed to set Swift language: {e}"))?;

        Ok(Self { parser, debug })
    }

    /// Parse Swift code and extract symbols
    pub fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut SymbolCounter) -> Vec<Symbol> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut symbols = Vec::new();

        if self.debug {
            eprintln!("[SWIFT DEBUG] Starting parse, root node kind: {}", root_node.kind());
            self.debug_print_tree(root_node, code, 0);
        }

        self.extract_symbols_from_node(root_node, code, file_id, symbol_counter, &mut symbols, None);

        if self.debug {
            eprintln!("[SWIFT DEBUG] Parse complete, found {} symbols", symbols.len());
            for symbol in &symbols {
                eprintln!("  - {} ({:?}) at line {}", symbol.name, symbol.kind, symbol.range.start_line);
            }
        }

        symbols
    }

    /// Debug helper to print the AST tree structure
    fn debug_print_tree(&self, node: Node, code: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        let text = if node.child_count() == 0 {
            let text = &code[node.byte_range()];
            if text.len() > 50 {
                // Find a safe UTF-8 boundary to truncate at
                let mut truncate_at = 50;
                while truncate_at > 0 && !text.is_char_boundary(truncate_at) {
                    truncate_at -= 1;
                }
                format!(" = '{}'...", &text[..truncate_at].replace('\n', "\\n"))
            } else {
                format!(" = '{}'", text.replace('\n', "\\n"))
            }
        } else {
            String::new()
        };
        
        eprintln!("{}[{}]{}", indent, node.kind(), text);
        
        // Recursively print children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if depth < 3 {  // Limit depth to avoid too much output
                    self.debug_print_tree(child, code, depth + 1);
                }
            }
        }
    }

    fn extract_symbols_from_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbol_counter: &mut SymbolCounter,
        symbols: &mut Vec<Symbol>,
        parent_context: Option<&str>,
    ) {
        if self.debug {
            eprintln!("[SWIFT DEBUG] Processing node: {} (parent: {:?})", node.kind(), parent_context);
        }

        // Determine if we're inside a type context (for method vs function detection)
        let current_context = match node.kind() {
            "class_declaration" | "struct_declaration" | "enum_declaration" | 
            "extension_declaration" | "protocol_declaration" => Some(node.kind()),
            _ => parent_context,
        };

        match node.kind() {
            // Function declarations
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    // Determine if this is a method or a function based on context
                    let kind = if parent_context.is_some() {
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Function '{}' inside {}, marking as Method", 
                                     &code[name_node.byte_range()], parent_context.unwrap());
                        }
                        SymbolKind::Method
                    } else {
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Top-level function '{}', marking as Function", 
                                     &code[name_node.byte_range()]);
                        }
                        SymbolKind::Function
                    };
                    
                    if let Some(symbol) = self.create_symbol_with_function_context(
                        symbol_counter,
                        name_node,
                        kind,
                        file_id,
                        code,
                        node, // Pass the function_declaration node for modifier detection
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            // class_declaration is used for struct, class, enum, and extension in tree-sitter-swift
            "class_declaration" => {
                // Check what type of declaration this actually is by looking for keyword tokens
                let mut is_struct = false;
                let mut _is_class = false;
                let mut is_enum = false;
                let mut is_extension = false;
                
                // Look for the keyword token to determine the actual type
                for child in node.children(&mut node.walk()) {
                    match child.kind() {
                        "struct" => {
                            is_struct = true;
                            break;
                        }
                        "class" => {
                            _is_class = true;
                            break;
                        }
                        "enum" => {
                            is_enum = true;
                            break;
                        }
                        "extension" => {
                            is_extension = true;
                            break;
                        }
                        _ => {}
                    }
                }
                
                // Handle extensions specially
                if is_extension {
                    // Extract the type being extended
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let extended_type = &code[name_node.byte_range()];
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found extension of type: {}", extended_type);
                        }
                        
                        // Process children with the extended type as context
                        for child in node.children(&mut node.walk()) {
                            self.extract_symbols_from_node(
                                child, 
                                code, 
                                file_id, 
                                symbol_counter, 
                                symbols, 
                                Some(extended_type)
                            );
                        }
                        return; // Don't process children again
                    }
                    // Continue processing children to find methods and properties in the extension
                } else if let Some(name_node) = node.child_by_field_name("name") {
                    let kind = if is_struct {
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found struct: {}", &code[name_node.byte_range()]);
                        }
                        SymbolKind::Struct
                    } else if is_enum {
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found enum: {}", &code[name_node.byte_range()]);
                        }
                        SymbolKind::Enum
                    } else {
                        // Default to class (includes actual classes and actor types)
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found class: {}", &code[name_node.byte_range()]);
                        }
                        SymbolKind::Class
                    };
                    
                    if let Some(symbol) = self.create_symbol_with_context(
                        symbol_counter,
                        name_node,
                        kind,
                        file_id,
                        code,
                        Some(node), // Pass the class_declaration node for @MainActor detection
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            // Protocol declarations (interfaces/traits in Swift)
            "protocol_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        symbol_counter,
                        name_node,
                        SymbolKind::Trait,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            // Type alias declarations
            "typealias_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Some(symbol) = self.create_symbol(
                        symbol_counter,
                        name_node,
                        SymbolKind::TypeAlias,
                        file_id,
                        code,
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            // Property/variable declarations
            "property_declaration" => {
                // First, look for property wrappers (attributes)
                let mut property_wrappers = Vec::new();
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "modifiers" {
                        // Look inside modifiers for attributes
                        for modifier_child in child.children(&mut child.walk()) {
                            if modifier_child.kind() == "attribute" {
                                // Extract the wrapper name (e.g., @State, @Published)
                                let wrapper_text = &code[modifier_child.byte_range()];
                                property_wrappers.push(wrapper_text.to_string());
                                if self.debug {
                                    eprintln!("[SWIFT DEBUG] Found property wrapper: {}", wrapper_text);
                                }
                            }
                        }
                    } else if child.kind() == "attribute" {
                        // Direct attribute (fallback)
                        let wrapper_text = &code[child.byte_range()];
                        property_wrappers.push(wrapper_text.to_string());
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found property wrapper: {}", wrapper_text);
                        }
                    }
                }
                
                // Try multiple strategies to find property names
                let mut found_property = false;
                
                // Strategy 1: Look for pattern bindings with identifiers
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "value_binding_pattern" {
                        // Look for the pattern child
                        for pattern_child in child.children(&mut child.walk()) {
                            if pattern_child.kind() == "pattern" {
                                // Look for simple_identifier within pattern
                                for id_child in pattern_child.children(&mut pattern_child.walk()) {
                                    if id_child.kind() == "simple_identifier" {
                                        if self.debug {
                                            eprintln!("[SWIFT DEBUG] Found property: {}", &code[id_child.byte_range()]);
                                        }
                                        if let Some(mut symbol) = self.create_symbol(
                                            symbol_counter,
                                            id_child,
                                            SymbolKind::Field,
                                            file_id,
                                            code,
                                        ) {
                                            // Add property wrapper info to signature if present
                                            if !property_wrappers.is_empty() {
                                                let wrapper_str = property_wrappers.join(" ");
                                                symbol = symbol.with_signature(wrapper_str);
                                                if self.debug {
                                                    eprintln!("[SWIFT DEBUG] Added wrappers to property: {}", property_wrappers.join(", "));
                                                }
                                            }
                                            symbols.push(symbol);
                                            found_property = true;
                                        }
                                    }
                                }
                            }
                        }
                    } else if child.kind() == "pattern" {
                        // Direct pattern child (older tree-sitter version compatibility)
                        for id_child in child.children(&mut child.walk()) {
                            if id_child.kind() == "simple_identifier" {
                                if self.debug {
                                    eprintln!("[SWIFT DEBUG] Found property (direct pattern): {}", &code[id_child.byte_range()]);
                                }
                                if let Some(mut symbol) = self.create_symbol(
                                    symbol_counter,
                                    id_child,
                                    SymbolKind::Field,
                                    file_id,
                                    code,
                                ) {
                                    // Add property wrapper info to signature if present
                                    if !property_wrappers.is_empty() {
                                        let wrapper_str = property_wrappers.join(" ");
                                        symbol = symbol.with_signature(wrapper_str);
                                        if self.debug {
                                            eprintln!("[SWIFT DEBUG] Added wrappers to property: {}", property_wrappers.join(", "));
                                        }
                                    }
                                    symbols.push(symbol);
                                    found_property = true;
                                }
                            }
                        }
                    }
                }
                
                if self.debug && !found_property {
                    eprintln!("[SWIFT DEBUG] Could not extract property name from property_declaration");
                }
            }
            // Initializer declarations
            "init_declaration" => {
                // Swift initializers are special methods
                if self.debug {
                    eprintln!("[SWIFT DEBUG] Found initializer");
                }
                
                // Create a synthetic name node for "init"
                let init_name = "init";
                let range = Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                );
                
                let symbol_id = symbol_counter.next_id();
                
                let mut symbol = Symbol::new(symbol_id, init_name, SymbolKind::Method, file_id, range);
                
                // Extract doc comments if available
                if let Some(doc) = self.extract_doc_comments(&node, code) {
                    symbol = symbol.with_doc(doc);
                }
                
                symbols.push(symbol);
            }
            // Computed properties
            "computed_property" => {
                // Look for the property name - usually in a pattern or identifier child
                let mut found_name = None;
                
                // First, check direct pattern child
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "pattern" {
                        for id_child in child.children(&mut child.walk()) {
                            if id_child.kind() == "simple_identifier" {
                                found_name = Some(id_child);
                                break;
                            }
                        }
                    } else if child.kind() == "simple_identifier" {
                        found_name = Some(child);
                        break;
                    }
                }
                
                // If we still haven't found it, look in parent's value_binding_pattern
                if found_name.is_none() {
                    if let Some(parent) = node.parent() {
                        for sibling in parent.children(&mut parent.walk()) {
                            if sibling.kind() == "value_binding_pattern" {
                                for pattern_child in sibling.children(&mut sibling.walk()) {
                                    if pattern_child.kind() == "pattern" {
                                        for id_child in pattern_child.children(&mut pattern_child.walk()) {
                                            if id_child.kind() == "simple_identifier" {
                                                found_name = Some(id_child);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            if found_name.is_some() {
                                break;
                            }
                        }
                    }
                }
                
                if let Some(name_node) = found_name {
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found computed property: {}", &code[name_node.byte_range()]);
                    }
                    
                    // Check for attributes like @ViewBuilder by looking at parent or sibling modifiers
                    let mut result_builder_attrs = Vec::new();
                    let mut is_main_actor = false;
                    
                    // Check if parent has modifiers (for patterns like @ViewBuilder var name: Type)
                    if let Some(parent) = name_node.parent() {
                        if let Some(grandparent) = parent.parent() {
                            for sibling in grandparent.children(&mut grandparent.walk()) {
                                if sibling.kind() == "modifiers" {
                                    // Extract result builder attributes
                                    let builders = self.extract_result_builder_attributes(sibling, code);
                                    result_builder_attrs.extend(builders);
                                    
                                    // Also check for @MainActor
                                    if self.extract_main_actor_attribute(sibling, code) {
                                        is_main_actor = true;
                                    }
                                    
                                    if self.debug && !result_builder_attrs.is_empty() {
                                        eprintln!("[SWIFT DEBUG] Found result builders on computed property {}: {}", 
                                            &code[name_node.byte_range()], result_builder_attrs.join(", "));
                                    }
                                }
                            }
                        }
                    }
                    
                    if let Some(mut symbol) = self.create_symbol(
                        symbol_counter,
                        name_node,
                        SymbolKind::Field,
                        file_id,
                        code,
                    ) {
                        // Build signature with attributes
                        let mut signature_parts = Vec::new();
                        if is_main_actor {
                            signature_parts.push("@MainActor");
                        }
                        signature_parts.extend(result_builder_attrs.iter().map(|s| s.as_str()));
                        
                        if !signature_parts.is_empty() {
                            let signature = signature_parts.join(" ");
                            symbol = symbol.with_signature(signature.clone());
                            if self.debug {
                                eprintln!("[SWIFT DEBUG] Added signature to computed property {}: {}", 
                                    &code[name_node.byte_range()], signature);
                            }
                        }
                        
                        symbols.push(symbol);
                    }
                } else if self.debug {
                    eprintln!("[SWIFT DEBUG] Could not extract name from computed_property");
                }
            }
            _ => {}
        }

        // Recursively process children with context
        for child in node.children(&mut node.walk()) {
            self.extract_symbols_from_node(child, code, file_id, symbol_counter, symbols, current_context);
        }
    }

    fn create_symbol_with_context(
        &self,
        counter: &mut SymbolCounter,
        name_node: Node,
        kind: SymbolKind,
        file_id: FileId,
        code: &str,
        context_node: Option<Node>,
    ) -> Option<Symbol> {
        let name = &code[name_node.byte_range()];

        let symbol_id = counter.next_id();

        let range = Range::new(
            name_node.start_position().row as u32,
            name_node.start_position().column as u16,
            name_node.end_position().row as u32,
            name_node.end_position().column as u16,
        );

        // Find the parent node that might have doc comments
        let doc_node = name_node.parent()?;
        let doc_comment = self.extract_doc_comments(&doc_node, code);

        // Default to Public visibility (represents Swift's default 'internal' visibility)
        // Swift's 'internal' means accessible within the module, which maps to Public in codanna
        let mut symbol = Symbol::new(symbol_id, name, kind, file_id, range)
            .with_visibility(Visibility::Public);

        // Check for explicit visibility modifiers
        if let Some(parent) = name_node.parent() {
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "modifiers" {
                    for modifier in child.children(&mut child.walk()) {
                        let modifier_text = &code[modifier.byte_range()];
                        match modifier_text {
                            "public" | "open" => {
                                // Already defaulted to Public
                                break;
                            }
                            "internal" => {
                                // Explicit internal is same as default (Public in codanna)
                                break;
                            }
                            "private" | "fileprivate" => {
                                symbol = symbol.with_visibility(Visibility::Private);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Check for modifiers (async, throws, @MainActor, @ViewBuilder) 
        let mut function_modifiers = Vec::new();
        let mut is_main_actor = false;
        let mut result_builder_attrs = Vec::new();
        
        // First check if we have a context node (e.g., function_declaration or class_declaration)
        if let Some(context) = context_node {
            // Check children of the context node for modifiers
            for child in context.children(&mut context.walk()) {
                match child.kind() {
                    "modifiers" => {
                        // Check for @MainActor attribute first
                        if self.extract_main_actor_attribute(child, code) {
                            is_main_actor = true;
                            if self.debug {
                                eprintln!("[SWIFT DEBUG] Found @MainActor attribute for {:?}: {}", kind, name);
                            }
                        }
                        
                        // Check for result builder attributes like @ViewBuilder
                        let builders = self.extract_result_builder_attributes(child, code);
                        if !builders.is_empty() {
                            result_builder_attrs.extend(builders.clone());
                            if self.debug {
                                eprintln!("[SWIFT DEBUG] Found result builders for {:?} {}: {}", kind, name, builders.join(", "));
                            }
                        }
                        
                        // Check inside modifiers node for async/throws
                        for modifier in child.children(&mut child.walk()) {
                            let modifier_text = &code[modifier.byte_range()];
                            match modifier_text {
                                "async" => {
                                    function_modifiers.push("async");
                                    if self.debug {
                                        eprintln!("[SWIFT DEBUG] Found async modifier in modifiers for {:?}: {}", kind, name);
                                    }
                                }
                                "throws" | "rethrows" => {
                                    function_modifiers.push(modifier_text);
                                    if self.debug {
                                        eprintln!("[SWIFT DEBUG] Found {} modifier in modifiers for {:?}: {}", modifier_text, kind, name);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "async" => {
                        // async is a direct child of function_declaration
                        function_modifiers.push("async");
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found async as direct child for function: {}", name);
                        }
                    }
                    "throws" => {
                        // throws is a direct child of function_declaration
                        function_modifiers.push("throws");
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found throws as direct child for function: {}", name);
                        }
                    }
                    "inheritance_specifier" => {
                        // Check for SwiftUI View protocol conformance
                        for inheritance_child in child.children(&mut child.walk()) {
                            if inheritance_child.kind() == "user_type" {
                                for type_child in inheritance_child.children(&mut inheritance_child.walk()) {
                                    if type_child.kind() == "type_identifier" {
                                        let type_text = &code[type_child.byte_range()];
                                        if type_text == "View" {
                                            function_modifiers.push("SwiftUI.View");
                                            if self.debug {
                                                eprintln!("[SWIFT DEBUG] Found SwiftUI View conformance for: {}", name);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            // Fallback to original logic for backwards compatibility
            if let Some(parent) = name_node.parent() {
                for child in parent.children(&mut parent.walk()) {
                    match child.kind() {
                        "modifiers" => {
                            // Check inside modifiers node
                            for modifier in child.children(&mut child.walk()) {
                                let modifier_text = &code[modifier.byte_range()];
                                match modifier_text {
                                    "async" => {
                                        function_modifiers.push("async");
                                        if self.debug {
                                            eprintln!("[SWIFT DEBUG] Found async modifier in modifiers for function: {}", name);
                                        }
                                    }
                                    "throws" | "rethrows" => {
                                        function_modifiers.push(modifier_text);
                                        if self.debug {
                                            eprintln!("[SWIFT DEBUG] Found {} modifier in modifiers for function: {}", modifier_text, name);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "async" => {
                            // async is a direct child of function_declaration
                            function_modifiers.push("async");
                            if self.debug {
                                eprintln!("[SWIFT DEBUG] Found async as direct child for function: {}", name);
                            }
                        }
                        "throws" => {
                            // throws is a direct child of function_declaration
                            function_modifiers.push("throws");
                            if self.debug {
                                eprintln!("[SWIFT DEBUG] Found throws as direct child for function: {}", name);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Build signature combining @MainActor, result builders, and function modifiers
        let mut signature_parts = Vec::new();
        if is_main_actor {
            signature_parts.push("@MainActor");
        }
        signature_parts.extend(result_builder_attrs.iter().map(|s| s.as_str()));
        signature_parts.extend(function_modifiers.iter().map(|s| &**s));
        
        if !signature_parts.is_empty() {
            let signature_str = signature_parts.join(" ");
            if self.debug {
                eprintln!("[SWIFT DEBUG] Added signature: {}", signature_str);
            }
            symbol = symbol.with_signature(signature_str);
        }

        if let Some(doc) = doc_comment {
            symbol = symbol.with_doc(doc);
        }

        Some(symbol)
    }

    /// Create a symbol with function context for modifier detection
    fn create_symbol_with_function_context(
        &self,
        counter: &mut SymbolCounter,
        name_node: Node,
        kind: SymbolKind,
        file_id: FileId,
        code: &str,
        function_node: Node, // The function_declaration node
    ) -> Option<Symbol> {
        let name = &code[name_node.byte_range()];

        let symbol_id = counter.next_id();

        let range = Range::new(
            name_node.start_position().row as u32,
            name_node.start_position().column as u16,
            name_node.end_position().row as u32,
            name_node.end_position().column as u16,
        );

        // Find the parent node that might have doc comments
        let doc_node = name_node.parent()?;
        let doc_comment = self.extract_doc_comments(&doc_node, code);

        // Default to Public visibility (represents Swift's default 'internal' visibility)
        let mut symbol = Symbol::new(symbol_id, name, kind, file_id, range)
            .with_visibility(Visibility::Public);

        // Check for explicit visibility modifiers
        if let Some(parent) = name_node.parent() {
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "modifiers" {
                    for modifier in child.children(&mut child.walk()) {
                        let modifier_text = &code[modifier.byte_range()];
                        match modifier_text {
                            "public" | "open" => {
                                // Already defaulted to Public
                                break;
                            }
                            "internal" => {
                                // Explicit internal is same as default (Public in codanna)
                                break;
                            }
                            "private" | "fileprivate" => {
                                symbol = symbol.with_visibility(Visibility::Private);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Check for function modifiers (async, throws, @MainActor, @ViewBuilder) directly in the function_declaration
        let mut function_modifiers = Vec::new();
        let mut is_main_actor = false;
        let mut result_builder_attrs = Vec::new();
        
        for child in function_node.children(&mut function_node.walk()) {
            match child.kind() {
                "async" => {
                    function_modifiers.push("async");
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found async modifier for function: {}", name);
                    }
                }
                "throws" => {
                    function_modifiers.push("throws");
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found throws modifier for function: {}", name);
                    }
                }
                "rethrows" => {
                    function_modifiers.push("rethrows");
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found rethrows modifier for function: {}", name);
                    }
                }
                "modifiers" => {
                    // Check for @MainActor attribute first
                    if self.extract_main_actor_attribute(child, code) {
                        is_main_actor = true;
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found @MainActor attribute for function: {}", name);
                        }
                    }
                    
                    // Check for result builder attributes like @ViewBuilder
                    let builders = self.extract_result_builder_attributes(child, code);
                    if !builders.is_empty() {
                        result_builder_attrs.extend(builders.clone());
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Found result builders for function {}: {}", name, builders.join(", "));
                        }
                    }
                    
                    // Also check inside modifiers node for async/throws
                    for modifier in child.children(&mut child.walk()) {
                        let modifier_text = &code[modifier.byte_range()];
                        match modifier_text {
                            "async" => {
                                function_modifiers.push("async");
                                if self.debug {
                                    eprintln!("[SWIFT DEBUG] Found async modifier (in modifiers) for function: {}", name);
                                }
                            }
                            "throws" | "rethrows" => {
                                function_modifiers.push(modifier_text);
                                if self.debug {
                                    eprintln!("[SWIFT DEBUG] Found {} modifier (in modifiers) for function: {}", modifier_text, name);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Build signature combining @MainActor, result builders, and function modifiers
        let mut signature_parts = Vec::new();
        if is_main_actor {
            signature_parts.push("@MainActor");
        }
        signature_parts.extend(result_builder_attrs.iter().map(|s| s.as_str()));
        signature_parts.extend(function_modifiers.iter().map(|s| &**s));
        
        if !signature_parts.is_empty() {
            let signature_str = signature_parts.join(" ");
            if self.debug {
                eprintln!("[SWIFT DEBUG] Added signature: {}", signature_str);
            }
            symbol = symbol.with_signature(signature_str);
        }

        if let Some(doc) = doc_comment {
            symbol = symbol.with_doc(doc);
        }

        Some(symbol)
    }

    fn create_symbol(
        &self,
        counter: &mut SymbolCounter,
        name_node: Node,
        kind: SymbolKind,
        file_id: FileId,
        code: &str,
    ) -> Option<Symbol> {
        let name = &code[name_node.byte_range()];

        let symbol_id = counter.next_id();

        let range = Range::new(
            name_node.start_position().row as u32,
            name_node.start_position().column as u16,
            name_node.end_position().row as u32,
            name_node.end_position().column as u16,
        );

        // Find the parent node that might have doc comments
        let doc_node = name_node.parent()?;
        let doc_comment = self.extract_doc_comments(&doc_node, code);

        // Default to Public visibility (represents Swift's default 'internal' visibility)
        let mut symbol = Symbol::new(symbol_id, name, kind, file_id, range)
            .with_visibility(Visibility::Public);

        // Check for explicit visibility modifiers
        if let Some(parent) = name_node.parent() {
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "modifiers" {
                    for modifier in child.children(&mut child.walk()) {
                        let modifier_text = &code[modifier.byte_range()];
                        match modifier_text {
                            "public" | "open" => {
                                // Already defaulted to Public
                                break;
                            }
                            "internal" => {
                                // Explicit internal is same as default (Public in codanna)
                                break;
                            }
                            "private" | "fileprivate" => {
                                symbol = symbol.with_visibility(Visibility::Private);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if let Some(doc) = doc_comment {
            symbol = symbol.with_doc(doc);
        }

        Some(symbol)
    }

    /// Extract specific attribute from a modifiers node
    fn extract_attribute(&self, modifiers_node: Node, code: &str, attribute_name: &str) -> bool {
        for child in modifiers_node.children(&mut modifiers_node.walk()) {
            if child.kind() == "attribute" {
                // Look for @ symbol and specific type identifier
                let mut has_at = false;
                let mut has_target_attribute = false;
                
                for attr_child in child.children(&mut child.walk()) {
                    if attr_child.kind() == "@" {
                        has_at = true;
                    } else if attr_child.kind() == "user_type" {
                        // Look for type_identifier matching our target
                        for type_child in attr_child.children(&mut attr_child.walk()) {
                            if type_child.kind() == "type_identifier" {
                                let type_text = &code[type_child.byte_range()];
                                if type_text == attribute_name {
                                    has_target_attribute = true;
                                }
                            }
                        }
                    }
                }
                
                if has_at && has_target_attribute {
                    return true;
                }
            }
        }
        false
    }

    /// Extract @MainActor attribute from a modifiers node
    fn extract_main_actor_attribute(&self, modifiers_node: Node, code: &str) -> bool {
        self.extract_attribute(modifiers_node, code, "MainActor")
    }

    /// Extract @ViewBuilder attribute from a modifiers node
    fn extract_viewbuilder_attribute(&self, modifiers_node: Node, code: &str) -> bool {
        self.extract_attribute(modifiers_node, code, "ViewBuilder")
    }

    /// Extract all result builder attributes from a modifiers node
    fn extract_result_builder_attributes(&self, modifiers_node: Node, code: &str) -> Vec<String> {
        let mut result_builders = Vec::new();
        
        for child in modifiers_node.children(&mut modifiers_node.walk()) {
            if child.kind() == "attribute" {
                let mut has_at = false;
                let mut attribute_name = None;
                
                for attr_child in child.children(&mut child.walk()) {
                    if attr_child.kind() == "@" {
                        has_at = true;
                    } else if attr_child.kind() == "user_type" {
                        for type_child in attr_child.children(&mut attr_child.walk()) {
                            if type_child.kind() == "type_identifier" {
                                let type_text = &code[type_child.byte_range()];
                                // Common result builders to detect
                                if type_text == "ViewBuilder" || type_text == "resultBuilder" || 
                                   type_text.ends_with("Builder") {
                                    attribute_name = Some(type_text.to_string());
                                }
                            }
                        }
                    }
                }
                
                if has_at {
                    if let Some(name) = attribute_name {
                        result_builders.push(format!("@{}", name));
                    }
                }
            }
        }
        
        result_builders
    }

    fn extract_doc_comments(&self, node: &Node, code: &str) -> Option<String> {
        let mut doc_lines = Vec::new();
        let mut current = node.prev_sibling();

        while let Some(sibling) = current {
            match sibling.kind() {
                "comment" => {
                    if let Ok(text) = sibling.utf8_text(code.as_bytes()) {
                        // Swift uses /// for doc comments
                        if text.starts_with("///") && !text.starts_with("////") {
                            let content = text.trim_start_matches("///").trim();
                            doc_lines.push(content.to_string());
                        } else if text.starts_with("/**") && !text.starts_with("/***") {
                            let content =
                                text.trim_start_matches("/**").trim_end_matches("*/").trim();
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

    /// Find function and method calls in Swift code
    pub fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_calls called");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut calls = Vec::new();

        self.find_calls_in_node(root_node, code, &mut calls);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_calls found {} calls", calls.len());
        }

        calls
    }

    fn find_calls_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        let containing_function = self.find_containing_function(node, code);

        if node.kind() == "call_expression" {
            if self.debug {
                eprintln!("[SWIFT DEBUG] find_calls: Found call_expression");
                eprintln!("[SWIFT DEBUG]   Containing: {:?}", containing_function);
            }
            
            // In tree-sitter-swift, the function is the first child (not a named field)
            let function_node = node.child(0);
                
            if let Some(function_node) = function_node {
                let mut target_name = None;
                let mut is_task_creation = false;
                let mut task_type = None;

                match function_node.kind() {
                    // Direct function call
                    "simple_identifier" => {
                        let name = &code[function_node.byte_range()];
                        target_name = Some(name);
                        
                        // Check for Task{} creation
                        if name == "Task" {
                            is_task_creation = true;
                            task_type = Some("Task");
                            if self.debug {
                                eprintln!("[SWIFT DEBUG] Found Task{{}} creation");
                            }
                        }
                    }
                    // Method call (e.g., object.method())
                    "navigation_expression" => {
                        if let Some(suffix) = function_node.child_by_field_name("suffix") {
                            let suffix_name = &code[suffix.byte_range()];
                            target_name = Some(suffix_name);
                            
                            // Check for Task.detached{} or Task.sleep() patterns
                            if let Some(target) = function_node.child_by_field_name("target") {
                                let target_name_str = &code[target.byte_range()];
                                if target_name_str == "Task" {
                                    match suffix_name {
                                        "detached" => {
                                            is_task_creation = true;
                                            task_type = Some("Task.detached");
                                            if self.debug {
                                                eprintln!("[SWIFT DEBUG] Found Task.detached{{}} creation");
                                            }
                                        }
                                        "sleep" => {
                                            task_type = Some("Task.sleep");
                                            if self.debug {
                                                eprintln!("[SWIFT DEBUG] Found Task.sleep() call");
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if let (Some(target), Some(caller)) = (target_name, containing_function) {
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    
                    // For Task creation patterns, we might want to track these differently
                    if is_task_creation {
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Recording Task creation: {} creates {} at line {}", 
                                     caller, task_type.unwrap_or("Task"), range.start_line);
                        }
                    }
                    
                    calls.push((caller, target, range));
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_calls_in_node(child, code, calls);
        }
    }

    fn find_containing_function<'a>(&self, mut node: Node, code: &'a str) -> Option<&'a str> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] Looking for containing function, starting at node: {}", node.kind());
        }
        
        loop {
            if self.debug && (node.kind().contains("function") || node.kind().contains("init")) {
                eprintln!("[SWIFT DEBUG]   Checking node kind: {}", node.kind());
            }
            
            if node.kind() == "function_declaration" {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let func_name = &code[name_node.byte_range()];
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found containing function: {}", func_name);
                    }
                    return Some(func_name);
                }
            }
            // Also check for init_declaration (Swift initializers)
            if node.kind() == "init_declaration" {
                if self.debug {
                    eprintln!("[SWIFT DEBUG] Found containing init");
                }
                return Some("init");
            }

            match node.parent() {
                Some(parent) => node = parent,
                None => {
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] No containing function found");
                    }
                    return None;
                }
            }
        }
    }

    /// Find Task{} creation patterns and async contexts
    pub fn find_task_contexts(&mut self, code: &str) -> Vec<TaskContext> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_task_contexts called");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut task_contexts = Vec::new();

        self.find_task_contexts_in_node(root_node, code, &mut task_contexts, root_node);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_task_contexts found {} task contexts", task_contexts.len());
        }
        
        task_contexts
    }

    fn find_task_contexts_in_node(
        &self,
        node: Node,
        code: &str,
        task_contexts: &mut Vec<TaskContext>,
        root_node: Node,
    ) {
        if node.kind() == "call_expression" {
            let containing_function = self.find_containing_function(node, code);
            
            if let Some(function_node) = node.child(0) {
                let mut task_info = None;
                
                match function_node.kind() {
                    // Direct Task{} call
                    "simple_identifier" => {
                        let name = &code[function_node.byte_range()];
                        if name == "Task" {
                            task_info = Some(TaskInfo {
                                task_type: TaskType::Standard,
                                priority: None,
                                inherits_context: true, // Inherits actor context by default
                            });
                        }
                    }
                    // Task.detached{} or Task.sleep() calls
                    "navigation_expression" => {
                        if let Some(target) = function_node.child_by_field_name("target") {
                            let target_name = &code[target.byte_range()];
                            if target_name == "Task" {
                                if let Some(suffix) = function_node.child_by_field_name("suffix") {
                                    let suffix_name = &code[suffix.byte_range()];
                                    match suffix_name {
                                        "detached" => {
                                            task_info = Some(TaskInfo {
                                                task_type: TaskType::Detached,
                                                priority: self.extract_task_priority(node, code),
                                                inherits_context: false, // Detached breaks context
                                            });
                                        }
                                        "sleep" => {
                                            task_info = Some(TaskInfo {
                                                task_type: TaskType::Sleep,
                                                priority: None,
                                                inherits_context: true,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                
                if let (Some(info), Some(creator)) = (task_info, containing_function) {
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    
                    // Check if the creating function has @MainActor or async context
                    let creator_context = self.analyze_function_context(creator, root_node, code);
                    
                    let task_context = TaskContext {
                        creator_function: creator.to_string(),
                        task_info: info,
                        creator_context,
                        range,
                    };
                    
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found task context: {} creates {:?} at line {}", 
                                 creator, task_context.task_info.task_type, range.start_line);
                    }
                    
                    task_contexts.push(task_context);
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_task_contexts_in_node(child, code, task_contexts, root_node);
        }
    }

    fn extract_task_priority(&self, node: Node, code: &str) -> Option<String> {
        // Look for priority parameter in Task(priority: .high) syntax
        for child in node.children(&mut node.walk()) {
            if child.kind() == "call_suffix" {
                // Look for labeled arguments
                for arg_child in child.children(&mut child.walk()) {
                    if arg_child.kind() == "call_argument" {
                        // Check if this is a priority argument
                        for label_child in arg_child.children(&mut arg_child.walk()) {
                            if label_child.kind() == "value_argument_label" {
                                let label = &code[label_child.byte_range()];
                                if label == "priority" {
                                    // Find the value
                                    for value_child in arg_child.children(&mut arg_child.walk()) {
                                        if value_child.kind() == "navigation_expression" {
                                            if let Some(suffix) = value_child.child_by_field_name("suffix") {
                                                return Some(code[suffix.byte_range()].to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn analyze_function_context(&self, function_name: &str, root_node: Node, code: &str) -> AsyncContext {
        // Use the already parsed tree to find the function and analyze its context
        if let Some(func_info) = self.find_function_info(root_node, code, function_name) {
            match (func_info.has_main_actor, func_info.is_async) {
                (true, true) => AsyncContext::AsyncMainActor,
                (true, false) => AsyncContext::MainActor,
                (false, true) => AsyncContext::Async,
                (false, false) => AsyncContext::Regular,
            }
        } else {
            AsyncContext::Unknown
        }
    }

    fn find_function_info(&self, node: Node, code: &str, target_function: &str) -> Option<FunctionInfo> {
        if node.kind() == "function_declaration" {
            if let Some(name_node) = node.child_by_field_name("name") {
                let func_name = &code[name_node.byte_range()];
                if func_name == target_function {
                    // Analyze this function's modifiers
                    let mut has_main_actor = false;
                    let mut is_async = false;
                    
                    for child in node.children(&mut node.walk()) {
                        match child.kind() {
                            "modifiers" => {
                                // Check for @MainActor attribute
                                if self.extract_main_actor_attribute(child, code) {
                                    has_main_actor = true;
                                }
                                
                                // Check for async modifier within modifiers
                                for modifier in child.children(&mut child.walk()) {
                                    let modifier_text = &code[modifier.byte_range()];
                                    if modifier_text == "async" {
                                        is_async = true;
                                    }
                                }
                            }
                            "async" => {
                                is_async = true;
                            }
                            _ => {}
                        }
                    }
                    
                    return Some(FunctionInfo {
                        has_main_actor,
                        is_async,
                    });
                }
            }
        }
        
        // Recursively search in children
        for child in node.children(&mut node.walk()) {
            if let Some(info) = self.find_function_info(child, code, target_function) {
                return Some(info);
            }
        }
        
        None
    }

    /// Find method calls with enhanced receiver information
    pub fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_method_calls called, parsing code");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut method_calls = Vec::new();

        self.find_method_calls_in_node(root_node, code, &mut method_calls);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_method_calls found {} method calls", method_calls.len());
        }
        
        method_calls
    }

    fn find_method_calls_in_node(
        &self,
        node: Node,
        code: &str,
        method_calls: &mut Vec<MethodCall>,
    ) {
        // Only look for containing function when we find a call_expression
        if node.kind() == "call_expression" {
            let containing_function = self.find_containing_function(node, code);
            
            if self.debug {
                eprintln!("[SWIFT DEBUG] Found call_expression node in find_method_calls");
                eprintln!("[SWIFT DEBUG]   Node text: {}", &code[node.byte_range()].chars().take(50).collect::<String>());
                eprintln!("[SWIFT DEBUG]   Containing function: {:?}", containing_function);
                
                // Debug what fields this node has
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        eprintln!("[SWIFT DEBUG]   Child {}: kind={}", i, child.kind());
                    }
                }
                
                // Try different field names
                if let Some(name_field) = node.child_by_field_name("name") {
                    eprintln!("[SWIFT DEBUG]   Has 'name' field: {}", name_field.kind());
                }
                if let Some(func_field) = node.child_by_field_name("function") {
                    eprintln!("[SWIFT DEBUG]   Has 'function' field: {}", func_field.kind());
                }
            }
            
            // In tree-sitter-swift, the function is the first child (not a named field)
            let function_node = node.child(0);
                
            if let Some(function_node) = function_node {
                match function_node.kind() {
                    // Direct function call
                    "simple_identifier" => {
                        let method_name = code[function_node.byte_range()].to_string();
                        if let Some(caller) = containing_function {
                            let range = Range::new(
                                node.start_position().row as u32,
                                node.start_position().column as u16,
                                node.end_position().row as u32,
                                node.end_position().column as u16,
                            );
                            let method_call = MethodCall::new(caller, &method_name, range);
                            method_calls.push(method_call);
                        }
                    }
                    // Method call (e.g., object.method())
                    "navigation_expression" => {
                        if let Some(suffix) = function_node.child_by_field_name("suffix") {
                            let method_name = code[suffix.byte_range()].to_string();
                            
                            if let Some(target) = function_node.child_by_field_name("target") {
                                let receiver_text = code[target.byte_range()].to_string();
                                
                                if let Some(caller) = containing_function {
                                    let range = Range::new(
                                        node.start_position().row as u32,
                                        node.start_position().column as u16,
                                        node.end_position().row as u32,
                                        node.end_position().column as u16,
                                    );
                                    
                                    let method_call = if receiver_text == "self" {
                                        MethodCall::new(caller, &method_name, range)
                                            .with_receiver("self")
                                    } else if receiver_text == "super" {
                                        MethodCall::new(caller, &method_name, range)
                                            .with_receiver("super")
                                    } else {
                                        // Check if it's a type (capitalized) for static calls
                                        let is_static = receiver_text.chars().next()
                                            .map(|c| c.is_uppercase())
                                            .unwrap_or(false);
                                        
                                        if is_static {
                                            MethodCall::new(caller, &method_name, range)
                                                .with_receiver(&receiver_text)
                                                .static_method()
                                        } else {
                                            MethodCall::new(caller, &method_name, range)
                                                .with_receiver(&receiver_text)
                                        }
                                    };
                                    method_calls.push(method_call);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_method_calls_in_node(child, code, method_calls);
        }
    }

    /// Extract import statements from Swift code
    pub fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
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
        if node.kind() == "import_declaration" {
            let mut path_parts = Vec::new();
            let mut is_testable = false;

            for child in node.children(&mut node.walk()) {
                match child.kind() {
                    "attribute" => {
                        // Check for @testable attribute
                        let attr_text = &code[child.byte_range()];
                        if attr_text.contains("testable") {
                            is_testable = true;
                        }
                    }
                    "identifier" => {
                        // The identifier node contains the module name
                        for id_child in child.children(&mut child.walk()) {
                            if id_child.kind() == "simple_identifier" {
                                path_parts.push(&code[id_child.byte_range()]);
                            }
                        }
                    }
                    "simple_identifier" => {
                        // Fallback for direct simple_identifier (compatibility)
                        path_parts.push(&code[child.byte_range()]);
                    }
                    _ => {}
                }
            }

            if !path_parts.is_empty() {
                let path = path_parts.join(".");
                let alias = if is_testable {
                    Some("@testable".to_string())
                } else {
                    None
                };
                
                imports.push(Import {
                    path,
                    alias,
                    file_id,
                    is_glob: false,
                });
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.extract_imports_from_node(child, code, file_id, imports);
        }
    }

    /// Find protocol conformances (trait implementations in Swift)
    pub fn find_implementations<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_implementations called");
        }
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut implementations = Vec::new();

        self.find_implementations_in_node(root_node, code, &mut implementations);

        implementations
    }

    fn find_implementations_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        implementations: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Check for class, struct, or enum with protocol conformance
        // Note: tree-sitter-swift uses "class_declaration" for struct, class, enum, and extension
        match node.kind() {
            "class_declaration" => {
                if self.debug {
                    eprintln!("[SWIFT DEBUG] Found class_declaration node");
                }
                if let Some(name_node) = node.child_by_field_name("name") {
                    let type_name = &code[name_node.byte_range()];

                    // Look for type_inheritance_clause
                    for child in node.children(&mut node.walk()) {
                        if self.debug {
                            eprintln!("[SWIFT DEBUG]   Child kind: {}", child.kind());
                        }
                        if child.kind() == "inheritance_specifier" || child.kind() == "type_inheritance_clause" {
                            if self.debug {
                                eprintln!("[SWIFT DEBUG]   Found inheritance for {}", type_name);
                            }
                            // Extract protocol names
                            for protocol_child in child.children(&mut child.walk()) {
                                if self.debug {
                                    eprintln!("[SWIFT DEBUG]     Protocol child kind: {}", protocol_child.kind());
                                }
                                if protocol_child.kind() == "user_type" {
                                    // tree-sitter-swift doesn't use named fields for type names
                                    // Look for type_identifier child
                                    for type_child in protocol_child.children(&mut protocol_child.walk()) {
                                        if type_child.kind() == "type_identifier" {
                                            let protocol_name = &code[type_child.byte_range()];
                                            let range = Range::new(
                                                node.start_position().row as u32,
                                                node.start_position().column as u16,
                                                node.end_position().row as u32,
                                                node.end_position().column as u16,
                                            );
                                            if self.debug {
                                                eprintln!("[SWIFT DEBUG] Found implementation: {} implements {}", type_name, protocol_name);
                                            }
                                            implementations.push((type_name, protocol_name, range));
                                        }
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
            self.find_implementations_in_node(child, code, implementations);
        }
    }

    /// Find type usage in Swift code (where types are referenced)
    pub fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_uses called");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut uses = Vec::new();

        self.find_uses_in_node(root_node, code, &mut uses, None);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_uses found {} type uses", uses.len());
        }

        uses
    }
    
    fn find_uses_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        context: Option<&'a str>,
    ) {
        match node.kind() {
            // Struct/class property with type annotation
            "property_declaration" => {
                // Get the property name
                let mut prop_name = None;
                let mut type_name = None;
                
                for child in node.children(&mut node.walk()) {
                    match child.kind() {
                        "value_binding_pattern" => {
                            // Look for the property name
                            for pattern_child in child.children(&mut child.walk()) {
                                if pattern_child.kind() == "pattern" {
                                    for id_child in pattern_child.children(&mut pattern_child.walk()) {
                                        if id_child.kind() == "simple_identifier" {
                                            prop_name = Some(&code[id_child.byte_range()]);
                                        }
                                    }
                                }
                            }
                        }
                        "type_annotation" => {
                            // Look for the type
                            for type_child in child.children(&mut child.walk()) {
                                if type_child.kind() == "user_type" {
                                    if let Some(type_id) = type_child.child_by_field_name("name") {
                                        type_name = Some(&code[type_id.byte_range()]);
                                    } else if let Some(first_child) = type_child.child(0) {
                                        if first_child.kind() == "type_identifier" {
                                            type_name = Some(&code[first_child.byte_range()]);
                                        }
                                    }
                                } else if type_child.kind() == "array_type" {
                                    // Handle array types like [Type]
                                    for array_child in type_child.children(&mut type_child.walk()) {
                                        if array_child.kind() == "user_type" {
                                            if let Some(first_child) = array_child.child(0) {
                                                if first_child.kind() == "type_identifier" {
                                                    type_name = Some(&code[first_child.byte_range()]);
                                                }
                                            }
                                        }
                                    }
                                } else if type_child.kind() == "optional_type" {
                                    // Handle optional types like Type?
                                    for opt_child in type_child.children(&mut type_child.walk()) {
                                        if opt_child.kind() == "user_type" {
                                            if let Some(first_child) = opt_child.child(0) {
                                                if first_child.kind() == "type_identifier" {
                                                    type_name = Some(&code[first_child.byte_range()]);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                
                // If we have a context (struct/class name) and a type, record the usage
                if let (Some(ctx), Some(type_str)) = (context, type_name) {
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    uses.push((ctx, type_str, range));
                    
                    if self.debug && prop_name.is_some() {
                        eprintln!("[SWIFT DEBUG] {} uses {} (property: {})", ctx, type_str, prop_name.unwrap());
                    }
                }
            }
            // Function parameters and return types
            "function_declaration" => {
                let mut func_name = None;
                
                // Get function name
                if let Some(name_node) = node.child_by_field_name("name") {
                    func_name = Some(&code[name_node.byte_range()]);
                }
                
                if let Some(func) = func_name {
                    // Look for parameters
                    for child in node.children(&mut node.walk()) {
                        if child.kind() == "parameter" {
                            // Look for type annotation in parameter
                            for param_child in child.children(&mut child.walk()) {
                                if param_child.kind() == "type_annotation" {
                                    for type_child in param_child.children(&mut param_child.walk()) {
                                        if type_child.kind() == "user_type" {
                                            if let Some(first_child) = type_child.child(0) {
                                                if first_child.kind() == "type_identifier" {
                                                    let type_name = &code[first_child.byte_range()];
                                                    let range = Range::new(
                                                        param_child.start_position().row as u32,
                                                        param_child.start_position().column as u16,
                                                        param_child.end_position().row as u32,
                                                        param_child.end_position().column as u16,
                                                    );
                                                    uses.push((func, type_name, range));
                                                    
                                                    if self.debug {
                                                        eprintln!("[SWIFT DEBUG] {} uses {} (parameter)", func, type_name);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Look for return type
                        else if child.kind() == "user_type" {
                            if let Some(first_child) = child.child(0) {
                                if first_child.kind() == "type_identifier" {
                                    let type_name = &code[first_child.byte_range()];
                                    let range = Range::new(
                                        child.start_position().row as u32,
                                        child.start_position().column as u16,
                                        child.end_position().row as u32,
                                        child.end_position().column as u16,
                                    );
                                    uses.push((func, type_name, range));
                                    
                                    if self.debug {
                                        eprintln!("[SWIFT DEBUG] {} uses {} (return type)", func, type_name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Track context for structs/classes
            "class_declaration" => {
                // Determine actual type (struct, class, enum)
                let mut _is_struct = false;
                let mut _is_class = false;
                let mut _is_enum = false;
                
                for child in node.children(&mut node.walk()) {
                    match child.kind() {
                        "struct" => { _is_struct = true; break; }
                        "class" => { _is_class = true; break; }
                        "enum" => { _is_enum = true; break; }
                        _ => {}
                    }
                }
                
                // Get the type name
                let type_name = node.child_by_field_name("name")
                    .and_then(|n| Some(&code[n.byte_range()]));
                
                if let Some(name) = type_name {
                    // Process children with this type as context
                    for child in node.children(&mut node.walk()) {
                        self.find_uses_in_node(child, code, uses, Some(name));
                    }
                    return; // Don't process children again below
                }
            }
            _ => {}
        }
        
        // Recursively process children (if not already processed above)
        if !matches!(node.kind(), "class_declaration") {
            for child in node.children(&mut node.walk()) {
                self.find_uses_in_node(child, code, uses, context);
            }
        }
    }

    /// Find method definitions in protocols and extensions
    pub fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_defines called");
        }
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut defines = Vec::new();

        self.find_defines_in_node(root_node, code, &mut defines);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_defines found {} definitions", defines.len());
        }

        defines
    }

    fn find_defines_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Check if this is an extension (class_declaration with extension keyword)
        if node.kind() == "class_declaration" {
            if self.debug {
                eprintln!("[SWIFT DEBUG] find_defines checking class_declaration node");
            }
            let mut is_extension = false;
            
            // Check for extension keyword
            for child in node.children(&mut node.walk()) {
                if child.kind() == "extension" {
                    is_extension = true;
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found extension keyword!");
                    }
                    break;
                }
            }
            
            if is_extension {
                // This is an extension - get the type being extended
                if let Some(type_node) = node.child_by_field_name("name") {
                    let type_name = &code[type_node.byte_range()];
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Processing extension of type: {}", type_name);
                    }
                    
                    // Find method and property declarations in the extension body
                    for child in node.children(&mut node.walk()) {
                        if child.kind() == "class_body" {
                            // Look inside the class body for methods and properties
                            for body_child in child.children(&mut child.walk()) {
                                if self.debug {
                                    eprintln!("[SWIFT DEBUG]   Looking at body child: {}", body_child.kind());
                                }
                                match body_child.kind() {
                                    "function_declaration" => {
                                        if let Some(method_name_node) = body_child.child_by_field_name("name") {
                                            let method_name = &code[method_name_node.byte_range()];
                                            let range = Range::new(
                                                body_child.start_position().row as u32,
                                                body_child.start_position().column as u16,
                                                body_child.end_position().row as u32,
                                                body_child.end_position().column as u16,
                                    );
                                    if self.debug {
                                        eprintln!("[SWIFT DEBUG] Extension adds method {} to {}", method_name, type_name);
                                    }
                                            defines.push((type_name, method_name, range));
                                        }
                                    }
                                    "property_declaration" => {
                                        // Also handle properties added by extensions
                                        for prop_child in body_child.children(&mut body_child.walk()) {
                                            if prop_child.kind() == "value_binding_pattern" {
                                                for pattern_child in prop_child.children(&mut prop_child.walk()) {
                                                    if pattern_child.kind() == "pattern" {
                                                        for id_child in pattern_child.children(&mut pattern_child.walk()) {
                                                            if id_child.kind() == "simple_identifier" {
                                                                let prop_name = &code[id_child.byte_range()];
                                                                let range = Range::new(
                                                                    body_child.start_position().row as u32,
                                                                    body_child.start_position().column as u16,
                                                                    body_child.end_position().row as u32,
                                                                    body_child.end_position().column as u16,
                                                                );
                                                                if self.debug {
                                                                    eprintln!("[SWIFT DEBUG] Extension adds property {} to {}", prop_name, type_name);
                                                                }
                                                                defines.push((type_name, prop_name, range));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_defines_in_node(child, code, defines);
        }
    }
}

impl SwiftParser {
    fn find_variable_types_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        variable_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        // Look for property/variable declarations with initializers
        if node.kind() == "property_declaration" {
            let mut var_name = None;
            let mut type_name = None;
            
            // Get variable name
            for child in node.children(&mut node.walk()) {
                if child.kind() == "value_binding_pattern" {
                    for pattern_child in child.children(&mut child.walk()) {
                        if pattern_child.kind() == "pattern" {
                            for id_child in pattern_child.children(&mut pattern_child.walk()) {
                                if id_child.kind() == "simple_identifier" {
                                    var_name = Some(&code[id_child.byte_range()]);
                                }
                            }
                        }
                    }
                }
            }
            
            // Look for initializer that tells us the type
            for child in node.children(&mut node.walk()) {
                // Direct call expression (Type() or Type.method())
                if child.kind() == "call_expression" {
                    if let Some(first_child) = child.child(0) {
                        match first_child.kind() {
                            // Simple constructor call: Type()
                            "simple_identifier" => {
                                type_name = Some(&code[first_child.byte_range()]);
                            }
                            // Static method or property: Type.shared, Type.new()
                            "navigation_expression" => {
                                // The first part is usually the type
                                if let Some(target) = first_child.child(0) {
                                    if target.kind() == "simple_identifier" {
                                        type_name = Some(&code[target.byte_range()]);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Navigation expression without call (Type.shared, Type.default)
                else if child.kind() == "navigation_expression" {
                    if let Some(target) = child.child(0) {
                        if target.kind() == "simple_identifier" {
                            // Check if it's capitalized (likely a type)
                            let text = &code[target.byte_range()];
                            if text.chars().next().map_or(false, |c| c.is_uppercase()) {
                                type_name = Some(text);
                            }
                        }
                    }
                }
                // Simple identifier that might be a type (for copy assignments)
                else if child.kind() == "simple_identifier" {
                    let text = &code[child.byte_range()];
                    // If capitalized, might be a type reference
                    if text.chars().next().map_or(false, |c| c.is_uppercase()) {
                        type_name = Some(text);
                    }
                }
            }
            
            // Record the variable -> type binding
            if let (Some(var), Some(typ)) = (var_name, type_name) {
                let range = Range::new(
                    node.start_position().row as u32,
                    node.start_position().column as u16,
                    node.end_position().row as u32,
                    node.end_position().column as u16,
                );
                variable_types.push((var, typ, range));
                
                if self.debug {
                    eprintln!("[SWIFT DEBUG] Variable '{}' has type '{}'", var, typ);
                }
            }
        }
        
        // Recursively process children
        for child in node.children(&mut node.walk()) {
            self.find_variable_types_in_node(child, code, variable_types);
        }
    }

    fn find_inherent_methods_in_node(
        &self,
        node: Node,
        code: &str,
        inherent_methods: &mut Vec<(String, String, Range)>,
    ) {
        match node.kind() {
            // Methods in class/struct declarations
            "class_declaration" => {
                // Determine if it's a struct, class, or enum
                let mut is_extension = false;
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "extension" {
                        is_extension = true;
                        break;
                    }
                }
                
                // Get the type name
                let type_name = if is_extension {
                    // For extensions, look for the type being extended
                    node.children(&mut node.walk())
                        .find(|c| c.kind() == "user_type")
                        .and_then(|t| t.child(0))
                        .filter(|c| c.kind() == "type_identifier")
                        .map(|n| &code[n.byte_range()])
                } else {
                    // For class/struct declarations, use the name field
                    node.child_by_field_name("name")
                        .map(|n| &code[n.byte_range()])
                };
                
                if let Some(type_name) = type_name {
                    // Find all function declarations within this type
                    self.find_methods_in_body(node, code, type_name, inherent_methods);
                }
            }
            // Protocol declarations - we exclude these as they're not inherent methods
            "protocol_declaration" => {
                // Skip - protocols define requirements, not implementations
                return;
            }
            _ => {}
        }
        
        // Recursively process children
        for child in node.children(&mut node.walk()) {
            self.find_inherent_methods_in_node(child, code, inherent_methods);
        }
    }
    
    fn find_methods_in_body(
        &self,
        node: Node,
        code: &str,
        type_name: &str,
        inherent_methods: &mut Vec<(String, String, Range)>,
    ) {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(method_name_node) = child.child_by_field_name("name") {
                        let method_name = &code[method_name_node.byte_range()];
                        let range = Range::new(
                            child.start_position().row as u32,
                            child.start_position().column as u16,
                            child.end_position().row as u32,
                            child.end_position().column as u16,
                        );
                        inherent_methods.push((type_name.to_string(), method_name.to_string(), range));
                        
                        if self.debug {
                            eprintln!("[SWIFT DEBUG] Type '{}' has method '{}'", type_name, method_name);
                        }
                    }
                }
                "init_declaration" => {
                    // Initializers are also inherent methods
                    let range = Range::new(
                        child.start_position().row as u32,
                        child.start_position().column as u16,
                        child.end_position().row as u32,
                        child.end_position().column as u16,
                    );
                    inherent_methods.push((type_name.to_string(), "init".to_string(), range));
                    
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Type '{}' has initializer", type_name);
                    }
                }
                // Recurse into nested structures like class_body
                "class_body" | "enum_class_body" | "protocol_body" => {
                    self.find_methods_in_body(child, code, type_name, inherent_methods);
                }
                _ => {}
            }
        }
    }
}

impl LanguageParser for SwiftParser {
    fn parse(&mut self, code: &str, file_id: FileId, symbol_counter: &mut SymbolCounter) -> Vec<Symbol> {
        self.parse(code, file_id, symbol_counter)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.extract_doc_comments(node, code)
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        self.find_calls(code)
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<MethodCall> {
        self.find_method_calls(code)
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
        self.find_imports(code, file_id)
    }

    fn language(&self) -> Language {
        Language::Swift
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_variable_types called");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut variable_types = Vec::new();

        self.find_variable_types_in_node(root_node, code, &mut variable_types);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_variable_types found {} variable type bindings", variable_types.len());
        }

        variable_types
    }


    fn find_inherent_methods(&mut self, code: &str) -> Vec<(String, String, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_inherent_methods called");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut inherent_methods = Vec::new();

        self.find_inherent_methods_in_node(root_node, code, &mut inherent_methods);

        if self.debug {
            eprintln!("[SWIFT DEBUG] find_inherent_methods found {} type-method bindings", inherent_methods.len());
        }

        inherent_methods
    }
}

impl SwiftParser {
    /// Find extensions and the types they extend
    /// Returns tuples of (extended_type_name, extension_info, range)
    pub fn find_extensions<'a>(&mut self, code: &'a str) -> Vec<(&'a str, ExtensionInfo, Range)> {
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_extensions called");
        }
        
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };
        
        let root_node = tree.root_node();
        let mut extensions = Vec::new();
        
        self.find_extensions_in_node(root_node, code, &mut extensions);
        
        if self.debug {
            eprintln!("[SWIFT DEBUG] find_extensions found {} extensions", extensions.len());
        }
        
        extensions
    }
    
    fn find_extensions_in_node<'a>(
        &self,
        node: Node,
        code: &'a str,
        extensions: &mut Vec<(&'a str, ExtensionInfo, Range)>,
    ) {
        if node.kind() == "class_declaration" {
            // Check if this is an extension
            let mut is_extension = false;
            
            for child in node.children(&mut node.walk()) {
                if child.kind() == "extension" {
                    is_extension = true;
                    break;
                }
            }
            
            if is_extension {
                // Get the type being extended
                if let Some(name_node) = node.child_by_field_name("name") {
                    let type_name = &code[name_node.byte_range()];
                    let range = Range::new(
                        node.start_position().row as u32,
                        node.start_position().column as u16,
                        node.end_position().row as u32,
                        node.end_position().column as u16,
                    );
                    
                    // Check for protocol conformances in the extension
                    let mut protocols = Vec::new();
                    for child in node.children(&mut node.walk()) {
                        if child.kind() == "inheritance_specifier" {
                            for protocol_child in child.children(&mut child.walk()) {
                                if protocol_child.kind() == "user_type" {
                                    for type_child in protocol_child.children(&mut protocol_child.walk()) {
                                        if type_child.kind() == "type_identifier" {
                                            protocols.push(code[type_child.byte_range()].to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    let info = ExtensionInfo {
                        protocols,
                        adds_methods: self.check_has_methods(&node, code),
                        adds_properties: self.check_has_properties(&node, code),
                    };
                    
                    if self.debug {
                        eprintln!("[SWIFT DEBUG] Found extension of {}: {:?}", type_name, info);
                    }
                    
                    extensions.push((type_name, info, range));
                }
            }
        }
        
        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.find_extensions_in_node(child, code, extensions);
        }
    }
    
    fn check_has_methods(&self, node: &Node, _code: &str) -> bool {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "function_declaration" {
                return true;
            }
        }
        false
    }
    
    fn check_has_properties(&self, node: &Node, _code: &str) -> bool {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "property_declaration" {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    pub protocols: Vec<String>,
    pub adds_methods: bool,
    pub adds_properties: bool,
}

/// Task creation context information
#[derive(Debug, Clone)]
pub struct TaskContext {
    pub creator_function: String,
    pub task_info: TaskInfo,
    pub creator_context: AsyncContext,
    pub range: Range,
}

#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_type: TaskType,
    pub priority: Option<String>,
    pub inherits_context: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskType {
    Standard,      // Task { }
    Detached,      // Task.detached { }
    Sleep,         // Task.sleep()
    WithPriority,  // Task(priority: .high) { }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AsyncContext {
    MainActor,     // @MainActor function
    Async,         // async function
    AsyncMainActor, // @MainActor async function
    Regular,       // regular function
    Unknown,       // context not determined
}

#[derive(Debug, Clone)]
struct FunctionInfo {
    has_main_actor: bool,
    is_async: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_function() {
        let mut parser = SwiftParser::new().unwrap();
        let code = "func add(a: Int, b: Int) -> Int { return a + b }";
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name.as_ref(), "add");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn test_parse_struct() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            struct Point {
                var x: Double
                var y: Double
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(symbols.iter().any(|s| s.name.as_ref() == "Point" && s.kind == SymbolKind::Struct));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "x" && s.kind == SymbolKind::Field));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "y" && s.kind == SymbolKind::Field));
        assert_eq!(symbols.len(), 3); // struct + 2 properties
    }

    #[test]
    fn test_parse_initializer() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            class Person {
                var name: String
                
                init(name: String) {
                    self.name = name
                }
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(symbols.iter().any(|s| s.name.as_ref() == "Person" && s.kind == SymbolKind::Class));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "name" && s.kind == SymbolKind::Field));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "init" && s.kind == SymbolKind::Method));
        assert_eq!(symbols.len(), 3); // class + property + init
    }

    #[test]
    fn test_parse_computed_property() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            struct Rectangle {
                var width: Double
                var height: Double
                
                var area: Double {
                    return width * height
                }
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        assert!(symbols.iter().any(|s| s.name.as_ref() == "Rectangle" && s.kind == SymbolKind::Struct));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "width" && s.kind == SymbolKind::Field));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "height" && s.kind == SymbolKind::Field));
        assert!(symbols.iter().any(|s| s.name.as_ref() == "area" && s.kind == SymbolKind::Field));
        assert_eq!(symbols.len(), 4); // struct + 3 properties
    }

    #[test]
    fn test_find_imports() {
        // Test simple import
        {
            let mut parser = SwiftParser::new().unwrap();
            let file_id = FileId::new(1).unwrap();
            let code = "import Foundation";
            let imports = parser.find_imports(code, file_id);
            assert_eq!(imports.len(), 1);
            assert_eq!(imports[0].path, "Foundation");
            assert_eq!(imports[0].alias, None);
        }

        // Test testable import
        {
            let mut parser = SwiftParser::new().unwrap();
            let file_id = FileId::new(1).unwrap();
            let code = "@testable import MyModule";
            let imports = parser.find_imports(code, file_id);
            assert_eq!(imports.len(), 1);
            assert_eq!(imports[0].path, "MyModule");
            assert_eq!(imports[0].alias, Some("@testable".to_string()));
        }
    }

    #[test]
    fn test_parse_async_function() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            func regularFunction() {
                print("regular")
            }
            
            func asyncFunction() async {
                print("async")
            }
            
            func asyncThrowsFunction() async throws -> String {
                return "result"
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        // Find the async function
        let async_func = symbols.iter()
            .find(|s| s.name.as_ref() == "asyncFunction" && s.kind == SymbolKind::Function)
            .expect("asyncFunction should be found");
        
        assert_eq!(async_func.signature.as_deref(), Some("async"));
        
        // Find the async throws function
        let async_throws_func = symbols.iter()
            .find(|s| s.name.as_ref() == "asyncThrowsFunction" && s.kind == SymbolKind::Function)
            .expect("asyncThrowsFunction should be found");
        
        assert_eq!(async_throws_func.signature.as_deref(), Some("async throws"));
        
        // Regular function should have no signature
        let regular_func = symbols.iter()
            .find(|s| s.name.as_ref() == "regularFunction" && s.kind == SymbolKind::Function)
            .expect("regularFunction should be found");
        
        assert_eq!(regular_func.signature, None);
    }

    #[test]
    fn test_debug_task_ast_structure() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            func testTask() {
                Task {
                    print("hello")
                }
                
                Task.detached {
                    print("detached")  
                }
                
                let result = Task<String, Never> {
                    return "value"
                }
                
                await Task.sleep(nanoseconds: 1000)
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        println!("\n=== PARSING TASK PATTERNS ===");
        let _symbols = parser.parse(code, file_id, &mut counter);
        
        println!("\n=== FINDING CALLS ===");
        let calls = parser.find_calls(code);
        for (caller, target, range) in calls {
            println!("Call: {} -> {} at line {}", caller, target, range.start_line);
        }
        
        println!("\n=== FINDING METHOD CALLS ===");
        let method_calls = parser.find_method_calls(code);
        for method_call in method_calls {
            println!("Method call: {} -> {} (receiver: {:?}) at line {}", 
                     method_call.caller, method_call.method_name, method_call.receiver, method_call.range.start_line);
        }
        
        println!("\n=== FINDING TASK CONTEXTS ===");
        let task_contexts = parser.find_task_contexts(code);
        for task_context in task_contexts {
            println!("Task context: {} creates {:?} (inherits_context: {}) at line {}", 
                     task_context.creator_function, 
                     task_context.task_info.task_type,
                     task_context.task_info.inherits_context,
                     task_context.range.start_line);
        }
    }

    #[test]
    fn test_parse_task_contexts() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            import Foundation

            // Regular function creates Task
            func regularFunction() {
                Task {
                    print("Regular function task")
                }
            }

            // Async function creates Task (inherits async context)
            func asyncFunction() async {
                Task {
                    print("Async function task")
                }
                
                Task.detached {
                    print("Detached from async context")
                }
                
                await Task.sleep(nanoseconds: 1000)
            }

            // @MainActor function creates Task (inherits MainActor context)
            @MainActor
            func mainActorFunction() {
                Task {
                    print("MainActor function task")
                }
                
                Task.detached {
                    print("Detached from MainActor")
                }
            }

            // @MainActor async function creates Task
            @MainActor
            func mainActorAsyncFunction() async {
                Task {
                    print("MainActor async task")
                }
            }

            // Task with priority
            func priorityTaskFunction() {
                Task(priority: .high) {
                    print("High priority task")
                }
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let _symbols = parser.parse(code, file_id, &mut counter);
        
        // Test task context detection
        let task_contexts = parser.find_task_contexts(code);
        
        // Verify we found all the Task creations
        assert!(task_contexts.len() >= 6, "Should find at least 6 task contexts, found {}", task_contexts.len());
        
        // Find specific task contexts
        let regular_task = task_contexts.iter()
            .find(|tc| tc.creator_function == "regularFunction" && tc.task_info.task_type == TaskType::Standard)
            .expect("Should find regular function Task");
        assert_eq!(regular_task.creator_context, AsyncContext::Regular);
        assert!(regular_task.task_info.inherits_context);

        let async_task = task_contexts.iter()
            .find(|tc| tc.creator_function == "asyncFunction" && tc.task_info.task_type == TaskType::Standard)
            .expect("Should find async function Task");
        assert_eq!(async_task.creator_context, AsyncContext::Async);
        assert!(async_task.task_info.inherits_context);

        let async_detached = task_contexts.iter()
            .find(|tc| tc.creator_function == "asyncFunction" && tc.task_info.task_type == TaskType::Detached)
            .expect("Should find async function detached Task");
        assert_eq!(async_detached.creator_context, AsyncContext::Async);
        assert!(!async_detached.task_info.inherits_context); // Detached breaks context

        let main_actor_task = task_contexts.iter()
            .find(|tc| tc.creator_function == "mainActorFunction" && tc.task_info.task_type == TaskType::Standard)
            .expect("Should find MainActor function Task");
        assert_eq!(main_actor_task.creator_context, AsyncContext::MainActor);
        assert!(main_actor_task.task_info.inherits_context);

        let main_actor_async_task = task_contexts.iter()
            .find(|tc| tc.creator_function == "mainActorAsyncFunction" && tc.task_info.task_type == TaskType::Standard)
            .expect("Should find MainActor async function Task");
        assert_eq!(main_actor_async_task.creator_context, AsyncContext::AsyncMainActor);
        assert!(main_actor_async_task.task_info.inherits_context);

        // Check for Task.sleep
        let sleep_task = task_contexts.iter()
            .find(|tc| tc.creator_function == "asyncFunction" && tc.task_info.task_type == TaskType::Sleep)
            .expect("Should find Task.sleep call");
        assert_eq!(sleep_task.creator_context, AsyncContext::Async);
        assert!(sleep_task.task_info.inherits_context);
    }

    #[test]
    fn test_parse_main_actor() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            // @MainActor on a class
            @MainActor
            class AppState: ObservableObject {
                @Published var count: Int = 0
                
                func increment() {
                    count += 1
                }
            }

            // @MainActor on individual function
            @MainActor
            func updateUI() {
                print("Updating UI on main actor")
            }

            // @MainActor on a property
            class ViewModel {
                @MainActor
                var uiData: String = ""
                
                @MainActor
                func processData() async {
                    uiData = "processed"
                }
            }

            // Combined with async
            @MainActor
            func asyncMainActorFunction() async throws -> String {
                return "result"
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        // Find the @MainActor class
        let main_actor_class = symbols.iter()
            .find(|s| s.name.as_ref() == "AppState" && s.kind == SymbolKind::Class)
            .expect("AppState should be found");
        
        assert_eq!(main_actor_class.signature.as_deref(), Some("@MainActor"));
        
        // Find the @MainActor function
        let main_actor_func = symbols.iter()
            .find(|s| s.name.as_ref() == "updateUI" && s.kind == SymbolKind::Function)
            .expect("updateUI should be found");
        
        assert_eq!(main_actor_func.signature.as_deref(), Some("@MainActor"));
        
        // Find the @MainActor property (should be captured as property wrapper)
        let main_actor_prop = symbols.iter()
            .find(|s| s.name.as_ref() == "uiData" && s.kind == SymbolKind::Field)
            .expect("uiData should be found");
        
        // Property should include @MainActor in signature along with other wrappers
        assert!(main_actor_prop.signature.is_some());
        assert!(main_actor_prop.signature.as_ref().unwrap().contains("@MainActor"));
        
        // Find the @MainActor async method
        let main_actor_async_method = symbols.iter()
            .find(|s| s.name.as_ref() == "processData" && s.kind == SymbolKind::Method)
            .expect("processData should be found");
        
        assert_eq!(main_actor_async_method.signature.as_deref(), Some("@MainActor async"));
        
        // Find the combined @MainActor async throws function
        let combined_func = symbols.iter()
            .find(|s| s.name.as_ref() == "asyncMainActorFunction" && s.kind == SymbolKind::Function)
            .expect("asyncMainActorFunction should be found");
        
        assert_eq!(combined_func.signature.as_deref(), Some("@MainActor async throws"));
    }

    #[test]
    fn test_parse_viewbuilder_and_result_builders() {
        let mut parser = SwiftParser::new().unwrap();
        let code = r#"
            import SwiftUI
            
            // SwiftUI View with @ViewBuilder computed properties
            struct ContainerView: View {
                var body: some View {
                    Text("Regular body")
                }
                
                @ViewBuilder
                var headerView: some View {
                    Text("Header")
                        .font(.headline)
                }
                
                @ViewBuilder
                var customSection: some View {
                    VStack {
                        Text("Custom")
                        Text("Section")
                    }
                }
            }
            
            // @ViewBuilder functions
            struct ContentBuilder {
                @ViewBuilder
                func buildMainContent() -> some View {
                    VStack {
                        Text("Main Content")
                    }
                }
                
                @ViewBuilder
                static func buildStaticContent() -> some View {
                    HStack {
                        Text("Static")
                        Text("Content")
                    }
                }
            }
            
            // Combined attributes
            struct ActorView: View {
                @MainActor
                @ViewBuilder
                var safeUIContent: some View {
                    Text("Safe UI")
                }
                
                @MainActor
                @ViewBuilder
                func buildSafeContent() async -> some View {
                    Text("Async Safe Content")
                }
            }
            
            // Custom result builder
            @resultBuilder
            struct MyBuilder {
                static func buildBlock(_ components: String...) -> String {
                    components.joined()
                }
            }
            
            @MyBuilder
            func testCustomBuilder() -> String {
                "Hello"
                "World"
            }
        "#;
        let file_id = FileId::new(1).unwrap();

        let mut counter = 1u32;
        let symbols = parser.parse(code, file_id, &mut counter);

        // Find @ViewBuilder computed properties
        let header_view = symbols.iter()
            .find(|s| s.name.as_ref() == "headerView" && s.kind == SymbolKind::Field)
            .expect("headerView should be found");
        
        assert!(header_view.signature.is_some());
        assert_eq!(header_view.signature.as_deref(), Some("@ViewBuilder"));

        let custom_section = symbols.iter()
            .find(|s| s.name.as_ref() == "customSection" && s.kind == SymbolKind::Field)
            .expect("customSection should be found");
        
        assert!(custom_section.signature.is_some());
        assert_eq!(custom_section.signature.as_deref(), Some("@ViewBuilder"));

        // Find @ViewBuilder functions
        let build_main = symbols.iter()
            .find(|s| s.name.as_ref() == "buildMainContent" && s.kind == SymbolKind::Method)
            .expect("buildMainContent should be found");
        
        assert!(build_main.signature.is_some());
        assert_eq!(build_main.signature.as_deref(), Some("@ViewBuilder"));

        let build_static = symbols.iter()
            .find(|s| s.name.as_ref() == "buildStaticContent" && s.kind == SymbolKind::Method)
            .expect("buildStaticContent should be found");
        
        assert!(build_static.signature.is_some());
        assert_eq!(build_static.signature.as_deref(), Some("@ViewBuilder"));

        // Find combined @MainActor @ViewBuilder
        let safe_ui_content = symbols.iter()
            .find(|s| s.name.as_ref() == "safeUIContent" && s.kind == SymbolKind::Field)
            .expect("safeUIContent should be found");
        
        assert!(safe_ui_content.signature.is_some());
        assert_eq!(safe_ui_content.signature.as_deref(), Some("@MainActor @ViewBuilder"));

        let build_safe = symbols.iter()
            .find(|s| s.name.as_ref() == "buildSafeContent" && s.kind == SymbolKind::Method)
            .expect("buildSafeContent should be found");
        
        assert!(build_safe.signature.is_some());
        assert_eq!(build_safe.signature.as_deref(), Some("@MainActor @ViewBuilder async"));

        // Find custom result builder
        let custom_builder_func = symbols.iter()
            .find(|s| s.name.as_ref() == "testCustomBuilder" && s.kind == SymbolKind::Function)
            .expect("testCustomBuilder should be found");
        
        assert!(custom_builder_func.signature.is_some());
        assert_eq!(custom_builder_func.signature.as_deref(), Some("@MyBuilder"));

        // Verify regular body property doesn't have @ViewBuilder (should be inferred from SwiftUI.View)
        let body_prop = symbols.iter()
            .find(|s| s.name.as_ref() == "body" && s.kind == SymbolKind::Field)
            .expect("body should be found");
        
        // body should have signature indicating SwiftUI View composition
        // This will be handled by existing SwiftUI View detection logic
        println!("Body signature: {:?}", body_prop.signature);
    }
}