//! Kotlin language parser implementation
//!
//! Provides symbol extraction for Kotlin using tree-sitter.

use crate::config;
use crate::parsing::Import;
use crate::parsing::parser::check_recursion_depth;
use crate::parsing::{
    HandledNode, Language, LanguageParser, NodeTracker, NodeTrackingState, ParserContext, ScopeType,
};
use crate::types::SymbolCounter;
use crate::{FileId, Range, Symbol, SymbolKind, Visibility};
use std::any::Any;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use tree_sitter::{Node, Parser};

// Constants for commonly accessed node kinds
const FILE_SCOPE: &str = "<file>";
const NODE_CLASS_DECLARATION: &str = "class_declaration";
const NODE_OBJECT_DECLARATION: &str = "object_declaration";
const NODE_FUNCTION_DECLARATION: &str = "function_declaration";
const NODE_PROPERTY_DECLARATION: &str = "property_declaration";
const NODE_SECONDARY_CONSTRUCTOR: &str = "secondary_constructor";
const NODE_PACKAGE_HEADER: &str = "package_header";
const NODE_MULTILINE_COMMENT: &str = "multiline_comment";
const NODE_LINE_COMMENT: &str = "line_comment";
const NODE_TYPE_IDENTIFIER: &str = "type_identifier";
const NODE_SIMPLE_IDENTIFIER: &str = "simple_identifier";
const NODE_CLASS_BODY: &str = "class_body";
const NODE_ENUM_CLASS_BODY: &str = "enum_class_body";
const NODE_FUNCTION_BODY: &str = "function_body";
const NODE_BLOCK: &str = "block";
const NODE_MODIFIERS: &str = "modifiers";
const NODE_INTERFACE: &str = "interface";
const NODE_ENUM: &str = "enum";
const NODE_ENUM_ENTRY: &str = "enum_entry";
const NODE_VARIABLE_DECLARATION: &str = "variable_declaration";
const NODE_CALL_EXPRESSION: &str = "call_expression";
const NODE_DELEGATION_SPECIFIER: &str = "delegation_specifier";
const NODE_TYPE_REFERENCE: &str = "type_reference";
const NODE_USER_TYPE: &str = "user_type";
const NODE_SIMPLE_USER_TYPE: &str = "simple_user_type";
const NODE_PARAMETER: &str = "parameter";
const NODE_CLASS_PARAMETER: &str = "class_parameter";
const NODE_FUNCTION_VALUE_PARAMETERS: &str = "function_value_parameters";
const NODE_PRIMARY_CONSTRUCTOR: &str = "primary_constructor";
const CALL_INFER_MAX_DEPTH: usize = 3;

// Lazy-initialized HashSet for primitive types
static KOTLIN_PRIMITIVE_TYPES: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn get_primitive_types() -> &'static HashSet<&'static str> {
    KOTLIN_PRIMITIVE_TYPES.get_or_init(|| {
        let mut set = HashSet::new();
        set.insert("Int");
        set.insert("Long");
        set.insert("Short");
        set.insert("Byte");
        set.insert("Float");
        set.insert("Double");
        set.insert("Boolean");
        set.insert("Char");
        set.insert("String");
        set.insert("Unit");
        set.insert("Any");
        set.insert("Nothing");
        set
    })
}

fn literal_type_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "integer_literal" => Some("Int"),
        "string_literal" => Some("String"),
        "boolean_literal" => Some("Boolean"),
        "real_literal" => Some("Double"),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct GenericParameter<'a> {
    name: &'a str,
    #[allow(dead_code)]
    constraints: Vec<&'a str>,
}

#[derive(Debug, Clone)]
struct FunctionSignature<'a> {
    name: &'a str,
    generic_params: Vec<GenericParameter<'a>>,
    param_types: Vec<&'a str>,
    return_type: Option<&'a str>,
}

impl<'a> FunctionSignature<'a> {
    #[inline]
    fn arity(&self) -> usize {
        self.param_types.len()
    }

    #[inline]
    fn has_generics(&self) -> bool {
        !self.generic_params.is_empty()
    }
}

type FunctionSignatureMap<'a> = HashMap<&'a str, Vec<FunctionSignature<'a>>>;

/// Parser for Kotlin source files
pub struct KotlinParser {
    parser: Parser,
    node_tracker: NodeTrackingState,
}

impl std::fmt::Debug for KotlinParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KotlinParser")
            .field("language", &"Kotlin")
            .finish()
    }
}

impl KotlinParser {
    /// Create a new parser instance
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_kotlin::language())
            .map_err(|e| format!("Failed to initialize Kotlin parser: {e}"))?;

        Ok(Self {
            parser,
            node_tracker: NodeTrackingState::new(),
        })
    }

    /// Convert a tree-sitter node into a Range
    fn node_to_range(&self, node: Node) -> Range {
        let start = node.start_position();
        let end = node.end_position();
        Range {
            start_line: start.row as u32,
            start_column: start.column as u16,
            end_line: end.row as u32,
            end_column: end.column as u16,
        }
    }

    /// Helper to register handled node kinds for audit tracking
    fn register_node(&mut self, node: &Node) {
        self.node_tracker
            .register_handled_node(node.kind(), node.kind_id());
    }

    /// Extract raw source text for a node
    fn text_for_node<'a>(&self, code: &'a str, node: Node) -> &'a str {
        &code[node.byte_range()]
    }

    #[inline]
    fn trimmed_text<'a>(&self, code: &'a str, node: Node) -> &'a str {
        self.text_for_node(code, node).trim()
    }

    /// Extract documentation comments (/** */ or //) - optimized version
    /// Uses stack-allocated buffer and minimizes allocations
    fn doc_comment_for(&self, node: &Node, code: &str) -> Option<String> {
        let mut result = String::new();
        let mut has_comment = false;

        // Stack to collect comments in reverse order (we traverse backwards)
        let mut comment_stack: [Option<&str>; 8] = [None; 8];
        let mut stack_len = 0;
        let mut current = node.prev_sibling();

        // Special case: if previous sibling is package_header, check its children for comments
        if let Some(sibling) = current {
            if sibling.kind() == NODE_PACKAGE_HEADER {
                let mut cursor = sibling.walk();
                for child in sibling.named_children(&mut cursor) {
                    let child_kind = child.kind();
                    if child_kind == NODE_MULTILINE_COMMENT || child_kind == NODE_LINE_COMMENT {
                        let raw = self.text_for_node(code, child);
                        if let Some(cleaned) = self.extract_comment_text(raw, &mut result) {
                            if has_comment {
                                result.push('\n');
                            }
                            result.push_str(cleaned);
                            has_comment = true;
                        }
                    }
                }
                if has_comment {
                    return Some(result);
                }
            }
        }

        // Standard case: traverse backwards through siblings collecting comments
        current = node.prev_sibling();
        while let Some(sibling) = current {
            let sibling_kind = sibling.kind();
            if sibling_kind != NODE_MULTILINE_COMMENT && sibling_kind != NODE_LINE_COMMENT {
                break;
            }

            let raw = self.text_for_node(code, sibling);
            // Try to use stack allocation for small numbers of comments
            if stack_len < comment_stack.len() {
                if let Some(cleaned) = self.peek_comment_text(raw) {
                    comment_stack[stack_len] = Some(cleaned);
                    stack_len += 1;
                    current = sibling.prev_sibling();
                    continue;
                }
            }
            break;
        }

        // Build result from stack (in reverse order to get correct ordering)
        if stack_len > 0 {
            for i in (0..stack_len).rev() {
                if let Some(comment) = comment_stack[i] {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(comment);
                    has_comment = true;
                }
            }
        }

        if has_comment { Some(result) } else { None }
    }

    /// Extract and clean comment text, writing directly to result buffer
    /// Returns a reference to the cleaned text within the raw string when possible
    fn extract_comment_text<'a>(&self, raw: &'a str, _result: &mut String) -> Option<&'a str> {
        let trimmed = raw.trim();
        if let Some(content) = trimmed
            .strip_prefix("/**")
            .and_then(|s| s.strip_suffix("*/"))
        {
            let cleaned = content.trim();
            return Some(cleaned);
        } else if let Some(content) = trimmed.strip_prefix("///") {
            let cleaned = content.trim();
            return Some(cleaned);
        }
        None
    }

    fn extract_generic_parameters<'a>(
        &self,
        params_node: Node,
        code: &'a str,
    ) -> Vec<GenericParameter<'a>> {
        let mut generics = Vec::new();
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            if child.kind() != "type_parameter" {
                continue;
            }

            let mut name = None;
            let mut constraints = Vec::new();
            let mut param_cursor = child.walk();
            for grandchild in child.children(&mut param_cursor) {
                match grandchild.kind() {
                    NODE_TYPE_IDENTIFIER | NODE_SIMPLE_IDENTIFIER => {
                        if name.is_none() {
                            name = Some(self.trimmed_text(code, grandchild));
                        }
                    }
                    NODE_USER_TYPE | NODE_TYPE_REFERENCE | NODE_SIMPLE_USER_TYPE => {
                        constraints.push(self.trimmed_text(code, grandchild));
                    }
                    _ => {}
                }
            }

            if let Some(name) = name {
                generics.push(GenericParameter { name, constraints });
            }
        }
        generics
    }

    fn extract_function_parameter_types<'a>(
        &self,
        params_node: Node,
        code: &'a str,
    ) -> Vec<&'a str> {
        let mut types = Vec::new();
        let mut cursor = params_node.walk();
        for param in params_node.children(&mut cursor) {
            if param.kind() != NODE_PARAMETER && param.kind() != NODE_CLASS_PARAMETER {
                continue;
            }
            if let Some(param_type) = self.extract_parameter_type(param, code) {
                types.push(param_type);
            }
        }
        types
    }

    fn extract_parameter_type<'a>(&self, param_node: Node, code: &'a str) -> Option<&'a str> {
        let mut cursor = param_node.walk();
        for child in param_node.children(&mut cursor) {
            match child.kind() {
                NODE_USER_TYPE | NODE_TYPE_REFERENCE | NODE_SIMPLE_USER_TYPE | "nullable_type" => {
                    return Some(self.trimmed_text(code, child));
                }
                "type_annotation" => {
                    if let Some(type_child) = child.child_by_field_name("type") {
                        return Some(self.trimmed_text(code, type_child));
                    }
                }
                _ => continue,
            }
        }
        None
    }

    fn extract_return_type_annotation<'a>(
        &self,
        func_node: Node,
        code: &'a str,
    ) -> Option<&'a str> {
        let mut cursor = func_node.walk();
        let mut seen_params = false;
        for child in func_node.children(&mut cursor) {
            match child.kind() {
                NODE_FUNCTION_VALUE_PARAMETERS | NODE_PRIMARY_CONSTRUCTOR => {
                    seen_params = true;
                }
                NODE_FUNCTION_BODY | NODE_CLASS_BODY | NODE_BLOCK => {
                    break;
                }
                NODE_USER_TYPE
                | NODE_TYPE_REFERENCE
                | NODE_SIMPLE_USER_TYPE
                | "nullable_type"
                | "type" => {
                    if seen_params {
                        return Some(self.trimmed_text(code, child));
                    }
                }
                _ => continue,
            }
        }
        None
    }

    fn extract_function_signature<'a>(
        &self,
        node: Node,
        code: &'a str,
    ) -> Option<FunctionSignature<'a>> {
        let mut cursor = node.walk();
        let mut name = None;
        let mut type_params = None;
        let mut params_node = None;

        for child in node.children(&mut cursor) {
            match child.kind() {
                NODE_SIMPLE_IDENTIFIER => {
                    if name.is_none() {
                        name = Some(self.trimmed_text(code, child));
                    }
                }
                "type_parameters" => type_params = Some(child),
                NODE_FUNCTION_VALUE_PARAMETERS => params_node = Some(child),
                _ => continue,
            }
        }

        let name = name?;
        let param_types = params_node
            .map(|node| self.extract_function_parameter_types(node, code))
            .unwrap_or_default();

        let generic_params = type_params
            .map(|node| self.extract_generic_parameters(node, code))
            .unwrap_or_default();

        let return_type = self.extract_return_type_annotation(node, code);

        Some(FunctionSignature {
            name,
            generic_params,
            param_types,
            return_type,
        })
    }

    fn collect_function_signatures<'a>(
        &self,
        node: Node,
        code: &'a str,
        signatures: &mut FunctionSignatureMap<'a>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        if node.kind() == NODE_FUNCTION_DECLARATION {
            if let Some(signature) = self.extract_function_signature(node, code) {
                signatures
                    .entry(signature.name)
                    .or_default()
                    .push(signature);
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_function_signatures(child, code, signatures, depth + 1);
        }
    }

    fn insert_var_type<'a>(
        &self,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
        expr: &'a str,
        typ: &'a str,
        range: Range,
    ) {
        if var_types.iter().any(|(existing, _, _)| *existing == expr) {
            return;
        }
        var_types.push((expr, typ, range));
    }

    fn lookup_var_type<'a>(
        &self,
        expr: &'a str,
        var_types: &[(&'a str, &'a str, Range)],
    ) -> Option<&'a str> {
        var_types
            .iter()
            .rev()
            .find(|(existing, _, _)| *existing == expr)
            .map(|(_, typ, _)| *typ)
    }

    fn record_literal_type<'a>(
        &self,
        node: Node,
        code: &'a str,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        if let Some(type_name) = literal_type_for_kind(node.kind()) {
            let text = self.trimmed_text(code, node);
            if self.lookup_var_type(text, var_types).is_none() {
                let range = self.node_to_range(node);
                self.insert_var_type(var_types, text, type_name, range);
            }
        }
    }

    fn extract_callee_name<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        if node.kind() == NODE_SIMPLE_IDENTIFIER {
            return Some(self.trimmed_text(code, node));
        }

        match node.kind() {
            "navigation_expression" | "navigation_suffix" => {
                self.find_last_simple_identifier(node, code)
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.extract_callee_name(child, code) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn find_last_simple_identifier<'a>(&self, node: Node, code: &'a str) -> Option<&'a str> {
        let mut last = if node.kind() == NODE_SIMPLE_IDENTIFIER {
            Some(self.trimmed_text(code, node))
        } else {
            None
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(name) = self.find_last_simple_identifier(child, code) {
                last = Some(name);
            }
        }

        last
    }

    fn collect_argument_types<'a>(
        &self,
        call_node: Node,
        code: &'a str,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
        signatures: &FunctionSignatureMap<'a>,
        infer_depth: usize,
    ) -> Option<Vec<&'a str>> {
        let mut types = Vec::new();
        let mut cursor = call_node.walk();
        for child in call_node.children(&mut cursor) {
            if child.kind() != "call_suffix" {
                continue;
            }

            let mut suffix_cursor = child.walk();
            for suffix_child in child.children(&mut suffix_cursor) {
                if suffix_child.kind() != "value_arguments" {
                    continue;
                }

                let mut arg_cursor = suffix_child.walk();
                for value_arg in suffix_child.children(&mut arg_cursor) {
                    if value_arg.kind() != "value_argument" {
                        continue;
                    }

                    if let Some(arg_type) = self.extract_value_argument_type(
                        value_arg,
                        code,
                        var_types,
                        signatures,
                        infer_depth,
                    ) {
                        types.push(arg_type);
                    } else {
                        return None;
                    }
                }
            }
        }
        Some(types)
    }

    fn extract_value_argument_type<'a>(
        &self,
        value_arg: Node,
        code: &'a str,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
        signatures: &FunctionSignatureMap<'a>,
        infer_depth: usize,
    ) -> Option<&'a str> {
        let mut cursor = value_arg.walk();
        for child in value_arg.children(&mut cursor) {
            if child.kind() == "value_argument_name" {
                continue;
            }
            if child.is_named() {
                return self.resolve_expression_type(
                    child,
                    code,
                    var_types,
                    signatures,
                    infer_depth,
                );
            }
        }
        None
    }

    fn resolve_expression_type<'a>(
        &self,
        node: Node,
        code: &'a str,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
        signatures: &FunctionSignatureMap<'a>,
        infer_depth: usize,
    ) -> Option<&'a str> {
        let expr_text = self.trimmed_text(code, node);
        if let Some(existing) = self.lookup_var_type(expr_text, var_types) {
            return Some(existing);
        }

        if let Some(literal_type) = literal_type_for_kind(node.kind()) {
            let range = self.node_to_range(node);
            self.insert_var_type(var_types, expr_text, literal_type, range);
            return Some(literal_type);
        }

        if node.kind() == NODE_CALL_EXPRESSION {
            return self.infer_call_expression_type(
                node,
                code,
                var_types,
                signatures,
                infer_depth + 1,
            );
        }

        None
    }

    fn apply_signature<'a>(
        &self,
        signature: &FunctionSignature<'a>,
        arg_types: &[&'a str],
    ) -> Option<Cow<'a, str>> {
        let return_type = signature.return_type?;
        if !signature.has_generics() {
            return Some(Cow::Borrowed(return_type));
        }

        let substitutions = self.build_generic_substitution(signature, arg_types);
        if substitutions.is_empty() {
            return Some(Cow::Borrowed(return_type));
        }

        // Check for simple substitution (entire return type is a generic param)
        let normalized_return = return_type.trim();
        if let Some(concrete_type) = substitutions.get(normalized_return) {
            return Some(Cow::Borrowed(concrete_type));
        }

        // Complex substitution needed (e.g., List<T> → List<Int>)
        let substituted = self.apply_type_substitution(return_type, &substitutions);
        Some(Cow::Owned(substituted))
    }

    fn infer_call_expression_type<'a>(
        &self,
        node: Node,
        code: &'a str,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
        signatures: &FunctionSignatureMap<'a>,
        infer_depth: usize,
    ) -> Option<&'a str> {
        if infer_depth >= CALL_INFER_MAX_DEPTH {
            return None;
        }

        let call_text = self.trimmed_text(code, node);
        if let Some(existing) = self.lookup_var_type(call_text, var_types) {
            return Some(existing);
        }

        let callee_node = node.child(0)?;
        let callee_name = self.extract_callee_name(callee_node, code)?;

        let arg_types =
            self.collect_argument_types(node, code, var_types, signatures, infer_depth)?;

        let candidates = signatures.get(callee_name)?;
        for signature in candidates {
            if signature.arity() != arg_types.len() {
                continue;
            }

            // For zero-copy path, we need a borrowed string
            // If it's owned (complex substitution), we can't return it here
            if let Some(Cow::Borrowed(result_type)) = self.apply_signature(signature, &arg_types) {
                let range = self.node_to_range(node);
                self.insert_var_type(var_types, call_text, result_type, range);
                if config::is_global_debug_enabled() {
                    eprintln!("[KOTLIN-VAR] Inferred type '{result_type}' for call '{call_text}'");
                }
                return Some(result_type);
            }
            // Complex substitution case - can't return borrowed, skip for zero-copy method
        }

        None
    }

    fn build_generic_substitution<'a>(
        &self,
        signature: &FunctionSignature<'a>,
        arg_types: &[&'a str],
    ) -> HashMap<&'a str, &'a str> {
        let mut substitutions = HashMap::new();
        if signature.generic_params.is_empty() {
            return substitutions;
        }

        // TODO: extend substitution to cover return-type expressions such as List<T> or T?
        for (param_type, arg_type) in signature.param_types.iter().zip(arg_types.iter()) {
            let normalized_param = param_type.trim();
            for generic in &signature.generic_params {
                if normalized_param == generic.name {
                    substitutions.insert(generic.name, *arg_type);
                    break;
                }
            }
        }

        substitutions
    }

    /// Apply generic substitutions to a type expression
    ///
    /// Handles complex types like `List<T>` → `List<Int>` by replacing generic
    /// parameters with their concrete types. Uses word boundaries to avoid
    /// incorrect substitutions (e.g., won't replace T in "Stream").
    fn apply_type_substitution(
        &self,
        type_expr: &str,
        substitutions: &HashMap<&str, &str>,
    ) -> String {
        let mut result = type_expr.to_string();

        for (generic_param, concrete_type) in substitutions {
            // Use regex with word boundaries to avoid partial matches
            // e.g., "T" won't match in "Stream", but will match in "List<T>"
            let pattern = format!(r"\b{}\b", regex::escape(generic_param));
            if let Ok(re) = regex::Regex::new(&pattern) {
                result = re.replace_all(&result, *concrete_type).to_string();
            }
        }

        result
    }

    /// Peek at comment text without allocating - returns a reference when possible
    fn peek_comment_text<'a>(&self, raw: &'a str) -> Option<&'a str> {
        let trimmed = raw.trim();
        if let Some(content) = trimmed
            .strip_prefix("/**")
            .and_then(|s| s.strip_suffix("*/"))
        {
            return Some(content.trim());
        } else if let Some(content) = trimmed.strip_prefix("///") {
            return Some(content.trim());
        }
        None
    }

    /// Determine visibility from modifiers
    fn determine_visibility(&self, node: Node, code: &str) -> Visibility {
        // Look for modifiers node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_MODIFIERS {
                let modifiers_text = self.text_for_node(code, child);
                if modifiers_text.contains("private") {
                    return Visibility::Private;
                } else if modifiers_text.contains("protected") {
                    return Visibility::Module; // Map protected to module-level
                } else if modifiers_text.contains("internal") {
                    return Visibility::Crate; // Map internal to crate-level
                }
            }
        }
        Visibility::Public // Kotlin default is public
    }

    /// Extract signature text for a node - optimized version
    /// Builds string directly to avoid intermediate allocations
    fn extract_signature(&self, node: Node, code: &str) -> String {
        // Pre-allocate with estimated size (most signatures are 50-150 chars)
        let mut signature = String::with_capacity(100);
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            let child_kind = child.kind();
            match child_kind {
                NODE_MODIFIERS => {
                    if !signature.is_empty() {
                        signature.push(' ');
                    }
                    signature.push_str(self.text_for_node(code, child));
                }
                NODE_SIMPLE_IDENTIFIER | NODE_TYPE_IDENTIFIER => {
                    if !signature.is_empty() {
                        signature.push(' ');
                    }
                    signature.push_str(self.text_for_node(code, child));
                }
                NODE_FUNCTION_VALUE_PARAMETERS | "class_parameters" => {
                    if !signature.is_empty() {
                        signature.push(' ');
                    }
                    signature.push_str(self.text_for_node(code, child));
                }
                "type" | NODE_USER_TYPE | NODE_TYPE_REFERENCE => {
                    signature.push_str(": ");
                    signature.push_str(self.text_for_node(code, child));
                }
                _ => {}
            }
        }

        signature
    }

    /// Extract signature and metadata in a single pass - used for functions
    /// Returns (name, visibility, signature_parts, body_node)
    fn extract_function_info<'a>(
        &self,
        node: Node<'a>,
        code: &str,
    ) -> (
        Option<String>,
        Option<String>,
        Visibility,
        String,
        Option<Node<'a>>,
    ) {
        let mut func_name = None;
        let mut receiver_type = None;
        let mut visibility = Visibility::Public;
        let mut signature = String::with_capacity(100);
        let mut body_node = None;
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            let child_kind = child.kind();
            match child_kind {
                "receiver_type" => {
                    // Extension function receiver (e.g., "Int" in "fun Int.bar()")
                    let receiver_text = self.text_for_node(code, child).trim();
                    receiver_type = Some(receiver_text.to_string());
                    signature.push_str(receiver_text);
                    signature.push('.');
                }
                NODE_SIMPLE_IDENTIFIER if func_name.is_none() => {
                    func_name = Some(self.text_for_node(code, child).trim().to_string());
                    if !signature.is_empty() && !signature.ends_with('.') {
                        signature.push(' ');
                    }
                    signature.push_str(self.text_for_node(code, child));
                }
                NODE_MODIFIERS => {
                    let modifiers_text = self.text_for_node(code, child);
                    // Extract visibility inline
                    if modifiers_text.contains("private") {
                        visibility = Visibility::Private;
                    } else if modifiers_text.contains("protected") {
                        visibility = Visibility::Module;
                    } else if modifiers_text.contains("internal") {
                        visibility = Visibility::Crate;
                    }
                    // Add to signature
                    if !signature.is_empty() {
                        signature.push(' ');
                    }
                    signature.push_str(modifiers_text);
                }
                NODE_FUNCTION_VALUE_PARAMETERS => {
                    if !signature.is_empty() {
                        signature.push(' ');
                    }
                    signature.push_str(self.text_for_node(code, child));
                }
                "type" | NODE_USER_TYPE | NODE_TYPE_REFERENCE => {
                    signature.push_str(": ");
                    signature.push_str(self.text_for_node(code, child));
                }
                NODE_FUNCTION_BODY => {
                    body_node = Some(child);
                }
                _ => {}
            }
        }

        (func_name, receiver_type, visibility, signature, body_node)
    }

    /// Process AST recursively and collect symbols
    fn extract_symbols_from_node(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        match node.kind() {
            NODE_CLASS_DECLARATION => {
                self.handle_class_declaration(
                    node, code, file_id, symbols, counter, context, depth,
                );
                return;
            }
            NODE_OBJECT_DECLARATION => {
                self.handle_object_declaration(
                    node, code, file_id, symbols, counter, context, depth,
                );
                return;
            }
            NODE_FUNCTION_DECLARATION => {
                self.handle_function_declaration(
                    node, code, file_id, symbols, counter, context, depth,
                );
                return;
            }
            NODE_PROPERTY_DECLARATION => {
                self.handle_property_declaration(node, code, file_id, symbols, counter, context);
            }
            NODE_SECONDARY_CONSTRUCTOR => {
                self.handle_secondary_constructor(node, code, file_id, symbols, counter, context);
            }
            NODE_PACKAGE_HEADER | "import_list" | "import_header" | "type_alias" => {
                self.register_node(&node);
            }
            "infix_expression" => {
                // Check for context receiver pattern: context(...) fun name() { }
                // AST: infix_expression > call_expression("context") + simple_identifier("fun") + call_expression(name + lambda)
                if self.try_extract_context_receiver_function(
                    node, code, file_id, symbols, counter, context, depth,
                ) {
                    return;
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols_from_node(
                child,
                code,
                file_id,
                symbols,
                counter,
                context,
                depth + 1,
            );
        }
    }

    fn handle_class_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type_parameters, value_parameters, etc.)
        self.register_node_recursively(node);

        // Check if this is actually an interface or enum (keywords are children of class_declaration)
        let mut is_interface = false;
        let mut is_enum = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_INTERFACE {
                is_interface = true;
                break;
            } else if child.kind() == NODE_ENUM {
                is_enum = true;
                break;
            }
        }

        // Extract class/interface name - find the type_identifier child
        let mut class_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_TYPE_IDENTIFIER {
                class_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let class_name = if let Some(name) = class_name {
            name
        } else {
            return;
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        // Determine symbol kind based on modifiers
        let symbol_kind = if is_interface {
            SymbolKind::Interface
        } else if is_enum {
            SymbolKind::Enum
        } else {
            SymbolKind::Class
        };

        let mut symbol = Symbol::new(symbol_id, class_name.as_str(), symbol_kind, file_id, range);
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }
        // Set scope context based on nesting
        // Nested class/object: ClassMember with parent class name
        // Top-level class/object: Module scope
        symbol.scope_context = Some(if let Some(parent_class) = context.current_class() {
            crate::symbol::ScopeContext::ClassMember {
                class_name: Some(parent_class.to_string().into()),
            }
        } else {
            crate::symbol::ScopeContext::Module
        });

        // Save parent context before entering new scope
        let saved_function = context.current_function().map(|s| s.to_string());
        let saved_class = context.current_class().map(|s| s.to_string());

        // Add to context
        context.enter_scope(ScopeType::Class);
        context.set_current_class(Some(class_name.clone()));
        symbols.push(symbol);

        // Process class/interface/enum body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_CLASS_BODY || child.kind() == NODE_ENUM_CLASS_BODY {
                let mut body_cursor = child.walk();
                for body_child in child.children(&mut body_cursor) {
                    // Extract enum entries as constants
                    if is_enum && body_child.kind() == NODE_ENUM_ENTRY {
                        self.handle_enum_entry(body_child, code, file_id, symbols, counter);
                    } else {
                        self.extract_symbols_from_node(
                            body_child,
                            code,
                            file_id,
                            symbols,
                            counter,
                            context,
                            depth + 1,
                        );
                    }
                }
                break;
            }
        }

        context.exit_scope();

        // Restore parent context
        context.set_current_function(saved_function);
        context.set_current_class(saved_class);
    }

    fn handle_object_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, etc.)
        self.register_node_recursively(node);

        // Extract object name - find the type_identifier child
        let mut object_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_TYPE_IDENTIFIER {
                object_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let object_name = if let Some(name) = object_name {
            name
        } else {
            return;
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        let mut symbol = Symbol::new(
            symbol_id,
            object_name.as_str(),
            SymbolKind::Class,
            file_id,
            range,
        );
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }
        // Set scope context based on nesting
        // Nested class/object: ClassMember with parent class name
        // Top-level class/object: Module scope
        symbol.scope_context = Some(if let Some(parent_class) = context.current_class() {
            crate::symbol::ScopeContext::ClassMember {
                class_name: Some(parent_class.to_string().into()),
            }
        } else {
            crate::symbol::ScopeContext::Module
        });

        // Save parent context before entering new scope
        let saved_function = context.current_function().map(|s| s.to_string());
        let saved_class = context.current_class().map(|s| s.to_string());

        context.enter_scope(ScopeType::Class);
        context.set_current_class(Some(object_name.clone()));
        symbols.push(symbol);

        // Process object body
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_CLASS_BODY {
                let mut body_cursor = child.walk();
                for body_child in child.children(&mut body_cursor) {
                    self.extract_symbols_from_node(
                        body_child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        depth + 1,
                    );
                }
                break;
            }
        }

        context.exit_scope();

        // Restore parent context
        context.set_current_function(saved_function);
        context.set_current_class(saved_class);
    }

    /// Try to extract a function from context receiver pattern
    /// Pattern: context(View, Database) fun save() { }
    /// AST: infix_expression > call_expression("context") + simple_identifier("fun") + call_expression(name + lambda)
    fn try_extract_context_receiver_function(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        _depth: usize,
    ) -> bool {
        self.register_node(&node);

        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        // Need at least 3 children: context(...), fun, name()
        if children.len() < 3 {
            return false;
        }

        // First child: call_expression with "context"
        let first = children[0];
        if first.kind() != "call_expression" {
            return false;
        }
        let mut fc = first.walk();
        let first_id = first
            .children(&mut fc)
            .find(|c| c.kind() == "simple_identifier");
        if let Some(id) = first_id {
            let name = &code[id.byte_range()];
            if name != "context" {
                return false;
            }
        } else {
            return false;
        }

        // Second child: simple_identifier "fun"
        let second = children[1];
        if second.kind() != "simple_identifier" {
            return false;
        }
        let fun_keyword = &code[second.byte_range()];
        if fun_keyword != "fun" {
            return false;
        }

        // Third child: call_expression with function name and lambda body
        let third = children[2];
        if third.kind() != "call_expression" {
            return false;
        }

        // Extract function name from third child
        let mut tc = third.walk();
        let func_name_node = third
            .children(&mut tc)
            .find(|c| c.kind() == "simple_identifier");
        let func_name = if let Some(id) = func_name_node {
            code[id.byte_range()].to_string()
        } else {
            return false;
        };

        // Create the function symbol
        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);

        let kind = if context.is_in_class() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let mut symbol = Symbol::new(symbol_id, func_name.as_str(), kind, file_id, range);
        symbol.visibility = Visibility::Public;
        symbol.signature = Some(format!("context fun {func_name}()").into());

        symbols.push(symbol);
        true
    }

    fn handle_function_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        context: &mut ParserContext,
        depth: usize,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type_parameters, value_parameters, etc.)
        self.register_node_recursively(node);

        // Extract all function info in a single pass
        let (func_name, receiver_type, visibility, signature, body_node) =
            self.extract_function_info(node, code);

        let func_name = if let Some(name) = func_name {
            name
        } else {
            return;
        };

        // For extension functions, use qualified name (e.g., "Int.bar" instead of "bar")
        let symbol_name = if let Some(ref receiver) = receiver_type {
            format!("{receiver}.{func_name}")
        } else {
            func_name.clone()
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let doc_comment = self.doc_comment_for(&node, code);

        // Determine if it's a method or top-level function
        let kind = if context.is_in_class() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let mut symbol = Symbol::new(symbol_id, symbol_name.as_str(), kind, file_id, range);
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        // Save parent context before entering new scope
        let saved_function = context.current_function().map(|s| s.to_string());
        let saved_class = context.current_class().map(|s| s.to_string());

        context.enter_scope(ScopeType::function());
        context.set_current_function(Some(func_name.clone()));
        symbols.push(symbol);

        // Lazy body traversal: Only traverse if body contains declarations
        // This optimization skips traversing function bodies that don't define nested symbols
        if let Some(body) = body_node {
            if self.body_contains_declarations(body) {
                let mut body_cursor = body.walk();
                for body_child in body.children(&mut body_cursor) {
                    self.extract_symbols_from_node(
                        body_child,
                        code,
                        file_id,
                        symbols,
                        counter,
                        context,
                        depth + 1,
                    );
                }
            }
        }

        context.exit_scope();

        // Restore parent context
        context.set_current_function(saved_function);
        context.set_current_class(saved_class);
    }

    /// Check if a function body contains any declaration nodes
    /// This allows us to skip traversing bodies that only contain expressions
    #[inline]
    fn body_contains_declarations(&self, body: Node) -> bool {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            let kind = child.kind();
            // Quick check for common declaration types
            if matches!(
                kind,
                NODE_CLASS_DECLARATION
                    | NODE_FUNCTION_DECLARATION
                    | NODE_PROPERTY_DECLARATION
                    | NODE_OBJECT_DECLARATION
            ) {
                return true;
            }
        }
        false
    }

    fn handle_property_declaration(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        _context: &mut ParserContext,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, type, variable_declaration, etc.)
        self.register_node_recursively(node);

        // Extract property name - find within variable_declaration
        let mut prop_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_VARIABLE_DECLARATION {
                let mut var_cursor = child.walk();
                for var_child in child.children(&mut var_cursor) {
                    if var_child.kind() == NODE_SIMPLE_IDENTIFIER {
                        prop_name = Some(self.text_for_node(code, var_child).trim().to_string());
                        break;
                    }
                }
                break;
            }
        }

        let prop_name = if let Some(name) = prop_name {
            name
        } else {
            return;
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let visibility = self.determine_visibility(node, code);
        let signature = self.extract_signature(node, code);
        let doc_comment = self.doc_comment_for(&node, code);

        let mut symbol = Symbol::new(
            symbol_id,
            prop_name.as_str(),
            SymbolKind::Field,
            file_id,
            range,
        );
        symbol.visibility = visibility;
        symbol.signature = Some(signature.into());
        if let Some(doc) = doc_comment {
            symbol.doc_comment = Some(doc.into());
        }

        symbols.push(symbol);
    }

    fn handle_secondary_constructor(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
        _context: &mut ParserContext,
    ) {
        // Register ALL child nodes recursively for audit (modifiers, value_parameters, etc.)
        self.register_node_recursively(node);

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let signature = self.extract_signature(node, code);

        let mut symbol = Symbol::new(symbol_id, "constructor", SymbolKind::Method, file_id, range);
        symbol.signature = Some(signature.into());

        symbols.push(symbol);
    }

    fn handle_enum_entry(
        &mut self,
        node: Node,
        code: &str,
        file_id: FileId,
        symbols: &mut Vec<Symbol>,
        counter: &mut SymbolCounter,
    ) {
        // Extract enum entry name - find the simple_identifier child
        let mut entry_name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == NODE_SIMPLE_IDENTIFIER {
                entry_name = Some(self.text_for_node(code, child).trim().to_string());
                break;
            }
        }

        let entry_name = if let Some(name) = entry_name {
            name
        } else {
            return;
        };

        let symbol_id = counter.next_id();
        let range = self.node_to_range(node);
        let signature = self.extract_signature(node, code);

        let mut symbol = Symbol::new(
            symbol_id,
            entry_name.as_str(),
            SymbolKind::Constant,
            file_id,
            range,
        );
        symbol.signature = Some(signature.into());
        symbol.visibility = Visibility::Public; // Enum entries are always public

        symbols.push(symbol);
    }

    /// Recursively find imports in the AST
    fn find_imports_in_node(
        &self,
        node: Node,
        code: &str,
        file_id: FileId,
        imports: &mut Vec<Import>,
    ) {
        if node.kind() == "import_header" {
            if let Some(identifier) = node.child_by_field_name("identifier") {
                let path = self.text_for_node(code, identifier).trim().to_string();
                if !path.is_empty() {
                    let is_glob = path.ends_with(".*") || path.contains("*");
                    imports.push(Import {
                        file_id,
                        path,
                        alias: None,
                        is_glob,
                        is_type_only: false,
                    });
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.find_imports_in_node(child, code, file_id, imports);
        }
    }

    /// Collect function calls recursively
    fn collect_calls<'a>(
        &self,
        node: Node,
        code: &'a str,
        calls: &mut Vec<(&'a str, &'a str, Range)>,
        current_function: Option<&'a str>,
    ) {
        if node.kind() == NODE_CALL_EXPRESSION {
            if let Some(callee) = node.child(0) {
                // Skip method calls (where callee is a navigation_expression like foo(3).bar)
                // Those are handled by find_method_calls
                if callee.kind() == "navigation_expression" {
                    // This is a method call, not a function call
                } else {
                    let caller = current_function.unwrap_or(FILE_SCOPE);
                    let callee_text = self.text_for_node(code, callee).trim();
                    if !callee_text.is_empty() {
                        calls.push((caller, callee_text, self.node_to_range(node)));
                    }
                }
            }
        }

        // Track current function
        let new_function = if node.kind() == NODE_FUNCTION_DECLARATION {
            // Find the simple_identifier child (function name)
            let mut cursor = node.walk();
            let mut func_name = None;
            for child in node.children(&mut cursor) {
                if child.kind() == NODE_SIMPLE_IDENTIFIER {
                    func_name = Some(self.text_for_node(code, child).trim());
                    break;
                }
            }
            func_name
        } else {
            None
        };

        let func_context = new_function.or(current_function);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_calls(child, code, calls, func_context);
        }
    }

    /// Collect method calls with enhanced receiver tracking
    fn collect_method_calls(
        &self,
        node: Node,
        code: &str,
        method_calls: &mut Vec<crate::parsing::MethodCall>,
        current_function: Option<&str>,
    ) {
        use crate::parsing::MethodCall;

        // Handle navigation_expression: receiver.method() or receiver.property
        if node.kind() == "navigation_expression" {
            // Get the receiver (left side of the dot)
            let receiver_text = if let Some(receiver_node) = node.child(0) {
                self.text_for_node(code, receiver_node).trim()
            } else {
                ""
            };

            // Get the navigation_suffix (right side of the dot)
            if let Some(nav_suffix) = node.child(1) {
                if nav_suffix.kind() == "navigation_suffix" {
                    // Find the simple_identifier child (skip the dot)
                    let mut method_name = String::new();
                    let mut cursor = nav_suffix.walk();
                    for child in nav_suffix.children(&mut cursor) {
                        if child.kind() == NODE_SIMPLE_IDENTIFIER {
                            method_name = self.text_for_node(code, child).trim().to_string();
                            break;
                        }
                    }

                    if !method_name.is_empty() {
                        // Check if this is followed by a call_suffix (making it a method call)
                        let parent = node.parent();
                        let is_method_call =
                            parent.is_some_and(|p| p.kind() == NODE_CALL_EXPRESSION);

                        if is_method_call {
                            let caller = current_function.unwrap_or(FILE_SCOPE);
                            let range = self.node_to_range(node);

                            let call = MethodCall::new(caller, &method_name, range)
                                .with_receiver(receiver_text);

                            method_calls.push(call);
                        }
                    }
                }
            }
        }

        // Note: Plain function calls (without receivers) are handled by find_calls,
        // not here, to avoid duplicates

        // Track current function context
        let new_function = if node.kind() == NODE_FUNCTION_DECLARATION {
            let mut cursor = node.walk();
            let mut func_name = None;
            for child in node.children(&mut cursor) {
                if child.kind() == NODE_SIMPLE_IDENTIFIER {
                    func_name = Some(self.text_for_node(code, child).trim());
                    break;
                }
            }
            func_name
        } else {
            None
        };

        let func_context = new_function.or(current_function);

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_method_calls(child, code, method_calls, func_context);
        }
    }

    /// Collect variable/expression type mappings for type inference.
    fn collect_variable_types<'a>(
        &self,
        node: Node,
        code: &'a str,
        var_types: &mut Vec<(&'a str, &'a str, Range)>,
        signatures: &FunctionSignatureMap<'a>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        // Visit children first so nested expressions are processed before parents.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_variable_types(child, code, var_types, signatures, depth + 1);
        }

        // Record literal expression types.
        self.record_literal_type(node, code, var_types);

        // Attempt to infer call expression return types.
        if node.kind() == NODE_CALL_EXPRESSION {
            let call_text = self.trimmed_text(code, node);
            if self.lookup_var_type(call_text, var_types).is_none() {
                self.infer_call_expression_type(node, code, var_types, signatures, 0);
            }
        }
    }

    /// Collect variable types with owned strings for complex generic substitution
    fn collect_variable_types_owned<'a>(
        &self,
        node: Node,
        code: &'a str,
        var_types: &mut Vec<(String, String, Range)>,
        signatures: &FunctionSignatureMap<'a>,
        depth: usize,
    ) {
        if !check_recursion_depth(depth, node) {
            return;
        }

        // Visit children first
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_variable_types_owned(child, code, var_types, signatures, depth + 1);
        }

        // Record literal types
        if let Some(literal_type) = literal_type_for_kind(node.kind()) {
            let text = self.trimmed_text(code, node).to_string();
            let range = self.node_to_range(node);
            if !var_types.iter().any(|(expr, _, _)| expr == &text) {
                var_types.push((text, literal_type.to_string(), range));
            }
        }

        // Infer call expression types with complex substitution
        if node.kind() == NODE_CALL_EXPRESSION {
            let call_text = self.trimmed_text(code, node).to_string();
            if var_types.iter().any(|(expr, _, _)| expr == &call_text) {
                return; // Already processed
            }

            if let Some(inferred_type) =
                self.infer_call_type_owned(node, code, var_types, signatures, 0)
            {
                let range = self.node_to_range(node);
                var_types.push((call_text, inferred_type, range));
            }
        }
    }

    /// Infer call expression type with owned string result (supports complex substitution)
    fn infer_call_type_owned<'a>(
        &self,
        node: Node,
        code: &'a str,
        var_types: &[(String, String, Range)],
        signatures: &FunctionSignatureMap<'a>,
        infer_depth: usize,
    ) -> Option<String> {
        if infer_depth >= CALL_INFER_MAX_DEPTH {
            return None;
        }

        let callee_node = node.child(0)?;
        let callee_name = self.extract_callee_name(callee_node, code)?;

        // Convert owned types to borrowed for compatibility with collect_argument_types
        // This allows nested calls to see previously-inferred types
        let mut borrowed_types: Vec<(&str, &str, Range)> = var_types
            .iter()
            .map(|(expr, ty, range)| (expr.as_str(), ty.as_str(), *range))
            .collect();

        let arg_types =
            self.collect_argument_types(node, code, &mut borrowed_types, signatures, infer_depth)?;

        let candidates = signatures.get(callee_name)?;
        for signature in candidates {
            if signature.arity() != arg_types.len() {
                continue;
            }

            if let Some(result_type_cow) = self.apply_signature(signature, &arg_types) {
                // Convert Cow to owned
                return Some(result_type_cow.into_owned());
            }
        }

        None
    }

    /// Collect inheritance relationships (extends/implements)
    fn collect_extends<'a>(
        &self,
        node: Node,
        code: &'a str,
        results: &mut Vec<(&'a str, &'a str, Range)>,
        current_class: Option<&'a str>,
    ) {
        // Track current class
        let new_class =
            if node.kind() == NODE_CLASS_DECLARATION || node.kind() == NODE_OBJECT_DECLARATION {
                // Find the type_identifier child (class name)
                let mut cursor = node.walk();
                let mut class_name = None;
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        class_name = Some(self.text_for_node(code, child).trim());
                        break;
                    }
                }
                class_name
            } else {
                None
            };

        let class_context = new_class.or(current_class);

        // Look for delegation specifiers (: SuperClass, Interface)
        if node.kind() == NODE_DELEGATION_SPECIFIER {
            if let Some(derived) = class_context {
                if let Some(type_node) = node.child(0) {
                    let base = self.text_for_node(code, type_node).trim();
                    // Remove constructor call syntax if present
                    let base_clean = base.split('(').next().unwrap_or(base).trim();
                    if !base_clean.is_empty() {
                        results.push((derived, base_clean, self.node_to_range(node)));
                    }
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_extends(child, code, results, class_context);
        }
    }

    /// Extract type usage relationships recursively
    fn extract_type_uses_recursive<'a>(
        &self,
        node: Node,
        code: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
        _current_context: Option<&'a str>,
    ) {
        match node.kind() {
            NODE_CLASS_DECLARATION | NODE_OBJECT_DECLARATION => {
                // Find the type_identifier child (class name)
                let mut class_name = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        class_name = Some(self.text_for_node(code, child).trim());
                        break;
                    }
                }

                if let Some(class_name) = class_name {
                    // Extract types from primary constructor parameters
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == NODE_PRIMARY_CONSTRUCTOR {
                            self.extract_parameter_types(child, code, class_name, uses);
                        }
                    }

                    // Process class body recursively
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == NODE_CLASS_BODY {
                            let mut body_cursor = child.walk();
                            for body_child in child.children(&mut body_cursor) {
                                self.extract_type_uses_recursive(
                                    body_child,
                                    code,
                                    uses,
                                    Some(class_name),
                                );
                            }
                        }
                    }
                }
                return;
            }
            NODE_FUNCTION_DECLARATION => {
                // Find the simple_identifier child (function name)
                let mut func_name = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_SIMPLE_IDENTIFIER {
                        func_name = Some(self.text_for_node(code, child).trim());
                        break;
                    }
                }

                if let Some(func_name) = func_name {
                    // Extract types from function parameters and return type
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == NODE_FUNCTION_VALUE_PARAMETERS {
                            self.extract_parameter_types(child, code, func_name, uses);
                        } else if child.kind() == NODE_USER_TYPE
                            || child.kind() == NODE_TYPE_REFERENCE
                        {
                            // This is the return type
                            if let Some(type_name) = self.extract_type_name(child, code) {
                                uses.push((func_name, type_name, self.node_to_range(child)));
                            }
                        }
                    }
                }
                return;
            }
            NODE_PROPERTY_DECLARATION => {
                // Property structure: property_declaration > variable_declaration > (simple_identifier, user_type)
                let mut prop_name = None;
                let mut prop_type = None;

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_VARIABLE_DECLARATION {
                        // Extract name and type from variable_declaration
                        let mut var_cursor = child.walk();
                        for var_child in child.children(&mut var_cursor) {
                            if var_child.kind() == NODE_SIMPLE_IDENTIFIER && prop_name.is_none() {
                                prop_name = Some(self.text_for_node(code, var_child).trim());
                            } else if (var_child.kind() == NODE_USER_TYPE
                                || var_child.kind() == NODE_TYPE_REFERENCE)
                                && prop_type.is_none()
                            {
                                if let Some(type_name) = self.extract_type_name(var_child, code) {
                                    prop_type = Some((type_name, self.node_to_range(var_child)));
                                }
                            }
                        }
                        break;
                    }
                }

                if let (Some(prop_name), Some((type_name, range))) = (prop_name, prop_type) {
                    uses.push((prop_name, type_name, range));
                }
                return;
            }
            _ => {}
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_type_uses_recursive(child, code, uses, _current_context);
        }
    }

    /// Extract parameter types from a parameters node
    fn extract_parameter_types<'a>(
        &self,
        params_node: Node,
        code: &'a str,
        context_name: &'a str,
        uses: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        let mut cursor = params_node.walk();
        for param in params_node.children(&mut cursor) {
            if param.kind() == NODE_PARAMETER || param.kind() == NODE_CLASS_PARAMETER {
                // Look for user_type or type_reference nodes within the parameter
                let mut param_cursor = param.walk();
                for child in param.children(&mut param_cursor) {
                    if child.kind() == NODE_USER_TYPE || child.kind() == NODE_TYPE_REFERENCE {
                        if let Some(type_name) = self.extract_type_name(child, code) {
                            uses.push((context_name, type_name, self.node_to_range(child)));
                        }
                    }
                }
            }
        }
    }

    /// Extract a simple type name from a type node
    fn extract_type_name<'a>(&self, type_node: Node, code: &'a str) -> Option<&'a str> {
        let primitives = get_primitive_types();

        // Handle different type node kinds
        match type_node.kind() {
            NODE_TYPE_REFERENCE | NODE_USER_TYPE | NODE_SIMPLE_USER_TYPE => {
                // Look for type_identifier or simple_identifier
                let mut cursor = type_node.walk();
                for child in type_node.children(&mut cursor) {
                    let child_kind = child.kind();
                    if child_kind == NODE_TYPE_IDENTIFIER || child_kind == NODE_SIMPLE_IDENTIFIER {
                        let type_name = self.text_for_node(code, child).trim();
                        // Filter out primitive types using HashSet
                        if !primitives.contains(type_name) {
                            return Some(type_name);
                        }
                    }
                }
            }
            NODE_TYPE_IDENTIFIER | NODE_SIMPLE_IDENTIFIER => {
                let type_name = self.text_for_node(code, type_node).trim();
                // Filter out primitive types using HashSet
                if !primitives.contains(type_name) {
                    return Some(type_name);
                }
            }
            _ => {}
        }
        None
    }

    /// Extract method definitions from classes and interfaces
    fn extract_method_defines_recursive<'a>(
        &self,
        node: Node,
        code: &'a str,
        defines: &mut Vec<(&'a str, &'a str, Range)>,
    ) {
        match node.kind() {
            NODE_CLASS_DECLARATION | NODE_OBJECT_DECLARATION => {
                // Find the type_identifier child (class name)
                let mut class_name = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_TYPE_IDENTIFIER {
                        class_name = Some(self.text_for_node(code, child).trim());
                        break;
                    }
                }

                let class_name = class_name.unwrap_or("anonymous");

                // Extract methods from class body
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == NODE_CLASS_BODY {
                        let mut body_cursor = child.walk();
                        for body_child in child.children(&mut body_cursor) {
                            if body_child.kind() == NODE_FUNCTION_DECLARATION {
                                // Find the simple_identifier child (method name)
                                let mut method_cursor = body_child.walk();
                                for method_child in body_child.children(&mut method_cursor) {
                                    if method_child.kind() == NODE_SIMPLE_IDENTIFIER {
                                        let method_name =
                                            self.text_for_node(code, method_child).trim();
                                        defines.push((
                                            class_name,
                                            method_name,
                                            self.node_to_range(body_child),
                                        ));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Recursively process children to handle nested classes
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_method_defines_recursive(child, code, defines);
        }
    }

    /// Access handled nodes for audit tooling
    pub fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    /// Register a node and all its children recursively for audit tracking
    /// This ensures nested nodes (modifiers, type_parameters, value_parameters, etc.) are tracked
    fn register_node_recursively(&mut self, node: Node) {
        self.register_handled_node(node.kind(), node.kind_id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.register_node_recursively(child);
        }
    }
}

impl LanguageParser for KotlinParser {
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

        let root = tree.root_node();
        let mut symbols = Vec::new();
        let mut context = ParserContext::new();

        // Create a file-level symbol to represent the file itself
        let module_id = symbol_counter.next_id();
        let mut module_symbol = Symbol::new(
            module_id,
            "<file>",
            SymbolKind::Module,
            file_id,
            self.node_to_range(root),
        );
        module_symbol.scope_context = Some(crate::symbol::ScopeContext::Module);
        symbols.push(module_symbol);

        self.extract_symbols_from_node(
            root,
            code,
            file_id,
            &mut symbols,
            symbol_counter,
            &mut context,
            0,
        );

        symbols
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn extract_doc_comment(&self, node: &Node, code: &str) -> Option<String> {
        self.doc_comment_for(node, code)
    }

    fn find_calls<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut calls = Vec::new();
        self.collect_calls(tree.root_node(), code, &mut calls, None);
        calls
    }

    fn find_implementations<'a>(&mut self, _code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        // Kotlin doesn't have explicit "implements" - it uses delegation specifiers
        // This is handled in find_extends
        Vec::new()
    }

    fn find_extends<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut results = Vec::new();
        self.collect_extends(tree.root_node(), code, &mut results, None);
        results
    }

    fn find_uses<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut uses = Vec::new();
        self.extract_type_uses_recursive(tree.root_node(), code, &mut uses, None);
        uses
    }

    fn find_defines<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut defines = Vec::new();
        self.extract_method_defines_recursive(tree.root_node(), code, &mut defines);
        defines
    }

    fn find_imports(&mut self, code: &str, file_id: FileId) -> Vec<Import> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut imports = Vec::new();

        self.find_imports_in_node(root_node, code, file_id, &mut imports);

        imports
    }

    fn find_method_calls(&mut self, code: &str) -> Vec<crate::parsing::MethodCall> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let mut method_calls = Vec::new();
        self.collect_method_calls(tree.root_node(), code, &mut method_calls, None);
        method_calls
    }

    fn find_variable_types<'a>(&mut self, code: &'a str) -> Vec<(&'a str, &'a str, Range)> {
        let tree = match self.parser.parse(code, None) {
            Some(tree) => tree,
            None => return Vec::new(),
        };

        let root = tree.root_node();
        let mut signatures: FunctionSignatureMap = HashMap::new();
        self.collect_function_signatures(root, code, &mut signatures, 0);

        let mut var_types = Vec::new();
        self.collect_variable_types(root, code, &mut var_types, &signatures, 0);
        var_types
    }

    fn find_variable_types_with_substitution(
        &mut self,
        code: &str,
    ) -> Option<Vec<(String, String, Range)>> {
        let tree = self.parser.parse(code, None)?;
        let root = tree.root_node();

        let mut signatures: FunctionSignatureMap = HashMap::new();
        self.collect_function_signatures(root, code, &mut signatures, 0);

        let mut owned_types = Vec::new();
        self.collect_variable_types_owned(root, code, &mut owned_types, &signatures, 0);

        Some(owned_types)
    }

    fn language(&self) -> Language {
        Language::Kotlin
    }
}

impl NodeTracker for KotlinParser {
    fn get_handled_nodes(&self) -> &std::collections::HashSet<HandledNode> {
        self.node_tracker.get_handled_nodes()
    }

    fn register_handled_node(&mut self, node_kind: &str, node_id: u16) {
        self.node_tracker.register_handled_node(node_kind, node_id);
    }
}
