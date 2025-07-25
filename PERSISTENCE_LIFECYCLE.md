# Persistence Lifecycle Analysis

## Current Architecture

### 1. **Two-Layer Persistence**
- **Bincode (.bin)**: Snapshot-based serialization of IndexData
- **Tantivy**: Real-time searchable index with all data

### 2. **Data Flow**

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Source     │────>│  Indexer    │────>│ Persistence │
│  Files      │     │  (Memory)   │     │  Layer      │
└─────────────┘     └─────────────┘     └─────────────┘
                           │                    │
                           │                    ├── Bincode (.bin)
                           │                    └── Tantivy (dir)
                           │
                    ┌──────▼──────┐
                    │  IndexData  │
                    │  - symbols  │
                    │  - relations│
                    │  - file_map │
                    └─────────────┘
```

## Lifecycle Stages

### 1. **Initial Indexing**
```rust
// main.rs: Commands::Index
1. Load existing index or create new
2. Index files (with hash-based incremental updates)
3. Save to both Bincode and Tantivy
```

**Key Points:**
- File hashes prevent re-indexing unchanged files
- Both persistence layers updated atomically
- Tantivy batching for performance

### 2. **Loading Strategy**
```rust
// persistence.rs: load()
1. Try loading bincode first (fast, complete snapshot)
2. If bincode missing/corrupt, fall back to Tantivy
3. Tantivy rebuilds IndexData via rebuild_index_data()
```

**Failure Modes:**
- ✅ Bincode corrupt → Tantivy fallback
- ✅ Bincode missing → Tantivy fallback
- ❌ Both missing → Error
- ⚠️  Tantivy corrupt → No fallback

### 3. **Incremental Updates**
```rust
// simple.rs: index_file_internal()
1. Calculate file hash
2. Compare with stored hash
3. Skip if unchanged, update if changed
4. Remove old symbols before re-indexing
```

**Edge Cases:**
- File deleted externally → symbols remain
- Partial index update → inconsistent state
- Crash during save → potential data loss

## Current Issues & Risks

### 1. **Atomicity Problems**
- Bincode save is "atomic" (temp file + rename)
- Tantivy commits are separate
- No transaction across both stores

### 2. **Synchronization Issues**
```rust
// Current flow:
1. Update in-memory IndexData
2. Update Tantivy (might fail)
3. Save bincode (might fail)
// Result: Stores can diverge
```

### 3. **Recovery Gaps**
- No versioning/checksums
- No recovery journal
- Silent fallback might hide corruption

## Proposed Improvements

### 1. **Unified Transaction Model**
```rust
pub struct Transaction {
    tantivy_batch: TantivyBatch,
    bincode_staging: IndexData,
    rollback_data: Option<IndexData>,
}

impl Transaction {
    pub fn commit(self) -> Result<()> {
        // 1. Commit Tantivy first (can rollback)
        self.tantivy_batch.commit()?;
        
        // 2. Save bincode snapshot
        persistence.save_atomic(&self.bincode_staging)?;
        
        Ok(())
    }
    
    pub fn rollback(self) {
        // Restore previous state
    }
}
```

### 2. **Integrity Checking**
```rust
pub struct IndexMetadata {
    version: u32,
    checksum: String,
    last_modified: u64,
    symbol_count: u32,
    file_count: u32,
}

// Store in both Tantivy and as .meta file
```

### 3. **Recovery Journal**
```rust
pub enum JournalEntry {
    FileIndexed { path: String, hash: String, symbols: Vec<SymbolId> },
    FileRemoved { path: String },
    SnapshotCreated { timestamp: u64, checksum: String },
}

// Write-ahead log for crash recovery
```

### 4. **Lifecycle State Machine**
```rust
enum IndexState {
    Empty,
    Loading,
    Ready,
    Indexing { transaction: Transaction },
    Saving,
    Corrupted { reason: String },
}

// Enforce valid transitions
```

## Bulletproofing Strategy

### 1. **Defensive Loading**
```rust
pub fn load_with_verification(&self) -> Result<SimpleIndexer> {
    // 1. Check metadata first
    let metadata = self.load_metadata()?;
    
    // 2. Try bincode with checksum
    if let Ok(data) = self.load_bincode_verified(&metadata) {
        return Ok(SimpleIndexer::from_data(data));
    }
    
    // 3. Try Tantivy with validation
    if let Ok(data) = self.load_from_tantivy(&metadata) {
        // 4. Repair bincode from Tantivy
        self.save_bincode(&data)?;
        return Ok(SimpleIndexer::from_data(data));
    }
    
    // 5. Offer recovery options
    Err(IndexError::Corrupted { 
        suggestions: vec![
            "Run 'codanna repair' to attempt recovery",
            "Run 'codanna index --force' to rebuild",
        ]
    })
}
```

### 2. **Safe Indexing**
```rust
pub fn index_file_safe(&mut self, path: &Path) -> Result<FileId> {
    let transaction = self.begin_transaction()?;
    
    match transaction.index_file(path) {
        Ok(file_id) => {
            transaction.commit()?;
            Ok(file_id)
        }
        Err(e) => {
            transaction.rollback();
            Err(e)
        }
    }
}
```

### 3. **Periodic Snapshots**
```rust
pub struct SnapshotScheduler {
    last_snapshot: Instant,
    changes_since_snapshot: usize,
}

impl SnapshotScheduler {
    pub fn should_snapshot(&self) -> bool {
        self.changes_since_snapshot > 1000 ||
        self.last_snapshot.elapsed() > Duration::from_secs(300)
    }
}
```

## Testing Strategy

### 1. **Corruption Tests**
- Truncate bincode file
- Delete Tantivy segments
- Mix incompatible versions
- Interrupt during save

### 2. **Recovery Tests**
- Load with only Tantivy
- Load with corrupted bincode
- Concurrent access
- Power failure simulation

### 3. **Performance Tests**
- Large repository indexing
- Incremental update speed
- Memory usage patterns
- Startup time with various sizes