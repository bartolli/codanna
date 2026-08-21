//! Scratch audit tool: dump every relationship in a codanna Tantivy index
//! as TSV with stable symbol identities (name@file:line/kind), for
//! edge-set diffing between index runs. Not part of the product surface;
//! reads through the same `DocumentIndex` scan as `codanna dump`.
//!
//! Usage: dump_edges <path-to-.codanna/index/tantivy> [--symbols|--dups]

use std::collections::HashMap;

use codanna::Symbol;
use codanna::config::Settings;
use codanna::storage::{DocumentIndex, StorageError};

/// (name, file_path, line, kind, module_path) -- stored line (0-indexed).
type SymbolIdentity = (String, String, u64, String, String);

fn identity(symbol: &Symbol) -> SymbolIdentity {
    (
        symbol.name.to_string(),
        symbol.file_path.to_string(),
        u64::from(symbol.range.start_line),
        format!("{:?}", symbol.kind),
        symbol
            .module_path
            .as_deref()
            .unwrap_or_default()
            .to_string(),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let index_dir = args
        .next()
        .expect("usage: dump_edges <tantivy-dir> [--symbols|--dups]");
    let mode = args.next();
    let dump_symbols = mode.as_deref() == Some("--symbols");
    let dump_dups = mode.as_deref() == Some("--dups");

    let settings = Settings::default();
    let index = DocumentIndex::new(&index_dir, &settings)?;

    // Pass 1: symbol_id -> identity
    let mut symbols: HashMap<u64, SymbolIdentity> = HashMap::new();
    let mut all_rows: HashMap<u64, Vec<SymbolIdentity>> = HashMap::new();
    let mut dup_ids = 0usize;
    index.for_each_symbol::<StorageError>(|symbol| {
        let id = u64::from(symbol.id.value());
        let ident = identity(&symbol);
        if dump_dups {
            all_rows.entry(id).or_default().push(ident.clone());
        }
        if symbols.insert(id, ident).is_some() {
            dup_ids += 1;
        }
        Ok(())
    })?;
    eprintln!("symbols: {} (duplicate ids: {dup_ids})", symbols.len());

    if dump_dups {
        let mut ids: Vec<u64> = all_rows
            .iter()
            .filter(|(_, rows)| rows.len() > 1)
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        for id in ids {
            let mut rows: Vec<String> = all_rows[&id]
                .iter()
                .map(|(n, fp, ln, k, mp)| format!("{id}\t{n}\t{fp}\t{ln}\t{k}\t{mp}"))
                .collect();
            rows.sort();
            for r in &rows {
                println!("{r}");
            }
        }
        return Ok(());
    }

    if dump_symbols {
        let mut rows: Vec<String> = symbols
            .values()
            .map(|(n, fp, ln, k, mp)| format!("{n}\t{fp}\t{ln}\t{k}\t{mp}"))
            .collect();
        rows.sort();
        for r in &rows {
            println!("{r}");
        }
        return Ok(());
    }

    // Pass 2: relationships
    let mut rows: Vec<String> = Vec::new();
    let mut orphans = 0usize;
    index.for_each_relationship::<StorageError>(|from, to, rel| {
        let rk = format!("{:?}", rel.kind);
        let meta = rel.metadata.as_ref();
        let line = meta
            .and_then(|m| m.line)
            .map(|l| l.to_string())
            .unwrap_or_default();
        let recv = meta.and_then(|m| m.receiver.as_deref()).unwrap_or_default();
        let is_static = if meta.is_some_and(|m| m.static_call) {
            "1"
        } else {
            ""
        };
        let fmt = |id: u64| -> String {
            match symbols.get(&id) {
                Some((n, fp, ln, k, _)) => format!("{n}@{fp}:{ln}/{k}"),
                None => format!("<orphan:Some({id})>"),
            }
        };
        let from_s = fmt(u64::from(from.value()));
        let to_s = fmt(u64::from(to.value()));
        if from_s.starts_with("<orphan") || to_s.starts_with("<orphan") {
            orphans += 1;
        }
        rows.push(format!(
            "{rk}\t{from_s}\t{to_s}\t{line}\t{recv}\t{is_static}"
        ));
        Ok(())
    })?;
    eprintln!(
        "relationships: {} (orphan endpoints: {orphans})",
        rows.len()
    );
    rows.sort();
    for r in &rows {
        println!("{r}");
    }
    Ok(())
}
