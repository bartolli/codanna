use codanna::{
    FileId, Range, Symbol, SymbolId, SymbolKind, config::Settings,
    indexing::pipeline::types::SymbolLookupCache, storage::DocumentIndex,
};

fn check_cache(count: u32) {
    let dir = tempfile::tempdir().unwrap();
    let index = DocumentIndex::new(dir.path(), &Settings::default()).unwrap();
    index.start_batch().unwrap();
    for id in 1..=count {
        let symbol = Symbol::new(
            SymbolId::new(id).unwrap(),
            "generated",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            Range::new(id, 0, id, 1),
        );
        index.add_document(&symbol, "generated.rs").unwrap();
    }
    index.commit_batch().unwrap();
    let cache = SymbolLookupCache::from_index(&index).unwrap();
    assert_eq!(cache.len(), count as usize);
    for id in 1..=count {
        assert!(cache.get_ref(SymbolId::new(id).unwrap()).is_some());
    }
}

#[test]
fn cache_contains_every_persisted_symbol() {
    check_cache(0);
    check_cache(7);
}

#[test]
#[ignore = "writes one million synthetic symbols; run explicitly for the former cache limit"]
fn cache_exceeds_one_million_symbols() {
    check_cache(1_000_001);
}
