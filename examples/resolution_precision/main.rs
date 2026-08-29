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
//! - class-match: the receiver's binding type names the type that owns
//!   the target.
//! - inherited: the owning type sits on the binding type's supply chain
//!   (extends / implements / php trait use / kotlin delegation / go
//!   struct embedding).
//! - implementor: the reverse hop — the binding type is a supertype of
//!   the owning type, so the resolver picked one implementation of an
//!   abstractly-typed receiver.
//! - mismatch: binding and owner disagree and no chain hop connects
//!   them either way. These rows print in full — they are the finding.
//! - unverifiable: no binding for the receiver, or no owning type at
//!   the target. Reported per reason, because a clean mismatch column
//!   under low coverage is instrument failure, not a verdict.
//!
//! JS-family runs on the original text heuristics (`js`); php, java,
//! kotlin and go run on tree-sitter (`ast`). Not part of the product
//! surface.
//!
//! Usage: resolution_precision <edge-dump-file> <corpus-root>

mod ast;
mod js;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Lang {
    Js,
    Php,
    Java,
    Kotlin,
    Go,
}

impl Lang {
    fn label(self) -> &'static str {
        match self {
            Lang::Js => "js",
            Lang::Php => "php",
            Lang::Java => "java",
            Lang::Kotlin => "kotlin",
            Lang::Go => "go",
        }
    }
}

fn lang_of(path: &str) -> Option<Lang> {
    match Path::new(path).extension().and_then(|e| e.to_str())? {
        "js" | "mjs" | "cjs" | "jsx" => Some(Lang::Js),
        "php" => Some(Lang::Php),
        "java" => Some(Lang::Java),
        "kt" | "kts" => Some(Lang::Kotlin),
        "go" => Some(Lang::Go),
        _ => None,
    }
}

pub fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

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

/// Name-keyed supertype sets, per language.
///
/// Same-named types across files union their parents rather than
/// overwriting. The bias is toward `inherited` over `mismatch`, which is
/// the safe direction: a false mismatch at scale destroys the instrument's
/// credibility, and same-name ambiguity is itself the population under
/// audit.
type Supply = BTreeMap<Lang, BTreeMap<String, BTreeSet<String>>>;

fn collect_supply(corpus_root: &Path) -> Supply {
    let mut supply: Supply = BTreeMap::new();
    for entry in walkdir::WalkDir::new(corpus_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let Some(lang) = path.to_str().and_then(lang_of) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let map = supply.entry(lang).or_default();
        let mut emit = |child: String, parent: String| {
            map.entry(child).or_default().insert(parent);
        };
        match lang {
            Lang::Js => js::supply_pairs(&source, emit),
            _ => {
                let Some(parsed) = ast::parse_standalone(lang, source) else {
                    continue;
                };
                ast::supply_pairs(lang, &parsed, &mut emit);
            }
        }
    }
    supply
}

/// Whether `to` is reachable from `from` over supply hops.
fn reaches(supply: &BTreeMap<String, BTreeSet<String>>, from: &str, to: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([from.to_string()]);
    while let Some(current) = queue.pop_front() {
        if !seen.insert(current.clone()) {
            continue;
        }
        let Some(parents) = supply.get(&current) else {
            continue;
        };
        for parent in parents {
            if parent == to {
                return true;
            }
            queue.push_back(parent.clone());
        }
    }
    false
}

#[derive(Default)]
struct Tally {
    verdicts: BTreeMap<&'static str, usize>,
    reasons: BTreeMap<&'static str, usize>,
}

impl Tally {
    fn classified(&self) -> usize {
        ["class-match", "inherited", "implementor", "mismatch"]
            .iter()
            .filter_map(|v| self.verdicts.get(v))
            .sum()
    }

    fn total(&self) -> usize {
        self.verdicts.values().sum()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: resolution_precision <edge-dump-file> <corpus-root>");
        std::process::exit(1);
    }
    let dump = std::fs::read_to_string(&args[1]).expect("edge-dump file readable");
    let corpus_root = Path::new(&args[2]);

    let supply = collect_supply(corpus_root);
    for (lang, map) in &supply {
        eprintln!("supply map {}: {} types", lang.label(), map.len());
    }
    let empty: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut ast = ast::Ast::default();
    let mut js_files: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut tallies: BTreeMap<Lang, Tally> = BTreeMap::new();
    let mut skipped_non_receiver = 0usize;
    let mut skipped_unsupported = 0usize;
    let mut skipped_non_variable = 0usize;

    for row in dump.lines() {
        let fields: Vec<&str> = row.split('\t').collect();
        if fields.len() < 5 || fields[0] != "Calls" {
            skipped_non_receiver += 1;
            continue;
        }
        let receiver = fields[4].trim();
        if receiver.is_empty()
            || matches!(receiver, "self" | "this" | "$this")
            || !receiver.chars().all(is_ident)
        {
            skipped_non_receiver += 1;
            continue;
        }
        let Ok(call_line) = fields[3].trim().parse::<usize>() else {
            skipped_non_receiver += 1;
            continue;
        };
        let (Some((from_ref, _)), Some((to_ref, _))) =
            (parse_symbol_ref(fields[1]), parse_symbol_ref(fields[2]))
        else {
            skipped_non_receiver += 1;
            continue;
        };
        let (Some(lang), to_lang) = (lang_of(from_ref.path), lang_of(to_ref.path)) else {
            skipped_unsupported += 1;
            continue;
        };
        if to_lang != Some(lang) {
            skipped_unsupported += 1;
            continue;
        }
        // A php variable receiver always carries the sigil, so a bare one
        // is `parent`/`static`/`self` or a class name — a static-dispatch
        // arm this tool does not audit. Without the gate, `parent::m()`
        // silently binds a `$parent` parameter and reports a mismatch.
        if lang == Lang::Php && !receiver.starts_with('$') {
            skipped_non_variable += 1;
            continue;
        }

        let extracted = if lang == Lang::Js {
            let caller = js_source(&mut js_files, from_ref.path);
            let binding = caller.and_then(|src| {
                let lines: Vec<&str> = src.lines().collect();
                js::backward_binding(&lines, receiver, call_line)
            });
            let target = js_source(&mut js_files, to_ref.path);
            let enclosing = target.and_then(|src| js::enclosing_class(src, to_ref.line));
            (binding, enclosing)
        } else {
            let binding = ast
                .file(lang, from_ref.path)
                .and_then(|parsed| ast::binding_type(lang, parsed, receiver, call_line));
            let enclosing = ast
                .file(lang, to_ref.path)
                .and_then(|parsed| ast::enclosing_type(lang, parsed, to_ref.line));
            (binding, enclosing)
        };

        let tally = tallies.entry(lang).or_default();
        let verdict = match &extracted {
            (Some(binding), Some(enclosing)) => {
                let map = supply.get(&lang).unwrap_or(&empty);
                if binding == enclosing {
                    "class-match"
                } else if reaches(map, binding, enclosing) {
                    "inherited"
                } else if reaches(map, enclosing, binding) {
                    "implementor"
                } else {
                    "mismatch"
                }
            }
            (None, Some(_)) => {
                *tally.reasons.entry("no-binding").or_default() += 1;
                "unverifiable"
            }
            (Some(_), None) => {
                *tally.reasons.entry("no-enclosing").or_default() += 1;
                "unverifiable"
            }
            (None, None) => {
                *tally.reasons.entry("neither").or_default() += 1;
                "unverifiable"
            }
        };
        *tally.verdicts.entry(verdict).or_default() += 1;
        if verdict == "mismatch" {
            let (binding, enclosing) = (
                extracted.0.as_deref().unwrap_or(""),
                extracted.1.as_deref().unwrap_or(""),
            );
            println!("MISMATCH\t{binding}\t{enclosing}\t{row}");
        }
    }

    for (lang, tally) in &tallies {
        let (total, classified) = (tally.total(), tally.classified());
        let coverage = if total == 0 {
            0.0
        } else {
            100.0 * classified as f64 / total as f64
        };
        eprintln!("--- {} ---", lang.label());
        for (verdict, count) in &tally.verdicts {
            eprintln!("{verdict}\t{count}");
        }
        for (reason, count) in &tally.reasons {
            eprintln!("  unverifiable:{reason}\t{count}");
        }
        eprintln!("coverage\t{classified}/{total}\t{coverage:.1}%");
        if coverage < 50.0 {
            eprintln!(
                "COVERAGE-LOW\t{} verdicts do not describe this population",
                lang.label()
            );
        }
    }
    eprintln!("skipped non-receiver/self rows\t{skipped_non_receiver}");
    eprintln!("skipped unsupported/cross-language rows\t{skipped_unsupported}");
    eprintln!("skipped static/keyword receiver rows\t{skipped_non_variable}");
}

fn js_source<'a>(cache: &'a mut BTreeMap<String, Option<String>>, path: &str) -> Option<&'a str> {
    cache
        .entry(path.to_string())
        .or_insert_with(|| std::fs::read_to_string(path).ok())
        .as_deref()
}
