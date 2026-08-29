//! JS-family extraction: the original text heuristics, unchanged.
//!
//! `backward_binding` and `enclosing_class` are verbatim against the
//! three.js gold run (1470 class-match / 87 inherited / 117
//! unverifiable / 0 mismatch on the `501b98f` slice), so the
//! tree-sitter languages land as an addition rather than a rewrite of a
//! verified path. Two shared changes do reach this path: the supply map
//! unions same-named parents instead of last-writer-wins, and the
//! `implementor` verdict splits one case out of `mismatch`. Both can
//! only move rows out of `mismatch`, and the gold had none, so it
//! reproduces.

use crate::is_ident;

/// Last `<receiver> = new Type(...)` capture at or above `before_line`
/// (0-indexed, position-aware last-binding-wins like the resolver).
/// Dotted constructors yield the tail segment (source truth for the
/// class, independent of the emitter's namespace-head divergence).
pub fn backward_binding(lines: &[&str], receiver: &str, before_line: usize) -> Option<String> {
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
pub fn enclosing_class(source: &str, target_line: usize) -> Option<String> {
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

/// `class X extends Y` pairs declared in one JS source.
pub fn supply_pairs(source: &str, mut emit: impl FnMut(String, String)) {
    for line in source.lines() {
        let mut rest = line;
        while let Some(pos) = rest.find("class ") {
            let after = rest[pos + 6..].trim_start();
            let child: String = after.chars().take_while(|&c| is_ident(c)).collect();
            let tail = after[child.len()..].trim_start();
            if !child.is_empty() {
                if let Some(parent_expr) = tail.strip_prefix("extends ") {
                    let chain: String = parent_expr
                        .trim_start()
                        .chars()
                        .take_while(|&c| is_ident(c) || c == '.')
                        .collect();
                    let parent = chain.rsplit('.').next().unwrap_or(&chain);
                    if !parent.is_empty() {
                        emit(child, parent.to_string());
                    }
                }
            }
            rest = &rest[pos + 6..];
        }
    }
}
