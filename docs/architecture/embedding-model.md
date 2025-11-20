# Embedding Model

How Codanna generates and uses semantic embeddings for code search.

## Supported Models

| Model | Dimensions | Languages | Use Case |
|-------|------------|-----------|----------|
| `AllMiniLML6V2` | 384 | English | Default, fast, English codebases |
| `MultilingualE5Small` | 384 | 94 | Multilingual, same performance |
| `MultilingualE5Base` | 768 | 94 | Better quality, slower |
| `MultilingualE5Large` | 1024 | 94 | Best quality, slowest |

## Model Selection

Configure in `.codanna/settings.toml`:

```toml
[semantic]
model = "AllMiniLML6V2"  # Default
# model = "MultilingualE5Small"  # For multilingual teams
```

**Note:** Changing models requires re-indexing:
```bash
codanna index . --force --progress
```

## Embedding Generation

### Input: Documentation Comments

```rust
/// Parse configuration from a TOML file and validate required fields
/// This handles missing files gracefully and provides helpful error messages
fn load_config(path: &Path) -> Result<Config, Error>
```

### Process

1. **Extract**: Doc comment text
2. **Tokenize**: Break into tokens
3. **Embed**: fastembed model generates vector
4. **Normalize**: L2 normalization for cosine similarity
5. **Store**: Memory-mapped vector cache

### Output: Dense Vector

```
[0.123, -0.456, 0.789, ..., 0.321]  // 384/768/1024 floats
```

## Semantic Understanding

Embeddings capture conceptual meaning, not just keywords. Query "authentication logic" matches "verify credentials and create tokens" but not "configuration parser".

## Similarity Computation

Cosine similarity scores: 0.7+ (very similar), 0.5-0.7 (related), 0.3-0.5 (somewhat related), <0.3 (different).

## Language Filtering

Each embedding tracks its source language. Filter before similarity computation with `lang:rust` to reduce search space by up to 75% in mixed codebases.

## IVFFlat Index

Vectors are organized using Inverted File with Flat vectors for fast search:

### K-means Clustering

1. **Cluster** similar vectors together
2. **Centroids** represent each cluster
3. **Search** checks nearby clusters first

### Search Algorithm

```
1. Query vector → find closest centroid
2. Search vectors in that cluster
3. Optionally search nearby clusters
4. Return top-k results
```

**Speed improvement**: O(sqrt(N)) instead of O(N) comparisons.

## Performance Characteristics

### First Use
- Model download: One-time (~25-560MB depending on model)
- Storage: `~/.cache/fastembed/`
- Subsequent runs: Instant model load

### Embedding Generation
- Per symbol: ~10ms
- Batch of 100: ~100ms
- Parallel: Scales with CPU cores

### Search
- With IVFFlat: <10ms for 100k vectors
- Without clustering: Would be ~1s

## Optimization

### Batch Processing
Generate embeddings in batches during indexing:
- More efficient GPU/CPU usage
- Amortizes model initialization
- Better throughput

### Caching
- Embeddings persist in memory-mapped files
- No re-generation unless code changes
- Symbol-level change detection

### Incremental Updates
Only re-embed changed symbols:
```rust
if symbol.doc_comment != old_symbol.doc_comment {
    regenerate_embedding(symbol);
}
```

## Troubleshooting

**Poor search results:** Check documentation quality, try multilingual model, use language filtering

**Slow first run:** Model downloads once (~25-560MB), subsequent runs use cache

**Model errors:** Check internet connection and `~/.cache/fastembed/` permissions

## Storage Requirements

**Formula:** `symbols × dimensions × 4 bytes`

**Examples (100k symbols):**
- 384-dim: ~154 MB
- 768-dim: ~307 MB
- 1024-dim: ~410 MB

## See Also

- [How It Works](how-it-works.md) - System overview
- [Memory Mapping](memory-mapping.md) - Vector storage details
- [Search Guide](../user-guide/search-guide.md) - Writing effective queries
- [Configuration](../user-guide/configuration.md) - Model selection