//! Tantivy schema for document chunk storage.
//!
//! This module defines the index schema for storing document chunks
//! with metadata for filtering and navigation.

use tantivy::schema::{
    FAST, Field, IndexRecordOption, NumericOptions, STORED, STRING, Schema, SchemaBuilder,
    TextFieldIndexing, TextOptions,
};

/// Schema fields for document chunk storage.
#[derive(Debug)]
pub struct DocumentSchema {
    /// Document type discriminator (always "chunk" for this index).
    pub doc_type: Field,

    /// Unique identifier for this chunk.
    pub chunk_id: Field,

    /// Collection name for filtering (e.g., "rust-book", "project-docs").
    pub collection_name: Field,

    /// Source file path relative to collection root.
    pub source_path: Field,

    /// Heading hierarchy as JSON array (e.g., ["Chapter 1", "Section 1.2"]).
    pub heading_context: Field,

    /// Full chunk content for search and embedding.
    pub content: Field,

    /// Content preview (first ~200 chars) for display.
    pub content_preview: Field,

    /// Start byte offset in source file.
    pub byte_start: Field,

    /// End byte offset in source file.
    pub byte_end: Field,

    /// Character count for the chunk.
    pub char_count: Field,

    /// File content hash for change detection.
    pub file_hash: Field,

    /// Timestamp when indexed (UTC seconds).
    pub indexed_at: Field,

    /// Metadata key field (for counters, etc.).
    pub meta_key: Field,

    /// Metadata value field.
    pub meta_value: Field,
}

impl DocumentSchema {
    /// Build the schema for document chunk storage.
    pub fn build() -> (Schema, Self) {
        let mut builder = SchemaBuilder::default();

        // Document type discriminator
        let doc_type = builder.add_text_field("doc_type", STRING | STORED | FAST);

        // Numeric options for indexed u64 fields
        let indexed_u64 = NumericOptions::default()
            .set_indexed()
            .set_stored()
            .set_fast();

        // Chunk ID
        let chunk_id = builder.add_u64_field("chunk_id", indexed_u64.clone());

        // Collection name - STRING for exact filtering
        let collection_name = builder.add_text_field("collection_name", STRING | STORED | FAST);

        // Source path - STRING for exact matching
        let source_path = builder.add_text_field("source_path", STRING | STORED);

        // Heading context - TEXT for search, STORED for retrieval
        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let heading_context = builder.add_text_field("heading_context", text_options.clone());

        // Full content - TEXT for full-text search
        let content = builder.add_text_field("content", text_options.clone());

        // Content preview - STORED only, not indexed
        let content_preview = builder.add_text_field("content_preview", STORED);

        // Byte offsets
        let byte_start = builder.add_u64_field("byte_start", STORED);
        let byte_end = builder.add_u64_field("byte_end", STORED);

        // Character count
        let char_count = builder.add_u64_field("char_count", STORED);

        // File hash for change detection
        let file_hash = builder.add_text_field("file_hash", STRING | STORED);

        // Indexed timestamp
        let indexed_at = builder.add_u64_field("indexed_at", STORED | FAST);

        // Metadata fields (for counters like chunk_counter)
        let meta_key = builder.add_text_field("meta_key", STRING | STORED | FAST);
        let meta_value = builder.add_u64_field("meta_value", STORED | FAST);

        let schema = builder.build();

        let document_schema = Self {
            doc_type,
            chunk_id,
            collection_name,
            source_path,
            heading_context,
            content,
            content_preview,
            byte_start,
            byte_end,
            char_count,
            file_hash,
            indexed_at,
            meta_key,
            meta_value,
        };

        (schema, document_schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_build() {
        let (schema, _fields) = DocumentSchema::build();

        // Verify all fields exist
        assert!(schema.get_field("doc_type").is_ok());
        assert!(schema.get_field("chunk_id").is_ok());
        assert!(schema.get_field("collection_name").is_ok());
        assert!(schema.get_field("source_path").is_ok());
        assert!(schema.get_field("heading_context").is_ok());
        assert!(schema.get_field("content").is_ok());
        assert!(schema.get_field("content_preview").is_ok());
        assert!(schema.get_field("byte_start").is_ok());
        assert!(schema.get_field("byte_end").is_ok());

        // Verify field count matches
        assert_eq!(schema.fields().count(), 14);
    }
}
