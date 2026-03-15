//! Shared helpers for grammar audit tests.

use super::abi15_exploration_common::print_node_tree;
use codanna::io::format::format_utc_timestamp as get_formatted_timestamp;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use tree_sitter::{Language, Node, Parser};

/// Configuration for a language grammar audit.
pub struct LanguageAuditConfig {
    pub language_name: &'static str,
    pub file_extension: &'static str,
    pub grammar_json_path: &'static str,
    pub example_file_path: &'static str,
    pub output_dir: &'static str,
}

/// Common audit data extracted from language-specific ParserAudit types.
pub struct AuditData {
    pub grammar_nodes: HashMap<String, u16>,
    pub implemented_nodes: HashSet<String>,
    pub extracted_symbol_kinds: HashSet<String>,
}

impl AuditData {
    pub fn new(
        grammar_nodes: HashMap<String, u16>,
        implemented_nodes: HashSet<String>,
        extracted_symbol_kinds: HashSet<String>,
    ) -> Self {
        Self {
            grammar_nodes,
            implemented_nodes,
            extracted_symbol_kinds,
        }
    }

    pub fn empty() -> Self {
        Self {
            grammar_nodes: HashMap::new(),
            implemented_nodes: HashSet::new(),
            extracted_symbol_kinds: HashSet::new(),
        }
    }

    pub fn example_nodes(&self) -> HashSet<String> {
        self.grammar_nodes.keys().cloned().collect()
    }
}

/// Result from loading grammar nodes from node-types.json.
pub struct GrammarLoadResult {
    pub nodes: HashSet<String>,
    pub warning: Option<String>,
}

/// Load named nodes from a node-types.json grammar file.
pub fn load_grammar_nodes(grammar_json_path: &str) -> GrammarLoadResult {
    match fs::read_to_string(grammar_json_path) {
        Ok(json) => match serde_json::from_str::<Value>(&json) {
            Ok(Value::Array(nodes)) => {
                let mut grammar_nodes = HashSet::new();
                for node in &nodes {
                    if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                        (node.get("named"), node.get("type"))
                    {
                        grammar_nodes.insert(node_type.clone());
                    }
                }
                GrammarLoadResult {
                    nodes: grammar_nodes,
                    warning: None,
                }
            }
            Ok(_) => GrammarLoadResult {
                nodes: HashSet::new(),
                warning: Some(format!(
                    "Unexpected grammar JSON structure in {grammar_json_path}."
                )),
            },
            Err(err) => GrammarLoadResult {
                nodes: HashSet::new(),
                warning: Some(format!(
                    "Failed to parse grammar JSON at {grammar_json_path}: {err}."
                )),
            },
        },
        Err(err) => GrammarLoadResult {
            nodes: HashSet::new(),
            warning: Some(format!("Missing {grammar_json_path} ({err}).")),
        },
    }
}

/// Run a full grammar analysis for a language.
///
/// Generates AUDIT_REPORT.md, GRAMMAR_ANALYSIS.md, and node_discovery.txt.
pub fn run_comprehensive_analysis<F>(
    config: &LanguageAuditConfig,
    ts_language: Language,
    fallback_code: &str,
    node_categories: &[(&str, Vec<&str>)],
    run_audit: F,
) where
    F: FnOnce(&str) -> Result<(AuditData, String), String>,
{
    println!(
        "=== {name} Comprehensive Grammar Analysis ===\n",
        name = config.language_name
    );

    fs::create_dir_all(config.output_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to create output directory {dir}: {e}",
            dir = config.output_dir
        )
    });

    let grammar_result = load_grammar_nodes(config.grammar_json_path);

    let (audit_data, report) = match run_audit(config.example_file_path) {
        Ok((data, report)) => (data, Some(report)),
        Err(e) => {
            println!(
                "Warning: Failed to audit {name} file: {e}",
                name = config.language_name
            );
            (AuditData::empty(), None)
        }
    };

    if let Some(report) = report {
        fs::write(
            format!("{dir}/AUDIT_REPORT.md", dir = config.output_dir),
            &report,
        )
        .unwrap_or_else(|e| {
            panic!(
                "Failed to write {name} audit report: {e}",
                name = config.language_name
            )
        });
    }

    let example_nodes = audit_data.example_nodes();

    let analysis = generate_grammar_analysis(
        config,
        &grammar_result.nodes,
        &audit_data,
        grammar_result.warning.as_deref(),
    );
    fs::write(
        format!("{dir}/GRAMMAR_ANALYSIS.md", dir = config.output_dir),
        &analysis,
    )
    .unwrap_or_else(|e| {
        panic!(
            "Failed to write {name} grammar analysis: {e}",
            name = config.language_name
        )
    });

    let mut parser = Parser::new();
    parser.set_language(&ts_language).unwrap();
    let code =
        fs::read_to_string(config.example_file_path).unwrap_or_else(|_| fallback_code.to_string());
    let tree = parser.parse(&code, None).unwrap();
    let root = tree.root_node();

    if std::env::var("DEBUG_TREE").is_ok() {
        println!(
            "\n=== {name} Tree Structure ===",
            name = config.language_name
        );
        print_node_tree(root, &code, 0);
    }

    let mut node_registry = HashMap::new();
    let mut found_in_file = HashSet::new();
    discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

    let discovery = format_node_discovery(
        config,
        ts_language.abi_version(),
        &node_registry,
        &found_in_file,
        node_categories,
    );
    fs::write(
        format!("{dir}/node_discovery.txt", dir = config.output_dir),
        &discovery,
    )
    .unwrap_or_else(|e| {
        panic!(
            "Failed to write {name} node discovery: {e}",
            name = config.language_name
        )
    });

    print_analysis_summary(
        config,
        grammar_result.nodes.len(),
        example_nodes.len(),
        audit_data.implemented_nodes.len(),
        &audit_data.extracted_symbol_kinds,
    );
}

/// Generate standardized GRAMMAR_ANALYSIS.md content.
fn generate_grammar_analysis(
    config: &LanguageAuditConfig,
    all_grammar_nodes: &HashSet<String>,
    audit: &AuditData,
    grammar_warning: Option<&str>,
) -> String {
    let example_nodes = audit.example_nodes();

    let mut analysis = String::new();
    analysis.push_str(&format!(
        "# {name} Grammar Analysis\n\n",
        name = config.language_name
    ));
    analysis.push_str(&format!(
        "*Generated: {ts}*\n\n",
        ts = get_formatted_timestamp()
    ));
    analysis.push_str("## Statistics\n");
    analysis.push_str(&format!(
        "- Total nodes in grammar JSON: {count}\n",
        count = all_grammar_nodes.len()
    ));
    analysis.push_str(&format!(
        "- Nodes found in comprehensive.{ext}: {count}\n",
        ext = config.file_extension,
        count = example_nodes.len()
    ));
    analysis.push_str(&format!(
        "- Nodes handled by parser: {count}\n",
        count = audit.implemented_nodes.len()
    ));
    analysis.push_str(&format!(
        "- Symbol kinds extracted: {count}\n",
        count = audit.extracted_symbol_kinds.len()
    ));
    analysis.push('\n');

    if let Some(warning) = grammar_warning {
        analysis.push_str("## Warning\n");
        analysis.push_str(warning);
        analysis.push_str("\n\n");
    }

    let mut handled: Vec<_> = audit
        .implemented_nodes
        .iter()
        .filter(|n| example_nodes.contains(n.as_str()))
        .collect();
    let mut gaps: Vec<_> = example_nodes
        .iter()
        .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
        .collect();
    let mut missing_from_examples: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();

    handled.sort();
    gaps.sort();
    missing_from_examples.sort();

    if !handled.is_empty() {
        analysis.push_str("## Successfully Handled Nodes\n");
        analysis.push_str("These nodes are in examples and handled by parser:\n");
        for node in &handled {
            analysis.push_str(&format!("- {node}\n"));
        }
        analysis.push('\n');
    }

    if !gaps.is_empty() {
        analysis.push_str("## Implementation Gaps\n");
        analysis.push_str(&format!(
            "These nodes appear in comprehensive.{ext} but aren't handled:\n",
            ext = config.file_extension
        ));
        for node in &gaps {
            analysis.push_str(&format!("- {node}\n"));
        }
        analysis.push('\n');
    }

    if !missing_from_examples.is_empty() {
        analysis.push_str("## Missing from Examples\n");
        analysis.push_str(&format!(
            "These grammar nodes aren't in comprehensive.{ext}:\n",
            ext = config.file_extension
        ));
        for node in &missing_from_examples {
            analysis.push_str(&format!("- {node}\n"));
        }
        analysis.push('\n');
    }

    if !audit.extracted_symbol_kinds.is_empty() {
        analysis.push_str("## Symbol Kinds Extracted\n");
        let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
        kinds.sort();
        for kind in kinds {
            analysis.push_str(&format!("- {kind}\n"));
        }
        analysis.push('\n');
    }

    analysis
}

/// Format node_discovery.txt with categorized nodes.
pub fn format_node_discovery(
    config: &LanguageAuditConfig,
    abi_version: usize,
    node_registry: &HashMap<String, u16>,
    found_in_file: &HashSet<String>,
    node_categories: &[(&str, Vec<&str>)],
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "=== {name} Language NODE MAPPING ===\n",
        name = config.language_name
    ));
    output.push_str(&format!(
        "  Generated: {ts}\n",
        ts = get_formatted_timestamp()
    ));
    output.push_str(&format!("  ABI Version: {abi_version}\n"));
    output.push_str(&format!(
        "  Node kind count: {count}\n\n",
        count = node_registry.len()
    ));

    for (category, expected_nodes) in node_categories {
        output.push_str(&format!("=== {category} ===\n"));
        for node_name in expected_nodes {
            if let Some(node_id) = node_registry.get(*node_name) {
                let status = if found_in_file.contains(*node_name) {
                    "+"
                } else {
                    "o"
                };
                output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
            } else {
                output.push_str(&format!("  x {node_name:35} NOT FOUND\n"));
            }
        }
        output.push('\n');
    }

    let mut uncategorized: Vec<_> = node_registry
        .keys()
        .filter(|k| {
            !node_categories
                .iter()
                .any(|(_, nodes)| nodes.contains(&k.as_str()))
        })
        .collect();
    uncategorized.sort();

    if !uncategorized.is_empty() {
        output.push_str("=== UNCATEGORIZED NODES ===\n");
        for node_name in uncategorized {
            if let Some(node_id) = node_registry.get(node_name) {
                let status = if found_in_file.contains(node_name.as_str()) {
                    "+"
                } else {
                    "o"
                };
                output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
            }
        }
    }

    output.push_str(
        "\nLegend: + = found in file, o = in grammar but not in file, x = not in grammar\n",
    );
    output
}

/// Recursively discover all nodes in a tree with their kind IDs.
pub fn discover_nodes_with_ids(
    node: Node,
    registry: &mut HashMap<String, u16>,
    found_in_file: &mut HashSet<String>,
) {
    let node_kind = node.kind();
    registry.insert(node_kind.to_string(), node.kind_id());
    found_in_file.insert(node_kind.to_string());

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        discover_nodes_with_ids(child, registry, found_in_file);
    }
}

/// Run tree structure analysis, generating TREE_STRUCT.md.
pub fn run_tree_structure_analysis(
    config: &LanguageAuditConfig,
    ts_language: Language,
    fallback_code: &str,
) {
    println!(
        "=== Generating {name} Tree Structure ===\n",
        name = config.language_name
    );

    let mut parser = Parser::new();
    parser.set_language(&ts_language).unwrap();
    let code =
        fs::read_to_string(config.example_file_path).unwrap_or_else(|_| fallback_code.to_string());

    if let Some(tree) = parser.parse(&code, None) {
        let mut output = String::new();
        output.push_str(&format!(
            "# {name} AST Tree Structure\n\n",
            name = config.language_name
        ));
        output.push_str(&format!(
            "Complete nested structure from comprehensive.{ext}\n\n",
            ext = config.file_extension
        ));
        output.push_str("```\n");
        generate_tree_structure(&mut output, tree.root_node(), &code, 0, None);
        output.push_str("```\n\n");

        let mut node_stats = HashMap::new();
        collect_node_statistics(tree.root_node(), &mut node_stats);

        output.push_str("## Node Type Statistics\n\n");
        output.push_str("| Node Type | Count | Max Depth |\n");
        output.push_str("|-----------|-------|----------|\n");

        let mut sorted_stats: Vec<_> = node_stats.iter().collect();
        sorted_stats.sort_by_key(|(name, _)| *name);

        for (node_type, (count, max_depth)) in sorted_stats {
            output.push_str(&format!("| {node_type} | {count} | {max_depth} |\n"));
        }

        output.push_str(&format!(
            "\n**Total unique node types**: {count}\n",
            count = node_stats.len()
        ));

        fs::write(
            format!("{dir}/TREE_STRUCT.md", dir = config.output_dir),
            output,
        )
        .unwrap_or_else(|e| {
            panic!(
                "Failed to write {name} tree structure: {e}",
                name = config.language_name
            )
        });

        println!(
            "{name} TREE_STRUCT.md generated",
            name = config.language_name
        );
    }
}

/// Generate tree structure showing all nodes and relationships.
fn generate_tree_structure(
    output: &mut String,
    node: Node,
    code: &str,
    depth: usize,
    field_name: Option<&str>,
) {
    if depth > 50 {
        output.push_str(&format!(
            "{:indent$}... (truncated at depth 50)\n",
            "",
            indent = depth * 2
        ));
        return;
    }

    let node_text = code.get(node.byte_range()).unwrap_or("<invalid>");
    let display_text = node_text
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect::<String>();

    let field_prefix = if let Some(fname) = field_name {
        format!("{fname}: ")
    } else {
        String::new()
    };

    output.push_str(&format!(
        "{:indent$}{field_prefix}{kind} [{id}]",
        "",
        kind = node.kind(),
        id = node.kind_id(),
        indent = depth * 2
    ));

    if node.child_count() == 0 || display_text.len() <= 40 {
        output.push_str(&format!(
            " = '{text}'",
            text = display_text.replace('\n', "\\n")
        ));
    }

    output.push('\n');

    let mut cursor = node.walk();
    for (i, child) in node.children(&mut cursor).enumerate() {
        let child_field = node.field_name_for_child(i as u32);
        generate_tree_structure(output, child, code, depth + 1, child_field);
    }
}

/// Collect statistics about node types (count and max depth).
fn collect_node_statistics(node: Node, stats: &mut HashMap<String, (usize, usize)>) {
    fn collect_recursive(node: Node, stats: &mut HashMap<String, (usize, usize)>, depth: usize) {
        let node_kind = node.kind().to_string();
        let entry = stats.entry(node_kind).or_insert((0, 0));
        entry.0 += 1;
        entry.1 = entry.1.max(depth);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_recursive(child, stats, depth + 1);
        }
    }

    collect_recursive(node, stats, 0);
}

/// Print analysis summary to stdout.
fn print_analysis_summary(
    config: &LanguageAuditConfig,
    grammar_node_count: usize,
    example_node_count: usize,
    handled_count: usize,
    symbol_kinds: &HashSet<String>,
) {
    println!("{name} Analysis:", name = config.language_name);
    println!("  - Grammar nodes: {grammar_node_count}");
    println!("  - Example nodes: {example_node_count}");
    println!("  - Handled nodes: {handled_count}");
    println!("  - Symbol kinds: {symbol_kinds:?}");
    if example_node_count > 0 {
        println!(
            "  - Coverage: {:.1}%",
            handled_count as f32 / example_node_count as f32 * 100.0
        );
    }
    println!("{name} files saved", name = config.language_name);
}
