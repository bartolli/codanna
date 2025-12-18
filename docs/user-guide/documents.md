[Documentation](../README.md) / [User Guide](README.md) / **Document Search**

---

# Document Search

Index markdown and text files for semantic search (RAG).

## Overview

While Codanna's primary focus is code intelligence, the document search feature lets you index documentation, notes, and other text files for semantic search. This is useful for:

- Searching project documentation alongside code
- Building RAG (Retrieval-Augmented Generation) pipelines
- Finding relevant context from markdown files

## Quick Start

```bash
# Add a collection
codanna documents add-collection docs docs/

# Index all collections
codanna documents index

# Search
codanna documents search "error handling"
```

## Configuration

Document settings live in `.codanna/settings.toml`:

```toml
[documents]
enabled = true

[documents.defaults]
strategy = "hybrid"      # Paragraph-based with size constraints
min_chunk_chars = 200    # Merge small paragraphs
max_chunk_chars = 1500   # Split large paragraphs
overlap_chars = 100      # Context overlap when splitting

[documents.search]
preview_mode = "kwic"    # "kwic" or "full"
preview_chars = 600      # Preview window size
highlight = true         # Highlight keywords with **markers**

[documents.collections.docs]
paths = ["docs/"]
patterns = ["**/*.md"]
```

## Collections

Collections organize documents into searchable groups.

### Add a Collection

```bash
codanna documents add-collection <name> <path>
```

Examples:
```bash
# Index project docs
codanna documents add-collection docs docs/

# Index external documentation
codanna documents add-collection rust-book /path/to/rust-book/

# Index multiple paths (edit settings.toml)
[documents.collections.guides]
paths = ["docs/", "examples/", "README.md"]
patterns = ["**/*.md", "**/*.txt"]
```

### Remove a Collection

```bash
codanna documents remove-collection <name>
```

This removes the collection from `settings.toml`. Run `documents index` to sync the index.

### List Collections

```bash
codanna documents list
```

### Collection Stats

```bash
codanna documents stats <name>
```

## Indexing

### Index All Collections

```bash
codanna documents index
```

With progress display:
```bash
codanna documents index --progress
```

### Index Specific Collection

```bash
codanna documents index --collection docs
```

### Sync Behavior

The index command syncs with `settings.toml`:
- Collections in config but not indexed: indexed
- Collections indexed but not in config: removed
- Files deleted from disk: chunks removed
- Files modified: re-indexed

## Chunking Strategy

The **hybrid** strategy preserves document structure:

```
Document with paragraphs:
┌─────────────────────────────────────────┐
│ Short paragraph (50 chars)              │ ← merged
│ Another short one (80 chars)            │ ← merged  } = 1 chunk
│ Medium paragraph (120 chars)            │ ← merged
├─────────────────────────────────────────┤
│ Normal paragraph (400 chars)            │ = 1 chunk
├─────────────────────────────────────────┤
│ Very long paragraph (2000 chars)...     │ = 2 chunks
│ ...with 100 char overlap                │   (overlap)
└─────────────────────────────────────────┘
```

- **min_chunk_chars** (200): Paragraphs smaller than this merge with neighbors
- **max_chunk_chars** (1500): Paragraphs larger than this split with overlap
- **overlap_chars** (100): Context preserved between split chunks

## Search

### Basic Search

```bash
codanna documents search "authentication flow"
```

### Filter by Collection

```bash
codanna documents search "error handling" --collection docs
```

### Limit Results

```bash
codanna documents search "configuration" --limit 5
```

### JSON Output

```bash
codanna documents search "setup guide" --json
```

## Search Result Display

### KWIC (Keyword In Context)

Default mode. Centers the preview window around the first keyword match:

```
1. docs/auth.md (score: 0.72)
   Preview: ...the **authentication** flow handles user login and session...
```

### Full Preview

Shows entire chunk content:

```toml
[documents.search]
preview_mode = "full"
```

### Keyword Highlighting

Keywords are wrapped with `**markers**`:

```
Preview: ## **Parser** Technology

Codanna uses **tree-sitter** for AST parsing...
```

Disable with:
```toml
[documents.search]
highlight = false
```

## Commands Reference

| Command | Description |
|---------|-------------|
| `documents add-collection <name> <path>` | Add collection to settings.toml |
| `documents remove-collection <name>` | Remove collection from settings.toml |
| `documents index` | Index all configured collections |
| `documents index --collection <name>` | Index specific collection |
| `documents index --progress` | Show progress during indexing |
| `documents search <query>` | Search indexed documents |
| `documents search <query> --collection <name>` | Search within collection |
| `documents search <query> --limit <n>` | Limit results |
| `documents search <query> --json` | JSON output |
| `documents list` | List indexed collections |
| `documents stats <name>` | Show collection statistics |

## MCP Tool

Document search is also available as an MCP tool for AI assistants:

```bash
codanna mcp search_documents query:"authentication flow" limit:5
```

See [MCP Tools](mcp-tools.md#search_documents) for details.

## Tips

1. **Chunk size considerations**: Larger chunks = more context but coarser matches. Smaller chunks = precise matches but may lose context. Choose based on your use case.

2. **Collection organization**: Group related documents. Search within a collection for focused results.

3. **Re-indexing**: Only changed files are re-indexed. Delete `.codanna/index/documents/` for full rebuild.

4. **Progress display**: Use `--progress` for large collections to see two-phase progress (file processing, then embedding generation).

[Back to User Guide](README.md)
