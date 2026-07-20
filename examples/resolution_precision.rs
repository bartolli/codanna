//! Scratch audit tool: per-edge precision verdicts for receiver-typed
//! call edges against corpus source truth.
//!
//! Promoted from the session-scratch classifier that caught the
//! three.js multi-copy mis-report class (468/1731 added edges
//! class-wrong) which 17/17 and 7-edge samples had missed. Takes an
//! edge-dump file (full `dump_edges` output or any line subset, e.g. a
//! `comm` diff of two dumps) plus the corpus root, and classifies every
//! receiver-carrying Calls row:
//!
//! - class-match: backward `new Type()` binding for the receiver names
//!   the class that encloses the target (brace-depth walk).
//! - inherited: the enclosing class sits on the binding type's extends
//!   chain (corpus-derived `class X extends Y` map).
//! - mismatch: binding and enclosing class disagree and no chain hop
//!   connects them. These rows print in full — they are the finding.
//! - unverifiable: no backward binding for the receiver, no enclosing
//!   class at the target, or a non-bare receiver.
//!
//! JS-family text heuristics (matches the scratch semantics; per-language
//! extraction can grow). Not part of the product surface.
//!
//! Usage: resolution_precision <edge-dump-file> <corpus-root>

use std::collections::BTreeMap;
use std::path::Path;

const JS_EXTS: [&str; 4] = ["js", "mjs", "cjs", "jsx"];

struct EdgeRef<'a> {
    path: &'a str,
    line: usize,
}

fn parse_symbol_ref(field: &str) -> Option<(EdgeRef<'_>, &str)> {
    let (rest, kind) = field.rsplit_once('/')?;
    let (name_path, line) = rest.rsplit_once(':')?;
    let (_name, path) = name_path.split_once('@')?;
    Some((
        EdgeRef {
            path,
            line: line.parse().ok()?,
        },
        kind,
    ))
}

fn is_js_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| JS_EXTS.contains(&e))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Last `<receiver> = new Type(...)` capture at or above `before_line`
/// (0-indexed, position-aware last-binding-wins like the resolver).
/// Dotted constructors yield the tail segment (source truth for the
/// class, independent of the emitter's namespace-head divergence).
fn backward_binding(lines: &[&str], receiver: &str, before_line: usize) -> Option<String> {
    let upper = before_line.min(lines.len().saturating_sub(1));
    for idx in (0..=upper).rev() {
        let line = lines[idx];
        let mut search_from = 0;
        let mut last: Option<String> = None;
        while let Some(pos) = line[search_from..].find(receiver) {
            let abs = search_from + pos;
            let before_ok = abs == 0 || !is_ident(line[..abs].chars().next_back().unwrap());
            let after = &line[abs + receiver.len()..];
            let after_ok = !after.starts_with(|c: char| is_ident(c));
            search_from = abs + receiver.len().max(1);
            if !(before_ok && after_ok) {
                continue;
            }
            let rest = after.trim_start();
            let rest = match rest.strip_prefix('=') {
                Some(r) if !r.starts_with('=') => r.trim_start(),
                _ => continue,
            };
            let Some(expr) = rest.strip_prefix("new ") else {
                continue;
            };
            let chain: String = expr
                .chars()
                .take_while(|&c| is_ident(c) || c == '.')
                .collect();
            let tail = chain.rsplit('.').next().unwrap_or(&chain);
            if !tail.is_empty() {
                last = Some(tail.to_string());
            }
        }
        if last.is_some() {
            return last;
        }
    }
    None
}

/// Innermost `class X` whose brace span contains the target line
/// (0-indexed). Text heuristic: char-level depth tracking, class name
/// captured when its opening brace arrives.
fn enclosing_class(source: &str, target_line: usize) -> Option<String> {
    let mut depth: i64 = 0;
    let mut stack: Vec<(String, i64)> = Vec::new();
    let mut pending: Option<String> = None;
    for (line_no, line) in source.lines().enumerate() {
        if line_no > target_line {
            break;
        }
        let mut rest = line;
        while let Some(pos) = rest.find("class ") {
            let boundary_ok = pos == 0 || !is_ident(rest[..pos].chars().next_back().unwrap());
            let name: String = rest[pos + 6..]
                .trim_start()
                .chars()
                .take_while(|&c| is_ident(c))
                .collect();
            if boundary_ok && !name.is_empty() {
                pending = Some(name);
            }
            rest = &rest[pos + 6..];
        }
        for c in line.chars() {
            match c {
                '{' => {
                    depth += 1;
                    if let Some(name) = pending.take() {
                        stack.push((name, depth));
                    }
                }
                '}' => {
                    depth -= 1;
                    while stack.last().is_some_and(|&(_, d)| d > depth) {
                        stack.pop();
                    }
                }
                _ => {}
            }
        }
        if line_no == target_line {
            return stack.last().map(|(name, _)| name.clone());
        }
    }
    stack.last().map(|(name, _)| name.clone())
}

fn collect_extends(corpus_root: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for entry in walkdir::WalkDir::new(corpus_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| JS_EXTS.contains(&e))
        {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in source.lines() {
            let mut rest = line;
            while let Some(pos) = rest.find("class ") {
                let after = rest[pos + 6..].trim_start();
                let child: String = after.chars().take_while(|&c| is_ident(c)).collect();
                let tail = &after[child.len()..];
                let tail = tail.trim_start();
                if !child.is_empty() {
                    if let Some(parent_expr) = tail.strip_prefix("extends ") {
                        let chain: String = parent_expr
                            .trim_start()
                            .chars()
                            .take_while(|&c| is_ident(c) || c == '.')
                            .collect();
                        let parent = chain.rsplit('.').next().unwrap_or(&chain);
                        if !parent.is_empty() {
                            map.insert(child.clone(), parent.to_string());
                        }
                    }
                }
                rest = &rest[pos + 6..];
            }
        }
    }
    map
}

fn on_chain(extends: &BTreeMap<String, String>, from: &str, to: &str) -> bool {
    let mut current = from.to_string();
    for _ in 0..10 {
        match extends.get(&current) {
            Some(parent) => {
                if parent == to {
                    return true;
                }
                current = parent.clone();
            }
            None => return false,
        }
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: resolution_precision <edge-dump-file> <corpus-root>");
        std::process::exit(1);
    }
    let dump = std::fs::read_to_string(&args[1]).expect("edge-dump file readable");
    let corpus_root = Path::new(&args[2]);

    let extends = collect_extends(corpus_root);
    eprintln!("extends map: {} entries", extends.len());

    let mut file_cache: BTreeMap<String, String> = BTreeMap::new();
    let read_file = |path: &str, cache: &mut BTreeMap<String, String>| -> Option<String> {
        if !cache.contains_key(path) {
            cache.insert(path.to_string(), std::fs::read_to_string(path).ok()?);
        }
        cache.get(path).cloned()
    };

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut skipped_non_receiver = 0usize;
    let mut skipped_non_js = 0usize;

    for row in dump.lines() {
        let fields: Vec<&str> = row.split('\t').collect();
        if fields.len() < 5 || fields[0] != "Calls" {
            skipped_non_receiver += 1;
            continue;
        }
        let receiver = fields[4].trim();
        if receiver.is_empty()
            || receiver == "self"
            || receiver == "this"
            || !receiver.chars().all(is_ident)
        {
            skipped_non_receiver += 1;
            continue;
        }
        let Some(call_line) = fields[3].trim().parse::<usize>().ok() else {
            skipped_non_receiver += 1;
            continue;
        };
        let (Some((from_ref, _)), Some((to_ref, _))) =
            (parse_symbol_ref(fields[1]), parse_symbol_ref(fields[2]))
        else {
            skipped_non_receiver += 1;
            continue;
        };
        if !is_js_file(from_ref.path) || !is_js_file(to_ref.path) {
            skipped_non_js += 1;
            continue;
        }

        let verdict = (|| {
            let caller_src = read_file(from_ref.path, &mut file_cache)?;
            let caller_lines: Vec<&str> = caller_src.lines().collect();
            let binding = backward_binding(&caller_lines, receiver, call_line);
            let target_src = read_file(to_ref.path, &mut file_cache)?;
            let enclosing = enclosing_class(&target_src, to_ref.line);
            Some(match (binding, enclosing) {
                (Some(b), Some(e)) if b == e => "class-match",
                (Some(b), Some(e)) if on_chain(&extends, &b, &e) => "inherited",
                (Some(_), Some(_)) => "mismatch",
                _ => "unverifiable",
            })
        })()
        .unwrap_or("unverifiable");

        *counts.entry(verdict).or_insert(0) += 1;
        if verdict == "mismatch" {
            println!("MISMATCH\t{row}");
        }
    }

    eprintln!("--- verdicts ---");
    for (verdict, count) in &counts {
        eprintln!("{verdict}\t{count}");
    }
    eprintln!("skipped non-receiver/self rows\t{skipped_non_receiver}");
    eprintln!("skipped non-js rows\t{skipped_non_js}");
}
