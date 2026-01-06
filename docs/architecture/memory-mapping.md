# Memory-Mapped Storage

Codanna uses memory-mapped files for instant loading and high-performance access.

## Storage Architecture

### Symbol Index (Tantivy)
- **Purpose**: Fast symbol lookups and full-text search
- **Access**: <10ms response time
- **Features**: Fuzzy matching, exact match, full-text search
- **Location**: `.codanna/index/tantivy/`

### Vector Cache (`segment_0.vec`)
- **Purpose**: Semantic similarity search
- **Dimensions**: Configurable (384/768/1024 based on model)
- **Access**: <1μs after OS page cache warm-up
- **Storage**: Binary-packed floating-point arrays
- **Organization**: IVFFlat clustering for fast lookup

## Memory-Mapped Benefits

### Instant Startup
- No deserialization on load
- OS maps file directly to memory
- Application sees it as regular memory
- First access triggers page loading

### Efficient Memory Usage
- OS manages paging automatically
- Inactive pages can be swapped out
- Multiple processes share same physical memory
- No manual cache management needed

### Persistence
- Data persists between runs
- No rebuild on restart
- Atomic writes prevent corruption
- File system handles durability

## Vector Cache Structure

```
segment_0.vec:
├── Header (metadata)
│   ├── Model name
│   ├── Dimensions
│   ├── Vector count
│   └── Cluster count
├── Cluster metadata
│   ├── Cluster centroids
│   └── Cluster boundaries
└── Vector data
    ├── Vector 0: [f32; dimensions]
    ├── Vector 1: [f32; dimensions]
    └── ...
```

**Storage format**: Binary-packed f32 arrays.

## IVFFlat Clustering

Vectors are organized using Inverted File with Flat vectors:

1. **K-means clustering** groups similar vectors
2. **Centroids** represent each cluster
3. **Search** checks nearby clusters first
4. **Reduces** comparisons from N to ~sqrt(N)

Example with 10,000 vectors:
- Without clustering: 10,000 comparisons
- With 100 clusters: ~1,000 comparisons (10x faster)

## Cache Warming

First access loads pages into OS cache:

```
Cold start: 100-200ms (loading from disk)
Warm cache: <1μs (already in RAM)
```

**Hot paths warm up quickly** - frequent queries benefit from OS caching.

## Write Operations

### Symbol Index Updates
1. Batch commits every 100 files for throughput
2. RwLock-based concurrent writes
3. Tantivy handles segment management

### Vector Cache Updates
1. Generate new embeddings
2. Re-cluster vectors with K-means
3. Write new segment file
4. Delete old embeddings
5. Update metadata

**Crash safety**: Old files remain valid until new ones are complete.

## Storage Layout

```
.codanna/index/
├── tantivy/                # Symbol index
│   └── ...                 # Tantivy segment files
├── semantic/               # Code vector storage
│   ├── segment_0.vec       # Vector data
│   ├── metadata.bin        # Index metadata
│   └── clusters.bin        # Cluster information
├── documents/              # Document collections (RAG)
│   ├── tantivy/            # Document metadata index
│   └── vectors/            # Document embeddings
└── resolvers/              # Path resolution rules
```

## Memory Requirements

For a project with 100,000 symbols:

**Vector cache (384-dim model):**
- 100,000 vectors × 384 floats × 4 bytes = 153.6 MB

**Tantivy index:** Variable, typically 10-50 MB depending on symbol metadata.

## Scalability

Memory-mapped files scale to:
- Millions of symbols
- Gigabytes of vector data
- Multiple concurrent readers
- OS handles paging automatically

## Performance Characteristics

### Read Performance
- Symbol lookup: <10ms via Tantivy
- Vector search: O(sqrt(N)) with IVFFlat
- Memory-mapped access

### Write Performance
- Batch updates preferred
- Atomic file replacement
- No locking for readers
- Background re-clustering

## Troubleshooting

### High Memory Usage
- OS maps entire file but doesn't load it all
- Use `vmstat` to see actual RAM usage
- Inactive pages get swapped naturally

### Slow First Search
- OS loading pages from disk
- Subsequent searches are fast
- Pre-warm with `cat .codanna/index/vectors/segment_0.vec > /dev/null`

### Corruption Recovery
- Delete corrupted cache files
- Re-run `codanna index` to rebuild
- Atomic writes prevent partial updates

## See Also

- [How It Works](how-it-works.md) - System overview
- [Embedding Model](embedding-model.md) - Vector generation
- [Performance](../advanced/performance.md) - Optimization tips