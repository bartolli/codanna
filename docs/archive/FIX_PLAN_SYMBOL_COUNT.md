# Fix Plan: Symbol Count After Incremental Updates

## Problem
After incrementally updating a file, the symbol count drops dramatically (from 608 to 6), suggesting documents are being deleted but not all symbols are being properly tracked.

## Symptoms
- First index: 602 symbols from 36 files ✓
- Add new file: 607 symbols from 37 files ✓  
- Modify existing file: 6 symbols from 38 files ✗

## Root Cause Analysis

### Hypothesis 1: Reader Not Refreshed
The searcher might be using stale data after deletions.

### Hypothesis 2: Transaction Issues
Documents might be deleted in one transaction and added in another, with the count happening between.

### Hypothesis 3: Metadata Mismatch
The metadata (symbol count, file count) might not match the actual document count.

## Investigation Steps

1. **Add debug logging**:
   ```rust
   pub fn remove_file_documents(&self, file_path: &str) -> StorageResult<()> {
       eprintln!("DEBUG: Removing documents for file: {}", file_path);
       let before_count = self.count_symbols()?;
       eprintln!("DEBUG: Symbol count before removal: {}", before_count);
       
       // ... removal logic ...
       
       let after_count = self.count_symbols()?;
       eprintln!("DEBUG: Symbol count after removal: {}", after_count);
   }
   ```

2. **Check commit/reload sequence**:
   ```rust
   // In reindex_file
   eprintln!("DEBUG: Starting reindex for {}", path_str);
   
   // Remove old documents
   self.document_index.remove_file_documents(path_str)?;
   
   // Is there a commit here? Should there be?
   
   // Add new documents
   self.index_file_content(...)?;
   ```

## Proposed Fix

### Option 1: Atomic Update Transaction
```rust
impl SimpleIndexer {
    fn reindex_file_atomic(&mut self, path: &Path) -> IndexResult<()> {
        // Start batch if not already started
        let batch_started = self.document_index.start_batch().is_ok();
        
        // Delete old documents (uses batch writer)
        self.document_index.remove_file_documents(path_str)?;
        
        // Add new documents (uses same batch writer)
        self.index_file_content(...)?;
        
        // Commit only if we started the batch
        if batch_started {
            self.document_index.commit_batch()?;
        }
    }
}
```

### Option 2: Fix remove_file_documents
```rust
pub fn remove_file_documents(&self, file_path: &str) -> StorageResult<()> {
    let mut writer_lock = self.writer.lock().map_err(|_| StorageError::LockPoisoned)?;
    let term = Term::from_field_text(self.schema.file_path, file_path);
    
    if let Some(writer) = writer_lock.as_mut() {
        // Use existing batch writer
        writer.delete_term(term);
        // Don't commit here - let the batch handle it
    } else {
        // Only create temporary writer if no batch is active
        drop(writer_lock); // Release the lock
        
        let mut writer = self.index.writer::<Document>(50_000_000)?;
        writer.delete_term(term);
        writer.commit()?;
        self.reader.reload()?;
    }
    
    Ok(())
}
```

## Testing Strategy

1. **Create incremental update test**:
   ```rust
   #[test]
   fn test_incremental_update_preserves_symbol_count() {
       let indexer = create_test_indexer();
       
       // Index multiple files
       indexer.index_file("file1.rs").unwrap();
       indexer.index_file("file2.rs").unwrap();
       let initial_count = indexer.symbol_count();
       
       // Modify one file
       write_file("file1.rs", "modified content");
       indexer.index_file("file1.rs").unwrap();
       
       let final_count = indexer.symbol_count();
       assert!(final_count >= initial_count - 10, 
           "Symbol count dropped too much: {} -> {}", 
           initial_count, final_count);
   }
   ```

2. **Add transaction verification test**
3. **Test with concurrent updates**

## Priority
This is HIGH priority as it affects the core indexing functionality and could lead to data loss/corruption in production use.