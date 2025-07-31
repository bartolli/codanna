use crate::{Symbol, SymbolId, FileId, SymbolKind};
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SymbolStore {
    symbols: Arc<DashMap<SymbolId, Symbol>>,
    by_name: Arc<DashMap<String, Vec<SymbolId>>>,
    by_file: Arc<DashMap<FileId, Vec<SymbolId>>>,
}

impl SymbolStore {
    pub fn new() -> Self {
        Self {
            symbols: Arc::new(DashMap::new()),
            by_name: Arc::new(DashMap::new()),
            by_file: Arc::new(DashMap::new()),
        }
    }

    pub fn insert(&self, symbol: Symbol) -> SymbolId {
        let id = symbol.id;
        let name = symbol.name.to_string();
        let file_id = symbol.file_id;
        
        self.symbols.insert(id, symbol);
        
        self.by_name
            .entry(name)
            .or_default()
            .push(id);
            
        self.by_file
            .entry(file_id)
            .or_default()
            .push(id);
            
        id
    }

    pub fn insert_batch(&self, symbols: impl IntoIterator<Item = Symbol>) {
        for symbol in symbols {
            self.insert(symbol);
        }
    }

    pub fn get(&self, id: SymbolId) -> Option<Symbol> {
        self.symbols.get(&id).map(|entry| entry.clone())
    }

    pub fn find_by_name(&self, name: &str) -> Vec<Symbol> {
        self.by_name
            .get(name)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.get(*id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn find_by_file(&self, file_id: FileId) -> Vec<Symbol> {
        self.by_file
            .get(&file_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.get(*id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn find_by_kind(&self, kind: SymbolKind) -> Vec<Symbol> {
        self.symbols
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn find_at_position(&self, file_id: FileId, line: u32, column: u16) -> Option<Symbol> {
        self.find_by_file(file_id)
            .into_iter()
            .find(|symbol| symbol.range.contains(line, column))
    }

    pub fn remove(&self, id: SymbolId) -> Option<Symbol> {
        if let Some((_, symbol)) = self.symbols.remove(&id) {
            // Remove from name index
            if let Some(mut ids) = self.by_name.get_mut(&symbol.name.to_string()) {
                ids.retain(|&sid| sid != id);
            }
            
            // Remove from file index
            if let Some(mut ids) = self.by_file.get_mut(&symbol.file_id) {
                ids.retain(|&sid| sid != id);
            }
            
            Some(symbol)
        } else {
            None
        }
    }

    pub fn clear(&self) {
        self.symbols.clear();
        self.by_name.clear();
        self.by_file.clear();
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.symbols.iter().map(|entry| entry.value().clone())
    }
    
    /// Get all symbols as a Vec - prefer iter() to avoid allocation when possible
    pub fn to_vec(&self) -> Vec<Symbol> {
        self.iter().collect()
    }
    
    /// Get a reference to all symbol IDs for a given name
    pub fn as_ids_for_name(&self, name: &str) -> Option<Vec<SymbolId>> {
        self.by_name.get(name).map(|ids| ids.clone())
    }
}

impl Default for SymbolStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Range;

    fn create_test_symbol(id: u32, name: &str, file_id: u32) -> Symbol {
        Symbol::new(
            SymbolId::new(id).unwrap(),
            name,
            SymbolKind::Function,
            FileId::new(file_id).unwrap(),
            Range::new(1, 0, 5, 0),
        )
    }

    #[test]
    fn test_symbol_store_insert_and_get() {
        let store = SymbolStore::new();
        let symbol = create_test_symbol(1, "test_function", 1);
        
        let id = store.insert(symbol.clone());
        assert_eq!(id, symbol.id);
        
        let retrieved = store.get(id).unwrap();
        assert_eq!(retrieved.name, symbol.name);
    }

    #[test]
    fn test_find_by_name() {
        let store = SymbolStore::new();
        
        store.insert(create_test_symbol(1, "foo", 1));
        store.insert(create_test_symbol(2, "bar", 1));
        store.insert(create_test_symbol(3, "foo", 2));
        
        let foos = store.find_by_name("foo");
        assert_eq!(foos.len(), 2);
        
        let bars = store.find_by_name("bar");
        assert_eq!(bars.len(), 1);
        
        let bazs = store.find_by_name("baz");
        assert_eq!(bazs.len(), 0);
    }

    #[test]
    fn test_find_by_file() {
        let store = SymbolStore::new();
        
        store.insert(create_test_symbol(1, "func1", 1));
        store.insert(create_test_symbol(2, "func2", 1));
        store.insert(create_test_symbol(3, "func3", 2));
        
        let file1_symbols = store.find_by_file(FileId::new(1).unwrap());
        assert_eq!(file1_symbols.len(), 2);
        
        let file2_symbols = store.find_by_file(FileId::new(2).unwrap());
        assert_eq!(file2_symbols.len(), 1);
    }

    #[test]
    fn test_find_by_kind() {
        let store = SymbolStore::new();
        
        let mut symbol1 = create_test_symbol(1, "func", 1);
        symbol1.kind = SymbolKind::Function;
        
        let mut symbol2 = create_test_symbol(2, "MyStruct", 1);
        symbol2.kind = SymbolKind::Struct;
        
        let mut symbol3 = create_test_symbol(3, "another_func", 2);
        symbol3.kind = SymbolKind::Function;
        
        store.insert(symbol1);
        store.insert(symbol2);
        store.insert(symbol3);
        
        let functions = store.find_by_kind(SymbolKind::Function);
        assert_eq!(functions.len(), 2);
        
        let structs = store.find_by_kind(SymbolKind::Struct);
        assert_eq!(structs.len(), 1);
    }

    #[test]
    fn test_find_at_position() {
        let store = SymbolStore::new();
        let file_id = FileId::new(1).unwrap();
        
        let symbol1 = Symbol::new(
            SymbolId::new(1).unwrap(),
            "func1",
            SymbolKind::Function,
            file_id,
            Range::new(1, 0, 5, 0),
        );
        
        let symbol2 = Symbol::new(
            SymbolId::new(2).unwrap(),
            "func2",
            SymbolKind::Function,
            file_id,
            Range::new(10, 0, 15, 0),
        );
        
        store.insert(symbol1.clone());
        store.insert(symbol2.clone());
        
        assert_eq!(
            store.find_at_position(file_id, 3, 0).unwrap().id,
            symbol1.id
        );
        assert_eq!(
            store.find_at_position(file_id, 12, 0).unwrap().id,
            symbol2.id
        );
        assert!(store.find_at_position(file_id, 7, 0).is_none());
    }

    #[test]
    fn test_batch_insert() {
        let store = SymbolStore::new();
        
        let symbols = vec![
            create_test_symbol(1, "func1", 1),
            create_test_symbol(2, "func2", 1),
            create_test_symbol(3, "func3", 2),
        ];
        
        store.insert_batch(symbols);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn test_remove() {
        let store = SymbolStore::new();
        let symbol = create_test_symbol(1, "test", 1);
        let id = symbol.id;
        
        store.insert(symbol);
        assert_eq!(store.len(), 1);
        
        let removed = store.remove(id).unwrap();
        assert_eq!(removed.name.as_ref(), "test");
        assert_eq!(store.len(), 0);
        assert!(store.get(id).is_none());
        assert!(store.find_by_name("test").is_empty());
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;
        
        let store = SymbolStore::new();
        let store_clone = store.clone();
        
        let handle = thread::spawn(move || {
            for i in 1..=100 {
                store_clone.insert(create_test_symbol(i, &format!("func{}", i), 1));
            }
        });
        
        for i in 101..=200 {
            store.insert(create_test_symbol(i, &format!("func{}", i), 2));
        }
        
        handle.join().unwrap();
        assert_eq!(store.len(), 200);
    }
}