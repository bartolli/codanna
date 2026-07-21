use crate::storage::{MetadataKey, StorageError, StorageResult};
use crate::{FileId, RelationKind, Relationship, SymbolId, SymbolKind};
use std::path::PathBuf;
use tantivy::{
    TantivyDocument as Document, Term,
    collector::{Count, TopDocs},
    query::{BooleanQuery, FuzzyTermQuery, Occur, Query, QueryParser, TermQuery},
    schema::{IndexRecordOption, Value},
};

use super::{DocumentIndex, SearchResult};

impl DocumentIndex {
    /// Search returning every match: count-first, then an exact-limit drain.
    ///
    /// Replaces fixed `with_limit(N)` bounds that silently truncated dense
    /// results (>1000 same-kind edges into one symbol).
    fn search_all(
        searcher: &tantivy::Searcher,
        query: &dyn Query,
    ) -> tantivy::Result<Vec<(f32, tantivy::DocAddress)>> {
        let count = searcher.search(query, &Count)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        searcher.search(query, &TopDocs::with_limit(count).order_by_score())
    }

    /// Search for documents
    pub fn search(
        &self,
        query_str: &str,
        limit: usize,
        kind_filter: Option<SymbolKind>,
        module_filter: Option<&str>,
        language_filter: Option<&str>,
    ) -> StorageResult<Vec<SearchResult>> {
        let searcher = self.reader.searcher();

        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.schema.name_text, // Use name_text for full-text search (tokenized)
                self.schema.doc_comment,
                self.schema.signature,
                self.schema.context,
            ],
        );

        // Try parsing as Tantivy query syntax first, fall back to literal matching
        // for queries with special characters (interface{}, Vec<T>, etc.)
        let main_query = match query_parser.parse_query(query_str) {
            Ok(query) => query,
            Err(_parse_error) => {
                // Query contains syntax that conflicts with Tantivy parser.
                // Fall back to literal term matching across searchable fields.
                let name_term = Term::from_field_text(self.schema.name_text, query_str);
                let doc_term = Term::from_field_text(self.schema.doc_comment, query_str);
                let sig_term = Term::from_field_text(self.schema.signature, query_str);
                let ctx_term = Term::from_field_text(self.schema.context, query_str);

                Box::new(BooleanQuery::new(vec![
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(name_term, IndexRecordOption::Basic))
                            as Box<dyn Query>,
                    ),
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(doc_term, IndexRecordOption::Basic))
                            as Box<dyn Query>,
                    ),
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(sig_term, IndexRecordOption::Basic))
                            as Box<dyn Query>,
                    ),
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(ctx_term, IndexRecordOption::Basic))
                            as Box<dyn Query>,
                    ),
                ])) as Box<dyn Query>
            }
        };

        // Fuzzy query for typo tolerance on the name_text field (ngram tokens)
        let name_text_term = Term::from_field_text(self.schema.name_text, query_str);
        let fuzzy_ngram_query = FuzzyTermQuery::new(name_text_term, 1, true);

        // ADDITIONAL: Fuzzy query on the non-tokenized name field for whole-word typo tolerance
        // This fixes the limitation where "ArchivService" (missing 'e') couldn't find "ArchiveService"
        // because ngram tokenization shifted all tokens after the typo
        let name_term = Term::from_field_text(self.schema.name, query_str);
        let fuzzy_whole_word_query = FuzzyTermQuery::new(name_term, 1, true);

        // All queries will be collected here.
        let mut all_clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        // The text search part: must match one of:
        // 1. Main query (ngram partial matching)
        // 2. Fuzzy on ngram tokens (typos in short queries)
        // 3. Fuzzy on whole word (typos in full symbol names)
        all_clauses.push((
            Occur::Must,
            Box::new(BooleanQuery::new(vec![
                (Occur::Should, main_query),
                (Occur::Should, Box::new(fuzzy_ngram_query)),
                (Occur::Should, Box::new(fuzzy_whole_word_query)),
            ])),
        ));

        // Add mandatory filters.
        all_clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "symbol"),
                IndexRecordOption::Basic,
            )),
        ));

        if let Some(kind) = kind_filter {
            let term = Term::from_field_text(self.schema.kind, &format!("{kind:?}"));
            all_clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }

        if let Some(module) = module_filter {
            let term = Term::from_field_text(self.schema.module_path, module);
            all_clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }

        // Add language filter if provided
        if let Some(lang) = language_filter {
            let term = Term::from_field_text(self.schema.language, lang);
            all_clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }

        let final_query = BooleanQuery::new(all_clauses);

        let top_docs =
            searcher.search(&final_query, &TopDocs::with_limit(limit).order_by_score())?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;

            // Extract fields
            let symbol_id = doc
                .get_first(self.schema.symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let name = doc
                .get_first(self.schema.name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let file_path = doc
                .get_first(self.schema.file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let line = doc
                .get_first(self.schema.line_number)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            let column = doc
                .get_first(self.schema.column)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;

            let doc_comment = doc
                .get_first(self.schema.doc_comment)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let signature = doc
                .get_first(self.schema.signature)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let context = doc
                .get_first(self.schema.context)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Extract kind from its stored Debug representation via the one
            // kind vocabulary (SymbolKind::from_str); a partial hand-rolled
            // map here misreported Class/Interface/Enum rows as Function.
            let kind_str = doc
                .get_first(self.schema.kind)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let kind = kind_str.to_lowercase().parse::<SymbolKind>().map_err(|e| {
                StorageError::InvalidFieldValue {
                    field: "kind".to_string(),
                    reason: e.to_string(),
                }
            })?;

            let module_path = doc
                .get_first(self.schema.module_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let language_id = doc
                .get_first(self.schema.language)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            results.push(SearchResult {
                symbol_id,
                name,
                kind,
                file_path,
                // Stored line_number is the 0-indexed range start; scalar
                // line fields are 1-indexed editor coordinates.
                line: line + 1,
                column,
                language_id,
                doc_comment,
                signature,
                module_path,
                score,
                highlights: Vec::new(), // TODO: Implement highlighting
                context,
            });
        }

        Ok(results)
    }

    /// Get total number of indexed documents
    pub fn document_count(&self) -> StorageResult<u64> {
        let searcher = self.reader.searcher();
        Ok(searcher.num_docs())
    }

    /// Find a symbol by its ID
    pub fn find_symbol_by_id(&self, id: SymbolId) -> StorageResult<Option<crate::Symbol>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_u64(self.schema.symbol_id, id.0 as u64),
            IndexRecordOption::Basic,
        );

        let top_docs = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;

        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;
            Ok(Some(self.document_to_symbol(&doc)?))
        } else {
            Ok(None)
        }
    }

    /// Find a symbol by its ID with language filter
    pub fn find_symbol_by_id_with_language(
        &self,
        id: SymbolId,
        language: &str,
    ) -> StorageResult<Option<crate::Symbol>> {
        let searcher = self.reader.searcher();

        // Build a compound query: symbol_id AND language
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.symbol_id, id.0 as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.language, language),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;

        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;
            Ok(Some(self.document_to_symbol(&doc)?))
        } else {
            Ok(None)
        }
    }

    /// Find symbols by name
    pub fn find_symbols_by_name(
        &self,
        name: &str,
        language_filter: Option<&str>,
    ) -> StorageResult<Vec<crate::Symbol>> {
        let searcher = self.reader.searcher();

        // Use exact term matching for symbol names (name field is STRING type, not TEXT)
        // This prevents tokenization issues that cause "MyService" to match "Main"
        let name_query = Box::new(TermQuery::new(
            Term::from_field_text(self.schema.name, name),
            IndexRecordOption::Basic,
        )) as Box<dyn Query>;

        // Build query clauses
        let mut query_clauses = vec![
            (Occur::Must, name_query),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "symbol"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ];

        // Add language filter if provided
        if let Some(lang) = language_filter {
            query_clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.language, lang),
                    IndexRecordOption::Basic,
                )),
            ));
        }

        let final_query = BooleanQuery::new(query_clauses);

        let top_docs = searcher.search(&final_query, &TopDocs::with_limit(100).order_by_score())?;
        let mut symbols = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            symbols.push(self.document_to_symbol(&doc)?);
        }

        Ok(symbols)
    }

    /// Find a symbol by name, file, and range
    ///
    /// Used for Defines relationships to disambiguate overloaded methods.
    /// Returns the symbol that matches both the name AND the exact range.
    pub fn find_symbol_by_name_and_range(
        &self,
        name: &str,
        file_id: FileId,
        range: &crate::Range,
    ) -> StorageResult<Option<crate::Symbol>> {
        let searcher = self.reader.searcher();

        // Query by name, file_id, and start line
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "symbol"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.name, name),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.file_id, file_id.0 as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.line_number, range.start_line as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(10).order_by_score())?;

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            let symbol = self.document_to_symbol(&doc)?;
            // Verify full range match (start_line matched in query, check end_line too)
            if symbol.range.end_line == range.end_line {
                return Ok(Some(symbol));
            }
        }

        Ok(None)
    }

    /// Find symbols by file ID
    pub fn find_symbols_by_file(&self, file_id: FileId) -> StorageResult<Vec<crate::Symbol>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "symbol"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.file_id, file_id.0 as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = Self::search_all(&searcher, &query)?;
        let mut symbols = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            symbols.push(self.document_to_symbol(&doc)?);
        }

        Ok(symbols)
    }

    /// Find all symbols in a specific module/package
    ///
    /// Used for same-package symbol resolution (Java, Kotlin, etc.)
    /// Returns all symbols that have the specified module_path.
    pub fn find_symbols_by_module(&self, module_path: &str) -> StorageResult<Vec<crate::Symbol>> {
        let searcher = self.reader.searcher();

        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "symbol"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.module_path, module_path),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = Self::search_all(&searcher, &query)?;
        let mut symbols = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            symbols.push(self.document_to_symbol(&doc)?);
        }

        Ok(symbols)
    }

    /// Get all symbols (use with caution on large indexes)
    pub fn get_all_symbols(&self, limit: usize) -> StorageResult<Vec<crate::Symbol>> {
        let searcher = self.reader.searcher();

        // Use pre-filtering query instead of AllQuery + post-filtering
        // This matches the pattern used in find_symbols_by_name and find_symbols_by_file
        let query = BooleanQuery::from(vec![(
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "symbol"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>,
        )]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;

        let mut symbols = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            symbols.push(self.document_to_symbol(&doc)?);
        }

        Ok(symbols)
    }

    /// Get file info by path
    /// Returns (file_id, hash, mtime). Mtime is 0 for legacy entries without mtime.
    pub fn get_file_info(&self, path: &str) -> StorageResult<Option<(FileId, String, u64)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "file_info"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.file_path, path),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;

        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;

            let file_id = doc
                .get_first(self.schema.file_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| FileId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "file_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let hash = doc
                .get_first(self.schema.file_hash)
                .and_then(|v| v.as_str())
                .ok_or(StorageError::InvalidFieldValue {
                    field: "file_hash".to_string(),
                    reason: "missing from document".to_string(),
                })?
                .to_string();

            let mtime = doc
                .get_first(self.schema.file_mtime)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            Ok(Some((file_id, hash, mtime)))
        } else {
            Ok(None)
        }
    }

    /// Get next file ID
    pub fn get_next_file_id(&self) -> StorageResult<u32> {
        // During batch operations, use and increment the pending counter
        if let Ok(mut pending_guard) = self.pending_file_counter.lock() {
            if let Some(ref mut counter) = *pending_guard {
                let next_id = *counter;
                *counter += 1;
                return Ok(next_id);
            }
        }

        // Otherwise, query the committed metadata
        let current = self.query_metadata(MetadataKey::FileCounter)?.unwrap_or(0) as u32;
        Ok(current + 1)
    }

    /// Get next symbol ID
    pub fn get_next_symbol_id(&self) -> StorageResult<u32> {
        // During batch operations, use and increment the pending counter
        if let Ok(mut pending_guard) = self.pending_symbol_counter.lock() {
            if let Some(ref mut counter) = *pending_guard {
                let next_id = *counter;
                *counter += 1;
                return Ok(next_id);
            }
        }

        // Otherwise, query the committed metadata
        let current = self
            .query_metadata(MetadataKey::SymbolCounter)?
            .unwrap_or(0) as u32;
        Ok(current + 1)
    }

    /// Count symbols
    pub fn count_symbols(&self) -> StorageResult<usize> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "symbol"),
            IndexRecordOption::Basic,
        );

        let count = searcher.search(&query, &tantivy::collector::Count)?;
        Ok(count)
    }

    /// Count total number of relationships
    pub fn count_relationships(&self) -> StorageResult<usize> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "relationship"),
            IndexRecordOption::Basic,
        );

        let count = searcher.search(&query, &tantivy::collector::Count)?;
        Ok(count)
    }

    /// Count files
    pub fn count_files(&self) -> StorageResult<usize> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "file_info"),
            IndexRecordOption::Basic,
        );

        let count = searcher.search(&query, &tantivy::collector::Count)?;
        Ok(count)
    }

    /// Get all indexed file paths for file watching
    /// Returns a vector of all file paths currently in the index
    pub fn get_all_indexed_paths(&self) -> StorageResult<Vec<PathBuf>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "file_info"),
            IndexRecordOption::Basic,
        );

        // Use TopDocs to get all file_info documents
        // Note: Adjust limit if you have more than 100k files
        let collector = TopDocs::with_limit(100_000).order_by_score();
        let top_docs = searcher.search(&query, &collector)?;

        let mut paths = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;

            // Extract file_path field
            if let Some(path_value) = doc.get_first(self.schema.file_path) {
                if let Some(path_str) = path_value.as_str() {
                    paths.push(PathBuf::from(path_str));
                }
            }
        }

        Ok(paths)
    }

    /// Get relationships from a symbol
    pub fn get_relationships_from(
        &self,
        from_id: SymbolId,
        kind: RelationKind,
    ) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "relationship"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.from_symbol_id, from_id.0 as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.relation_kind, &format!("{kind:?}")),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = Self::search_all(&searcher, &query)?;
        let mut relationships = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;

            let to_id = doc
                .get_first(self.schema.to_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "to_symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let mut relationship = Relationship::new(kind);
            if let Some(metadata) = self.metadata_for_relationship_doc(&doc) {
                relationship = relationship.with_metadata(metadata);
            }

            relationships.push((from_id, to_id, relationship));
        }

        Ok(relationships)
    }

    /// Get relationships to a symbol
    pub fn get_relationships_to(
        &self,
        to_id: SymbolId,
        kind: RelationKind,
    ) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "relationship"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.to_symbol_id, to_id.0 as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.relation_kind, &format!("{kind:?}")),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = Self::search_all(&searcher, &query)?;
        let mut relationships = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;

            let from_id = doc
                .get_first(self.schema.from_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "from_symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let mut relationship = Relationship::new(kind);
            if let Some(metadata) = self.metadata_for_relationship_doc(&doc) {
                relationship = relationship.with_metadata(metadata);
            }

            relationships.push((from_id, to_id, relationship));
        }

        Ok(relationships)
    }

    /// Get all relationships of a specific kind
    pub fn get_all_relationships_by_kind(
        &self,
        kind: RelationKind,
    ) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "relationship"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.relation_kind, &format!("{kind:?}")),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(10000).order_by_score())?;
        let mut relationships = Vec::new();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;

            let from_id = doc
                .get_first(self.schema.from_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "from_symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let to_id = doc
                .get_first(self.schema.to_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "to_symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            relationships.push((from_id, to_id, Relationship::new(kind)));
        }

        Ok(relationships)
    }

    /// Get file path by ID
    pub fn get_file_path(&self, file_id: FileId) -> StorageResult<Option<String>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "file_info"),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.file_id, file_id.0 as u64),
                    IndexRecordOption::Basic,
                )) as Box<dyn Query>,
            ),
        ]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;

        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;

            let path = doc
                .get_first(self.schema.file_path)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            Ok(path)
        } else {
            Ok(None)
        }
    }

    /// Get all imports for a specific file
    ///
    /// Returns raw import metadata - resolution happens in the resolution layer.
    pub fn get_imports_for_file(
        &self,
        file_id: FileId,
    ) -> StorageResult<Vec<crate::parsing::Import>> {
        let query = BooleanQuery::new(vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.doc_type, "import"),
                    IndexRecordOption::Basic,
                )),
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.schema.import_file_id, file_id.value() as u64),
                    IndexRecordOption::Basic,
                )),
            ),
        ]);

        let searcher = self.reader.searcher();
        let top_docs = Self::search_all(&searcher, &query)
            .map_err(|e| StorageError::General(format!("Import search failed: {e}")))?;

        let mut imports = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address).map_err(|e| {
                StorageError::General(format!("Failed to retrieve import document: {e}"))
            })?;

            // Extract fields from document
            let import_path = doc
                .get_first(self.schema.import_path)
                .and_then(|v| v.as_str())
                .ok_or_else(|| StorageError::General("Missing import_path".to_string()))?
                .to_string();

            let alias = doc
                .get_first(self.schema.import_alias)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let is_glob = doc
                .get_first(self.schema.import_is_glob)
                .and_then(|v| v.as_u64())
                .map(|v| v == 1)
                .unwrap_or(false);

            let is_type_only = doc
                .get_first(self.schema.import_is_type_only)
                .and_then(|v| v.as_u64())
                .map(|v| v == 1)
                .unwrap_or(false);

            imports.push(crate::parsing::Import {
                path: import_path,
                alias,
                file_id,
                is_glob,
                is_type_only,
            });
        }

        Ok(imports)
    }

    /// Query all relationships from the index
    #[cfg(test)]
    pub(crate) fn query_relationships(
        &self,
    ) -> StorageResult<Vec<(SymbolId, SymbolId, crate::Relationship)>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "relationship"),
            IndexRecordOption::Basic,
        );

        // Use a collector that retrieves all documents
        let collector = TopDocs::with_limit(1_000_000).order_by_score(); // Adjust limit as needed
        let top_docs = searcher.search(&query, &collector)?;

        let mut relationships = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;

            let from_id = doc
                .get_first(self.schema.from_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "from_symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let to_id = doc
                .get_first(self.schema.to_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "to_symbol_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let kind_str = doc
                .get_first(self.schema.relation_kind)
                .and_then(|v| v.as_str())
                .ok_or(StorageError::InvalidFieldValue {
                    field: "relation_kind".to_string(),
                    reason: "missing from document".to_string(),
                })?;

            let weight = doc
                .get_first(self.schema.relation_weight)
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;

            // Parse RelationKind from string
            let kind = match kind_str {
                "Calls" => RelationKind::Calls,
                "CalledBy" => RelationKind::CalledBy,
                "Extends" => RelationKind::Extends,
                "ExtendedBy" => RelationKind::ExtendedBy,
                "Implements" => RelationKind::Implements,
                "ImplementedBy" => RelationKind::ImplementedBy,
                "Uses" => RelationKind::Uses,
                "UsedBy" => RelationKind::UsedBy,
                "Defines" => RelationKind::Defines,
                "DefinedIn" => RelationKind::DefinedIn,
                "References" => RelationKind::References,
                "ReferencedBy" => RelationKind::ReferencedBy,
                _ => continue, // Skip unknown relation kinds
            };

            let mut relationship = Relationship::new(kind).with_weight(weight);

            if let Some(metadata) = self.metadata_for_relationship_doc(&doc) {
                relationship = relationship.with_metadata(metadata);
            }

            relationships.push((from_id, to_id, relationship));
        }

        Ok(relationships)
    }

    /// Query all file information from the index
    #[cfg(test)]
    pub(crate) fn query_file_info(&self) -> StorageResult<Vec<(FileId, String, String, u64)>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "file_info"),
            IndexRecordOption::Basic,
        );

        let collector = TopDocs::with_limit(100_000).order_by_score(); // Adjust as needed
        let top_docs = searcher.search(&query, &collector)?;

        let mut files = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;

            let file_id = doc
                .get_first(self.schema.file_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| FileId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue {
                    field: "file_id".to_string(),
                    reason: "not a valid u32".to_string(),
                })?;

            let path = doc
                .get_first(self.schema.file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let hash = doc
                .get_first(self.schema.file_hash)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let timestamp = doc
                .get_first(self.schema.file_timestamp)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            files.push((file_id, path, hash, timestamp));
        }

        Ok(files)
    }

    /// Query metadata value by key
    pub(crate) fn query_metadata(&self, key: MetadataKey) -> StorageResult<Option<u64>> {
        let searcher = self.reader.searcher();

        // Build a compound query for doc_type="metadata" AND meta_key=key
        let doc_type_query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "metadata"),
            IndexRecordOption::Basic,
        );
        let key_query = TermQuery::new(
            Term::from_field_text(self.schema.meta_key, key.as_str()),
            IndexRecordOption::Basic,
        );

        let query = BooleanQuery::new(vec![
            (Occur::Must, Box::new(doc_type_query) as Box<dyn Query>),
            (Occur::Must, Box::new(key_query) as Box<dyn Query>),
        ]);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;

        if let Some((_score, doc_address)) = top_docs.into_iter().next() {
            let doc: Document = searcher.doc(doc_address)?;
            let value = doc
                .get_first(self.schema.meta_value)
                .and_then(|v| v.as_u64());
            Ok(value)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::pipeline::FileRegistration;
    use crate::parsing::registry::LanguageId;
    use crate::relationship::RelationshipMetadata;
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_add_and_search_document() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        // Add a document
        let symbol_id = SymbolId::new(1).unwrap();
        let file_id = FileId::new(1).unwrap();
        let sym = crate::Symbol::new(
            symbol_id,
            "parse_json",
            SymbolKind::Function,
            file_id,
            crate::Range::new(42, 5, 50, 0),
        )
        .with_doc("Parse JSON string into a Value")
        .with_signature("fn parse_json(input: &str) -> StorageResult<Value, Error>")
        .with_module_path("crate::parser")
        .with_visibility(crate::Visibility::Public)
        .with_scope(crate::ScopeContext::Module);
        index.add_document(&sym, "src/parser.rs").unwrap();

        // Commit batch
        index.commit_batch().unwrap();

        // Search for it
        let results = index.search("json", 10, None, None, None).unwrap();
        assert_eq!(results.len(), 1);

        let result = &results[0];
        assert_eq!(result.name, "parse_json");
        // Scalar line fields are 1-indexed editor coordinates: stored
        // range.start_line 42 emits as 43.
        assert_eq!(result.line, 43);
        assert_eq!(result.file_path, "src/parser.rs");
    }

    #[test]
    fn test_store_and_retrieve_symbol_with_language() {
        use crate::parsing::registry::LanguageId;

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        // Create a symbol with language_id
        let symbol = crate::Symbol::new(
            SymbolId::new(1).unwrap(),
            "test_func",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(10, 0, 15, 0),
        )
        .with_language_id(LanguageId::new("rust"))
        .with_signature("fn test_func() -> Result<()>")
        .with_doc("Test function");

        // Store the symbol
        index.index_symbol(&symbol, "src/test.rs").unwrap();
        index.commit_batch().unwrap();

        // Retrieve the symbol
        let retrieved = index.find_symbol_by_id(symbol.id).unwrap();
        assert!(retrieved.is_some());

        let retrieved_symbol = retrieved.unwrap();
        // Language ID is now properly stored and retrieved through the registry
        assert_eq!(retrieved_symbol.language_id, Some(LanguageId::new("rust")));
        assert_eq!(retrieved_symbol.name.as_ref(), "test_func");
        assert_eq!(
            retrieved_symbol.signature.as_deref(),
            Some("fn test_func() -> Result<()>")
        );
    }

    #[test]
    fn test_fuzzy_search() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        let symbol_id = SymbolId::new(1).unwrap();
        let file_id = FileId::new(1).unwrap();
        let sym = crate::Symbol::new(
            symbol_id,
            "handle_request",
            SymbolKind::Function,
            file_id,
            crate::Range::new(100, 0, 120, 0),
        )
        .with_doc("Handle incoming HTTP request")
        .with_module_path("crate::server")
        .with_visibility(crate::Visibility::Private)
        .with_scope(crate::ScopeContext::Module);
        index.add_document(&sym, "src/server.rs").unwrap();

        // Commit batch
        index.commit_batch().unwrap();

        // Search with typo - try searching for a single word with typo
        let results = index.search("handle", 10, None, None, None).unwrap();
        assert!(!results.is_empty(), "Should find exact match");

        // Now try with a small typo
        let results = index.search("handl", 10, None, None, None).unwrap();
        assert!(!results.is_empty(), "Should find with fuzzy search");
    }

    #[test]
    fn test_relationship_storage() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let rel = crate::Relationship::new(crate::RelationKind::Calls).with_weight(0.8);

        index.store_relationship(from_id, to_id, &rel).unwrap();

        // Commit batch
        index.commit_batch().unwrap();

        // Query relationships
        let relationships = index.query_relationships().unwrap();
        assert_eq!(relationships.len(), 1);

        let (f, t, r) = &relationships[0];
        assert_eq!(*f, from_id);
        assert_eq!(*t, to_id);
        assert_eq!(r.kind, crate::RelationKind::Calls);
        assert_eq!(r.weight, 0.8);
    }

    #[test]
    fn test_relation_metadata_roundtrip_from_query() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let meta = RelationshipMetadata::new()
            .at_position(10, 4)
            .with_receiver("RawSymbol")
            .static_call(true);
        let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);

        index.store_relationship(from_id, to_id, &rel).unwrap();
        index.commit_batch().unwrap();

        let rels = index
            .get_relationships_from(from_id, crate::RelationKind::Calls)
            .unwrap();
        assert_eq!(rels.len(), 1);

        let (_, _, r) = &rels[0];
        let m = r.metadata.as_ref().expect("metadata should be present");
        assert_eq!(m.line, Some(10));
        assert_eq!(m.column, Some(4));
        assert_eq!(m.receiver.as_deref(), Some("RawSymbol"));
        assert!(m.static_call);
    }

    #[test]
    fn test_relation_metadata_roundtrip_to_query() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let meta = RelationshipMetadata::new()
            .at_position(10, 4)
            .with_receiver("RawSymbol")
            .static_call(true);
        let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);

        index.store_relationship(from_id, to_id, &rel).unwrap();
        index.commit_batch().unwrap();

        let rels = index
            .get_relationships_to(to_id, crate::RelationKind::Calls)
            .unwrap();
        assert_eq!(rels.len(), 1);

        let (_, _, r) = &rels[0];
        let m = r.metadata.as_ref().expect("metadata should be present");
        assert_eq!(m.receiver.as_deref(), Some("RawSymbol"));
        assert!(m.static_call);
    }

    #[test]
    fn test_relation_metadata_roundtrip_all_query() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let meta = RelationshipMetadata::new()
            .at_position(10, 4)
            .with_receiver("RawSymbol")
            .static_call(true);
        let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);

        index.store_relationship(from_id, to_id, &rel).unwrap();
        index.commit_batch().unwrap();

        let rels = index.query_relationships().unwrap();
        assert_eq!(rels.len(), 1);

        let (_, _, r) = &rels[0];
        let m = r.metadata.as_ref().expect("metadata should be present");
        assert_eq!(m.receiver.as_deref(), Some("RawSymbol"));
        assert!(m.static_call);
    }

    #[test]
    fn test_relation_metadata_persists_across_reopen() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let path = temp_dir.path().to_path_buf();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();

        {
            let index = DocumentIndex::new(&path, &settings).unwrap();
            index.start_batch().unwrap();
            let meta = RelationshipMetadata::new()
                .at_position(10, 4)
                .with_receiver("RawSymbol")
                .static_call(true);
            let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);
            index.store_relationship(from_id, to_id, &rel).unwrap();
            index.commit_batch().unwrap();
        } // drop the writer; release the directory lock

        let reopened = DocumentIndex::new(&path, &settings).unwrap();
        let rels = reopened
            .get_relationships_from(from_id, crate::RelationKind::Calls)
            .unwrap();
        assert_eq!(rels.len(), 1, "relationship must persist across reopen");

        let (_, _, r) = &rels[0];
        let m = r
            .metadata
            .as_ref()
            .expect("metadata must persist across reopen");
        assert_eq!(m.line, Some(10));
        assert_eq!(m.column, Some(4));
        assert_eq!(m.receiver.as_deref(), Some("RawSymbol"));
        assert!(m.static_call);
    }

    #[test]
    fn test_relation_metadata_legacy_shape_across_reopen() {
        // In-band proxy for "legacy `.codanna/index/` data written before
        // slice 2": same on-disk doc shape (no `relation_receiver`,
        // no `relation_static_call`), confirms additive schema reads back
        // with defaults after a full close/reopen.
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let path = temp_dir.path().to_path_buf();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();

        {
            let index = DocumentIndex::new(&path, &settings).unwrap();
            index.start_batch().unwrap();
            let meta = RelationshipMetadata::new().at_position(7, 2);
            let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);
            index.store_relationship(from_id, to_id, &rel).unwrap();
            index.commit_batch().unwrap();
        }

        let reopened = DocumentIndex::new(&path, &settings).unwrap();
        let rels = reopened
            .get_relationships_from(from_id, crate::RelationKind::Calls)
            .unwrap();
        assert_eq!(rels.len(), 1);

        let (_, _, r) = &rels[0];
        let m = r
            .metadata
            .as_ref()
            .expect("metadata must persist across reopen");
        assert_eq!(m.line, Some(7));
        assert_eq!(m.column, Some(2));
        assert_eq!(
            m.receiver, None,
            "legacy-shape doc must read back with receiver=None"
        );
        assert!(
            !m.static_call,
            "legacy-shape doc must read back with static_call=false"
        );
    }

    #[test]
    fn test_file_info_storage() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        let file_id = crate::FileId::new(1).unwrap();
        let registration = FileRegistration {
            path: PathBuf::from("src/main.rs"),
            file_id,
            content_hash: "abc123".to_string(),
            language_id: LanguageId::new("rust"),
            timestamp: 1234567890,
            mtime: 0,
        };
        index.store_file_registration(&registration).unwrap();

        // Commit batch
        index.commit_batch().unwrap();

        // Query file info
        let files = index.query_file_info().unwrap();
        assert_eq!(files.len(), 1);

        let (id, path, hash, timestamp) = &files[0];
        assert_eq!(*id, file_id);
        assert_eq!(path, "src/main.rs");
        assert_eq!(hash, "abc123");
        assert_eq!(*timestamp, 1234567890);
    }

    #[test]
    fn test_get_all_indexed_paths() {
        println!("=== TEST: get_all_indexed_paths() ===");

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Initially should have no paths
        println!("Step 1: Testing empty index...");
        let paths = index.get_all_indexed_paths().unwrap();
        assert_eq!(paths.len(), 0);
        println!("  ✓ Empty index returns 0 paths");

        // Add some file info documents
        println!("\nStep 2: Adding file info documents...");
        index.start_batch().unwrap();

        // Add multiple files with different paths
        let test_files = vec![
            (1, "src/main.rs", "hash1"),
            (2, "src/lib.rs", "hash2"),
            (3, "tests/integration.rs", "hash3"),
            (4, "src/module/helper.rs", "hash4"),
            (5, "benches/benchmark.rs", "hash5"),
        ];

        for (id, path, hash) in &test_files {
            let file_id = crate::FileId::new(*id).unwrap();
            let registration = FileRegistration {
                path: PathBuf::from(*path),
                file_id,
                content_hash: hash.to_string(),
                language_id: LanguageId::new("rust"),
                timestamp: 1234567890,
                mtime: 0,
            };
            index.store_file_registration(&registration).unwrap();
            println!("  - Added: {path}");
        }

        index.commit_batch().unwrap();
        println!("  ✓ Added {} file info documents", test_files.len());

        // Now get all paths
        println!("\nStep 3: Retrieving all indexed paths...");
        let paths = index.get_all_indexed_paths().unwrap();

        println!("  Retrieved {} paths:", paths.len());
        for (i, path) in paths.iter().enumerate() {
            println!("    [{}] {}", i + 1, path.display());
        }

        // Verify we got all paths
        assert_eq!(paths.len(), test_files.len());
        println!("  ✓ Correct number of paths returned");

        // Verify all expected paths are present
        let path_strings: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        for (_, expected_path, _) in &test_files {
            assert!(
                path_strings.contains(&expected_path.to_string()),
                "Missing path: {expected_path}"
            );
        }
        println!("  ✓ All expected paths are present");

        // Add a symbol document (should not appear in paths)
        println!("\nStep 4: Adding a symbol document (should be ignored)...");
        index.start_batch().unwrap();

        let symbol_id = SymbolId::new(100).unwrap();
        let file_id = FileId::new(1).unwrap();
        let sym = crate::Symbol::new(
            symbol_id,
            "test_function",
            SymbolKind::Function,
            file_id,
            crate::Range::new(42, 5, 50, 0),
        )
        .with_doc("Test function")
        .with_signature("fn test_function()")
        .with_module_path("crate")
        .with_visibility(crate::Visibility::Public)
        .with_scope(crate::ScopeContext::Module);
        index.add_document(&sym, "src/main.rs").unwrap();

        index.commit_batch().unwrap();
        println!("  - Added symbol document");

        // Verify paths count hasn't changed (symbols are not files)
        let paths_after_symbol = index.get_all_indexed_paths().unwrap();
        assert_eq!(paths_after_symbol.len(), test_files.len());
        println!("  ✓ Symbol documents correctly ignored");

        println!("\n=== TEST PASSED: get_all_indexed_paths() works correctly ===");
    }

    #[test]
    fn test_metadata_storage() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        index.store_metadata(MetadataKey::FileCounter, 42).unwrap();
        index
            .store_metadata(MetadataKey::SymbolCounter, 100)
            .unwrap();

        // Commit batch
        index.commit_batch().unwrap();

        // Query metadata
        assert_eq!(
            index.query_metadata(MetadataKey::FileCounter).unwrap(),
            Some(42)
        );
        assert_eq!(
            index.query_metadata(MetadataKey::SymbolCounter).unwrap(),
            Some(100)
        );
    }

    #[test]
    fn test_find_symbols_by_name_with_language_filter() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        // Add symbols in different languages
        // Rust main function
        let sym = crate::Symbol::new(
            SymbolId::new(1).unwrap(),
            "main",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(0, 0, 5, 0),
        )
        .with_doc("Entry point")
        .with_signature("fn main() {}")
        .with_module_path("crate")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("rust"));
        index.add_document(&sym, "src/main.rs").unwrap();

        // Python main function
        let sym = crate::Symbol::new(
            SymbolId::new(2).unwrap(),
            "main",
            SymbolKind::Function,
            FileId::new(2).unwrap(),
            crate::Range::new(0, 0, 5, 0),
        )
        .with_doc("Python entry point")
        .with_signature("def main():")
        .with_module_path("__main__")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("python"));
        index.add_document(&sym, "src/main.py").unwrap();

        // TypeScript main function
        let sym = crate::Symbol::new(
            SymbolId::new(3).unwrap(),
            "main",
            SymbolKind::Function,
            FileId::new(3).unwrap(),
            crate::Range::new(0, 0, 5, 0),
        )
        .with_doc("TypeScript entry")
        .with_signature("function main(): void")
        .with_module_path("app")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("typescript"));
        index.add_document(&sym, "src/main.ts").unwrap();

        // Commit the batch
        index.commit_batch().unwrap();

        println!("\n=== Testing find_symbols_by_name with language filtering ===");

        // Test 1: Find all symbols named "main" without language filter
        let all_symbols = index.find_symbols_by_name("main", None).unwrap();
        println!("Test 1 - No filter: Found {} symbols", all_symbols.len());
        for symbol in &all_symbols {
            println!("  - Symbol ID: {:?}, File: {}", symbol.id, symbol.file_id.0);
        }
        assert_eq!(
            all_symbols.len(),
            3,
            "Should find 3 'main' functions across all languages"
        );

        // Test 2: Find only Rust symbols
        let rust_symbols = index.find_symbols_by_name("main", Some("rust")).unwrap();
        println!("Test 2 - Rust filter: Found {} symbols", rust_symbols.len());
        for symbol in &rust_symbols {
            println!(
                "  - Symbol ID: {:?}, Module: {:?}",
                symbol.id, symbol.module_path
            );
        }
        assert_eq!(rust_symbols.len(), 1, "Should find 1 Rust 'main' function");
        assert_eq!(rust_symbols[0].id, SymbolId::new(1).unwrap());

        // Test 3: Find only Python symbols
        let python_symbols = index.find_symbols_by_name("main", Some("python")).unwrap();
        println!(
            "Test 3 - Python filter: Found {} symbols",
            python_symbols.len()
        );
        for symbol in &python_symbols {
            println!(
                "  - Symbol ID: {:?}, Module: {:?}",
                symbol.id, symbol.module_path
            );
        }
        assert_eq!(
            python_symbols.len(),
            1,
            "Should find 1 Python 'main' function"
        );
        assert_eq!(python_symbols[0].id, SymbolId::new(2).unwrap());

        // Test 4: Find only TypeScript symbols
        let ts_symbols = index
            .find_symbols_by_name("main", Some("typescript"))
            .unwrap();
        println!(
            "Test 4 - TypeScript filter: Found {} symbols",
            ts_symbols.len()
        );
        for symbol in &ts_symbols {
            println!(
                "  - Symbol ID: {:?}, Module: {:?}",
                symbol.id, symbol.module_path
            );
        }
        assert_eq!(
            ts_symbols.len(),
            1,
            "Should find 1 TypeScript 'main' function"
        );
        assert_eq!(ts_symbols[0].id, SymbolId::new(3).unwrap());

        // Test 5: Find symbols with non-existent language (should return empty)
        let java_symbols = index.find_symbols_by_name("main", Some("java")).unwrap();
        println!(
            "Test 5 - Java filter (non-existent): Found {} symbols",
            java_symbols.len()
        );
        assert_eq!(java_symbols.len(), 0, "Should find no Java symbols");

        println!("=== All find_symbols_by_name tests completed ===\n");
    }

    #[test]
    fn test_search_with_language_filter() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        // Add symbols with "parse" in different languages
        // Rust parse function
        let sym = crate::Symbol::new(
            SymbolId::new(10).unwrap(),
            "parse_config",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(10, 0, 20, 0),
        )
        .with_doc("Parse configuration from file")
        .with_signature("fn parse_config(path: &str) -> Config")
        .with_module_path("crate::config")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("rust"));
        index.add_document(&sym, "src/config.rs").unwrap();

        // Python parse function
        let sym = crate::Symbol::new(
            SymbolId::new(11).unwrap(),
            "parse_json",
            SymbolKind::Function,
            FileId::new(2).unwrap(),
            crate::Range::new(5, 0, 10, 0),
        )
        .with_doc("Parse JSON data")
        .with_signature("def parse_json(data: str) -> dict")
        .with_module_path("parser")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("python"));
        index.add_document(&sym, "src/parser.py").unwrap();

        // TypeScript parse function
        let sym = crate::Symbol::new(
            SymbolId::new(12).unwrap(),
            "parseXML",
            SymbolKind::Function,
            FileId::new(3).unwrap(),
            crate::Range::new(1, 0, 8, 0),
        )
        .with_doc("Parse XML string")
        .with_signature("function parseXML(xml: string): Document")
        .with_module_path("utils.parser")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("typescript"));
        index.add_document(&sym, "src/parser.ts").unwrap();

        // Commit the batch
        index.commit_batch().unwrap();

        println!("\n=== Testing search with language filtering ===");

        // Test 1: Search for "parse" without language filter
        let all_results = index.search("parse", 10, None, None, None).unwrap();
        println!(
            "Test 1 - Search 'parse' no filter: Found {} results",
            all_results.len()
        );
        for result in &all_results {
            println!(
                "  - Symbol ID: {:?}, Name: {}",
                result.symbol_id, result.name
            );
        }
        assert_eq!(
            all_results.len(),
            3,
            "Should find 3 parse functions across all languages"
        );

        // Test 2: Search for "parse" in Rust only
        let rust_results = index.search("parse", 10, None, None, Some("rust")).unwrap();
        println!(
            "Test 2 - Search 'parse' Rust filter: Found {} results",
            rust_results.len()
        );
        for result in &rust_results {
            println!(
                "  - Symbol ID: {:?}, Name: {}",
                result.symbol_id, result.name
            );
        }
        assert_eq!(rust_results.len(), 1, "Should find 1 Rust parse function");
        assert_eq!(rust_results[0].symbol_id, SymbolId::new(10).unwrap());

        // Test 3: Search for "parse" in Python only
        let python_results = index
            .search("parse", 10, None, None, Some("python"))
            .unwrap();
        println!(
            "Test 3 - Search 'parse' Python filter: Found {} results",
            python_results.len()
        );
        for result in &python_results {
            println!(
                "  - Symbol ID: {:?}, Name: {}",
                result.symbol_id, result.name
            );
        }
        assert_eq!(
            python_results.len(),
            1,
            "Should find 1 Python parse function"
        );
        assert_eq!(python_results[0].symbol_id, SymbolId::new(11).unwrap());

        // Test 4: Combine language filter with kind filter
        let rust_functions = index
            .search("parse", 10, Some(SymbolKind::Function), None, Some("rust"))
            .unwrap();
        println!(
            "Test 4 - Search 'parse' Rust+Function filter: Found {} results",
            rust_functions.len()
        );
        assert_eq!(
            rust_functions.len(),
            1,
            "Should find 1 Rust function with 'parse'"
        );

        // Test 5: Search with language that has no matches
        let java_results = index.search("parse", 10, None, None, Some("java")).unwrap();
        println!(
            "Test 5 - Search 'parse' Java filter (non-existent): Found {} results",
            java_results.len()
        );
        assert_eq!(java_results.len(), 0, "Should find no Java parse functions");

        println!("=== All search tests completed ===\n");
    }

    #[test]
    fn test_language_filter_with_module_filter() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        // Start batch
        index.start_batch().unwrap();

        // Add symbols with same module name but different languages
        let sym = crate::Symbol::new(
            SymbolId::new(20).unwrap(),
            "Handler",
            SymbolKind::Struct,
            FileId::new(1).unwrap(),
            crate::Range::new(1, 0, 10, 0),
        )
        .with_doc("Request handler")
        .with_signature("struct Handler")
        .with_module_path("server")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("rust"));
        index.add_document(&sym, "src/server.rs").unwrap();

        let sym = crate::Symbol::new(
            SymbolId::new(21).unwrap(),
            "Handler",
            SymbolKind::Class,
            FileId::new(2).unwrap(),
            crate::Range::new(1, 0, 12, 0),
        )
        .with_doc("Request handler class")
        .with_signature("class Handler")
        .with_module_path("server")
        .with_visibility(crate::Visibility::Public)
        .with_language_id(LanguageId::new("python"));
        index.add_document(&sym, "src/server.py").unwrap();

        // Commit the batch
        index.commit_batch().unwrap();

        println!("\n=== Testing combined module and language filters ===");

        // Test combining module and language filters
        let rust_server = index
            .search("Handler", 10, None, Some("server"), Some("rust"))
            .unwrap();
        println!(
            "Test 1 - Search 'Handler' in server module + Rust: Found {} results",
            rust_server.len()
        );
        for result in &rust_server {
            println!(
                "  - Symbol ID: {:?}, Kind: {:?}",
                result.symbol_id, result.kind
            );
        }
        assert_eq!(
            rust_server.len(),
            1,
            "Should find 1 Rust Handler in server module"
        );
        assert_eq!(rust_server[0].symbol_id, SymbolId::new(20).unwrap());

        let python_server = index
            .search("Handler", 10, None, Some("server"), Some("python"))
            .unwrap();
        println!(
            "Test 2 - Search 'Handler' in server module + Python: Found {} results",
            python_server.len()
        );
        for result in &python_server {
            println!(
                "  - Symbol ID: {:?}, Kind: {:?}",
                result.symbol_id, result.kind
            );
        }
        assert_eq!(
            python_server.len(),
            1,
            "Should find 1 Python Handler in server module"
        );
        assert_eq!(python_server[0].symbol_id, SymbolId::new(21).unwrap());

        println!("=== All combined filter tests completed ===\n");
    }

    #[test]
    fn test_ngram_partial_matching() {
        println!("\n=== NGRAM TOKENIZER TEST ===\n");

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        // Add C# symbols with typical naming patterns
        let file_id = crate::FileId::new(1).unwrap();

        println!("Step 1: Indexing symbols...");

        // Symbol 1: ArchiveAppService (should match "Archive" query)
        let sym1 = crate::Symbol::new(
            SymbolId::new(1).unwrap(),
            "ArchiveAppService",
            SymbolKind::Class,
            file_id,
            crate::Range::new(10, 5, 50, 10),
        )
        .with_module_path("Services")
        .with_doc("Application service for archiving")
        .with_signature("class ArchiveAppService");

        println!("  - Indexed: ArchiveAppService");
        index
            .index_symbol(&sym1, "src/Services/ArchiveAppService.cs")
            .unwrap();

        // Symbol 2: DocumentArchiver (should match "Archive" query)
        let sym2 = crate::Symbol::new(
            SymbolId::new(2).unwrap(),
            "DocumentArchiver",
            SymbolKind::Class,
            file_id,
            crate::Range::new(20, 5, 60, 10),
        )
        .with_module_path("Utils")
        .with_doc("Archives documents")
        .with_signature("class DocumentArchiver");

        println!("  - Indexed: DocumentArchiver");
        index
            .index_symbol(&sym2, "src/Utils/DocumentArchiver.cs")
            .unwrap();

        // Symbol 3: UserService (should NOT match "Archive" query)
        let sym3 = crate::Symbol::new(
            SymbolId::new(3).unwrap(),
            "UserService",
            SymbolKind::Class,
            file_id,
            crate::Range::new(30, 5, 70, 10),
        )
        .with_module_path("Services")
        .with_doc("User management service")
        .with_signature("class UserService");

        println!("  - Indexed: UserService");
        index
            .index_symbol(&sym3, "src/Services/UserService.cs")
            .unwrap();

        index.commit_batch().unwrap();
        println!("\nStep 2: Testing partial search with 'Archive'...");

        // Test partial matching with "Archive" using search() method
        let results = index.search("Archive", 10, None, None, None).unwrap();

        println!("\nResults from search('Archive'):");
        for (i, result) in results.iter().enumerate() {
            let kind = format!("{:?}", result.kind);
            println!("  {}. {} ({})", i + 1, result.name, kind);
        }

        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();

        println!(
            "\nExpectation: Should find 'ArchiveAppService' and 'DocumentArchiver', NOT 'UserService'"
        );
        println!("Actual matches: {names:?}\n");

        // Should find both ArchiveAppService and DocumentArchiver
        assert!(
            names.contains(&"ArchiveAppService"),
            "Ngram tokenizer should find ArchiveAppService with partial query 'Archive'. Found: {names:?}"
        );
        assert!(
            names.contains(&"DocumentArchiver"),
            "Ngram tokenizer should find DocumentArchiver with partial query 'Archive'. Found: {names:?}"
        );

        // Should NOT find UserService
        assert!(
            !names.contains(&"UserService"),
            "Should not match unrelated symbols. Found: {names:?}"
        );

        println!("Step 3: Testing exact lookup with 'ArchiveAppService'...");

        // Test exact lookup still works (uses STRING field, not ngram)
        let exact_results = index
            .find_symbols_by_name("ArchiveAppService", None)
            .unwrap();
        println!("Exact lookup results: {} match(es)", exact_results.len());
        for result in &exact_results {
            println!("  - {}", result.name);
        }

        assert_eq!(exact_results.len(), 1);
        assert_eq!(exact_results[0].name.as_ref(), "ArchiveAppService");

        println!(
            "\nStep 4: Testing exact lookup with partial name 'Archive' (should find nothing)..."
        );

        // Test that exact lookup doesn't return partial matches
        let no_match = index.find_symbols_by_name("Archive", None).unwrap();
        println!("Exact lookup for 'Archive': {} match(es)", no_match.len());

        assert_eq!(
            no_match.len(),
            0,
            "Exact lookup should not return partial matches. Found: {:?}",
            no_match.iter().map(|s| &s.name).collect::<Vec<_>>()
        );

        println!("\n=== NGRAM TEST PASSED ===\n");
    }

    #[test]
    fn test_fuzzy_search_typo_tolerance() {
        println!("\n=== FUZZY SEARCH TEST (Typo Tolerance) ===\n");

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let file_id = crate::FileId::new(1).unwrap();

        println!("Step 1: Indexing symbol 'ArchiveService'...");
        let sym = crate::Symbol::new(
            SymbolId::new(1).unwrap(),
            "ArchiveService",
            SymbolKind::Class,
            file_id,
            crate::Range::new(10, 5, 50, 10),
        )
        .with_doc("Archive service");

        index.index_symbol(&sym, "src/ArchiveService.cs").unwrap();
        index.commit_batch().unwrap();

        println!("\nStep 2: Testing fuzzy search with typos...\n");

        // Test 1: Correct spelling
        println!("Query: 'ArchiveService' (correct spelling)");
        let correct = index
            .search("ArchiveService", 10, None, None, None)
            .unwrap();
        println!("  Found: {} result(s)", correct.len());
        assert_eq!(correct.len(), 1);

        // Test 2: Missing one character (edit distance = 1)
        println!("\nQuery: 'ArchivService' (missing 'e', edit distance = 1)");
        let typo1 = index.search("ArchivService", 10, None, None, None).unwrap();
        println!("  Found: {} result(s)", typo1.len());
        if !typo1.is_empty() {
            println!("  Match: {}", typo1[0].name);
        }

        // Test 3: Wrong character (edit distance = 1)
        println!("\nQuery: 'ArchaveService' (i→a, edit distance = 1)");
        let typo2 = index
            .search("ArchaveService", 10, None, None, None)
            .unwrap();
        println!("  Found: {} result(s)", typo2.len());
        if !typo2.is_empty() {
            println!("  Match: {}", typo2[0].name);
        }

        // Test 4: Extra character (edit distance = 1)
        println!("\nQuery: 'Archivee' (partial with extra 'e', edit distance = 1)");
        let typo3 = index.search("Archivee", 10, None, None, None).unwrap();
        println!("  Found: {} result(s)", typo3.len());
        if !typo3.is_empty() {
            println!("  Match: {}", typo3[0].name);
        }

        // Test 5: Too many errors (edit distance > 1, should not match with fuzzy)
        println!("\nQuery: 'Archhive' (2 errors: extra 'h' and wrong 'h', edit distance = 2)");
        let too_many = index.search("Archhive", 10, None, None, None).unwrap();
        println!("  Found: {} result(s)", too_many.len());
        println!("  Expectation: May find via ngram partial match, but not via fuzzy (distance=2)");

        println!("\n=== FUZZY SEARCH EXPLANATION ===");
        println!("Fuzzy search (edit distance=1) handles typos like:");
        println!("  - Missing character: 'Archiv' finds 'Archive'");
        println!("  - Wrong character: 'Archave' finds 'Archive'");
        println!("  - Extra character: 'Archivee' finds 'Archive'");
        println!("\nNgram tokenizer handles partial matching:");
        println!("  - 'Archive' finds 'ArchiveService', 'DocumentArchiver'");
        println!("\nBoth work together in the same search query!");
        println!("\n=== FUZZY SEARCH TEST COMPLETE ===\n");
    }

    #[test]
    fn test_ngram_vs_fuzzy_interaction() {
        println!("\n=== UNDERSTANDING NGRAM + FUZZY INTERACTION ===\n");

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let file_id = crate::FileId::new(1).unwrap();

        println!("Indexed: 'ArchiveService'\n");
        let sym = crate::Symbol::new(
            SymbolId::new(1).unwrap(),
            "ArchiveService",
            SymbolKind::Class,
            file_id,
            crate::Range::new(10, 5, 50, 10),
        );

        index.index_symbol(&sym, "src/ArchiveService.cs").unwrap();
        index.commit_batch().unwrap();

        println!("HOW NGRAM TOKENIZATION WORKS:");
        println!("'ArchiveService' gets broken into ngrams (min=3, max=10):");
        println!("  3-grams: Arc, rch, chi, hiv, ive, veS, eSe, Ser, erv, rvi, vic, ice");
        println!("  4-grams: Arch, rchi, chiv, hive, iveS, veSe, eSer, Serv, ervi, rvic, vice");
        println!("  ... up to 10-grams\n");

        println!("TEST CASES:\n");

        // Test 1: Short partial match (should work via ngram)
        println!("1. Query: 'Arch' (4 chars, exact ngram match)");
        let short_match = index.search("Arch", 10, None, None, None).unwrap();
        println!("   Result: {} match(es) ✓", short_match.len());
        println!("   Why: 'Arch' is an exact 4-gram token in 'ArchiveService'\n");

        // Test 2: Short typo (should work via fuzzy on ngrams)
        println!("2. Query: 'Arsh' (1 typo: c→s, edit distance = 1)");
        let short_typo = index.search("Arsh", 10, None, None, None).unwrap();
        println!("   Result: {} match(es)", short_typo.len());
        if short_typo.is_empty() {
            println!("   Why: Fuzzy matches 'Arsh' against ngrams like 'Arch' (distance=1)");
            println!("        But may not find it depending on Tantivy's fuzzy implementation\n");
        } else {
            println!("   Why: Fuzzy matched 'Arsh' to ngram 'Arch' (distance=1) ✓\n");
        }

        // Test 3: Long query missing char (NOW FIXED!)
        println!("3. Query: 'ArchivService' (missing 'e', 13 chars)");
        let long_typo = index.search("ArchivService", 10, None, None, None).unwrap();
        println!("   Result: {} match(es) ✓", long_typo.len());
        println!("   Why: FIXED by adding fuzzy search on non-tokenized 'name' field!");
        println!("        Fuzzy matches 'ArchivService' → 'ArchiveService' (edit distance=1)");
        println!("        This works BEFORE ngram tokenization, avoiding misalignment\n");
        assert!(
            !long_typo.is_empty(),
            "Should find ArchiveService with typo"
        );

        // Test 4: Partial match that works (ngram overlap)
        println!("4. Query: 'Archive' (7 chars, prefix of indexed word)");
        let partial = index.search("Archive", 10, None, None, None).unwrap();
        println!("   Result: {} match(es) ✓", partial.len());
        println!("   Why: 'Archive' ngrams (Arc, rch, chi, hiv, ive, etc.)");
        println!("        overlap with 'ArchiveService' ngrams\n");

        println!("CONCLUSION:");
        println!("- Ngram tokenizer: Great for partial matching (prefix/substring) ✓");
        println!("- Fuzzy on ngrams: Works for typos in SHORT queries ✓");
        println!("- Fuzzy on whole word: FIXED - Now handles typos in LONG words ✓");
        println!("\nSOLUTION IMPLEMENTED:");
        println!("  Added fuzzy search on non-tokenized 'name' field (STRING type)");
        println!("  Now search queries try BOTH:");
        println!("    1. Fuzzy on ngram tokens (for short queries)");
        println!("    2. Fuzzy on whole words (for full symbol names)");
        println!("  Result: 'ArchivService' correctly finds 'ArchiveService' ✓");

        println!("\n=== TEST COMPLETE ===\n");
    }

    #[test]
    fn test_import_persistence_across_reload() {
        // This test verifies the fix for: external imports are lost after index reload,
        // causing external symbols (e.g., indicatif::ProgressBar) to incorrectly
        // resolve to local symbols with the same name.

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();

        // Create initial index
        {
            let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();
            index.start_batch().unwrap();

            let file_id = FileId::new(1).unwrap();

            // Store file registration
            let registration = FileRegistration {
                path: PathBuf::from("src/main.rs"),
                file_id,
                content_hash: "hash123".to_string(),
                language_id: LanguageId::new("rust"),
                timestamp: 1234567890,
                mtime: 0,
            };
            index.store_file_registration(&registration).unwrap();

            // Store external imports (the data we're testing persistence for)
            let import1 = crate::parsing::Import {
                path: "indicatif::ProgressBar".to_string(),
                alias: None,
                file_id,
                is_glob: false,
                is_type_only: false,
            };

            let import2 = crate::parsing::Import {
                path: "serde::Serialize".to_string(),
                alias: Some("SerTrait".to_string()),
                file_id,
                is_glob: false,
                is_type_only: false,
            };

            index.store_import(&import1).unwrap();
            index.store_import(&import2).unwrap();

            index.commit_batch().unwrap();
        } // Drop index to simulate app shutdown

        // Reload index (simulate app restart)
        {
            let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

            // CRITICAL: Verify imports survived the reload
            let loaded_imports = index.get_imports_for_file(FileId::new(1).unwrap()).unwrap();

            assert_eq!(
                loaded_imports.len(),
                2,
                "Should load 2 imports after reload"
            );

            // Verify first import
            let import1 = loaded_imports
                .iter()
                .find(|i| i.path == "indicatif::ProgressBar")
                .unwrap();
            assert_eq!(import1.alias, None);
            assert!(!import1.is_glob);
            assert!(!import1.is_type_only);

            // Verify second import (with alias)
            let import2 = loaded_imports
                .iter()
                .find(|i| i.path == "serde::Serialize")
                .unwrap();
            assert_eq!(import2.alias.as_deref(), Some("SerTrait"));
            assert!(!import2.is_glob);
            assert!(!import2.is_type_only);
        }
    }

    #[test]
    fn test_import_deletion_on_file_removal() {
        // Verify that deleting a file also deletes its imports

        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let file_id = FileId::new(1).unwrap();

        // Store file registration and import
        let registration = FileRegistration {
            path: PathBuf::from("src/main.rs"),
            file_id,
            content_hash: "hash123".to_string(),
            language_id: LanguageId::new("rust"),
            timestamp: 1234567890,
            mtime: 0,
        };
        index.store_file_registration(&registration).unwrap();

        let import = crate::parsing::Import {
            path: "std::collections::HashMap".to_string(),
            alias: None,
            file_id,
            is_glob: false,
            is_type_only: false,
        };
        index.store_import(&import).unwrap();

        index.commit_batch().unwrap();

        // Verify import exists
        let imports = index.get_imports_for_file(file_id).unwrap();
        assert_eq!(imports.len(), 1);

        // Delete imports for this file
        index.start_batch().unwrap();
        index.delete_imports_for_file(file_id).unwrap();
        index.commit_batch().unwrap();

        // Verify imports are gone
        let imports_after = index.get_imports_for_file(file_id).unwrap();
        assert_eq!(imports_after.len(), 0, "Imports should be deleted");
    }

    #[test]
    #[ignore] // Run with: cargo test test_java_owner_extends_person -- --ignored --nocapture
    fn test_java_owner_extends_person() {
        // Verify Java import resolution: Owner extends Person across packages
        // This test queries the production .codanna/index to verify relationships exist

        let index_base = Path::new(".codanna/index");
        let tantivy_path = index_base.join("tantivy");
        if !tantivy_path.exists() {
            eprintln!(
                "Skipping test: .codanna/index/tantivy not found. Run: ./target/release/codanna index test_monorepos/spring-petclinic"
            );
            return;
        }

        let settings = crate::config::Settings::default();
        let index =
            DocumentIndex::new(&tantivy_path, &settings).expect("Failed to open production index");

        // Find Owner class in org.springframework.samples.petclinic.owner
        let owner_candidates = index
            .find_symbols_by_name("Owner", None)
            .expect("Failed to find Owner");

        println!("Found {} Owner symbols:", owner_candidates.len());
        for candidate in &owner_candidates {
            println!(
                "  - Owner at {:?}, module: {:?}",
                candidate.file_id, candidate.module_path
            );
        }

        let owner = owner_candidates
            .iter()
            .find(|s| {
                s.module_path
                    .as_ref()
                    .map(|m| m.as_ref() == "org.springframework.samples.petclinic.owner")
                    .unwrap_or(false)
            })
            .expect("Owner class in petclinic.owner package should exist");

        println!(
            "Found Owner: symbol_id={}, module={:?}",
            owner.id.0, owner.module_path
        );

        // Find Person class in org.springframework.samples.petclinic.model
        let person_candidates = index
            .find_symbols_by_name("Person", None)
            .expect("Failed to find Person");

        let person = person_candidates
            .iter()
            .find(|s| {
                s.module_path
                    .as_ref()
                    .map(|m| m.as_ref() == "org.springframework.samples.petclinic.model")
                    .unwrap_or(false)
            })
            .expect("Person class in petclinic.model package should exist");

        println!(
            "Found Person: symbol_id={}, module={:?}",
            person.id.0, person.module_path
        );

        // Query ALL relationship types FROM Owner to see what exists
        let all_rel_kinds = [
            RelationKind::Calls,
            RelationKind::Extends,
            RelationKind::Implements,
            RelationKind::Uses,
            RelationKind::Defines,
            RelationKind::References,
        ];

        for kind in &all_rel_kinds {
            let rels = index
                .get_relationships_from(owner.id, *kind)
                .expect("Failed to query relationships");
            if !rels.is_empty() {
                println!("Owner has {} {:?} relationships", rels.len(), kind);
                for (from_id, to_id, rel) in rels.iter().take(5) {
                    println!(
                        "  {:?}: {} -> {} (weight: {})",
                        kind, from_id.0, to_id.0, rel.weight
                    );
                    // Show target symbol name
                    if let Ok(Some(target)) = index.find_symbol_by_id(*to_id) {
                        println!("    -> {}", target.name);
                    }
                }
            }
        }

        // Query Extends relationships FROM Owner
        let extends_rels = index
            .get_relationships_from(owner.id, RelationKind::Extends)
            .expect("Failed to query Extends relationships");

        println!(
            "\nFinal check: Owner has {} Extends relationships",
            extends_rels.len()
        );

        // ALSO check if Person has any relationships (to see if it's Owner-specific or all Java)
        let person_extends = index
            .get_relationships_to(person.id, RelationKind::Extends)
            .expect("Failed to query who extends Person");
        println!("Person is extended by {} classes", person_extends.len());

        // Check total relationship count to compare with reported numbers
        let total_rel_count = index
            .count_relationships()
            .expect("Failed to count relationships");
        println!("Total relationships in index: {total_rel_count}");

        // Check if ANY Extends relationships exist in the entire index
        let all_extends = index
            .get_all_relationships_by_kind(RelationKind::Extends)
            .expect("Failed to query all Extends relationships");
        println!(
            "Total Extends relationships in entire index: {}",
            all_extends.len()
        );
        if !all_extends.is_empty() {
            println!("Sample Extends relationships:");
            for (from_id, to_id, _) in all_extends.iter().take(5) {
                if let (Ok(Some(from_sym)), Ok(Some(to_sym))) = (
                    index.find_symbol_by_id(*from_id),
                    index.find_symbol_by_id(*to_id),
                ) {
                    println!("  {} extends {}", from_sym.name, to_sym.name);
                }
            }
        }

        // Verify Owner extends Person
        let extends_person = extends_rels
            .iter()
            .any(|(_from, to_id, _rel)| *to_id == person.id);

        assert!(
            extends_person,
            "Owner (id={}) should extend Person (id={}), but Extends relationship not found. Found {} Extends relationships.",
            owner.id.0,
            person.id.0,
            extends_rels.len()
        );

        println!("✅ SUCCESS: Owner extends Person relationship verified!");
    }

    #[test]
    fn test_get_relationships_to_uncapped_beyond_1000() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        let hub = SymbolId::new(1).unwrap();
        index.start_batch().unwrap();
        for i in 2..=1102u32 {
            let from = SymbolId::new(i).unwrap();
            index
                .store_relationship(from, hub, &Relationship::new(RelationKind::Extends))
                .unwrap();
        }
        index.commit_batch().unwrap();

        let edges = index
            .get_relationships_to(hub, RelationKind::Extends)
            .unwrap();
        assert_eq!(edges.len(), 1101, "expected all edges, got a capped result");

        let no_edges = index
            .get_relationships_to(SymbolId::new(7777).unwrap(), RelationKind::Extends)
            .unwrap();
        assert!(no_edges.is_empty());
    }

    #[test]
    fn test_get_relationships_from_uncapped_beyond_1000() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        let hub = SymbolId::new(1).unwrap();
        index.start_batch().unwrap();
        for i in 2..=1102u32 {
            let to = SymbolId::new(i).unwrap();
            index
                .store_relationship(hub, to, &Relationship::new(RelationKind::Calls))
                .unwrap();
        }
        index.commit_batch().unwrap();

        let edges = index
            .get_relationships_from(hub, RelationKind::Calls)
            .unwrap();
        assert_eq!(edges.len(), 1101, "expected all edges, got a capped result");

        let no_edges = index
            .get_relationships_from(SymbolId::new(7777).unwrap(), RelationKind::Calls)
            .unwrap();
        assert!(no_edges.is_empty());
    }

    #[test]
    fn test_find_symbols_by_file_uncapped_beyond_1000() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        let file_id = FileId::new(1).unwrap();
        index.start_batch().unwrap();
        for i in 1..=1101u32 {
            let sym = crate::Symbol::new(
                SymbolId::new(i).unwrap(),
                format!("sym_{i}").as_str(),
                SymbolKind::Function,
                file_id,
                crate::Range::new(i, 0, i, 10),
            );
            index.add_document(&sym, "src/generated.rs").unwrap();
        }
        index.commit_batch().unwrap();

        let symbols = index.find_symbols_by_file(file_id).unwrap();
        assert_eq!(
            symbols.len(),
            1101,
            "expected all symbols in file, got a capped result"
        );

        let none = index
            .find_symbols_by_file(FileId::new(99).unwrap())
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn test_find_symbols_by_module_uncapped_beyond_1000() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();
        for i in 1..=1101u32 {
            let sym = crate::Symbol::new(
                SymbolId::new(i).unwrap(),
                format!("sym_{i}").as_str(),
                SymbolKind::Function,
                FileId::new(1).unwrap(),
                crate::Range::new(i, 0, i, 10),
            )
            .with_module_path("crate::generated");
            index.add_document(&sym, "src/generated.rs").unwrap();
        }
        index.commit_batch().unwrap();

        let symbols = index.find_symbols_by_module("crate::generated").unwrap();
        assert_eq!(
            symbols.len(),
            1101,
            "expected all symbols in module, got a capped result"
        );

        let none = index.find_symbols_by_module("crate::absent").unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn test_get_imports_for_file_uncapped_beyond_1000() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        let file_id = FileId::new(1).unwrap();
        index.start_batch().unwrap();
        for i in 1..=1101u32 {
            index
                .store_import(&crate::parsing::Import {
                    path: format!("dep::module_{i}"),
                    alias: None,
                    file_id,
                    is_glob: false,
                    is_type_only: false,
                })
                .unwrap();
        }
        index.commit_batch().unwrap();

        let imports = index.get_imports_for_file(file_id).unwrap();
        assert_eq!(
            imports.len(),
            1101,
            "expected all imports for file, got a capped result"
        );

        let none = index
            .get_imports_for_file(FileId::new(99).unwrap())
            .unwrap();
        assert!(none.is_empty());
    }
}
