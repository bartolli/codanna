# Codanna

A high-performance code intelligence system that gives AI assistants deep understanding of your codebase through semantic search and relationship tracking.

## What It Does

Codanna indexes your code and provides:
- **Semantic search** - Find code using natural language: "authentication logic", "parse JSON data"
- **Relationship tracking** - Who calls what, implementation hierarchies, dependency graphs
- **MCP integration** - Claude can navigate and understand your codebase in real-time
- **Lightning fast** - Indexes thousands of files per second, searches in <10ms

## The Engineering Story

Traditional code search tools use text matching. Codanna understands what your code *means*:

```bash
# Traditional search - finds text matches
grep -r "parse.*json" .

# Codanna semantic search - finds conceptually related code
codanna mcp semantic_search_docs --args '{"query": "convert JSON to internal format"}'
```

Under the hood, Codanna:
1. Parses your code with tree-sitter (currently Rust, more languages coming)
2. Extracts symbols and their relationships using type-aware analysis
3. Generates embeddings from documentation using AllMiniLML6V2 (384 dimensions)
4. Stores everything in a Tantivy full-text index with integrated vector search
5. Serves it via MCP so Claude can use it naturally

## Quick Start

```bash
# Install (binary releases coming soon)
cargo install --path .

# Initialize in your project
cd your-project
codanna init

# Index your code
codanna index src

```

## Claude Integration

Once running, Claude can use these tools:

- `find_symbol` - Locate symbols by exact name
- `search_symbols` - Fuzzy text search across your codebase  
- `semantic_search_docs` - Natural language code search
- `semantic_search_with_context` - The "powerhorse" - returns full context including dependencies, callers, and impact analysis
- `get_calls` / `find_callers` - Trace function relationships
- `analyze_impact` - Understand change ripple effects

Configure Claude Desktop by adding to your claude_desktop_config.json:

```json
{
  "mcpServers": {
    "codanna": {
      "command": "codanna",
      "args": ["serve"]
    }
  }
}
```

## Performance Characteristics

Real measurements from a production Rust codebase:
- **Indexing**: 44 files in <0.01s (effectively instant for most projects)
- **Symbol capacity**: ~100 bytes per symbol in memory
- **Semantic search**: <10ms query latency
- **Incremental updates**: Hash-based caching skips unchanged files

## Smart Ignore Patterns

Codanna respects `.gitignore` and adds its own `.codannaignore`:

```bash
# Created automatically by codanna init
.codanna/       # Don't index own data
target/         # Skip build artifacts
node_modules/   # Skip dependencies
*_test.rs       # Optionally skip tests
```

## Configuration

Settings live in `.codanna/settings.toml`:

```toml
[semantic_search]
enabled = true
model = "AllMiniLML6V2"
threshold = 0.6  # Similarity threshold (0-1)

[indexing]
parallel_threads = 16  # Auto-detected by default
include_tests = true   # Index test files
```

## Architecture Highlights

Some interesting implementation details:

**Memory-mapped vector storage**: Semantic embeddings are stored in memory-mapped files for instant loading. Access time is essentially zero after the OS page cache warms up.

**Embedding lifecycle management**: When files are re-indexed, old embeddings are automatically cleaned up. No accumulation over time.

**Lock-free concurrency**: Uses DashMap for concurrent symbol access and Arc<Mutex<_>> only where necessary for write coordination.

**Single-pass indexing**: Extracts symbols, relationships, and generates embeddings in one pass through the AST.

## CLI Examples

```bash
# Index with progress display
codanna index . --progress

# Dry run to see what would be indexed
codanna index src --dry-run

# Force complete rebuild
codanna index . --force

# Find a specific symbol
codanna retrieve symbol MyStruct

# See what calls a function
codanna retrieve callers process_data

# Natural language search (requires semantic search enabled)
codanna mcp semantic_search_docs --args '{"query": "error handling"}'
```

## Requirements

- Rust 1.75+ (for development)
- ~150MB for model storage (downloaded on first use)
- A few MB for index storage (varies by codebase size)

## Current Limitations

- Only indexes Rust code (TypeScript, Python, JavaScript coming soon)
- Semantic search requires English documentation/comments
- Windows support is experimental

## License

MIT - See LICENSE file

## Contributing

This is an early release focused on core functionality. Contributions welcome! See CONTRIBUTING.md for guidelines.

---

Built with 🦀 by developers who wanted their AI assistants to actually understand their code.