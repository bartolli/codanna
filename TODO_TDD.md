# TDD Progress: Vector Search Update and Reindexing

## Test 10: File Update with Vector Reindexing

### Design Goals
- [ ] Design API for detecting symbol-level changes when files are updated
- [ ] Create test scenarios for different update patterns
- [ ] Define SymbolChangeDetector trait for comparing old/new symbols
- [ ] Extend IndexTransaction to handle vector updates alongside document updates
- [ ] Design vector storage update API that maintains consistency with Tantivy segments

### Test Scenarios to Implement
- [ ] File with unchanged symbols (hash changed but symbols identical)
- [ ] File with modified function signatures (embedding should update)
- [ ] File with added/removed functions (vector add/delete)
- [ ] File with renamed functions (old vector removed, new added)
- [ ] Batch update of multiple files
- [ ] Concurrent update handling
- [ ] Update rollback on failure

### Integration Points
- [ ] SimpleIndexer in src/indexing/simple.rs handles file updates
- [ ] DocumentIndex in src/storage/tantivy.rs manages document storage
- [ ] IndexTransaction in src/indexing/transaction.rs for atomic updates
- [ ] FileInfo in src/indexing/file_info.rs tracks file hashes

### Performance Targets
- [ ] Incremental update <100ms per file
- [ ] Only regenerate embeddings for changed symbols
- [ ] Maintain <10ms query latency during updates
- [ ] Memory usage proportional to changed symbols only

### Files to Create
- [ ] tests/vector_update_test.rs - Main test file for update scenarios
- [ ] Define production API structure in src/vector/update.rs (future)
- [ ] Extend IVFFlatIndex with update capabilities

### Status
- [ ] Ready for test creation
- [ ] Tests written and passing
- [ ] Production implementation started
- [ ] Migration complete