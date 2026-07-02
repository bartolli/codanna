use tantivy::schema::{
    FAST, Field, IndexRecordOption, NumericOptions, STORED, STRING, Schema, SchemaBuilder,
    TextFieldIndexing, TextOptions,
};

/// Schema fields for the document index
#[derive(Debug)]
pub struct IndexSchema {
    // Document type discriminator
    pub doc_type: Field,

    // Symbol fields
    pub symbol_id: Field,
    pub name: Field,      // STRING field for exact matching
    pub name_text: Field, // TEXT field for full-text search
    pub doc_comment: Field,
    pub signature: Field,
    pub module_path: Field,
    pub kind: Field,
    pub file_path: Field,
    pub line_number: Field,
    pub column: Field,
    pub end_line: Field,
    pub end_column: Field,
    pub context: Field,
    pub visibility: Field,
    pub scope_context: Field,
    pub language: Field, // Language identifier for the symbol

    // Relationship fields
    pub from_symbol_id: Field,
    pub to_symbol_id: Field,
    pub relation_kind: Field,
    pub relation_weight: Field,
    pub relation_line: Field,
    pub relation_column: Field,
    pub relation_context: Field,
    pub relation_receiver: Field,
    pub relation_static_call: Field,

    // File info fields
    pub file_id: Field,
    pub file_hash: Field,
    pub file_timestamp: Field,
    pub file_mtime: Field,

    // Metadata fields
    pub meta_key: Field,
    pub meta_value: Field,

    // Vector search fields
    pub cluster_id: Field,
    pub vector_id: Field,
    pub has_vector: Field,

    // Import fields (for cross-session persistence)
    pub import_file_id: Field,      // Which file has this import
    pub import_path: Field,         // Full import path (e.g., "indicatif::ProgressBar")
    pub import_alias: Field,        // Optional alias
    pub import_is_glob: Field,      // Boolean (0/1) for glob imports
    pub import_is_type_only: Field, // Boolean (0/1) for type-only imports (TypeScript)
}

impl IndexSchema {
    /// Create the schema for indexing code documentation
    pub fn build() -> (Schema, IndexSchema) {
        let mut builder = SchemaBuilder::default();

        // Document type discriminator (for symbols, relationships, files, metadata)
        let doc_type = builder.add_text_field("doc_type", STRING | STORED | FAST);

        // Numeric options for indexed u64 fields
        let indexed_u64_options = NumericOptions::default()
            .set_indexed()
            .set_stored()
            .set_fast();

        // Symbol fields (existing)
        let symbol_id = builder.add_u64_field("symbol_id", indexed_u64_options.clone());
        let file_path = builder.add_text_field("file_path", STRING | STORED);
        let line_number = builder.add_u64_field("line_number", indexed_u64_options.clone());
        let column = builder.add_u64_field("column", STORED);
        let end_line = builder.add_u64_field("end_line", STORED | FAST);
        let end_column = builder.add_u64_field("end_column", STORED);

        // Text fields for search
        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        // IMPORTANT: Use STRING for exact matching of symbol names without tokenization
        // This prevents partial matches and ensures "MyService" doesn't match "Main"
        let name = builder.add_text_field("name", STRING | STORED);

        // ALSO add name_text for full-text search with ngram tokenization
        // This allows partial matching: "Archive" will match "ArchiveAppService"
        let ngram_text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("ngram")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let name_text = builder.add_text_field("name_text", ngram_text_options);

        let doc_comment = builder.add_text_field("doc_comment", text_options.clone());
        let signature = builder.add_text_field("signature", text_options.clone());
        let context = builder.add_text_field("context", text_options.clone());

        // String fields for filtering (using STRING for exact match)
        let module_path = builder.add_text_field("module_path", STRING | STORED);
        let kind = builder.add_text_field("kind", STRING | STORED);
        let visibility = builder.add_u64_field("visibility", STORED);
        let scope_context = builder.add_text_field("scope_context", STRING | STORED);
        let language = builder.add_text_field("language", STRING | STORED | FAST);

        // Relationship fields
        let from_symbol_id = builder.add_u64_field("from_symbol_id", indexed_u64_options.clone());
        let to_symbol_id = builder.add_u64_field("to_symbol_id", indexed_u64_options.clone());
        let relation_kind = builder.add_text_field("relation_kind", STRING | STORED | FAST);
        let relation_weight = builder.add_f64_field("relation_weight", STORED);
        let relation_line = builder.add_u64_field("relation_line", STORED);
        let relation_column = builder.add_u64_field("relation_column", STORED);
        let relation_context = builder.add_text_field("relation_context", text_options.clone());
        let relation_receiver = builder.add_text_field("relation_receiver", text_options.clone());
        let relation_static_call = builder.add_u64_field("relation_static_call", STORED);

        // File info fields
        let file_id = builder.add_u64_field("file_id", indexed_u64_options.clone());
        let file_hash = builder.add_text_field("file_hash", STRING | STORED);
        let file_timestamp = builder.add_u64_field("file_timestamp", STORED | FAST);
        let file_mtime = builder.add_u64_field("file_mtime", STORED | FAST);

        // Metadata fields (for counters, etc.)
        let meta_key = builder.add_text_field("meta_key", STRING | STORED | FAST);
        let meta_value = builder.add_u64_field("meta_value", STORED | FAST);

        // Vector search fields
        let cluster_id = builder.add_u64_field("cluster_id", FAST | STORED);
        let vector_id = builder.add_u64_field("vector_id", FAST | STORED);
        let has_vector = builder.add_u64_field("has_vector", FAST | STORED); // Using u64 as bool for FAST field

        // Import fields (for cross-session persistence of import metadata)
        let import_file_id = builder.add_u64_field("import_file_id", indexed_u64_options.clone());
        let import_path = builder.add_text_field("import_path", STRING | STORED);
        let import_alias = builder.add_text_field("import_alias", STRING | STORED);
        let import_is_glob = builder.add_u64_field("import_is_glob", STORED);
        let import_is_type_only = builder.add_u64_field("import_is_type_only", STORED);

        let schema = builder.build();
        let index_schema = IndexSchema {
            doc_type,
            symbol_id,
            name,
            name_text,
            doc_comment,
            signature,
            module_path,
            kind,
            file_path,
            line_number,
            column,
            end_line,
            end_column,
            context,
            visibility,
            scope_context,
            language,
            from_symbol_id,
            to_symbol_id,
            relation_kind,
            relation_weight,
            relation_line,
            relation_column,
            relation_context,
            relation_receiver,
            relation_static_call,
            file_id,
            file_hash,
            file_timestamp,
            file_mtime,
            meta_key,
            meta_value,
            cluster_id,
            vector_id,
            has_vector,
            import_file_id,
            import_path,
            import_alias,
            import_is_glob,
            import_is_type_only,
        };

        (schema, index_schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_has_language_field() {
        let (schema, _) = IndexSchema::build();

        // Check that language field exists in schema
        let language_field = schema.get_field("language");
        assert!(
            language_field.is_ok(),
            "Schema should have 'language' field"
        );

        // Verify field is configured correctly
        let field = language_field.unwrap();
        let field_entry = schema.get_field_entry(field);
        assert!(field_entry.is_indexed(), "Language field should be indexed");
        assert!(field_entry.is_stored(), "Language field should be stored");
    }
}
