//! Scratch audit tool: dump every relationship in a codanna Tantivy index
//! as TSV with stable symbol identities (name@file:line/kind), for
//! edge-set diffing between index runs. Not part of the product surface.
//!
//! Usage: dump_edges <path-to-.codanna/index/tantivy> [--symbols]

use std::collections::HashMap;
use std::path::Path;

use tantivy::collector::DocSetCollector;
use tantivy::query::TermQuery;
use tantivy::schema::{IndexRecordOption, Term, Value};
use tantivy::{Index, TantivyDocument};

fn text_field(doc: &TantivyDocument, field: tantivy::schema::Field) -> Option<String> {
    doc.get_first(field)
        .and_then(|v| v.as_str().map(str::to_owned))
}

fn u64_field(doc: &TantivyDocument, field: tantivy::schema::Field) -> Option<u64> {
    doc.get_first(field).and_then(|v| v.as_u64())
}

fn main() -> tantivy::Result<()> {
    let mut args = std::env::args().skip(1);
    let index_dir = args
        .next()
        .expect("usage: dump_edges <tantivy-dir> [--symbols]");
    let dump_symbols = args.next().as_deref() == Some("--symbols");

    let index = Index::open_in_dir(Path::new(&index_dir))?;
    let schema = index.schema();
    let reader = index.reader()?;
    let searcher = reader.searcher();

    let f = |name: &str| {
        schema
            .get_field(name)
            .unwrap_or_else(|_| panic!("schema missing field {name}"))
    };
    let doc_type = f("doc_type");
    let symbol_id = f("symbol_id");
    let name = f("name");
    let file_path = f("file_path");
    let line_number = f("line_number");
    let kind = f("kind");
    let module_path = f("module_path");
    let from_symbol_id = f("from_symbol_id");
    let to_symbol_id = f("to_symbol_id");
    let relation_kind = f("relation_kind");
    let relation_line = f("relation_line");
    let relation_receiver = f("relation_receiver");
    let relation_static_call = f("relation_static_call");

    // Pass 1: symbol_id -> identity
    let sym_query = TermQuery::new(
        Term::from_field_text(doc_type, "symbol"),
        IndexRecordOption::Basic,
    );
    let sym_docs = searcher.search(&sym_query, &DocSetCollector)?;
    let mut symbols: HashMap<u64, (String, String, u64, String, String)> = HashMap::new();
    let mut dup_ids = 0usize;
    for addr in sym_docs {
        let doc: TantivyDocument = searcher.doc(addr)?;
        let Some(id) = u64_field(&doc, symbol_id) else {
            continue;
        };
        let ident = (
            text_field(&doc, name).unwrap_or_default(),
            text_field(&doc, file_path).unwrap_or_default(),
            u64_field(&doc, line_number).unwrap_or(0),
            text_field(&doc, kind).unwrap_or_default(),
            text_field(&doc, module_path).unwrap_or_default(),
        );
        if symbols.insert(id, ident).is_some() {
            dup_ids += 1;
        }
    }
    eprintln!("symbols: {} (duplicate ids: {dup_ids})", symbols.len());

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
    let rel_query = TermQuery::new(
        Term::from_field_text(doc_type, "relationship"),
        IndexRecordOption::Basic,
    );
    let rel_docs = searcher.search(&rel_query, &DocSetCollector)?;
    let mut rows: Vec<String> = Vec::new();
    let mut orphans = 0usize;
    for addr in rel_docs {
        let doc: TantivyDocument = searcher.doc(addr)?;
        let rk = text_field(&doc, relation_kind).unwrap_or_default();
        let from = u64_field(&doc, from_symbol_id);
        let to = u64_field(&doc, to_symbol_id);
        let line = u64_field(&doc, relation_line)
            .map(|l| l.to_string())
            .unwrap_or_default();
        let recv = text_field(&doc, relation_receiver).unwrap_or_default();
        let is_static = u64_field(&doc, relation_static_call)
            .map(|v| v.to_string())
            .unwrap_or_default();
        let fmt = |id: Option<u64>| -> String {
            match id.and_then(|i| symbols.get(&i)) {
                Some((n, fp, ln, k, _)) => format!("{n}@{fp}:{ln}/{k}"),
                None => {
                    format!("<orphan:{id:?}>")
                }
            }
        };
        let from_s = fmt(from);
        let to_s = fmt(to);
        if from_s.starts_with("<orphan") || to_s.starts_with("<orphan") {
            orphans += 1;
        }
        rows.push(format!(
            "{rk}\t{from_s}\t{to_s}\t{line}\t{recv}\t{is_static}"
        ));
    }
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
