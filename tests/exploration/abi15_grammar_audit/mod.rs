//! Grammar Audit and Node Discovery Tests
//!
//! Per-language analysis combining:
//! 1. Grammar JSON analysis - nodes available in tree-sitter grammar
//! 2. Node discovery - nodes appearing in example files
//! 3. Parser audit - nodes our parser handles
//!
//! Outputs per language (in contributing/parsers/<lang>/):
//! - AUDIT_REPORT.md - parser implementation coverage
//! - node_discovery.txt - grammar node mapping
//! - GRAMMAR_ANALYSIS.md - grammar vs example vs parser analysis
//!
//! Run with: cargo test abi15_grammar_audit -- --nocapture

extern crate tree_sitter_kotlin_codanna as tree_sitter_kotlin;

#[path = "../abi15_exploration_common.rs"]
mod abi15_exploration_common;

mod helpers;

mod c;
mod clojure;
mod cpp;
mod csharp;
mod gdscript;
mod go;
mod java;
mod javascript;
mod kotlin;
mod lua;
mod php;
mod python;
mod rust_lang;
mod swift;
mod typescript;
