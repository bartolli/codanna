#!/bin/bash

echo "Installing codebase-intelligence dependencies..."

# Core dependencies
echo "Installing core dependencies..."
cargo add tokio --features full
cargo add sqlx --features runtime-tokio-native-tls,sqlite
cargo add serde --features derive
cargo add serde_json
cargo add thiserror
cargo add anyhow
cargo add tracing
cargo add tracing-subscriber

# Search and indexing
echo "Installing search and indexing dependencies..."
cargo add tantivy
cargo add tree-sitter
cargo add tree-sitter-rust
cargo add tree-sitter-javascript
cargo add tree-sitter-typescript
cargo add tree-sitter-python
cargo add tree-sitter-go
cargo add tree-sitter-java

# Data structures
echo "Installing data structure dependencies..."
cargo add petgraph
cargo add dashmap
cargo add rayon
cargo add crossbeam-channel
cargo add parking_lot

# ML and embeddings
echo "Installing ML dependencies..."
cargo add candle-core
cargo add candle-nn
cargo add candle-transformers
cargo add hnsw

# Serialization
echo "Installing serialization dependencies..."
cargo add rkyv --features bytecheck,std

# MCP server
echo "Installing MCP server dependency..."
cargo add rmcp@0.2.0

# Utilities
echo "Installing utility dependencies..."
cargo add clap --features derive
cargo add dirs
cargo add walkdir
cargo add ignore
cargo add memmap2
cargo add lz4_flex

echo "All dependencies installed successfully!"