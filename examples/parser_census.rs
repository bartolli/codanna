//! Scratch audit tool: per-language census of parser evidence emission.
//!
//! Measures what each language parser emits against the evidence fields
//! resolution depends on: caller identity, receiver (self-form vs named),
//! static flag, caller_range, qualified plain-call targets, ClassMember
//! scope on methods, Defines/Extends emission. Walks a fixture tree,
//! buckets files by registered extension, aggregates per language.
//! Not part of the product surface.
//!
//! Usage: parser_census [fixture-dir (default: examples)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codanna::Settings;
use codanna::parsing::get_registry;
use codanna::symbol::ScopeContext;
use codanna::types::SymbolCounter;
use codanna::{FileId, SymbolKind};

#[derive(Default)]
struct LangStats {
    files: usize,
    mc_total: usize,
    mc_caller_empty: usize,
    mc_recv_self: usize,
    mc_recv_named: usize,
    mc_recv_none: usize,
    mc_static: usize,
    mc_caller_range_none: usize,
    pc_total: usize,
    pc_qualified: usize,
    methods: usize,
    m_classmember_named: usize,
    m_classmember_anon: usize,
    m_scope_other: usize,
    m_scope_none: usize,
    defines: usize,
    extends: usize,
    implements: usize,
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "ts-cache",
    "target",
    "dist",
    ".next",
];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_ref()) {
                walk(&path, out);
            }
        } else if !name.contains(".min.") {
            out.push(path);
        }
    }
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples".to_string());
    let settings = Settings::default();

    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.sort();

    // Bucket under a short-lived registry lock: parser/behavior
    // construction and parse calls may re-enter get_registry().
    let mut buckets: BTreeMap<&'static str, Vec<PathBuf>> = BTreeMap::new();
    {
        let registry = get_registry().lock().expect("registry lock");
        for file in &files {
            let Some(ext) = file.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if let Some(def) = registry.get_by_extension(ext) {
                buckets
                    .entry(def.id().as_str())
                    .or_default()
                    .push(file.clone());
            }
        }
    }

    let mut stats: BTreeMap<&'static str, LangStats> = BTreeMap::new();

    for (id, lang_files) in &buckets {
        let (parser, behavior) = {
            let registry = get_registry().lock().expect("registry lock");
            let Some(def) = registry.iter_all().find(|d| d.id().as_str() == *id) else {
                continue;
            };
            match def.create_parser(&settings) {
                Ok(p) => (p, def.create_behavior()),
                Err(e) => {
                    eprintln!("skip {id}: parser creation failed: {e}");
                    continue;
                }
            }
        };
        let mut parser = parser;
        let self_aliases = behavior.self_receiver_aliases();
        let separator = behavior.module_separator();

        for file in lang_files {
            eprintln!("census: {id} {}", file.display());
            let Ok(code) = std::fs::read_to_string(file) else {
                continue;
            };
            let s = stats.entry(id).or_default();
            s.files += 1;

            let mut counter = SymbolCounter::new();
            let file_id = FileId::new(1).expect("nonzero");
            for sym in parser.parse(&code, file_id, &mut counter) {
                if sym.kind != SymbolKind::Method {
                    continue;
                }
                s.methods += 1;
                match sym.scope_context {
                    Some(ScopeContext::ClassMember {
                        class_name: Some(_),
                    }) => s.m_classmember_named += 1,
                    Some(ScopeContext::ClassMember { class_name: None }) => {
                        s.m_classmember_anon += 1
                    }
                    Some(_) => s.m_scope_other += 1,
                    None => s.m_scope_none += 1,
                }
            }

            for call in parser.find_method_calls(&code) {
                s.mc_total += 1;
                if call.caller.is_empty() {
                    s.mc_caller_empty += 1;
                }
                match call.receiver.as_deref() {
                    Some(r) if self_aliases.contains(&r) => s.mc_recv_self += 1,
                    Some(_) => s.mc_recv_named += 1,
                    None => s.mc_recv_none += 1,
                }
                if call.is_static {
                    s.mc_static += 1;
                }
                if call.caller_range.is_none() {
                    s.mc_caller_range_none += 1;
                }
            }

            for (_, target, _) in parser.find_calls(&code) {
                s.pc_total += 1;
                if target.contains(separator) || target.contains('.') || target.contains(':') {
                    s.pc_qualified += 1;
                }
            }

            s.defines += parser.find_defines(&code).len();
            s.extends += parser.find_extends(&code).len();
            s.implements += parser.find_implementations(&code).len();
        }
    }

    println!(
        "lang\tfiles\tmc_total\tmc_caller_empty\tmc_recv_self\tmc_recv_named\tmc_recv_none\tmc_static\tmc_caller_range_none\tpc_total\tpc_qualified\tmethods\tm_classmember_named\tm_classmember_anon\tm_scope_other\tm_scope_none\tdefines\textends\timplements"
    );
    for (id, s) in &stats {
        println!(
            "{id}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            s.files,
            s.mc_total,
            s.mc_caller_empty,
            s.mc_recv_self,
            s.mc_recv_named,
            s.mc_recv_none,
            s.mc_static,
            s.mc_caller_range_none,
            s.pc_total,
            s.pc_qualified,
            s.methods,
            s.m_classmember_named,
            s.m_classmember_anon,
            s.m_scope_other,
            s.m_scope_none,
            s.defines,
            s.extends,
            s.implements,
        );
    }
}
