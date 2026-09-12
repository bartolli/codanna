use codanna::config::Settings;
use codanna::indexing::pipeline::types::SymbolLookupCache;
use codanna::storage::DocumentIndex;
use codanna::{FileId, Range, Symbol, SymbolId, SymbolKind};

#[test]
fn hardening_symbol_cache_from_index_visits_every_symbol() {
    let dir = tempfile::tempdir().unwrap();
    let index = DocumentIndex::new(dir.path(), &Settings::default()).unwrap();
    index.start_batch().unwrap();
    for id in 1..=257u32 {
        let symbol = Symbol::new(
            SymbolId::new(id).unwrap(),
            format!("generated_{id}"),
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(id, 0, id, 1),
        );
        index.add_document(&symbol, "generated.rs").unwrap();
    }
    index.commit_batch().unwrap();

    let cache = SymbolLookupCache::from_index(&index).unwrap();
    assert_eq!(cache.len(), 257);
    assert!(cache.get(SymbolId::new(257).unwrap()).is_some());
}
