//! Tree-sitter extraction for php / java / kotlin / go.
//!
//! Queries are written here against the grammars the crate pins; no
//! codanna parser or resolver code is in the loop, so a verdict stays
//! independent of the pick it audits. Text heuristics do not survive
//! contact with go (no class syntax; the owning type is the method
//! receiver), kotlin (`: Base()` supertypes, expression bodies) or php
//! (namespaces, trait `use` inside a class body).

use std::collections::BTreeMap;
use tree_sitter::{Node, Parser, Tree};

use crate::Lang;

pub struct Parsed {
    pub source: String,
    tree: Tree,
}

impl Parsed {
    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }
}

#[derive(Default)]
pub struct Ast {
    parsers: BTreeMap<Lang, Parser>,
    files: BTreeMap<String, Option<Parsed>>,
}

impl Ast {
    pub fn file(&mut self, lang: Lang, path: &str) -> Option<&Parsed> {
        if !self.files.contains_key(path) {
            let parsed = std::fs::read_to_string(path).ok().and_then(|source| {
                let parser = self
                    .parsers
                    .entry(lang)
                    .or_insert_with(|| parser_for(lang).expect("grammar loads"));
                let tree = parser.parse(&source, None)?;
                Some(Parsed { source, tree })
            });
            self.files.insert(path.to_string(), parsed);
        }
        self.files.get(path).and_then(Option::as_ref)
    }
}

pub fn parse_standalone(lang: Lang, source: String) -> Option<Parsed> {
    let mut parser = parser_for(lang)?;
    let tree = parser.parse(&source, None)?;
    Some(Parsed { source, tree })
}

fn parser_for(lang: Lang) -> Option<Parser> {
    let mut parser = Parser::new();
    let language = match lang {
        Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Lang::Java => tree_sitter_java::LANGUAGE.into(),
        Lang::Kotlin => tree_sitter_kotlin_codanna::language(),
        Lang::Go => tree_sitter_go::LANGUAGE.into(),
        Lang::Js => return None,
    };
    parser.set_language(&language).ok()?;
    Some(parser)
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn named_children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn child_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    named_children(node).into_iter().find(|c| c.kind() == kind)
}

fn fields<'a>(node: Node<'a>, name: &str) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children_by_field_name(name, &mut cursor).collect()
}

/// Nodes from the root down to the deepest one whose row span covers `line`.
fn chain_at(root: Node<'_>, line: usize) -> Vec<Node<'_>> {
    let mut chain = vec![root];
    let mut current = root;
    while let Some(next) = named_children(current)
        .into_iter()
        .find(|c| c.start_position().row <= line && c.end_position().row >= line)
    {
        chain.push(next);
        current = next;
    }
    chain
}

fn is_scope_boundary(lang: Lang, node: Node<'_>) -> bool {
    let kinds: &[&str] = match lang {
        Lang::Php => &[
            "function_definition",
            "method_declaration",
            "anonymous_function",
            "arrow_function",
            "anonymous_function_creation_expression",
        ],
        Lang::Java => &[
            "method_declaration",
            "constructor_declaration",
            "lambda_expression",
        ],
        Lang::Kotlin => &[
            "function_declaration",
            "lambda_literal",
            "anonymous_function",
        ],
        Lang::Go => &["function_declaration", "method_declaration", "func_literal"],
        Lang::Js => &[],
    };
    kinds.contains(&node.kind())
}

// ---------------------------------------------------------------- bindings

/// The type a receiver name is declared or constructed with at `call_line`.
///
/// Scopes are walked innermost-first and sibling function bodies are not
/// entered, so an inner shadow beats an outer declaration the way the
/// language's own scoping does.
pub fn binding_type(
    lang: Lang,
    parsed: &Parsed,
    receiver: &str,
    call_line: usize,
) -> Option<String> {
    let receiver = receiver.strip_prefix('$').unwrap_or(receiver);
    let chain = chain_at(parsed.root(), call_line);
    for scope in chain.iter().rev() {
        let mut hits: Vec<(usize, Node<'_>)> = Vec::new();
        collect_bindings(lang, *scope, &chain, parsed, receiver, &mut hits);
        let best = hits
            .iter()
            .filter(|(row, _)| *row <= call_line)
            .max_by_key(|(row, _)| *row)
            .or_else(|| hits.iter().min_by_key(|(row, _)| *row));
        if let Some((_, ty)) = best {
            if let Some(name) = type_name(lang, *ty, &parsed.source) {
                return Some(name);
            }
        }
    }
    None
}

fn collect_bindings<'a>(
    lang: Lang,
    scope: Node<'a>,
    chain: &[Node<'a>],
    parsed: &'a Parsed,
    receiver: &str,
    out: &mut Vec<(usize, Node<'a>)>,
) {
    let mut stack = vec![scope];
    while let Some(node) = stack.pop() {
        for (name, ty) in binding_sites(lang, node, &parsed.source) {
            if name == receiver {
                out.push((node.start_position().row, ty));
            }
        }
        for child in named_children(node) {
            let crosses_boundary = is_scope_boundary(lang, child)
                && !chain.iter().any(|ancestor| ancestor.id() == child.id());
            if !crosses_boundary {
                stack.push(child);
            }
        }
    }
}

/// `(bound name, node naming its type)` pairs declared by one node.
fn binding_sites<'a>(lang: Lang, node: Node<'a>, source: &str) -> Vec<(String, Node<'a>)> {
    match lang {
        Lang::Php => php_bindings(node, source),
        Lang::Java => java_bindings(node, source),
        Lang::Kotlin => kotlin_bindings(node, source),
        Lang::Go => go_bindings(node, source),
        Lang::Js => Vec::new(),
    }
}

fn php_var_name(node: Node<'_>, source: &str) -> Option<String> {
    let var = if node.kind() == "variable_name" {
        node
    } else {
        child_kind(node, "variable_name")?
    };
    child_kind(var, "name").map(|n| text(n, source).to_string())
}

fn php_bindings<'a>(node: Node<'a>, source: &str) -> Vec<(String, Node<'a>)> {
    match node.kind() {
        "simple_parameter" | "property_promotion_parameter" | "variadic_parameter" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| php_var_name(n, source));
            match (name, node.child_by_field_name("type")) {
                (Some(name), Some(ty)) => vec![(name, ty)],
                _ => Vec::new(),
            }
        }
        "property_declaration" => {
            let Some(ty) = node.child_by_field_name("type") else {
                return Vec::new();
            };
            named_children(node)
                .into_iter()
                .filter(|c| c.kind() == "property_element")
                .filter_map(|c| php_var_name(c, source).map(|n| (n, ty)))
                .collect()
        }
        "assignment_expression" => {
            let left = node
                .child_by_field_name("left")
                .filter(|n| n.kind() == "variable_name")
                .and_then(|n| php_var_name(n, source));
            let right = node
                .child_by_field_name("right")
                .filter(|n| n.kind() == "object_creation_expression");
            match (left, right) {
                (Some(name), Some(right)) => vec![(name, right)],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn java_bindings<'a>(node: Node<'a>, source: &str) -> Vec<(String, Node<'a>)> {
    match node.kind() {
        "formal_parameter"
        | "spread_parameter"
        | "catch_formal_parameter"
        | "enhanced_for_statement"
        | "resource" => match (
            node.child_by_field_name("name"),
            node.child_by_field_name("type"),
        ) {
            (Some(name), Some(ty)) => vec![(text(name, source).to_string(), ty)],
            _ => Vec::new(),
        },
        "local_variable_declaration" | "field_declaration" => {
            let Some(ty) = node.child_by_field_name("type") else {
                return Vec::new();
            };
            // `var x = new T()`: the declared type carries no name, so the
            // initializer's constructor is the only syntactic evidence.
            let inferred = text(ty, source) == "var";
            fields(node, "declarator")
                .into_iter()
                .filter_map(|d| {
                    let name = d.child_by_field_name("name")?;
                    let ty = if inferred {
                        d.child_by_field_name("value")
                            .filter(|v| v.kind() == "object_creation_expression")?
                    } else {
                        ty
                    };
                    Some((text(name, source).to_string(), ty))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn kotlin_bindings<'a>(node: Node<'a>, source: &str) -> Vec<(String, Node<'a>)> {
    let declared =
        |n: Node<'a>| child_kind(n, "user_type").or_else(|| child_kind(n, "nullable_type"));
    match node.kind() {
        "parameter" | "class_parameter" => {
            match (child_kind(node, "simple_identifier"), declared(node)) {
                (Some(ident), Some(ty)) => vec![(text(ident, source).to_string(), ty)],
                _ => Vec::new(),
            }
        }
        "property_declaration" => {
            let Some(decl) = child_kind(node, "variable_declaration") else {
                return Vec::new();
            };
            let Some(ident) = child_kind(decl, "simple_identifier") else {
                return Vec::new();
            };
            // Undeclared type: `val output = Output()` names its class in
            // the initializer. A navigation_expression callee (factory) is
            // not a constructor and yields nothing.
            let ty = declared(decl).or_else(|| {
                child_kind(node, "call_expression")
                    .and_then(|call| child_kind(call, "simple_identifier"))
            });
            ty.map(|ty| vec![(text(ident, source).to_string(), ty)])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn go_bindings<'a>(node: Node<'a>, source: &str) -> Vec<(String, Node<'a>)> {
    match node.kind() {
        "parameter_declaration" | "var_spec" => {
            let Some(ty) = node.child_by_field_name("type") else {
                return Vec::new();
            };
            fields(node, "name")
                .into_iter()
                .map(|n| (text(n, source).to_string(), ty))
                .collect()
        }
        "short_var_declaration" => {
            let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) else {
                return Vec::new();
            };
            named_children(left)
                .into_iter()
                .zip(named_children(right))
                .filter(|(name, _)| name.kind() == "identifier")
                .filter_map(|(name, value)| {
                    Some((text(name, source).to_string(), composite_type(value)?))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The type of a go composite literal, through one `&` if present.
/// A call expression's return type is not syntactic evidence.
fn composite_type(value: Node<'_>) -> Option<Node<'_>> {
    let literal = match value.kind() {
        "composite_literal" => value,
        "unary_expression" => {
            let operand = value.child_by_field_name("operand")?;
            operand.kind().eq("composite_literal").then_some(operand)?
        }
        _ => return None,
    };
    literal.child_by_field_name("type")
}

fn type_name(lang: Lang, node: Node<'_>, source: &str) -> Option<String> {
    let first = || named_children(node).into_iter().next();
    match (lang, node.kind()) {
        (Lang::Php, "name") => Some(text(node, source).to_string()),
        (Lang::Php, "qualified_name") => named_children(node)
            .into_iter()
            .rfind(|c| c.kind() == "name")
            .map(|n| text(n, source).to_string()),
        (Lang::Php, "named_type" | "optional_type" | "object_creation_expression") => {
            type_name(lang, first()?, source)
        }

        (Lang::Java, "type_identifier") => Some(text(node, source).to_string()),
        (Lang::Java, "scoped_type_identifier") => named_children(node)
            .into_iter()
            .rfind(|c| c.kind() == "type_identifier")
            .map(|n| text(n, source).to_string()),
        (Lang::Java, "generic_type" | "annotated_type") => named_children(node)
            .into_iter()
            .find_map(|c| type_name(lang, c, source)),
        (Lang::Java, "object_creation_expression") => {
            type_name(lang, node.child_by_field_name("type")?, source)
        }

        (Lang::Kotlin, "simple_identifier") => Some(text(node, source).to_string()),
        (Lang::Kotlin, "user_type") => {
            type_name(lang, child_kind(node, "type_identifier")?, source)
        }
        (Lang::Kotlin, "type_identifier") => Some(text(node, source).to_string()),
        (Lang::Kotlin, "nullable_type") => type_name(lang, first()?, source),

        (Lang::Go, "type_identifier") => Some(text(node, source).to_string()),
        (Lang::Go, "pointer_type" | "generic_type") => type_name(lang, first()?, source),
        (Lang::Go, "qualified_type") => type_name(lang, node.child_by_field_name("name")?, source),

        _ => None,
    }
}

// --------------------------------------------------------------- enclosing

/// The type that owns the symbol at `target_line`.
pub fn enclosing_type(lang: Lang, parsed: &Parsed, target_line: usize) -> Option<String> {
    let chain = chain_at(parsed.root(), target_line);
    let source = &parsed.source;
    for node in chain.iter().rev() {
        // go has no class syntax: a method's owner is its receiver's type,
        // and a plain function has no owner at all.
        if lang == Lang::Go {
            if node.kind() == "method_declaration" {
                let receiver = node.child_by_field_name("receiver")?;
                let decl = child_kind(receiver, "parameter_declaration")?;
                return type_name(lang, decl.child_by_field_name("type")?, source);
            }
            continue;
        }
        if !container_kinds(lang).contains(&node.kind()) {
            continue;
        }
        return match lang {
            Lang::Kotlin => {
                child_kind(*node, "type_identifier").map(|n| text(n, source).to_string())
            }
            _ => node
                .child_by_field_name("name")
                .map(|n| text(n, source).to_string()),
        };
    }
    None
}

fn container_kinds(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Php => &[
            "class_declaration",
            "trait_declaration",
            "interface_declaration",
            "enum_declaration",
        ],
        Lang::Java => &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
            "record_declaration",
            "annotation_type_declaration",
        ],
        Lang::Kotlin => &["class_declaration", "object_declaration"],
        Lang::Go | Lang::Js => &[],
    }
}

// ------------------------------------------------------------------ supply

/// `(type, supertype)` pairs declared in one source: extends, implements,
/// php trait `use`, kotlin delegation specifiers, go struct embedding.
///
/// One-to-many by construction — a php class using six traits supplies six
/// method sets, and an extends-only chain would call every trait method a
/// mismatch.
pub fn supply_pairs(lang: Lang, parsed: &Parsed, mut emit: impl FnMut(String, String)) {
    let source = &parsed.source;
    let mut stack = vec![parsed.root()];
    while let Some(node) = stack.pop() {
        for child in named_children(node) {
            stack.push(child);
        }
        let Some(child_name) = declared_name(lang, node, source) else {
            continue;
        };
        for parent in supertypes(lang, node, source) {
            emit(child_name.clone(), parent);
        }
    }
}

fn declared_name(lang: Lang, node: Node<'_>, source: &str) -> Option<String> {
    match lang {
        Lang::Kotlin => container_kinds(lang)
            .contains(&node.kind())
            .then(|| child_kind(node, "type_identifier"))
            .flatten()
            .map(|n| text(n, source).to_string()),
        Lang::Go => (node.kind() == "type_spec")
            .then(|| node.child_by_field_name("name"))
            .flatten()
            .map(|n| text(n, source).to_string()),
        _ => container_kinds(lang)
            .contains(&node.kind())
            .then(|| node.child_by_field_name("name"))
            .flatten()
            .map(|n| text(n, source).to_string()),
    }
}

fn supertypes(lang: Lang, node: Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |n: Node<'_>| {
        if let Some(name) = type_name(lang, n, source) {
            out.push(name);
        }
    };
    match lang {
        Lang::Php => {
            for clause in named_children(node)
                .into_iter()
                .filter(|c| matches!(c.kind(), "base_clause" | "class_interface_clause"))
            {
                for name in named_children(clause) {
                    push(name);
                }
            }
            // `use Trait;` inside the body, not the file-level import of the
            // same keyword (namespace_use_declaration).
            if let Some(body) = node.child_by_field_name("body") {
                for used in named_children(body)
                    .into_iter()
                    .filter(|c| c.kind() == "use_declaration")
                {
                    for name in named_children(used)
                        .into_iter()
                        .filter(|c| matches!(c.kind(), "name" | "qualified_name"))
                    {
                        push(name);
                    }
                }
            }
        }
        Lang::Java => {
            if let Some(sup) = node.child_by_field_name("superclass") {
                for ty in named_children(sup) {
                    push(ty);
                }
            }
            for clause in named_children(node)
                .into_iter()
                .filter(|c| matches!(c.kind(), "super_interfaces" | "extends_interfaces"))
            {
                for list in named_children(clause) {
                    for ty in named_children(list) {
                        push(ty);
                    }
                }
            }
        }
        Lang::Kotlin => {
            for spec in named_children(node)
                .into_iter()
                .filter(|c| c.kind() == "delegation_specifier")
            {
                let target = child_kind(spec, "constructor_invocation")
                    .and_then(|c| child_kind(c, "user_type"))
                    .or_else(|| child_kind(spec, "user_type"));
                if let Some(target) = target {
                    push(target);
                }
            }
        }
        Lang::Go => {
            let Some(struct_type) = node
                .child_by_field_name("type")
                .filter(|t| t.kind() == "struct_type")
            else {
                return out;
            };
            let Some(list) = child_kind(struct_type, "field_declaration_list") else {
                return out;
            };
            // An embedded field declares a type and no name.
            for field in named_children(list).into_iter().filter(|f| {
                f.kind() == "field_declaration" && f.child_by_field_name("name").is_none()
            }) {
                if let Some(ty) = field.child_by_field_name("type") {
                    push(ty);
                }
            }
        }
        Lang::Js => {}
    }
    out
}
