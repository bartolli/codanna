//! Grammar Audit and Node Discovery Test
//!
//! Comprehensive analysis combining:
//! 1. Grammar JSON analysis - ALL nodes available in tree-sitter grammar
//! 2. Node discovery - What nodes appear in our comprehensive examples
//! 3. Parser audit - What our parser actually handles
//!
//! Outputs:
//! - AUDIT_REPORT.md files (parser implementation coverage)
//! - node_discovery.txt files (grammar exploration)
//! - GRAMMAR_ANALYSIS.md files (complete grammar vs example vs parser analysis)
//!
//! Run with: cargo test abi15_grammar_audit -- --nocapture

// Alias for tree-sitter-kotlin dependency (matches src/lib.rs)
extern crate tree_sitter_kotlin_codanna as tree_sitter_kotlin;

// Import the common utilities at the module level
mod abi15_exploration_common;

#[cfg(test)]
mod tests {
    // Alias for tree-sitter-kotlin dependency (matches src/lib.rs)
    use super::tree_sitter_kotlin;

    // Import the timestamp utility from the main codebase
    use codanna::io::format::format_utc_timestamp as get_formatted_timestamp;
    use codanna::parsing::{
        c::audit::CParserAudit, cpp::audit::CppParserAudit, csharp::audit::CSharpParserAudit,
        gdscript::audit::GdscriptParserAudit, go::audit::GoParserAudit,
        java::audit::JavaParserAudit, javascript::audit::JavaScriptParserAudit,
        kotlin::audit::KotlinParserAudit, lua::audit::LuaParserAudit, php::audit::PhpParserAudit,
        python::audit::PythonParserAudit, rust::audit::RustParserAudit,
        swift::audit::SwiftParserAudit, typescript::audit::TypeScriptParserAudit,
    };
    use serde_json::Value;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use tree_sitter::{Language, Node, Parser};

    #[test]
    fn comprehensive_php_analysis() {
        println!("=== PHP Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/php/node-types.json")
            .expect("Failed to read PHP grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match PhpParserAudit::audit_file("examples/php/comprehensive.php") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit PHP file: {e}");
                // Create empty audit for fallback
                PhpParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/php/AUDIT_REPORT.md", &report)
            .expect("Failed to write PHP audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# PHP Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.php: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.php but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.php:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/php/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write PHP grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_php_node_discovery();
        fs::write(
            "contributing/parsers/php/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write PHP node discovery");

        println!("📄 PHP Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ PHP node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_go_analysis() {
        println!("=== Go Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/go/node-types.json")
            .expect("Failed to read Go grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match GoParserAudit::audit_file("examples/go/comprehensive.go") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Go file: {e}");
                // Create empty audit for fallback
                GoParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/go/AUDIT_REPORT.md", &report)
            .expect("Failed to write Go audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Go Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.go: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.go but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.go:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/go/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Go grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_go_node_discovery();
        fs::write("contributing/parsers/go/node_discovery.txt", node_discovery)
            .expect("Failed to write Go node discovery");

        println!("📄 Go Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ Go node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_lua_analysis() {
        println!("=== Lua Comprehensive Grammar Analysis ===\n");

        fs::create_dir_all("contributing/parsers/lua")
            .expect("Failed to create Lua parser output directory");

        let grammar_path = "contributing/parsers/lua/node-types.json";
        let mut all_grammar_nodes = HashSet::new();
        let mut grammar_warning = None;

        match fs::read_to_string(grammar_path) {
            Ok(json) => match serde_json::from_str::<Value>(&json) {
                Ok(Value::Array(nodes)) => {
                    for node in nodes {
                        if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                            (node.get("named"), node.get("type"))
                        {
                            all_grammar_nodes.insert(node_type.clone());
                        }
                    }
                }
                Ok(_) => {
                    grammar_warning =
                        Some("Unexpected grammar JSON structure for Lua.".to_string());
                }
                Err(err) => {
                    grammar_warning = Some(format!(
                        "Failed to parse Lua grammar JSON: {err}. \
Run `tree-sitter generate` and copy node-types.json to {grammar_path}."
                    ));
                }
            },
            Err(err) => {
                grammar_warning = Some(format!(
                    "Missing node-types.json for Lua ({err}). \
Run `./contributing/tree-sitter/scripts/setup.sh lua` and copy \
tree-sitter-lua/src/node-types.json to {grammar_path}."
                ));
            }
        }

        let audit = match LuaParserAudit::audit_file("examples/lua/comprehensive.lua") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Lua file: {e}");
                LuaParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        let report = audit.generate_report();
        fs::write("contributing/parsers/lua/AUDIT_REPORT.md", &report)
            .expect("Failed to write Lua audit report");

        let mut analysis = String::new();
        analysis.push_str("# Lua Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.lua: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        if let Some(warning) = &grammar_warning {
            analysis.push_str("## Warning\n");
            analysis.push_str(warning);
            analysis.push_str("\n\n");
        }

        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## ⭕ Missing from Examples\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🔍 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write(
            "contributing/parsers/lua/GRAMMAR_ANALYSIS.md",
            &analysis,
        )
        .expect("Failed to write Lua grammar analysis");

        let node_discovery = generate_lua_node_discovery();
        fs::write(
            "contributing/parsers/lua/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Lua node discovery");

        println!("✅ Lua Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ Lua node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_python_analysis() {
        println!("=== Python Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/python/node-types.json")
            .expect("Failed to read Python grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match PythonParserAudit::audit_file("examples/python/comprehensive.py") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Python file: {e}");
                // Create empty audit for fallback
                PythonParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/python/AUDIT_REPORT.md", &report)
            .expect("Failed to write Python audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Python Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.py: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.py but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.py:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/python/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Python grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_python_node_discovery();
        fs::write(
            "contributing/parsers/python/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Python node discovery");

        println!("📄 Python Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ Python node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_rust_analysis() {
        println!("=== Rust Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/rust/node-types.json")
            .expect("Failed to read Rust grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match RustParserAudit::audit_file("examples/rust/comprehensive.rs") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Rust file: {e}");
                // Create empty audit for fallback
                RustParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/rust/AUDIT_REPORT.md", &report)
            .expect("Failed to write Rust audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Rust Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.rs: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.rs but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.rs:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/rust/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Rust grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_rust_node_discovery();
        fs::write(
            "contributing/parsers/rust/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Rust node discovery");

        println!("📄 Rust Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ Rust node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_typescript_analysis() {
        println!("=== TypeScript Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/typescript/node-types.json")
            .expect("Failed to read TypeScript grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match TypeScriptParserAudit::audit_file("examples/typescript/comprehensive.ts")
        {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit TypeScript file: {e}");
                // Create empty audit for fallback
                TypeScriptParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/typescript/AUDIT_REPORT.md", &report)
            .expect("Failed to write TypeScript audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# TypeScript Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.ts: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.ts but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.ts:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write(
            "contributing/parsers/typescript/GRAMMAR_ANALYSIS.md",
            &analysis,
        )
        .expect("Failed to write TypeScript grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_typescript_node_discovery();
        fs::write(
            "contributing/parsers/typescript/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write TypeScript node discovery");

        println!("📄 TypeScript Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ TypeScript node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_javascript_analysis() {
        println!("=== JavaScript Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/javascript/node-types.json")
            .expect("Failed to read JavaScript grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match JavaScriptParserAudit::audit_file("examples/javascript/comprehensive.js")
        {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit JavaScript file: {e}");
                // Create empty audit for fallback
                JavaScriptParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/javascript/AUDIT_REPORT.md", &report)
            .expect("Failed to write JavaScript audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# JavaScript Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.js: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.js but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.js:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write(
            "contributing/parsers/javascript/GRAMMAR_ANALYSIS.md",
            &analysis,
        )
        .expect("Failed to write JavaScript grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_javascript_node_discovery();
        fs::write(
            "contributing/parsers/javascript/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write JavaScript node discovery");

        println!("JavaScript Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("JavaScript node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_c_analysis() {
        println!("=== C Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/c/node-types.json")
            .expect("Failed to read C grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match CParserAudit::audit_file("examples/c/comprehensive.c") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit C file: {e}");
                // Create empty audit for fallback
                CParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/c/AUDIT_REPORT.md", &report)
            .expect("Failed to write C audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# C Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.c: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.c but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.c:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/c/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write C grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_c_node_discovery();
        fs::write("contributing/parsers/c/node_discovery.txt", node_discovery)
            .expect("Failed to write C node discovery");

        println!("📄 C Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ C node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_cpp_analysis() {
        println!("=== C++ Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/cpp/node-types.json")
            .expect("Failed to read C++ grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match CppParserAudit::audit_file("examples/cpp/comprehensive.cpp") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit C++ file: {e}");
                // Create empty audit for fallback
                CppParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/cpp/AUDIT_REPORT.md", &report)
            .expect("Failed to write C++ audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# C++ Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.cpp: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.cpp but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.cpp:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/cpp/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write C++ grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_cpp_node_discovery();
        fs::write(
            "contributing/parsers/cpp/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write C++ node discovery");

        println!("📄 C++ Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ C++ node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_gdscript_analysis() {
        println!("=== GDScript Comprehensive Grammar Analysis ===\n");

        fs::create_dir_all("contributing/parsers/gdscript")
            .expect("Failed to create GDScript parser output directory");

        let grammar_path = "contributing/parsers/gdscript/node-types.json";
        let mut all_grammar_nodes = HashSet::new();
        let mut grammar_warning = None;

        match fs::read_to_string(grammar_path) {
            Ok(json) => match serde_json::from_str::<Value>(&json) {
                Ok(Value::Array(nodes)) => {
                    for node in nodes {
                        if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                            (node.get("named"), node.get("type"))
                        {
                            all_grammar_nodes.insert(node_type.clone());
                        }
                    }
                }
                Ok(_) => {
                    grammar_warning =
                        Some("Unexpected grammar JSON structure for GDScript.".to_string());
                }
                Err(err) => {
                    grammar_warning = Some(format!(
                        "Failed to parse GDScript grammar JSON: {err}. \
Run `tree-sitter generate` and copy node-types.json to {grammar_path}."
                    ));
                }
            },
            Err(err) => {
                grammar_warning = Some(format!(
                    "Missing node-types.json for GDScript ({err}). \
Run `./contributing/tree-sitter/scripts/setup.sh gdscript` and copy \
tree-sitter-gdscript/src/node-types.json to {grammar_path}."
                ));
            }
        }

        let audit = match GdscriptParserAudit::audit_file("examples/gdscript/comprehensive.gd") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit GDScript file: {e}");
                GdscriptParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        let report = audit.generate_report();
        fs::write("contributing/parsers/gdscript/AUDIT_REPORT.md", &report)
            .expect("Failed to write GDScript audit report");

        let mut analysis = String::new();
        analysis.push_str("# GDScript Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.gd: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        if let Some(warning) = &grammar_warning {
            analysis.push_str("## Warning\n");
            analysis.push_str(warning);
            analysis.push_str("\n\n");
        }

        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## ⭕ Missing from Examples\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🔍 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write(
            "contributing/parsers/gdscript/GRAMMAR_ANALYSIS.md",
            &analysis,
        )
        .expect("Failed to write GDScript grammar analysis");

        let node_discovery = generate_gdscript_node_discovery();
        fs::write(
            "contributing/parsers/gdscript/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write GDScript node discovery");

        println!("✅ GDScript Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ GDScript node_discovery.txt saved");
    }

    #[test]
    fn generate_node_discovery_php() {
        println!("=== PHP Node Discovery ===\n");

        let node_discovery = generate_php_node_discovery();
        fs::write(
            "contributing/parsers/php/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write PHP node discovery");
        println!("✅ PHP node_discovery.txt saved");
    }

    #[test]
    fn generate_node_discovery_go() {
        println!("=== Go Node Discovery ===\n");

        let node_discovery = generate_go_node_discovery();
        fs::write("contributing/parsers/go/node_discovery.txt", node_discovery)
            .expect("Failed to write Go node discovery");
        println!("✅ Go node_discovery.txt saved");
    }

    #[test]
    fn generate_node_discovery_gdscript() {
        println!("=== GDScript Node Discovery ===\n");

        let node_discovery = generate_gdscript_node_discovery();
        fs::write(
            "contributing/parsers/gdscript/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write GDScript node discovery");
        println!("? GDScript node_discovery.txt saved");
    }

    #[test]
    fn generate_node_discovery_python() {
        println!("=== Python Node Discovery ===\n");

        let node_discovery = generate_python_node_discovery();
        fs::write(
            "contributing/parsers/python/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Python node discovery");
        println!("✅ Python node_discovery.txt saved");
    }

    #[test]
    fn generate_node_discovery_rust() {
        println!("=== Rust Node Discovery ===\n");

        let node_discovery = generate_rust_node_discovery();
        fs::write(
            "contributing/parsers/rust/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Rust node discovery");
        println!("✅ Rust node_discovery.txt saved");
    }

    #[test]
    fn generate_node_discovery_typescript() {
        println!("=== TypeScript Node Discovery ===\n");

        let node_discovery = generate_typescript_node_discovery();
        fs::write(
            "contributing/parsers/typescript/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write TypeScript node discovery");
        println!("✅ TypeScript node_discovery.txt saved");
    }

    fn generate_php_node_discovery() -> String {
        use tree_sitter::{Language, Parser};
        // Import from the common module properly
        use super::abi15_exploration_common::print_node_tree;

        let mut output = String::new();
        output.push_str("=== PHP Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");

        let language: Language = tree_sitter_php::LANGUAGE_PHP.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/php/comprehensive.php")
            .unwrap_or_else(|_| "<?php\nclass Example {}\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== PHP Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define PHP node categories for organization
        let node_categories = vec![
            (
                "NAMESPACE & IMPORT NODES",
                vec![
                    "namespace_definition",
                    "namespace_use_declaration",
                    "namespace_use_clause",
                    "namespace_use_group",
                    "namespace_name",
                    "namespace_aliasing_clause",
                ],
            ),
            (
                "CLASS & TRAIT NODES",
                vec![
                    "class_declaration",
                    "interface_declaration",
                    "trait_declaration",
                    "enum_declaration",
                    "abstract_modifier",
                    "final_modifier",
                    "readonly_modifier",
                    "base_clause",
                    "class_interface_clause",
                    "trait_use_clause",
                    "trait_alias",
                    "trait_precedence",
                ],
            ),
            (
                "METHOD & FUNCTION NODES",
                vec![
                    "method_declaration",
                    "function_definition",
                    "arrow_function",
                    "anonymous_function",
                    "anonymous_function_creation_expression",
                    "anonymous_function_use_clause",
                    "formal_parameters",
                    "simple_parameter",
                    "property_promotion_parameter",
                    "variadic_parameter",
                    "reference_parameter",
                    "typed_parameter",
                ],
            ),
            (
                "PROPERTY & CONSTANT NODES",
                vec![
                    "property_declaration",
                    "property_element",
                    "const_declaration",
                    "const_element",
                    "class_constant_access_expression",
                    "visibility_modifier",
                    "static_modifier",
                    "var_modifier",
                ],
            ),
            (
                "ATTRIBUTE NODES",
                vec![
                    "attribute_list",
                    "attribute_group",
                    "attribute",
                    "attribute_arguments",
                    "named_argument",
                ],
            ),
            (
                "TYPE NODES",
                vec![
                    "union_type",
                    "intersection_type",
                    "nullable_type",
                    "primitive_type",
                    "named_type",
                    "optional_type",
                    "bottom_type",
                    "void_type",
                    "mixed_type",
                    "never_type",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    fn generate_go_node_discovery() -> String {
        use tree_sitter::{Language, Parser};
        // Import from the common module properly
        use super::abi15_exploration_common::print_node_tree;

        let mut output = String::new();
        output.push_str("=== Go Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_go::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/go/comprehensive.go")
            .unwrap_or_else(|_| "package main\n\nfunc main() {}\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== Go Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define Go node categories for organization
        let node_categories = vec![
            (
                "PACKAGE AND IMPORT NODES",
                vec![
                    "package_clause",
                    "package_identifier",
                    "import_declaration",
                    "import_spec",
                    "import_spec_list",
                    "interpreted_string_literal",
                    "dot",
                    "blank_identifier",
                    "import_alias",
                ],
            ),
            (
                "STRUCT-RELATED NODES",
                vec![
                    "type_declaration",
                    "type_spec",
                    "struct_type",
                    "field_declaration",
                    "field_declaration_list",
                    "type_identifier",
                    "field_identifier",
                    "tag",
                    "struct_field",
                    "embedded_field",
                ],
            ),
            (
                "INTERFACE-RELATED NODES",
                vec![
                    "interface_type",
                    "method_elem",
                    "method_spec",
                    "method_spec_list",
                    "type_elem",
                    "type_constraint",
                    "type_set",
                    "embedded_interface",
                ],
            ),
            (
                "FUNCTION-RELATED NODES",
                vec![
                    "function_declaration",
                    "func_literal",
                    "function_type",
                    "method_declaration",
                    "receiver",
                    "parameter_declaration",
                    "parameter_list",
                    "result",
                    "variadic_parameter_declaration",
                    "type_parameter_declaration",
                    "type_parameter_list",
                ],
            ),
            (
                "VARIABLE/CONSTANT NODES",
                vec![
                    "var_declaration",
                    "var_spec",
                    "const_declaration",
                    "const_spec",
                    "short_var_declaration",
                    "assignment_statement",
                    "inc_statement",
                    "dec_statement",
                    "expression_list",
                    "identifier_list",
                ],
            ),
            (
                "TYPE-RELATED NODES",
                vec![
                    "type_alias",
                    "pointer_type",
                    "array_type",
                    "slice_type",
                    "map_type",
                    "channel_type",
                    "generic_type",
                    "type_instantiation",
                    "type_arguments",
                    "type_parameter",
                    "qualified_type",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    fn generate_lua_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== Lua Language COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_lua::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/lua/comprehensive.lua")
            .unwrap_or_else(|_| "-- Lua module\nlocal M = {}\nreturn M\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== Lua Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        let node_categories = vec![
            (
                "FUNCTION NODES",
                vec![
                    "function_declaration",
                    "function_definition",
                    "function_call",
                    "parameters",
                    "return_statement",
                ],
            ),
            (
                "VARIABLE NODES",
                vec![
                    "variable_declaration",
                    "assignment_statement",
                    "variable_list",
                    "expression_list",
                    "identifier",
                ],
            ),
            (
                "TABLE NODES",
                vec![
                    "table_constructor",
                    "field",
                    "dot_index_expression",
                    "bracket_index_expression",
                    "method_index_expression",
                ],
            ),
            (
                "CONTROL FLOW NODES",
                vec![
                    "if_statement",
                    "elseif_statement",
                    "else_statement",
                    "for_statement",
                    "for_in_statement",
                    "while_statement",
                    "repeat_statement",
                    "do_statement",
                    "block",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "binary_expression",
                    "unary_expression",
                    "parenthesized_expression",
                    "string",
                    "number",
                    "true",
                    "false",
                    "nil",
                ],
            ),
            ("COMMENT NODES", vec!["comment"]),
        ];

        for (category_name, expected_nodes) in &node_categories {
            output.push_str(&format!("== {category_name} ==\n"));
            for node_name in expected_nodes {
                if let Some(&id) = node_registry.get(*node_name) {
                    let in_file = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("{in_file} {node_name} (ID: {id})\n"));
                } else {
                    output.push_str(&format!("✗ {node_name} (not found)\n"));
                }
            }
            output.push('\n');
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    fn generate_gdscript_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== GDScript Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_gdscript::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/gdscript/comprehensive.gd")
            .unwrap_or_else(|_| "class_name Temp\nextends Node\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== GDScript Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        let node_categories = vec![
            (
                "SCRIPT DECLARATIONS",
                vec![
                    "class_name_statement",
                    "class_definition",
                    "extends_statement",
                    "enum_definition",
                ],
            ),
            (
                "SIGNALS & VARIABLES",
                vec![
                    "signal_statement",
                    "variable_statement",
                    "const_statement",
                    "export_variable_statement",
                ],
            ),
            (
                "FUNCTIONS",
                vec![
                    "constructor_definition",
                    "function_definition",
                    "parameters",
                    "block",
                ],
            ),
            (
                "CONTROL FLOW",
                vec![
                    "if_statement",
                    "while_statement",
                    "for_statement",
                    "match_statement",
                    "pattern_section",
                ],
            ),
            (
                "EXPRESSIONS",
                vec!["assignment", "call", "binary_operator", "unary_operator"],
            ),
        ];

        for (category, nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in nodes {
                if let Some(id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✅"
                    } else {
                        "  "
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {id}\n"));
                } else {
                    output.push_str(&format!("  ❓ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        let mut categorized = HashSet::new();
        for (_, nodes) in &node_categories {
            for node in nodes {
                categorized.insert(node.to_string());
            }
        }

        let mut uncategorized: Vec<_> = node_registry
            .keys()
            .filter(|k| !categorized.contains(*k))
            .collect();
        uncategorized.sort();

        if !uncategorized.is_empty() {
            output.push_str("--- UNCATEGORIZED NODES ---\n");
            for name in uncategorized {
                let id = node_registry[name];
                let status = if found_in_file.contains(name.as_str()) {
                    "✅"
                } else {
                    "  "
                };
                output.push_str(&format!("  {status} {name:35} -> ID: {id}\n"));
            }
        }

        output.push_str("\nLegend: ✅ = found in example,   = only in grammar, ❓ = not present\n");
        output
    }

    fn generate_python_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== Python Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_python::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/python/comprehensive.py")
            .unwrap_or_else(|_| "def main():\n    pass\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== Python Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define Python node categories for organization
        let node_categories = vec![
            (
                "IMPORT NODES",
                vec![
                    "import_statement",
                    "import_from_statement",
                    "aliased_import",
                    "dotted_name",
                    "relative_import",
                    "wildcard_import",
                ],
            ),
            (
                "CLASS & FUNCTION NODES",
                vec![
                    "class_definition",
                    "function_definition",
                    "decorated_definition",
                    "lambda",
                    "parameters",
                    "default_parameter",
                    "typed_parameter",
                    "typed_default_parameter",
                    "list_splat_parameter",
                    "dictionary_splat_parameter",
                ],
            ),
            (
                "ASYNC NODES",
                vec![
                    "async_function_definition",
                    "async_for_statement",
                    "async_with_statement",
                    "await",
                    "async_for_in_clause",
                    "async_comprehension",
                ],
            ),
            (
                "STATEMENT NODES",
                vec![
                    "if_statement",
                    "elif_clause",
                    "else_clause",
                    "while_statement",
                    "for_statement",
                    "try_statement",
                    "except_clause",
                    "finally_clause",
                    "with_statement",
                    "match_statement",
                    "case_clause",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "assignment",
                    "augmented_assignment",
                    "annotated_assignment",
                    "binary_operator",
                    "unary_operator",
                    "comparison_operator",
                    "conditional_expression",
                    "named_expression",
                    "as_pattern",
                ],
            ),
            (
                "COMPREHENSION NODES",
                vec![
                    "list_comprehension",
                    "dictionary_comprehension",
                    "set_comprehension",
                    "generator_expression",
                    "for_in_clause",
                    "if_clause",
                ],
            ),
            (
                "TYPE NODES",
                vec![
                    "type",
                    "type_alias_statement",
                    "type_parameter",
                    "type_comment",
                    "generic_type",
                    "union_type",
                    "constrained_type",
                    "member_type",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    fn generate_rust_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== Rust Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_rust::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/rust/comprehensive.rs")
            .unwrap_or_else(|_| "fn main() {}\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== Rust Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define Rust node categories for organization
        let node_categories = vec![
            (
                "MODULE & USE NODES",
                vec![
                    "mod_item",
                    "use_declaration",
                    "use_clause",
                    "use_list",
                    "use_as_clause",
                    "use_wildcard",
                    "scoped_use_list",
                    "extern_crate",
                ],
            ),
            (
                "STRUCT & ENUM NODES",
                vec![
                    "struct_item",
                    "enum_item",
                    "enum_variant",
                    "enum_variant_list",
                    "field_declaration",
                    "field_declaration_list",
                    "ordered_field_declaration_list",
                    "struct_expression",
                    "struct_pattern",
                    "tuple_struct_pattern",
                ],
            ),
            (
                "TRAIT & IMPL NODES",
                vec![
                    "trait_item",
                    "impl_item",
                    "associated_type",
                    "trait_bounds",
                    "where_clause",
                    "where_predicate",
                    "higher_ranked_trait_bound",
                    "removed_trait_bound",
                    "trait_type",
                    "abstract_type",
                ],
            ),
            (
                "FUNCTION NODES",
                vec![
                    "function_item",
                    "function_signature_item",
                    "parameters",
                    "parameter",
                    "self_parameter",
                    "variadic_parameter",
                    "optional_type_parameter",
                    "closure_expression",
                    "closure_parameters",
                    "async_block",
                ],
            ),
            (
                "TYPE NODES",
                vec![
                    "type_alias",
                    "type_item",
                    "generic_type",
                    "generic_type_with_turbofish",
                    "function_type",
                    "tuple_type",
                    "array_type",
                    "pointer_type",
                    "reference_type",
                    "empty_type",
                    "dynamic_type",
                    "bounded_type",
                ],
            ),
            (
                "PATTERN NODES",
                vec![
                    "tuple_pattern",
                    "slice_pattern",
                    "tuple_struct_pattern",
                    "struct_pattern",
                    "remaining_field_pattern",
                    "mut_pattern",
                    "range_pattern",
                    "ref_pattern",
                    "captured_pattern",
                    "reference_pattern",
                    "or_pattern",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "macro_invocation",
                    "macro_definition",
                    "macro_rule",
                    "token_tree",
                    "match_expression",
                    "match_arm",
                    "match_pattern",
                    "if_expression",
                    "while_expression",
                    "loop_expression",
                    "for_expression",
                    "const_item",
                    "static_item",
                    "attribute_item",
                    "inner_attribute_item",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    fn generate_typescript_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== TypeScript Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/typescript/comprehensive.ts")
            .unwrap_or_else(|_| "function main() {}\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== TypeScript Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define TypeScript node categories for organization
        let node_categories = vec![
            (
                "IMPORT/EXPORT NODES",
                vec![
                    "import_statement",
                    "import_clause",
                    "named_imports",
                    "namespace_import",
                    "export_statement",
                    "export_clause",
                    "export_specifier",
                    "import_specifier",
                    "import_alias",
                    "default_type",
                ],
            ),
            (
                "CLASS NODES",
                vec![
                    "class_declaration",
                    "class_body",
                    "method_definition",
                    "method_signature",
                    "public_field_definition",
                    "private_field_definition",
                    "abstract_method_signature",
                    "class_heritage",
                    "extends_clause",
                    "implements_clause",
                    "decorator",
                    "computed_property_name",
                ],
            ),
            (
                "INTERFACE & TYPE NODES",
                vec![
                    "interface_declaration",
                    "interface_body",
                    "type_alias_declaration",
                    "enum_declaration",
                    "enum_body",
                    "enum_assignment",
                    "type_parameter",
                    "type_parameters",
                    "type_arguments",
                    "type_annotation",
                    "type_predicate",
                    "type_predicate_annotation",
                ],
            ),
            (
                "FUNCTION NODES",
                vec![
                    "function_declaration",
                    "function_expression",
                    "arrow_function",
                    "generator_function",
                    "generator_function_declaration",
                    "formal_parameters",
                    "required_parameter",
                    "optional_parameter",
                    "rest_parameter",
                    "async_function",
                    "async_arrow_function",
                ],
            ),
            (
                "TYPE SYSTEM NODES",
                vec![
                    "union_type",
                    "intersection_type",
                    "conditional_type",
                    "generic_type",
                    "type_query",
                    "index_type_query",
                    "lookup_type",
                    "literal_type",
                    "template_literal_type",
                    "flow_maybe_type",
                    "parenthesized_type",
                    "predefined_type",
                    "type_identifier",
                ],
            ),
            (
                "JSX NODES",
                vec![
                    "jsx_element",
                    "jsx_self_closing_element",
                    "jsx_opening_element",
                    "jsx_closing_element",
                    "jsx_fragment",
                    "jsx_expression",
                    "jsx_attribute",
                    "jsx_namespace_name",
                    "jsx_text",
                ],
            ),
            (
                "MODULE NODES",
                vec![
                    "module",
                    "internal_module",
                    "module_body",
                    "ambient_declaration",
                    "namespace_declaration",
                    "namespace_body",
                    "export_assignment",
                    "export_default_declaration",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    fn discover_nodes_with_ids(
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

    #[test]
    fn generate_php_tree_structure() {
        println!("=== Generating PHP Tree Structure ===\n");

        let mut parser = Parser::new();
        let language: Language = tree_sitter_php::LANGUAGE_PHP.into();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/php/comprehensive.php")
            .unwrap_or_else(|_| "<?php\nclass Example {}\n".to_string());

        if let Some(tree) = parser.parse(&code, None) {
            let mut output = String::new();
            output.push_str("# PHP AST Tree Structure\n\n");
            output.push_str("Complete nested structure from comprehensive.php\n\n");
            output.push_str("```\n");

            // Generate complete tree structure
            generate_tree_structure(&mut output, tree.root_node(), &code, 0, None);

            output.push_str("```\n\n");

            // Now collect and analyze all unique node types found
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
                "\n**Total unique node types**: {}\n",
                node_stats.len()
            ));

            fs::write("contributing/parsers/php/TREE_STRUCT.md", output)
                .expect("Failed to write PHP tree structure");

            println!("✅ PHP TREE_STRUCT.md generated with complete AST structure");
        }
    }

    #[test]
    fn generate_go_tree_structure() {
        println!("=== Generating Go Tree Structure ===\n");

        let mut parser = Parser::new();
        let language: Language = tree_sitter_go::LANGUAGE.into();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/go/comprehensive.go")
            .unwrap_or_else(|_| "package main\n\nfunc main() {}\n".to_string());

        if let Some(tree) = parser.parse(&code, None) {
            let mut output = String::new();
            output.push_str("# Go AST Tree Structure\n\n");
            output.push_str("Complete nested structure from comprehensive.go\n\n");
            output.push_str("```\n");

            // Generate complete tree structure
            generate_tree_structure(&mut output, tree.root_node(), &code, 0, None);

            output.push_str("```\n\n");

            // Now collect and analyze all unique node types found
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
                "\n**Total unique node types**: {}\n",
                node_stats.len()
            ));

            fs::write("contributing/parsers/go/TREE_STRUCT.md", output)
                .expect("Failed to write Go tree structure");

            println!("✅ Go TREE_STRUCT.md generated with complete AST structure");
        }
    }

    /// Generate a complete tree structure showing ALL nodes and their relationships
    fn generate_tree_structure(
        output: &mut String,
        node: Node,
        code: &str,
        depth: usize,
        field_name: Option<&str>,
    ) {
        // Skip if we're too deep to avoid huge output
        if depth > 50 {
            output.push_str(&format!(
                "{:indent$}... (truncated at depth 50)\n",
                "",
                indent = depth * 2
            ));
            return;
        }

        let node_text = code.get(node.byte_range()).unwrap_or("<invalid>");

        // Get first line for display
        let display_text = node_text
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();

        // Format the node info
        let field_prefix = if let Some(fname) = field_name {
            format!("{fname}: ")
        } else {
            String::new()
        };

        output.push_str(&format!(
            "{:indent$}{}{} [{}]",
            "",
            field_prefix,
            node.kind(),
            node.kind_id(),
            indent = depth * 2
        ));

        // Add text preview for leaf nodes or short content
        if node.child_count() == 0 || display_text.len() <= 40 {
            output.push_str(&format!(" = '{}'", display_text.replace('\n', "\\n")));
        }

        output.push('\n');

        // Recursively show all children
        let mut cursor = node.walk();
        for (i, child) in node.children(&mut cursor).enumerate() {
            let child_field = node.field_name_for_child(i as u32);
            generate_tree_structure(output, child, code, depth + 1, child_field);
        }
    }

    /// Collect statistics about node types (count and max depth)
    fn collect_node_statistics(node: Node, stats: &mut HashMap<String, (usize, usize)>) {
        fn collect_recursive(
            node: Node,
            stats: &mut HashMap<String, (usize, usize)>,
            depth: usize,
        ) {
            let node_kind = node.kind().to_string();
            let entry = stats.entry(node_kind).or_insert((0, 0));
            entry.0 += 1; // increment count
            entry.1 = entry.1.max(depth); // update max depth

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_recursive(child, stats, depth + 1);
            }
        }

        collect_recursive(node, stats, 0);
    }

    #[test]
    fn generate_node_discovery_c() {
        println!("=== C Node Discovery ===\n");

        let node_discovery = generate_c_node_discovery();
        fs::write("contributing/parsers/c/node_discovery.txt", node_discovery)
            .expect("Failed to write C node discovery");
        println!("✅ C node_discovery.txt saved");
    }

    fn generate_c_node_discovery() -> String {
        use tree_sitter::{Language, Parser};
        // Import from the common module properly
        use super::abi15_exploration_common::print_node_tree;

        let mut output = String::new();
        output.push_str("=== C Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_c::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/c/comprehensive.c").unwrap_or_else(|_| {
            "#include <stdio.h>\n\nint main(void) {\n    return 0;\n}\n".to_string()
        });

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== C Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define C node categories for organization
        let node_categories = vec![
            (
                "PREPROCESSOR AND INCLUDE NODES",
                vec![
                    "translation_unit",
                    "preproc_include",
                    "preproc_define",
                    "preproc_function_def",
                    "preproc_call",
                    "preproc_def",
                    "preproc_if",
                    "preproc_ifdef",
                    "preproc_ifndef",
                    "preproc_else",
                    "preproc_elif",
                    "preproc_endif",
                    "system_lib_string",
                    "string_literal",
                    "identifier",
                ],
            ),
            (
                "FUNCTION-RELATED NODES",
                vec![
                    "function_definition",
                    "function_declarator",
                    "function_type",
                    "parameter_declaration",
                    "parameter_list",
                    "variadic_parameter",
                    "abstract_function_declarator",
                    "call_expression",
                    "argument_list",
                ],
            ),
            (
                "STRUCT AND UNION NODES",
                vec![
                    "struct_specifier",
                    "union_specifier",
                    "field_declaration",
                    "field_declaration_list",
                    "field_identifier",
                    "bitfield_clause",
                    "field_designator",
                    "init_declarator",
                ],
            ),
            (
                "ENUM-RELATED NODES",
                vec!["enum_specifier", "enumerator", "enumerator_list"],
            ),
            (
                "DECLARATION AND TYPEDEF NODES",
                vec![
                    "declaration",
                    "typedef_declaration",
                    "type_definition",
                    "declarator",
                    "init_declarator",
                    "storage_class_specifier",
                    "type_qualifier",
                    "pointer_declarator",
                    "array_declarator",
                    "abstract_pointer_declarator",
                    "abstract_array_declarator",
                    "type_descriptor",
                ],
            ),
            (
                "TYPE-RELATED NODES",
                vec![
                    "primitive_type",
                    "sized_type_specifier",
                    "type_identifier",
                    "pointer_type",
                    "array_type",
                    "struct_type",
                    "union_type",
                    "enum_type",
                ],
            ),
            (
                "STATEMENT NODES",
                vec![
                    "compound_statement",
                    "expression_statement",
                    "labeled_statement",
                    "if_statement",
                    "switch_statement",
                    "case_statement",
                    "while_statement",
                    "for_statement",
                    "do_statement",
                    "break_statement",
                    "continue_statement",
                    "return_statement",
                    "goto_statement",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "assignment_expression",
                    "update_expression",
                    "cast_expression",
                    "sizeof_expression",
                    "alignof_expression",
                    "offsetof_expression",
                    "generic_expression",
                    "subscript_expression",
                    "field_expression",
                    "comma_expression",
                    "conditional_expression",
                    "binary_expression",
                    "unary_expression",
                    "postfix_expression",
                    "parenthesized_expression",
                    "initializer_list",
                    "initializer_pair",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    #[test]
    fn generate_c_tree_structure() {
        println!("=== Generating C Tree Structure ===\n");

        let mut parser = Parser::new();
        let language: Language = tree_sitter_c::LANGUAGE.into();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/c/comprehensive.c").unwrap_or_else(|_| {
            "#include <stdio.h>\n\nint main(void) {\n    return 0;\n}\n".to_string()
        });

        if let Some(tree) = parser.parse(&code, None) {
            let mut output = String::new();
            output.push_str("# C AST Tree Structure\n\n");
            output.push_str("Complete nested structure from comprehensive.c\n\n");
            output.push_str("```\n");

            // Generate complete tree structure
            generate_tree_structure(&mut output, tree.root_node(), &code, 0, None);

            output.push_str("```\n\n");

            // Now collect and analyze all unique node types found
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
                "\n**Total unique node types**: {}\n",
                node_stats.len()
            ));

            fs::write("contributing/parsers/c/TREE_STRUCT.md", output)
                .expect("Failed to write C tree structure");

            println!("✅ C TREE_STRUCT.md generated with complete AST structure");
        }
    }

    #[test]
    fn generate_node_discovery_cpp() {
        println!("=== C++ Node Discovery ===\n");

        let node_discovery = generate_cpp_node_discovery();
        fs::write(
            "contributing/parsers/cpp/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write C++ node discovery");
        println!("✅ C++ node_discovery.txt saved");
    }

    fn generate_cpp_node_discovery() -> String {
        use tree_sitter::{Language, Parser};
        // Import from the common module properly
        use super::abi15_exploration_common::print_node_tree;

        let mut output = String::new();
        output.push_str("=== C++ Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_cpp::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/cpp/comprehensive.cpp").unwrap_or_else(|_| {
            "#include <iostream>\n\nint main() {\n    return 0;\n}\n".to_string()
        });

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== C++ Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define C++ node categories for organization
        let node_categories = vec![
            (
                "PREPROCESSOR AND INCLUDE NODES",
                vec![
                    "translation_unit",
                    "preproc_include",
                    "preproc_define",
                    "preproc_function_def",
                    "preproc_call",
                    "preproc_def",
                    "preproc_if",
                    "preproc_ifdef",
                    "preproc_ifndef",
                    "preproc_else",
                    "preproc_elif",
                    "preproc_endif",
                    "system_lib_string",
                    "string_literal",
                    "identifier",
                ],
            ),
            (
                "NAMESPACE AND USING NODES",
                vec![
                    "namespace_definition",
                    "namespace_identifier",
                    "using_declaration",
                    "using_directive",
                    "alias_declaration",
                    "qualified_identifier",
                    "scope_resolution",
                    "nested_namespace_specifier",
                ],
            ),
            (
                "CLASS AND STRUCT NODES",
                vec![
                    "class_specifier",
                    "struct_specifier",
                    "access_specifier",
                    "field_declaration",
                    "field_declaration_list",
                    "field_identifier",
                    "bitfield_clause",
                    "base_class_clause",
                    "virtual_specifier",
                    "explicit_function_specifier",
                ],
            ),
            (
                "TEMPLATE-RELATED NODES",
                vec![
                    "template_declaration",
                    "template_instantiation",
                    "template_type",
                    "template_function",
                    "template_method",
                    "template_parameter_list",
                    "type_parameter_declaration",
                    "optional_parameter_declaration",
                    "variadic_declaration",
                    "template_template_parameter_declaration",
                    "template_argument_list",
                    "type_descriptor",
                ],
            ),
            (
                "FUNCTION-RELATED NODES",
                vec![
                    "function_definition",
                    "function_declarator",
                    "function_type",
                    "method_definition",
                    "constructor_definition",
                    "destructor_definition",
                    "operator_overload",
                    "parameter_declaration",
                    "parameter_list",
                    "variadic_parameter",
                    "abstract_function_declarator",
                    "call_expression",
                    "argument_list",
                    "trailing_return_type",
                ],
            ),
            (
                "INHERITANCE AND VIRTUAL NODES",
                vec![
                    "virtual_function_specifier",
                    "override_specifier",
                    "final_specifier",
                    "pure_virtual_function_definition",
                    "virtual_specifier",
                    "access_specifier",
                ],
            ),
            (
                "ENUM-RELATED NODES",
                vec![
                    "enum_specifier",
                    "scoped_enum_specifier",
                    "enumerator",
                    "enumerator_list",
                ],
            ),
            (
                "DECLARATION AND TYPEDEF NODES",
                vec![
                    "declaration",
                    "simple_declaration",
                    "typedef_declaration",
                    "type_definition",
                    "declarator",
                    "init_declarator",
                    "storage_class_specifier",
                    "type_qualifier",
                    "pointer_declarator",
                    "array_declarator",
                    "abstract_pointer_declarator",
                    "abstract_array_declarator",
                    "reference_declarator",
                    "structured_binding_declarator",
                ],
            ),
            (
                "TYPE-RELATED NODES",
                vec![
                    "primitive_type",
                    "sized_type_specifier",
                    "type_identifier",
                    "pointer_type",
                    "reference_type",
                    "array_type",
                    "auto",
                    "decltype",
                    "placeholder_type_specifier",
                    "dependent_type",
                    "qualified_type",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "assignment_expression",
                    "update_expression",
                    "cast_expression",
                    "sizeof_expression",
                    "alignof_expression",
                    "typeid_expression",
                    "new_expression",
                    "delete_expression",
                    "subscript_expression",
                    "field_expression",
                    "comma_expression",
                    "conditional_expression",
                    "binary_expression",
                    "unary_expression",
                    "postfix_expression",
                    "parenthesized_expression",
                    "initializer_list",
                    "initializer_pair",
                    "lambda_expression",
                    "lambda_capture_specifier",
                    "parameter_pack_expansion",
                ],
            ),
            (
                "STATEMENT NODES",
                vec![
                    "compound_statement",
                    "expression_statement",
                    "labeled_statement",
                    "if_statement",
                    "switch_statement",
                    "case_statement",
                    "while_statement",
                    "for_statement",
                    "for_range_loop",
                    "do_statement",
                    "break_statement",
                    "continue_statement",
                    "return_statement",
                    "goto_statement",
                    "try_statement",
                    "catch_clause",
                    "throw_statement",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    #[test]
    fn generate_cpp_tree_structure() {
        println!("=== Generating C++ Tree Structure ===\n");

        let mut parser = Parser::new();
        let language: Language = tree_sitter_cpp::LANGUAGE.into();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/cpp/comprehensive.cpp").unwrap_or_else(|_| {
            "#include <iostream>\n\nint main() {\n    return 0;\n}\n".to_string()
        });

        if let Some(tree) = parser.parse(&code, None) {
            let mut output = String::new();
            output.push_str("# C++ AST Tree Structure\n\n");
            output.push_str("Complete nested structure from comprehensive.cpp\n\n");
            output.push_str("```\n");

            // Generate complete tree structure
            generate_tree_structure(&mut output, tree.root_node(), &code, 0, None);

            output.push_str("```\n\n");

            // Now collect and analyze all unique node types found
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
                "\n**Total unique node types**: {}\n",
                node_stats.len()
            ));

            fs::write("contributing/parsers/cpp/TREE_STRUCT.md", output)
                .expect("Failed to write C++ tree structure");

            println!("✅ C++ TREE_STRUCT.md generated with complete AST structure");
        }
    }

    #[test]
    fn comprehensive_csharp_analysis() {
        println!("=== C# Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/csharp/node-types.json")
            .expect("Failed to read C# grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match CSharpParserAudit::audit_file("examples/csharp/comprehensive.cs") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit C# file: {e}");
                // Create empty audit for fallback
                CSharpParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/csharp/AUDIT_REPORT.md", &report)
            .expect("Failed to write C# audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# C# Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.cs: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> =
            example_nodes.difference(&audit.implemented_nodes).collect();
        let mut handled: Vec<_> = audit.implemented_nodes.iter().collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled.sort();

        // Write comprehensive analysis
        analysis.push_str("## Nodes in Grammar but Not in Example\n");
        analysis.push_str(&format!("**Count**: {}\n\n", in_grammar_only.len()));
        for node in &in_grammar_only {
            analysis.push_str(&format!("- `{node}`\n"));
        }
        analysis.push('\n');

        analysis.push_str("## Nodes in Example but Not Handled by Parser\n");
        analysis.push_str(&format!("**Count**: {}\n\n", in_example_not_handled.len()));
        for node in &in_example_not_handled {
            analysis.push_str(&format!("- `{node}`\n"));
        }
        analysis.push('\n');

        analysis.push_str("## Nodes Handled by Parser\n");
        analysis.push_str(&format!("**Count**: {}\n\n", handled.len()));
        for node in &handled {
            analysis.push_str(&format!("- `{node}`\n"));
        }
        analysis.push('\n');

        fs::write("contributing/parsers/csharp/GRAMMAR_ANALYSIS.md", analysis)
            .expect("Failed to write C# grammar analysis");

        // Print summary
        println!("C# Grammar Analysis Summary:");
        println!("- Total nodes in grammar: {}", all_grammar_nodes.len());
        println!("- Nodes in comprehensive.cs: {}", example_nodes.len());
        println!(
            "- Nodes handled by parser: {}",
            audit.implemented_nodes.len()
        );
        println!(
            "- Symbol kinds extracted: {}",
            audit.extracted_symbol_kinds.len()
        );
        println!();
        // Also generate node_discovery.txt
        let node_discovery = generate_csharp_node_discovery();
        fs::write(
            "contributing/parsers/csharp/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write C# node discovery");

        println!("Generated files:");
        println!("  - contributing/parsers/csharp/AUDIT_REPORT.md");
        println!("  - contributing/parsers/csharp/GRAMMAR_ANALYSIS.md");
        println!("  - contributing/parsers/csharp/node_discovery.txt");
    }

    fn generate_csharp_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== C# Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_c_sharp::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/csharp/comprehensive.cs")
            .unwrap_or_else(|_| "class Program { static void Main() {} }".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== C# Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define C# node categories for organization
        let node_categories = vec![
            (
                "NAMESPACE & USING NODES",
                vec![
                    "using_directive",
                    "namespace_declaration",
                    "file_scoped_namespace_declaration",
                    "qualified_name",
                ],
            ),
            (
                "TYPE DEFINITION NODES",
                vec![
                    "class_declaration",
                    "struct_declaration",
                    "interface_declaration",
                    "enum_declaration",
                    "record_declaration",
                    "delegate_declaration",
                    "type_parameter_list",
                    "type_parameter",
                    "type_parameter_constraint",
                ],
            ),
            (
                "MEMBER NODES",
                vec![
                    "method_declaration",
                    "property_declaration",
                    "field_declaration",
                    "event_declaration",
                    "indexer_declaration",
                    "constructor_declaration",
                    "destructor_declaration",
                    "operator_declaration",
                    "accessor_declaration",
                ],
            ),
            (
                "STATEMENT NODES",
                vec![
                    "if_statement",
                    "switch_statement",
                    "switch_expression",
                    "for_statement",
                    "foreach_statement",
                    "while_statement",
                    "do_statement",
                    "try_statement",
                    "catch_clause",
                    "finally_clause",
                    "using_statement",
                    "lock_statement",
                    "return_statement",
                    "throw_statement",
                    "yield_statement",
                    "break_statement",
                    "continue_statement",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "invocation_expression",
                    "member_access_expression",
                    "assignment_expression",
                    "binary_expression",
                    "prefix_unary_expression",
                    "postfix_unary_expression",
                    "conditional_expression",
                    "lambda_expression",
                    "object_creation_expression",
                    "array_creation_expression",
                    "element_access_expression",
                    "cast_expression",
                    "as_expression",
                    "is_expression",
                    "await_expression",
                    "query_expression",
                    "interpolated_string_expression",
                ],
            ),
            (
                "ASYNC & LINQ NODES",
                vec![
                    "query_expression",
                    "from_clause",
                    "select_clause",
                    "where_clause",
                    "order_by_clause",
                    "group_clause",
                    "join_clause",
                    "await_expression",
                ],
            ),
            (
                "PATTERN NODES",
                vec![
                    "switch_expression_arm",
                    "when_clause",
                    "declaration_pattern",
                    "recursive_pattern",
                    "var_pattern",
                    "discard_pattern",
                ],
            ),
            (
                "TYPE NODES",
                vec![
                    "predefined_type",
                    "nullable_type",
                    "array_type",
                    "tuple_type",
                    "pointer_type",
                    "generic_name",
                ],
            ),
            (
                "LITERAL NODES",
                vec![
                    "integer_literal",
                    "real_literal",
                    "string_literal",
                    "verbatim_string_literal",
                    "interpolated_string_text",
                    "character_literal",
                    "boolean_literal",
                    "null_literal",
                ],
            ),
            ("COMMENT & DOCUMENTATION NODES", vec!["comment"]),
        ];

        // Output nodes organized by category
        for (category_name, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category_name} ===\n"));

            for node_kind in expected_nodes {
                if let Some(id) = node_registry.get(*node_kind) {
                    if found_in_file.contains(*node_kind) {
                        output.push_str(&format!("  ✓ {node_kind:<40} -> ID: {id}\n"));
                    } else {
                        output
                            .push_str(&format!("  ✓ {node_kind:<40} -> ID: {id} (not verified)\n"));
                    }
                } else {
                    output.push_str(&format!("  ✗ {node_kind:<40} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // List any remaining nodes not in categories
        let mut categorized = HashSet::new();
        for (_, nodes) in &node_categories {
            for node in nodes {
                categorized.insert(node.to_string());
            }
        }

        let mut uncategorized: Vec<_> = node_registry
            .keys()
            .filter(|k| !categorized.contains(*k))
            .collect();
        uncategorized.sort();

        if !uncategorized.is_empty() {
            output.push_str("--- UNCATEGORIZED NODES ---\n");
            for node in uncategorized {
                let id = node_registry[node];
                output.push_str(&format!("  {node} (ID: {id})\n"));
            }
        }

        output
    }

    #[test]
    fn comprehensive_kotlin_analysis() {
        println!("=== Kotlin Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/kotlin/node-types.json")
            .expect("Failed to read Kotlin grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match KotlinParserAudit::audit_file("examples/kotlin/comprehensive.kt") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Kotlin file: {e}");
                // Create empty audit for fallback
                KotlinParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/kotlin/AUDIT_REPORT.md", &report)
            .expect("Failed to write Kotlin audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Kotlin Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.kt: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.kt but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📝 Missing from Examples\n");
            analysis.push_str("These grammar nodes aren't in comprehensive.kt:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Add extracted symbol kinds info
        if !audit.extracted_symbol_kinds.is_empty() {
            analysis.push_str("## 🎯 Symbol Kinds Extracted\n");
            let mut kinds: Vec<_> = audit.extracted_symbol_kinds.iter().collect();
            kinds.sort();
            for kind in kinds {
                analysis.push_str(&format!("- {kind}\n"));
            }
            analysis.push('\n');
        }

        fs::write("contributing/parsers/kotlin/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Kotlin grammar analysis");

        // Also generate node_discovery.txt
        let node_discovery = generate_kotlin_node_discovery();
        fs::write(
            "contributing/parsers/kotlin/node_discovery.txt",
            node_discovery,
        )
        .expect("Failed to write Kotlin node discovery");

        println!("📄 Kotlin Analysis:");
        println!("  - Grammar nodes: {}", all_grammar_nodes.len());
        println!("  - Example nodes: {}", example_nodes.len());
        println!("  - Handled nodes: {}", audit.implemented_nodes.len());
        println!("  - Symbol kinds: {:?}", audit.extracted_symbol_kinds);
        println!(
            "  - Coverage: {:.1}%",
            audit.implemented_nodes.len() as f32 / example_nodes.len() as f32 * 100.0
        );
        println!("✅ Kotlin AUDIT_REPORT.md saved");
        println!("✅ Kotlin GRAMMAR_ANALYSIS.md saved");
        println!("✅ Kotlin node_discovery.txt saved");
    }

    #[test]
    fn comprehensive_java_analysis() {
        println!("=== Java Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/java/node-types.json")
            .expect("Failed to read Java grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match JavaParserAudit::audit_file("examples/java/comprehensive.java") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Java file: {e}");
                // Create empty audit for fallback
                JavaParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/java/AUDIT_REPORT.md", &report)
            .expect("Failed to write Java audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Java Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.java: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## ✅ Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
                println!("✓ {node}");
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## ⚠️ Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.java but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## 📚 Grammar-Only Nodes\n");
            analysis.push_str("These nodes exist in grammar but not in comprehensive.java:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Save comprehensive analysis
        fs::write("contributing/parsers/java/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Java grammar analysis");

        // Save node discovery
        let node_discovery = generate_java_node_discovery();
        fs::write(
            "contributing/parsers/java/node_discovery.txt",
            &node_discovery,
        )
        .expect("Failed to write Java node discovery");

        // Summary
        println!("\n=== Summary ===");
        println!(
            "Grammar nodes: {} | Example nodes: {} | Handled: {}",
            all_grammar_nodes.len(),
            example_nodes.len(),
            audit.implemented_nodes.len()
        );
        println!("✅ Java AUDIT_REPORT.md saved");
        println!("✅ Java GRAMMAR_ANALYSIS.md saved");
        println!("✅ Java node_discovery.txt saved");
    }

    fn generate_kotlin_node_discovery() -> String {
        let mut parser = Parser::new();
        let language = tree_sitter_kotlin::language();
        parser
            .set_language(&language)
            .expect("Failed to set Kotlin language");

        let code = fs::read_to_string("examples/kotlin/comprehensive.kt")
            .expect("Failed to read comprehensive.kt");

        let tree = parser.parse(&code, None).expect("Failed to parse Kotlin");

        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file: HashSet<String> = HashSet::new();

        fn collect_nodes(
            node: Node,
            registry: &mut HashMap<String, u16>,
            found: &mut HashSet<String>,
        ) {
            let kind = node.kind();
            registry.insert(kind.to_string(), node.kind_id());
            found.insert(kind.to_string());

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_nodes(child, registry, found);
            }
        }

        collect_nodes(tree.root_node(), &mut node_registry, &mut found_in_file);

        let mut output = String::new();
        output.push_str("=== Kotlin Grammar Node Discovery ===\n");
        output.push_str(&format!("Generated: {}\n\n", get_formatted_timestamp()));
        output.push_str(&format!(
            "Total unique node types found: {}\n\n",
            node_registry.len()
        ));

        // Define key node categories for Kotlin
        let node_categories = vec![
            (
                "CLASS & OBJECT DECLARATIONS",
                vec![
                    "class_declaration",
                    "object_declaration",
                    "interface",
                    "enum_class",
                    "data_class",
                    "sealed_class",
                    "companion_object",
                ],
            ),
            (
                "FUNCTION DECLARATIONS",
                vec![
                    "function_declaration",
                    "primary_constructor",
                    "secondary_constructor",
                    "anonymous_function",
                    "lambda_literal",
                ],
            ),
            (
                "PROPERTY & VARIABLE DECLARATIONS",
                vec![
                    "property_declaration",
                    "variable_declaration",
                    "class_parameter",
                    "function_value_parameter",
                ],
            ),
            (
                "TYPE SYSTEM",
                vec![
                    "type_alias",
                    "type_reference",
                    "nullable_type",
                    "user_type",
                    "function_type",
                    "type_projection",
                ],
            ),
            (
                "MODIFIERS & VISIBILITY",
                vec![
                    "modifiers",
                    "visibility_modifier",
                    "inheritance_modifier",
                    "function_modifier",
                    "property_modifier",
                ],
            ),
            (
                "EXPRESSIONS",
                vec![
                    "call_expression",
                    "string_literal",
                    "integer_literal",
                    "boolean_literal",
                    "binary_expression",
                    "prefix_expression",
                ],
            ),
        ];

        // Output nodes organized by category
        for (category_name, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category_name} ===\n"));

            for node_kind in expected_nodes {
                if let Some(id) = node_registry.get(*node_kind) {
                    if found_in_file.contains(*node_kind) {
                        output.push_str(&format!("  ✓ {node_kind:<40} -> ID: {id}\n"));
                    } else {
                        output
                            .push_str(&format!("  ✓ {node_kind:<40} -> ID: {id} (not verified)\n"));
                    }
                } else {
                    output.push_str(&format!("  ✗ {node_kind:<40} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // List any remaining nodes not in categories
        let mut categorized = HashSet::new();
        for (_, nodes) in &node_categories {
            for node in nodes {
                categorized.insert(node.to_string());
            }
        }

        let mut uncategorized: Vec<_> = node_registry
            .keys()
            .filter(|k| !categorized.contains(*k))
            .collect();
        uncategorized.sort();

        if !uncategorized.is_empty() {
            output.push_str("--- UNCATEGORIZED NODES ---\n");
            for node in uncategorized {
                let id = node_registry[node];
                output.push_str(&format!("  {node} (ID: {id})\n"));
            }
        }

        output
    }

    fn generate_java_node_discovery() -> String {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_java::LANGUAGE.into();
        parser
            .set_language(&language)
            .expect("Failed to set Java language");

        let code = fs::read_to_string("examples/java/comprehensive.java")
            .expect("Failed to read comprehensive.java");

        let tree = parser.parse(&code, None).expect("Failed to parse Java");

        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file: HashSet<String> = HashSet::new();

        fn collect_nodes(
            node: Node,
            registry: &mut HashMap<String, u16>,
            found: &mut HashSet<String>,
        ) {
            let kind = node.kind();
            registry.insert(kind.to_string(), node.kind_id());
            found.insert(kind.to_string());

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_nodes(child, registry, found);
            }
        }

        collect_nodes(tree.root_node(), &mut node_registry, &mut found_in_file);

        let mut output = String::new();
        output.push_str("=== Java Grammar Node Discovery ===\n");
        output.push_str(&format!("Generated: {}\n\n", get_formatted_timestamp()));
        output.push_str(&format!(
            "Total unique node types found: {}\n\n",
            node_registry.len()
        ));

        // Sort nodes by name for consistent output
        let mut sorted_nodes: Vec<_> = node_registry.iter().collect();
        sorted_nodes.sort_by_key(|(name, _)| *name);

        output.push_str("--- ALL NODES ---\n");
        for (node, id) in sorted_nodes {
            output.push_str(&format!("  {node} (ID: {id})\n"));
        }

        output
    }

    fn generate_javascript_node_discovery() -> String {
        use super::abi15_exploration_common::print_node_tree;
        use tree_sitter::{Language, Parser};

        let mut output = String::new();
        output.push_str("=== JavaScript Language ABI-15 COMPREHENSIVE NODE MAPPING ===\n");
        output.push_str(&format!("  Generated: {}\n", get_formatted_timestamp()));

        let language: Language = tree_sitter_javascript::LANGUAGE.into();
        output.push_str(&format!("  ABI Version: {}\n", language.abi_version()));

        // Parse the comprehensive example
        let mut parser = Parser::new();
        parser.set_language(&language).unwrap();

        let code = fs::read_to_string("examples/javascript/comprehensive.js")
            .unwrap_or_else(|_| "function main() {}\n".to_string());

        let tree = parser.parse(&code, None).unwrap();
        let root = tree.root_node();

        // Debug: print tree structure if verbose env var is set
        if std::env::var("DEBUG_TREE").is_ok() {
            println!("\n=== JavaScript Tree Structure ===");
            print_node_tree(root, &code, 0);
        }

        // Collect all nodes with their actual IDs from the parsed file
        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file = HashSet::new();
        discover_nodes_with_ids(root, &mut node_registry, &mut found_in_file);

        output.push_str(&format!("  Node kind count: {}\n\n", node_registry.len()));

        // Define JavaScript node categories for organization
        let node_categories = vec![
            (
                "IMPORT/EXPORT NODES",
                vec![
                    "import_statement",
                    "import_clause",
                    "named_imports",
                    "namespace_import",
                    "export_statement",
                    "export_clause",
                    "export_specifier",
                    "import_specifier",
                ],
            ),
            (
                "CLASS NODES",
                vec![
                    "class_declaration",
                    "class_body",
                    "class_heritage",
                    "method_definition",
                    "field_definition",
                    "static_block",
                    "computed_property_name",
                ],
            ),
            (
                "FUNCTION NODES",
                vec![
                    "function_declaration",
                    "function_expression",
                    "arrow_function",
                    "generator_function",
                    "generator_function_declaration",
                    "formal_parameters",
                    "rest_pattern",
                ],
            ),
            (
                "VARIABLE NODES",
                vec![
                    "variable_declaration",
                    "lexical_declaration",
                    "variable_declarator",
                    "assignment_expression",
                    "assignment_pattern",
                    "object_pattern",
                    "array_pattern",
                ],
            ),
            (
                "EXPRESSION NODES",
                vec![
                    "call_expression",
                    "new_expression",
                    "member_expression",
                    "binary_expression",
                    "unary_expression",
                    "update_expression",
                    "ternary_expression",
                    "await_expression",
                    "yield_expression",
                    "spread_element",
                    "parenthesized_expression",
                ],
            ),
            (
                "JSX NODES",
                vec![
                    "jsx_element",
                    "jsx_self_closing_element",
                    "jsx_opening_element",
                    "jsx_closing_element",
                    "jsx_expression",
                    "jsx_attribute",
                    "jsx_text",
                ],
            ),
            (
                "CONTROL FLOW NODES",
                vec![
                    "if_statement",
                    "for_statement",
                    "for_in_statement",
                    "while_statement",
                    "do_statement",
                    "switch_statement",
                    "try_statement",
                    "catch_clause",
                    "return_statement",
                    "throw_statement",
                ],
            ),
            (
                "LITERAL NODES",
                vec![
                    "string",
                    "string_fragment",
                    "template_string",
                    "template_substitution",
                    "number",
                    "true",
                    "false",
                    "null",
                    "regex",
                    "object",
                    "array",
                ],
            ),
        ];

        // Output node mappings with discovered IDs
        for (category, expected_nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for node_name in expected_nodes {
                if let Some(node_id) = node_registry.get(*node_name) {
                    let status = if found_in_file.contains(*node_name) {
                        "✓"
                    } else {
                        "○" // In grammar but not in example file
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                } else {
                    output.push_str(&format!("  ✗ {node_name:35} NOT FOUND\n"));
                }
            }
            output.push('\n');
        }

        // Find additional nodes not in our categories
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
                        "✓"
                    } else {
                        "○"
                    };
                    output.push_str(&format!("  {status} {node_name:35} -> ID: {node_id}\n"));
                }
            }
        }

        output.push_str(
            "\nLegend: ✓ = found in file, ○ = in grammar but not in file, ✗ = not in grammar\n",
        );
        output
    }

    #[test]
    #[ignore]
    fn comprehensive_swift_analysis() {
        println!("=== Swift Comprehensive Grammar Analysis ===\n");

        // 1. Load ALL nodes from grammar JSON
        let grammar_json = fs::read_to_string("contributing/parsers/swift/node-types.json")
            .expect("Failed to read Swift grammar file");
        let grammar: Value =
            serde_json::from_str(&grammar_json).expect("Failed to parse grammar JSON");

        let mut all_grammar_nodes = HashSet::new();
        if let Value::Array(nodes) = &grammar {
            for node in nodes {
                if let (Some(Value::Bool(true)), Some(Value::String(node_type))) =
                    (node.get("named"), node.get("type"))
                {
                    all_grammar_nodes.insert(node_type.clone());
                }
            }
        }

        // 2. Run the REAL parser audit to get everything at once
        let audit = match SwiftParserAudit::audit_file("examples/swift/comprehensive.swift") {
            Ok(audit) => audit,
            Err(e) => {
                println!("Warning: Failed to audit Swift file: {e}");
                // Create empty audit for fallback
                SwiftParserAudit {
                    grammar_nodes: HashMap::new(),
                    implemented_nodes: HashSet::new(),
                    extracted_symbol_kinds: HashSet::new(),
                }
            }
        };

        // The audit already discovered all nodes in the example file!
        let example_nodes: HashSet<String> = audit.grammar_nodes.keys().cloned().collect();

        // Save the audit report
        let report = audit.generate_report();
        fs::write("contributing/parsers/swift/AUDIT_REPORT.md", &report)
            .expect("Failed to write Swift audit report");

        // 3. Generate comprehensive analysis comparing all three sources
        let mut analysis = String::new();
        analysis.push_str("# Swift Grammar Analysis\n\n");
        analysis.push_str(&format!("*Generated: {}*\n\n", get_formatted_timestamp()));
        analysis.push_str("## Statistics\n");
        analysis.push_str(&format!(
            "- Total nodes in grammar JSON: {}\n",
            all_grammar_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes found in comprehensive.swift: {}\n",
            example_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Nodes handled by parser: {}\n",
            audit.implemented_nodes.len()
        ));
        analysis.push_str(&format!(
            "- Symbol kinds extracted: {}\n",
            audit.extracted_symbol_kinds.len()
        ));
        analysis.push('\n');

        // Categorize nodes
        let mut in_grammar_only: Vec<_> = all_grammar_nodes.difference(&example_nodes).collect();
        let mut in_example_not_handled: Vec<_> = example_nodes
            .iter()
            .filter(|n| !audit.implemented_nodes.contains(n.as_str()))
            .collect();
        let mut handled_well: Vec<_> = audit
            .implemented_nodes
            .iter()
            .filter(|n| example_nodes.contains(n.as_str()))
            .collect();

        in_grammar_only.sort();
        in_example_not_handled.sort();
        handled_well.sort();

        if !handled_well.is_empty() {
            analysis.push_str("## Successfully Handled Nodes\n");
            analysis.push_str("These nodes are in examples and handled by parser:\n");
            for node in &handled_well {
                analysis.push_str(&format!("- {node}\n"));
                println!("handled: {node}");
            }
            analysis.push('\n');
        }

        if !in_example_not_handled.is_empty() {
            analysis.push_str("## Implementation Gaps\n");
            analysis.push_str("These nodes appear in comprehensive.swift but aren't handled:\n");
            for node in &in_example_not_handled {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        if !in_grammar_only.is_empty() {
            analysis.push_str("## Grammar-Only Nodes\n");
            analysis.push_str("These nodes exist in grammar but not in comprehensive.swift:\n");
            for node in &in_grammar_only {
                analysis.push_str(&format!("- {node}\n"));
            }
            analysis.push('\n');
        }

        // Save comprehensive analysis
        fs::write("contributing/parsers/swift/GRAMMAR_ANALYSIS.md", &analysis)
            .expect("Failed to write Swift grammar analysis");

        // Save node discovery
        let node_discovery = generate_swift_node_discovery();
        fs::write(
            "contributing/parsers/swift/node_discovery.txt",
            &node_discovery,
        )
        .expect("Failed to write Swift node discovery");

        // Summary
        println!("\n=== Summary ===");
        println!(
            "Grammar nodes: {} | Example nodes: {} | Handled: {}",
            all_grammar_nodes.len(),
            example_nodes.len(),
            audit.implemented_nodes.len()
        );
        println!("Swift AUDIT_REPORT.md saved");
        println!("Swift GRAMMAR_ANALYSIS.md saved");
        println!("Swift node_discovery.txt saved");
    }

    fn generate_swift_node_discovery() -> String {
        let mut parser = Parser::new();
        let language: Language = tree_sitter_swift::LANGUAGE.into();
        parser
            .set_language(&language)
            .expect("Failed to set Swift language");

        let code = fs::read_to_string("examples/swift/comprehensive.swift")
            .expect("Failed to read comprehensive.swift");

        let tree = parser.parse(&code, None).expect("Failed to parse Swift");

        let mut node_registry: HashMap<String, u16> = HashMap::new();
        let mut found_in_file: HashSet<String> = HashSet::new();

        fn collect_nodes(
            node: Node,
            registry: &mut HashMap<String, u16>,
            found: &mut HashSet<String>,
        ) {
            let kind = node.kind();
            registry.insert(kind.to_string(), node.kind_id());
            found.insert(kind.to_string());

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_nodes(child, registry, found);
            }
        }

        collect_nodes(tree.root_node(), &mut node_registry, &mut found_in_file);

        // Swift key nodes for symbol extraction
        let node_categories = vec![
            (
                "Type Declarations",
                vec![
                    "class_declaration",
                    "protocol_declaration",
                    "struct_declaration",
                    "enum_declaration",
                    "actor_declaration",
                    "extension_declaration",
                ],
            ),
            (
                "Member Declarations",
                vec![
                    "function_declaration",
                    "init_declaration",
                    "deinit_declaration",
                    "property_declaration",
                    "subscript_declaration",
                    "typealias_declaration",
                ],
            ),
            ("Enum Members", vec!["enum_entry"]),
            ("Imports", vec!["import_declaration"]),
            ("Modifiers", vec!["modifiers", "visibility_modifier"]),
            (
                "Inheritance",
                vec!["inheritance_specifier", "type_constraint"],
            ),
        ];

        let mut output = String::new();
        output.push_str("=== Swift Node Discovery ===\n\n");
        output.push_str(&format!(
            "Total unique nodes found: {}\n\n",
            found_in_file.len()
        ));

        for (category, nodes) in &node_categories {
            output.push_str(&format!("=== {category} ===\n"));
            for &node_name in nodes {
                let status = if found_in_file.contains(node_name) {
                    "found"
                } else {
                    "not found"
                };
                let id = node_registry
                    .get(node_name)
                    .map(|id| format!("{id}"))
                    .unwrap_or_else(|| "-".to_string());
                output.push_str(&format!("  {status:10} {node_name:35} -> ID: {id}\n"));
            }
            output.push('\n');
        }

        // List all found nodes
        output.push_str("=== ALL NODES FOUND IN FILE ===\n");
        let mut all_nodes: Vec<_> = found_in_file.iter().collect();
        all_nodes.sort();
        for node_name in all_nodes {
            if let Some(node_id) = node_registry.get(node_name) {
                output.push_str(&format!("  {node_name:35} -> ID: {node_id}\n"));
            }
        }

        output.push_str(
            "\nLegend: found = in example file, not found = in grammar but not in file\n",
        );
        output
    }
}
