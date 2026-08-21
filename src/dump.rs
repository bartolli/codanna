//! `codanna dump`: stream the resolved graph as JSON Lines in the envelope's
//! streaming mode (`begin`, one `result` per item, terminal `summary`).

use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;

use crate::indexing::IndexFacade;
use crate::io::envelope::{EntityType, Envelope};
use crate::relationship::RelationKind;
use crate::storage::StorageError;
use crate::{Symbol, SymbolKind};

/// Index provenance carried into the summary row.
#[derive(Debug, Clone, Default)]
pub struct DumpStamp {
    pub emission_version: Option<u32>,
    pub builder_commit: Option<String>,
}

/// Payload of the terminal `summary` envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DumpSummary {
    pub symbols: usize,
    pub relationships: usize,
    pub orphan_edges_dropped: usize,
    pub duplicate_symbol_ids: usize,
    pub emission_version: Option<u32>,
    pub builder_commit: Option<String>,
}

/// Which result rows a dump emits. `begin` and `summary` are always emitted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Rows {
    #[default]
    All,
    Symbols,
    Relationships,
}

/// Emission filter. The identity map is always built from the full symbol
/// scan; filters gate which rows are written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DumpFilter {
    pub rows: Rows,
    /// Emit only relationship rows of this kind.
    pub relation: Option<RelationKind>,
    /// Emit only symbol rows of this kind; relationship rows are unaffected.
    pub kind: Option<SymbolKind>,
}

/// Relation kinds persisted by the indexer, i.e. the values `--relation`
/// accepts (case-insensitive); directional twins are never stored.
pub const PERSISTED_RELATION_KINDS: [(&str, RelationKind); 5] = [
    ("calls", RelationKind::Calls),
    ("defines", RelationKind::Defines),
    ("uses", RelationKind::Uses),
    ("implements", RelationKind::Implements),
    ("extends", RelationKind::Extends),
];

/// Parse a `--relation` value. Unknown names list the accepted set.
pub fn parse_relation_kind(text: &str) -> Result<RelationKind, String> {
    let lower = text.to_lowercase();
    PERSISTED_RELATION_KINDS
        .iter()
        .find(|(name, _)| *name == lower)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| {
            let accepted: Vec<&str> = PERSISTED_RELATION_KINDS.iter().map(|(n, _)| *n).collect();
            format!(
                "unknown relation kind '{text}'; accepted: {}",
                accepted.join(", ")
            )
        })
}

/// Parse a `--kind` value through the shared `SymbolKind` vocabulary.
pub fn parse_symbol_kind(text: &str) -> Result<SymbolKind, String> {
    text.parse::<SymbolKind>().map_err(|e| e.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    #[error("index read failed: {0}")]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

struct Identity {
    name: Box<str>,
    kind: SymbolKind,
    file_path: Box<str>,
    start_line: u32,
}

#[derive(Serialize)]
struct Endpoint<'a> {
    id: u32,
    name: &'a str,
    kind: SymbolKind,
    file_path: &'a str,
    /// 1-indexed, like every scalar line field at the JSON boundary.
    line: u32,
}

#[derive(Serialize)]
struct EdgeMetadata<'a> {
    line: Option<u32>,
    column: Option<u16>,
    receiver: Option<&'a str>,
    static_call: bool,
    context: Option<&'a str>,
}

#[derive(Serialize)]
struct RelationshipItem<'a> {
    relation: RelationKind,
    from: Endpoint<'a>,
    to: Endpoint<'a>,
    metadata: Option<EdgeMetadata<'a>>,
}

fn endpoint(id: u32, identity: &Identity) -> Endpoint<'_> {
    Endpoint {
        id,
        name: &identity.name,
        kind: identity.kind,
        file_path: &identity.file_path,
        line: identity.start_line + 1,
    }
}

fn write_line<W: Write, T: Serialize>(
    out: &mut W,
    envelope: &Envelope<T>,
) -> Result<(), DumpError> {
    writeln!(out, "{}", envelope.to_json_compact()?)?;
    Ok(())
}

/// Write the whole graph: `begin`, one `result` per symbol, one `result`
/// per relationship, terminal `summary`. Symbol items stream; relationship
/// endpoints resolve through an in-memory `id -> identity` map built on
/// the symbol pass. Edges with an endpoint absent from that map are dropped
/// and counted. Ordering within each pass is unspecified.
pub fn write_dump<W: Write>(
    facade: &IndexFacade,
    stamp: DumpStamp,
    filter: &DumpFilter,
    mut out: W,
) -> Result<DumpSummary, DumpError> {
    let begin: Envelope<()> = Envelope::begin("dump").with_entity_type(EntityType::Graph);
    write_line(&mut out, &begin)?;

    let mut summary = DumpSummary {
        emission_version: stamp.emission_version,
        builder_commit: stamp.builder_commit,
        ..DumpSummary::default()
    };
    let mut identities: HashMap<u32, Identity> = HashMap::new();

    let emit_symbols = filter.rows != Rows::Relationships;
    let emit_relationships = filter.rows != Rows::Symbols;

    facade.for_each_symbol(|symbol: Symbol| -> Result<(), DumpError> {
        if emit_symbols && filter.kind.is_none_or(|kind| kind == symbol.kind) {
            let item = Envelope::success(&symbol)
                .with_entity_type(EntityType::Symbol)
                .with_message("");
            write_line(&mut out, &item)?;
            summary.symbols += 1;
        }
        let identity = Identity {
            name: Box::from(&*symbol.name),
            kind: symbol.kind,
            file_path: symbol.file_path.clone(),
            start_line: symbol.range.start_line,
        };
        if identities.insert(symbol.id.value(), identity).is_some() {
            summary.duplicate_symbol_ids += 1;
        }
        Ok(())
    })?;

    if emit_relationships {
        facade.for_each_relationship(|from, to, relationship| -> Result<(), DumpError> {
            if filter
                .relation
                .is_some_and(|kind| kind != relationship.kind)
            {
                return Ok(());
            }
            let (Some(from_identity), Some(to_identity)) =
                (identities.get(&from.value()), identities.get(&to.value()))
            else {
                summary.orphan_edges_dropped += 1;
                return Ok(());
            };
            let metadata = relationship.metadata.as_ref().map(|m| EdgeMetadata {
                line: m.line.map(|l| l + 1),
                column: m.column,
                receiver: m.receiver.as_deref(),
                static_call: m.static_call,
                context: m.context.as_deref(),
            });
            let item = RelationshipItem {
                relation: relationship.kind,
                from: endpoint(from.value(), from_identity),
                to: endpoint(to.value(), to_identity),
                metadata,
            };
            let envelope = Envelope::success(item)
                .with_entity_type(EntityType::Relationship)
                .with_message("");
            write_line(&mut out, &envelope)?;
            summary.relationships += 1;
            Ok(())
        })?;
    }

    let count = summary.symbols + summary.relationships;
    let end = Envelope::summary(&summary)
        .with_entity_type(EntityType::Graph)
        .with_count(count)
        .with_message("dump complete");
    write_line(&mut out, &end)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::indexing::IndexFacade;
    use crate::io::envelope::{EntityType, Envelope, MessageType};
    use std::sync::Arc;

    fn indexed_fixture(source: &str) -> (tempfile::TempDir, IndexFacade) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fixture");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("lib.rs"), source).unwrap();
        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let mut facade = IndexFacade::new(Arc::new(settings)).unwrap();
        facade.index_directory(&root, false).unwrap();
        (dir, facade)
    }

    fn parse_lines(buf: &[u8]) -> Vec<Envelope<serde_json::Value>> {
        std::str::from_utf8(buf)
            .unwrap()
            .lines()
            .map(|l| {
                serde_json::from_str(l).unwrap_or_else(|e| panic!("line not an Envelope: {e}: {l}"))
            })
            .collect()
    }

    #[test]
    fn dump_streams_begin_items_summary_with_matching_counts() {
        let (_dir, facade) = indexed_fixture("fn callee() {}\n\nfn caller() {\n    callee();\n}\n");
        assert!(
            facade.relationship_count() >= 1,
            "fixture must carry a Calls edge"
        );

        let mut out = Vec::new();
        let summary = write_dump(
            &facade,
            DumpStamp::default(),
            &DumpFilter::default(),
            &mut out,
        )
        .unwrap();
        let lines = parse_lines(&out);

        let first = lines.first().expect("begin line");
        assert_eq!(first.message_type, MessageType::Begin);
        assert_eq!(first.meta.entity_type, Some(EntityType::Graph));
        let last = lines.last().expect("summary line");
        assert_eq!(last.message_type, MessageType::Summary);
        assert_eq!(last.meta.entity_type, Some(EntityType::Graph));
        assert_eq!(last.exit_code, 0);

        let items = |t: EntityType| {
            lines
                .iter()
                .filter(|e| e.message_type == MessageType::Result && e.meta.entity_type == Some(t))
                .count()
        };
        assert_eq!(items(EntityType::Symbol), facade.symbol_count());
        assert_eq!(items(EntityType::Relationship), facade.relationship_count());
        assert_eq!(
            lines.len(),
            2 + facade.symbol_count() + facade.relationship_count()
        );

        assert_eq!(summary.symbols, facade.symbol_count());
        assert_eq!(summary.relationships, facade.relationship_count());
        let data = last.data.as_ref().expect("summary data");
        assert_eq!(data["symbols"], facade.symbol_count());
        assert_eq!(data["relationships"], facade.relationship_count());
        assert_eq!(
            last.meta.count,
            Some(summary.symbols + summary.relationships)
        );
    }
    #[test]
    fn relationship_items_round_trip_to_the_per_symbol_surface() {
        use std::collections::{HashMap, HashSet};
        let (_dir, facade) = indexed_fixture(
            "fn callee() {}\n\nfn other() {}\n\nfn caller() {\n    callee();\n    other();\n}\n",
        );
        let mut out = Vec::new();
        write_dump(
            &facade,
            DumpStamp::default(),
            &DumpFilter::default(),
            &mut out,
        )
        .unwrap();
        let lines = parse_lines(&out);

        let mut calls_by_from: HashMap<u32, HashSet<(u32, Option<u32>)>> = HashMap::new();
        let mut endpoints: Vec<serde_json::Value> = Vec::new();
        let mut symbol_items: HashMap<u32, serde_json::Value> = HashMap::new();
        for line in &lines {
            if line.message_type != MessageType::Result {
                continue;
            }
            let data = line.data.clone().expect("result data");
            match line.meta.entity_type {
                Some(EntityType::Symbol) => {
                    symbol_items.insert(data["id"].as_u64().unwrap() as u32, data);
                }
                Some(EntityType::Relationship) => {
                    endpoints.push(data["from"].clone());
                    endpoints.push(data["to"].clone());
                    if data["relation"] == "Calls" {
                        let from = data["from"]["id"].as_u64().unwrap() as u32;
                        let to = data["to"]["id"].as_u64().unwrap() as u32;
                        let line = data["metadata"]["line"].as_u64().map(|l| l as u32);
                        calls_by_from.entry(from).or_default().insert((to, line));
                    }
                }
                other => panic!("unexpected result entity {other:?}"),
            }
        }
        assert_eq!(calls_by_from.len(), 1, "one caller in the fixture");

        for (from, got) in &calls_by_from {
            let expected: HashSet<(u32, Option<u32>)> = facade
                .get_called_functions_with_metadata(crate::SymbolId::new(*from).unwrap())
                .into_iter()
                .map(|(callee, meta)| (callee.id.value(), meta.and_then(|m| m.line).map(|l| l + 1)))
                .collect();
            assert_eq!(expected.len(), 2, "caller calls callee and other");
            assert_eq!(
                got, &expected,
                "Calls items for {from} equal retrieve calls"
            );
        }

        for endpoint in &endpoints {
            let id = endpoint["id"].as_u64().unwrap() as u32;
            let symbol = facade
                .get_symbol(crate::SymbolId::new(id).unwrap())
                .expect("endpoint symbol");
            assert_eq!(endpoint["name"], &*symbol.name);
            assert_eq!(endpoint["file_path"], &*symbol.file_path);
            assert_eq!(endpoint["line"], symbol.range.start_line + 1);
            assert_eq!(endpoint["kind"], serde_json::to_value(symbol.kind).unwrap());
        }

        for (id, item) in &symbol_items {
            let symbol = facade
                .get_symbol(crate::SymbolId::new(*id).unwrap())
                .expect("symbol row");
            assert_eq!(item, &serde_json::to_value(&symbol).unwrap());
        }
    }
    #[test]
    fn orphan_edges_are_dropped_and_counted_alongside_duplicate_ids() {
        use crate::relationship::{RelationKind, Relationship};
        use crate::storage::DocumentIndex;
        use crate::{FileId, Range, Symbol, SymbolId, SymbolKind};

        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            index_path: dir.path().join("index"),
            workspace_root: None,
            ..Default::default()
        };
        let file = FileId::new(1).unwrap();
        let a = Symbol::new(
            SymbolId::new(1).unwrap(),
            "a",
            SymbolKind::Function,
            file,
            Range::new(0, 0, 0, 5),
        );
        let b = Symbol::new(
            SymbolId::new(2).unwrap(),
            "b",
            SymbolKind::Function,
            file,
            Range::new(2, 0, 2, 5),
        );
        {
            let index = DocumentIndex::new(settings.index_path.join("tantivy"), &settings).unwrap();
            index.start_batch().unwrap();
            index.add_document(&a, "src/x.rs").unwrap();
            index.add_document(&b, "src/x.rs").unwrap();
            index.add_document(&b, "src/x.rs").unwrap();
            index
                .store_relationship(a.id, b.id, &Relationship::new(RelationKind::Calls))
                .unwrap();
            index
                .store_relationship(
                    a.id,
                    SymbolId::new(99).unwrap(),
                    &Relationship::new(RelationKind::Calls),
                )
                .unwrap();
            index.commit_batch().unwrap();
        }
        let facade = IndexFacade::new(Arc::new(settings)).unwrap();
        assert_eq!(facade.relationship_count(), 2, "both rows persisted");

        let mut out = Vec::new();
        let summary = write_dump(
            &facade,
            DumpStamp::default(),
            &DumpFilter::default(),
            &mut out,
        )
        .unwrap();
        let lines = parse_lines(&out);

        let edges: Vec<&Envelope<serde_json::Value>> = lines
            .iter()
            .filter(|e| e.meta.entity_type == Some(EntityType::Relationship))
            .collect();
        assert_eq!(edges.len(), 1, "the orphan edge is absent");
        assert_eq!(edges[0].data.as_ref().unwrap()["to"]["id"], 2);
        assert_eq!(summary.relationships, 1);
        assert_eq!(summary.orphan_edges_dropped, 1);
        assert_eq!(summary.duplicate_symbol_ids, 1, "id 2 stored twice");
        let last = lines.last().unwrap().data.as_ref().unwrap();
        assert_eq!(last["orphan_edges_dropped"], 1);
        assert_eq!(last["duplicate_symbol_ids"], 1);
    }
    /// Struct + method + free fn: Defines (Widget -> render) and Calls
    /// (render -> helper) in one file.
    fn widget_fixture() -> (tempfile::TempDir, IndexFacade) {
        indexed_fixture(
            "pub struct Widget;\n\nimpl Widget {\n    pub fn render(&self) {\n        helper();\n    }\n}\n\nfn helper() {}\n",
        )
    }

    fn count(lines: &[Envelope<serde_json::Value>], t: EntityType) -> usize {
        lines
            .iter()
            .filter(|e| e.message_type == MessageType::Result && e.meta.entity_type == Some(t))
            .count()
    }

    #[test]
    fn symbols_only_filter_emits_no_relationship_rows() {
        let (_dir, facade) = widget_fixture();
        assert!(
            facade.relationship_count() >= 2,
            "fixture carries Defines + Calls"
        );

        let filter = DumpFilter {
            rows: Rows::Symbols,
            ..DumpFilter::default()
        };
        let mut out = Vec::new();
        let summary = write_dump(&facade, DumpStamp::default(), &filter, &mut out).unwrap();
        let lines = parse_lines(&out);

        assert_eq!(lines.first().unwrap().message_type, MessageType::Begin);
        assert_eq!(lines.last().unwrap().message_type, MessageType::Summary);
        assert_eq!(count(&lines, EntityType::Symbol), facade.symbol_count());
        assert_eq!(count(&lines, EntityType::Relationship), 0);
        assert_eq!(summary.symbols, facade.symbol_count());
        assert_eq!(summary.relationships, 0);
        assert_eq!(lines.last().unwrap().meta.count, Some(summary.symbols));
    }
    #[test]
    fn relationships_only_filter_keeps_endpoint_identity() {
        let (_dir, facade) = widget_fixture();
        let filter = DumpFilter {
            rows: Rows::Relationships,
            ..DumpFilter::default()
        };
        let mut out = Vec::new();
        let summary = write_dump(&facade, DumpStamp::default(), &filter, &mut out).unwrap();
        let lines = parse_lines(&out);

        assert_eq!(count(&lines, EntityType::Symbol), 0);
        assert_eq!(
            count(&lines, EntityType::Relationship),
            facade.relationship_count()
        );
        assert_eq!(summary.symbols, 0);
        assert_eq!(summary.relationships, facade.relationship_count());
        assert_eq!(
            summary.orphan_edges_dropped, 0,
            "identity map still built from the full symbol scan"
        );
        for line in lines
            .iter()
            .filter(|e| e.meta.entity_type == Some(EntityType::Relationship))
        {
            let data = line.data.as_ref().unwrap();
            for end in ["from", "to"] {
                assert!(
                    data[end]["name"].is_string(),
                    "{end} carries identity: {data}"
                );
                assert!(data[end]["file_path"].is_string());
                assert!(data[end]["line"].is_u64());
            }
        }
    }
    #[test]
    fn relation_filter_keeps_only_that_kind_and_leaves_symbol_rows_alone() {
        use crate::relationship::RelationKind;
        let (_dir, facade) = widget_fixture();

        let mut all = Vec::new();
        write_dump(
            &facade,
            DumpStamp::default(),
            &DumpFilter::default(),
            &mut all,
        )
        .unwrap();
        let all = parse_lines(&all);
        let relations = |lines: &[Envelope<serde_json::Value>], kind: &str| {
            lines
                .iter()
                .filter(|e| e.meta.entity_type == Some(EntityType::Relationship))
                .filter(|e| e.data.as_ref().unwrap()["relation"] == kind)
                .count()
        };
        let calls_total = relations(&all, "Calls");
        assert!(
            calls_total >= 1 && relations(&all, "Defines") >= 1,
            "fixture carries both kinds"
        );

        let filter = DumpFilter {
            relation: Some(RelationKind::Calls),
            ..DumpFilter::default()
        };
        let mut out = Vec::new();
        let summary = write_dump(&facade, DumpStamp::default(), &filter, &mut out).unwrap();
        let lines = parse_lines(&out);

        assert_eq!(relations(&lines, "Calls"), calls_total);
        assert_eq!(relations(&lines, "Defines"), 0);
        assert_eq!(count(&lines, EntityType::Relationship), calls_total);
        assert_eq!(summary.relationships, calls_total);
        assert_eq!(
            count(&lines, EntityType::Symbol),
            facade.symbol_count(),
            "symbol rows unaffected"
        );
    }
    #[test]
    fn kind_filter_gates_symbol_rows_only() {
        use crate::SymbolKind;
        let (_dir, facade) = widget_fixture();
        let functions = facade
            .get_all_symbols()
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .count();
        assert!(
            functions >= 1 && functions < facade.symbol_count(),
            "fixture mixes kinds"
        );

        let filter = DumpFilter {
            kind: Some(SymbolKind::Function),
            ..DumpFilter::default()
        };
        let mut out = Vec::new();
        let summary = write_dump(&facade, DumpStamp::default(), &filter, &mut out).unwrap();
        let lines = parse_lines(&out);

        assert_eq!(count(&lines, EntityType::Symbol), functions);
        assert!(
            lines
                .iter()
                .filter(|e| e.meta.entity_type == Some(EntityType::Symbol))
                .all(|e| e.data.as_ref().unwrap()["kind"] == "Function")
        );
        assert_eq!(summary.symbols, functions);
        assert_eq!(
            count(&lines, EntityType::Relationship),
            facade.relationship_count(),
            "relationship rows unaffected by --kind"
        );
        assert_eq!(
            summary.orphan_edges_dropped, 0,
            "identity map still covers every kind"
        );
    }
}
