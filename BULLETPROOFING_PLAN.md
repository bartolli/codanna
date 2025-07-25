# Bulletproofing the Indexing Logic

## Critical Issues Found

### 1. **Non-Atomic Dual Persistence**
```rust
// Current problem in main.rs
match persistence.save(&indexer) {  // Only saves bincode!
    Ok(_) => println!("Index saved"),
    Err(e) => eprintln!("Warning: Could not save index: {}", e),
}
// Tantivy commits happen separately during indexing
```

**Risk**: Bincode and Tantivy can diverge, leading to inconsistent state.

### 2. **Unsafe Unwraps in Critical Paths**
```rust
// tantivy.rs - Multiple locations
let mut writer_lock = self.writer.lock().unwrap();  // Can panic!
```

**Risk**: Poisoned mutex from previous panic causes cascade failures.

### 3. **Silent Corruption Recovery**
```rust
// simple.rs: from_data_with_settings()
if data.symbols.is_empty() && indexer.document_index.is_some() {
    // Silently loads from Tantivy without user awareness
    eprintln!("Bincode data is empty but Tantivy has {} documents...");
}
```

**Risk**: User unaware data source changed; corruption goes unnoticed.

### 4. **No Transaction Rollback**
```rust
// simple.rs: index_file_internal()
self.remove_file_symbols(file_id);  // Deletes old symbols
// If reindex fails, symbols are lost permanently!
```

**Risk**: Failed re-indexing leaves index in broken state.

### 5. **Missing Error Context**
```rust
// Many places just propagate errors without context
doc_index.store_file_info(file_id, &path_str, &content_hash, timestamp)
    .map_err(|e| eprintln!("Failed to update file info: {}", e));
// Error is logged but indexing continues!
```

## Immediate Fixes Required

### 1. **Implement Transactional Indexing**
```rust
pub struct IndexTransaction<'a> {
    indexer: &'a mut SimpleIndexer,
    snapshot: IndexData,  // Backup for rollback
    tantivy_batch: Option<TantivyBatch>,
}

impl<'a> IndexTransaction<'a> {
    pub fn new(indexer: &'a mut SimpleIndexer) -> Result<Self> {
        let snapshot = indexer.data.clone();  // Deep clone for safety
        let tantivy_batch = indexer.start_tantivy_batch().ok();
        Ok(Self { indexer, snapshot, tantivy_batch })
    }
    
    pub fn commit(self) -> Result<()> {
        // 1. Commit Tantivy first (can fail safely)
        if let Some(batch) = self.tantivy_batch {
            batch.commit()?;
        }
        
        // 2. Save bincode snapshot
        let persistence = IndexPersistence::new(&self.indexer.settings.index_path);
        persistence.save(&self.indexer)?;
        
        Ok(())
    }
    
    pub fn rollback(mut self) {
        // Restore snapshot
        self.indexer.data = self.snapshot;
        // Tantivy batch auto-rollbacks on drop
    }
}
```

### 2. **Safe Mutex Handling**
```rust
pub fn with_writer<F, R>(&self, f: F) -> Result<R>
where
    F: FnOnce(&mut IndexWriter<Document>) -> Result<R>
{
    match self.writer.lock() {
        Ok(mut lock) => {
            let writer = lock.as_mut()
                .ok_or("No active writer")?;
            f(writer)
        }
        Err(poisoned) => {
            // Recover from poisoned mutex
            eprintln!("Warning: Recovering from poisoned writer mutex");
            let mut lock = poisoned.into_inner();
            // Reset writer state
            *lock = None;
            Err("Writer was in invalid state, please retry".into())
        }
    }
}
```

### 3. **Explicit Data Source Tracking**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub version: u32,
    pub source: DataSource,
    pub symbol_count: u32,
    pub last_modified: u64,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    Bincode { path: PathBuf, size: u64 },
    Tantivy { path: PathBuf, doc_count: u64 },
    Hybrid { primary: Box<DataSource>, fallback: Box<DataSource> },
}

// Always inform user of data source
println!("Loaded index from {:?} ({} symbols)", metadata.source, metadata.symbol_count);
```

### 4. **Atomic File Updates**
```rust
pub fn reindex_file_atomic(&mut self, path: &Path) -> Result<FileId> {
    let mut transaction = IndexTransaction::new(self)?;
    
    // Store old symbols for potential rollback
    let old_symbols = if let Some(&file_id) = transaction.indexer.data.file_map.get(&path_str) {
        transaction.indexer.symbol_store.find_by_file(file_id)
    } else {
        vec![]
    };
    
    match transaction.indexer.index_file_internal(path) {
        Ok(file_id) => {
            transaction.commit()?;
            Ok(file_id)
        }
        Err(e) => {
            eprintln!("Indexing failed, rolling back: {}", e);
            transaction.rollback();
            
            // Restore old symbols if needed
            for symbol in old_symbols {
                transaction.indexer.symbol_store.insert(symbol);
            }
            
            Err(e)
        }
    }
}
```

### 5. **Comprehensive Error Handling**
```rust
pub enum IndexingError {
    // Specific error types with context
    FileNotFound { path: PathBuf },
    ParseError { path: PathBuf, line: u32, error: String },
    HashMismatch { path: PathBuf, expected: String, actual: String },
    TantivyError { operation: String, cause: Box<dyn Error> },
    BincodeError { operation: String, cause: Box<dyn Error> },
    TransactionFailed { operations: Vec<String>, cause: Box<dyn Error> },
}

impl IndexingError {
    pub fn recovery_suggestions(&self) -> Vec<&str> {
        match self {
            Self::TantivyError { .. } => vec![
                "Try running 'codanna repair --tantivy'",
                "Or force rebuild with 'codanna index --force'"
            ],
            Self::BincodeError { .. } => vec![
                "Index will load from Tantivy on next start",
                "Run 'codanna verify' to check integrity"
            ],
            _ => vec![]
        }
    }
}
```

## Long-term Improvements

### 1. **Write-Ahead Log (WAL)**
- Record all operations before executing
- Enable crash recovery
- Support incremental checkpoints

### 2. **Index Verification Command**
```bash
codanna verify [--fix]
# Checks:
# - Bincode vs Tantivy consistency
# - Symbol reference integrity  
# - File hash validation
# - Orphaned symbols
```

### 3. **Progressive Enhancement**
- Start with Tantivy-only for speed
- Background thread creates bincode snapshot
- User notified when snapshot ready

### 4. **Monitoring & Metrics**
- Track indexing performance
- Monitor error rates
- Alert on corruption detection

## Testing Requirements

1. **Corruption Scenarios**
   - Kill process during save
   - Corrupt bincode file
   - Delete Tantivy segments
   - Mix version mismatches

2. **Recovery Testing**
   - Verify fallback mechanisms
   - Test rollback functionality
   - Ensure data consistency

3. **Performance Under Failure**
   - Measure recovery time
   - Test with large codebases
   - Concurrent access handling

## Implementation Priority

1. **Critical** (Do immediately):
   - Fix unsafe unwraps
   - Add transaction support
   - Implement proper error handling

2. **Important** (Next sprint):
   - Add metadata tracking
   - Implement verification command
   - Create recovery documentation

3. **Nice to have** (Future):
   - WAL implementation
   - Progressive enhancement
   - Advanced monitoring