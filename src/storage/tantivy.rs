//! Tantivy-based full-text search for documentation and code
//! 
//! This module provides rich full-text search capabilities using Tantivy,
//! enabling semantic search across documentation, code, and symbols.

use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::{BooleanQuery, FuzzyTermQuery, Occur, Query, QueryParser, TermQuery},
    schema::{
        Field, IndexRecordOption, Schema, SchemaBuilder,
        TextFieldIndexing, TextOptions, Value, NumericOptions,
        FAST, STORED, STRING,
    },
    Index, IndexReader, IndexWriter, IndexSettings, ReloadPolicy, Term,
    TantivyDocument as Document,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use crate::{SymbolId, SymbolKind, FileId, RelationKind, Relationship};
use crate::relationship::RelationshipMetadata;
use super::{StorageError, StorageResult, MetadataKey};

/// Schema fields for the document index
#[derive(Debug)]
pub struct IndexSchema {
    // Document type discriminator
    pub doc_type: Field,
    
    // Symbol fields
    pub symbol_id: Field,
    pub name: Field,
    pub doc_comment: Field,
    pub signature: Field,
    pub module_path: Field,
    pub kind: Field,
    pub file_path: Field,
    pub line_number: Field,
    pub column: Field,
    pub context: Field,
    
    // Relationship fields
    pub from_symbol_id: Field,
    pub to_symbol_id: Field,
    pub relation_kind: Field,
    pub relation_weight: Field,
    pub relation_line: Field,
    pub relation_column: Field,
    pub relation_context: Field,
    
    // File info fields
    pub file_id: Field,
    pub file_hash: Field,
    pub file_timestamp: Field,
    
    // Metadata fields
    pub meta_key: Field,
    pub meta_value: Field,
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
        let line_number = builder.add_u64_field("line_number", STORED | FAST);
        let column = builder.add_u64_field("column", STORED);
        
        // Text fields for search
        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("default")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions)
            )
            .set_stored();
            
        let name = builder.add_text_field("name", text_options.clone());
        let doc_comment = builder.add_text_field("doc_comment", text_options.clone());
        let signature = builder.add_text_field("signature", text_options.clone());
        let context = builder.add_text_field("context", text_options.clone());
        
        // String fields for filtering (using STRING for exact match)
        let module_path = builder.add_text_field("module_path", STRING | STORED);
        let kind = builder.add_text_field("kind", STRING | STORED);
        
        // Relationship fields
        let from_symbol_id = builder.add_u64_field("from_symbol_id", indexed_u64_options.clone());
        let to_symbol_id = builder.add_u64_field("to_symbol_id", indexed_u64_options.clone());
        let relation_kind = builder.add_text_field("relation_kind", STRING | STORED | FAST);
        let relation_weight = builder.add_f64_field("relation_weight", STORED);
        let relation_line = builder.add_u64_field("relation_line", STORED);
        let relation_column = builder.add_u64_field("relation_column", STORED);
        let relation_context = builder.add_text_field("relation_context", text_options);
        
        // File info fields
        let file_id = builder.add_u64_field("file_id", indexed_u64_options.clone());
        let file_hash = builder.add_text_field("file_hash", STRING | STORED);
        let file_timestamp = builder.add_u64_field("file_timestamp", STORED | FAST);
        
        // Metadata fields (for counters, etc.)
        let meta_key = builder.add_text_field("meta_key", STRING | STORED | FAST);
        let meta_value = builder.add_u64_field("meta_value", STORED);
        
        let schema = builder.build();
        let index_schema = IndexSchema {
            doc_type,
            symbol_id,
            name,
            doc_comment,
            signature,
            module_path,
            kind,
            file_path,
            line_number,
            column,
            context,
            from_symbol_id,
            to_symbol_id,
            relation_kind,
            relation_weight,
            relation_line,
            relation_column,
            relation_context,
            file_id,
            file_hash,
            file_timestamp,
            meta_key,
            meta_value,
        };
        
        (schema, index_schema)
    }
}

/// Search result with rich metadata
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub symbol_id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: u32,
    pub column: u16,
    pub doc_comment: Option<String>,
    pub signature: Option<String>,
    pub module_path: String,
    pub score: f32,
    pub highlights: Vec<TextHighlight>,
    pub context: Option<String>,
}

/// Highlighted text region
#[derive(Debug, Clone)]
pub struct TextHighlight {
    pub field: String,
    pub start: usize,
    pub end: usize,
}

/// Document index for full-text search
pub struct DocumentIndex {
    index: Index,
    reader: IndexReader,
    schema: IndexSchema,
    index_path: PathBuf,
    pub(crate) writer: Mutex<Option<IndexWriter<Document>>>,
}

impl std::fmt::Debug for DocumentIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentIndex")
            .field("index_path", &self.index_path)
            .field("schema", &self.schema)
            .finish()
    }
}

impl DocumentIndex {
    /// Create a new document index
    pub fn new(index_path: impl AsRef<Path>) -> StorageResult<Self> {
        let index_path = index_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&index_path)?;
        
        let (schema, index_schema) = IndexSchema::build();
        
        // Create or open the index
        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(&index_path)?
        } else {
            let dir = MmapDirectory::open(&index_path)?;
            Index::create(dir, schema, IndexSettings::default())?
        };
        
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        
        // If opening existing index, reload to get latest segments
        if index_path.join("meta.json").exists() {
            reader.reload()?;
        }
            
        Ok(Self {
            index,
            reader,
            schema: index_schema,
            index_path,
            writer: Mutex::new(None),
        })
    }
    
    /// Start a batch operation for adding multiple documents
    pub fn start_batch(&self) -> StorageResult<()> {
        let mut writer_lock = self.writer.lock().map_err(|_| StorageError::LockPoisoned)?;
        if writer_lock.is_none() {
            let writer = self.index.writer::<Document>(100_000_000)?; // 100MB buffer
            *writer_lock = Some(writer);
        }
        Ok(())
    }
    
    /// Add a document to the index (must call start_batch first)
    pub fn add_document(
        &self,
        symbol_id: SymbolId,
        name: &str,
        kind: SymbolKind,
        file_id: FileId,
        file_path: &str,
        line: u32,
        column: u16,
        doc_comment: Option<&str>,
        signature: Option<&str>,
        module_path: &str,
        context: Option<&str>,
    ) -> StorageResult<()> {
        let mut writer_lock = self.writer.lock().map_err(|_| StorageError::LockPoisoned)?;
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
        
        let mut doc = Document::new();
        doc.add_text(self.schema.doc_type, "symbol");
        doc.add_u64(self.schema.symbol_id, symbol_id.value() as u64);
        doc.add_u64(self.schema.file_id, file_id.value() as u64);
        doc.add_text(self.schema.name, name);
        doc.add_text(self.schema.file_path, file_path);
        doc.add_u64(self.schema.line_number, line as u64);
        doc.add_u64(self.schema.column, column as u64);
        
        if let Some(comment) = doc_comment {
            doc.add_text(self.schema.doc_comment, comment);
        }
        
        if let Some(sig) = signature {
            doc.add_text(self.schema.signature, sig);
        }
        
        if let Some(ctx) = context {
            doc.add_text(self.schema.context, ctx);
        }
        
        // Add string fields for filtering
        doc.add_text(self.schema.module_path, module_path);
        doc.add_text(self.schema.kind, &format!("{:?}", kind));
        
        writer.add_document(doc)?;
        
        Ok(())
    }
    
    /// Commit the current batch and reload the reader
    pub fn commit_batch(&self) -> StorageResult<()> {
        let mut writer_lock = match self.writer.lock() {
            Ok(lock) => lock,
            Err(poisoned) => {
                eprintln!("Warning: Recovering from poisoned writer mutex in commit_batch");
                poisoned.into_inner()
            }
        };
        if let Some(mut writer) = writer_lock.take() {
            writer.commit()?;
            // Reload the reader to see new documents
            self.reader.reload()?;
        }
        Ok(())
    }
    
    /// Remove documents for a specific file
    pub fn remove_file_documents(&self, file_path: &str) -> StorageResult<()> {
        // Use existing batch writer if available, otherwise create temporary one
        let mut writer_lock = self.writer.lock().map_err(|_| StorageError::LockPoisoned)?;
        let term = Term::from_field_text(self.schema.file_path, file_path);
        
        if let Some(writer) = writer_lock.as_mut() {
            // Use existing batch writer
            writer.delete_term(term);
        } else {
            // Create temporary writer for single operation
            let mut writer = self.index.writer::<Document>(50_000_000)?;
            writer.delete_term(term);
            writer.commit()?;
            self.reader.reload()?;
        }
        
        Ok(())
    }
    
    /// Search for documents
    pub fn search(
        &self,
        query_str: &str,
        limit: usize,
        kind_filter: Option<SymbolKind>,
        module_filter: Option<&str>,
    ) -> StorageResult<Vec<SearchResult>> {
        let searcher = self.reader.searcher();
        
        // Build the query
        let mut subqueries: Vec<Box<dyn Query>> = Vec::new();
        
        // Always filter for symbol documents only
        let doc_type_query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "symbol"),
            IndexRecordOption::Basic,
        );
        subqueries.push(Box::new(doc_type_query));
        
        // Try multiple search strategies
        let mut search_queries: Vec<Box<dyn Query>> = Vec::new();
        
        // First, try a regular query parser for exact matches
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.schema.name,
                self.schema.doc_comment,
                self.schema.signature,
                self.schema.context,
            ],
        );
        
        if let Ok(parsed_query) = query_parser.parse_query(query_str) {
            search_queries.push(parsed_query);
        }
        
        // Also add fuzzy search on the name field for typo tolerance
        let term = Term::from_field_text(self.schema.name, query_str);
        let fuzzy_query = FuzzyTermQuery::new(term, 1, true); // distance=1, transposition_cost_one=true
        search_queries.push(Box::new(fuzzy_query));
        
        // Combine search queries with OR (Should) to get results from either
        if !search_queries.is_empty() {
            let mut bool_clauses = Vec::new();
            for q in search_queries {
                bool_clauses.push((Occur::Should, q));
            }
            subqueries.push(Box::new(BooleanQuery::new(bool_clauses)));
        }
        
        // Add filters
        if let Some(kind) = kind_filter {
            let term = Term::from_field_text(self.schema.kind, &format!("{:?}", kind));
            let term_query = TermQuery::new(term, IndexRecordOption::Basic);
            subqueries.push(Box::new(term_query));
        }
        
        if let Some(module) = module_filter {
            let term = Term::from_field_text(self.schema.module_path, module);
            let term_query = TermQuery::new(term, IndexRecordOption::Basic);
            subqueries.push(Box::new(term_query));
        }
        
        // Build the final query
        let query: Box<dyn Query> = if subqueries.len() == 1 {
            // If there's only one query, use it directly
            subqueries.into_iter().next().unwrap()
        } else {
            // Multiple queries, combine with BooleanQuery
            let mut bool_query_clauses = Vec::new();
            for query in subqueries {
                bool_query_clauses.push((Occur::Must, query));
            }
            Box::new(BooleanQuery::new(bool_query_clauses))
        };
        
        let top_docs = searcher.search(&*query, &TopDocs::with_limit(limit))?;
        
        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;
            
            // Extract fields
            let symbol_id = doc.get_first(self.schema.symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            let name = doc.get_first(self.schema.name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
                
            let file_path = doc.get_first(self.schema.file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
                
            let line = doc.get_first(self.schema.line_number)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
                
            let column = doc.get_first(self.schema.column)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;
                
            let doc_comment = doc.get_first(self.schema.doc_comment)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
                
            let signature = doc.get_first(self.schema.signature)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
                
            let context = doc.get_first(self.schema.context)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
                
            // Extract kind from facet (stored as string representation)
            let kind_str = doc.get_first(self.schema.kind)
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            
            let kind = match kind_str {
                "Function" => SymbolKind::Function,
                "Struct" => SymbolKind::Struct,
                "Trait" => SymbolKind::Trait,
                "Method" => SymbolKind::Method,
                "Field" => SymbolKind::Field,
                "Module" => SymbolKind::Module,
                "Constant" => SymbolKind::Constant,
                _ => SymbolKind::Function, // Default fallback
            };
            
            let module_path = doc.get_first(self.schema.module_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
                
            results.push(SearchResult {
                symbol_id,
                name,
                kind,
                file_path,
                line,
                column,
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
    
    /// Clear all documents from the index
    pub fn clear(&self) -> StorageResult<()> {
        let mut writer = self.index.writer::<Document>(50_000_000)?;
        writer.delete_all_documents()?;
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }
    
    /// Find a symbol by its ID
    pub fn find_symbol_by_id(&self, id: SymbolId) -> StorageResult<Option<crate::Symbol>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_u64(self.schema.symbol_id, id.0 as u64),
            IndexRecordOption::Basic,
        );
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;
        
        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;
            Ok(Some(self.document_to_symbol(&doc)?))
        } else {
            Ok(None)
        }
    }
    
    /// Find symbols by name
    pub fn find_symbols_by_name(&self, name: &str) -> StorageResult<Vec<crate::Symbol>> {
        let searcher = self.reader.searcher();
        
        // Create a simple term query for the name field
        // This avoids issues with special characters in symbol names
        let query = TermQuery::new(
            Term::from_field_text(self.schema.name, &name.to_lowercase()),
            IndexRecordOption::Basic,
        );
        
        // First search for exact matches
        let top_docs = searcher.search(&query, &TopDocs::with_limit(100))?;
        let mut symbols = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            // Only include symbol documents
            if let Some(doc_type) = doc.get_first(self.schema.doc_type) {
                if doc_type.as_str() == Some("symbol") {
                    symbols.push(self.document_to_symbol(&doc)?);
                }
            }
        }
        
        Ok(symbols)
    }
    
    /// Find symbols by file ID
    pub fn find_symbols_by_file(&self, file_id: FileId) -> StorageResult<Vec<crate::Symbol>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "symbol"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_u64(self.schema.file_id, file_id.0 as u64),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
        ]);
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1000))?;
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
        let all_query = BooleanQuery::from(vec![]);
        
        let top_docs = searcher.search(&all_query, &TopDocs::with_limit(limit))?;
        let mut symbols = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            if let Some(doc_type) = doc.get_first(self.schema.doc_type) {
                if doc_type.as_str() == Some("symbol") {
                    symbols.push(self.document_to_symbol(&doc)?);
                }
            }
        }
        
        Ok(symbols)
    }
    
    /// Convert a Tantivy document to a Symbol
    fn document_to_symbol(&self, doc: &Document) -> StorageResult<crate::Symbol> {
        use crate::{Symbol, Range, SymbolKind, Visibility};
        
        let symbol_id = doc.get_first(self.schema.symbol_id)
            .and_then(|v| v.as_u64())
            .ok_or(StorageError::InvalidFieldValue { field: "symbol_id".to_string(), reason: "missing from document".to_string() })?;
            
        let name = doc.get_first(self.schema.name)
            .and_then(|v| v.as_str())
            .ok_or(StorageError::InvalidFieldValue { field: "name".to_string(), reason: "missing from document".to_string() })?
            .to_string();
            
        let kind_str = doc.get_first(self.schema.kind)
            .and_then(|v| v.as_str())
            .ok_or(StorageError::InvalidFieldValue { field: "kind".to_string(), reason: "missing from document".to_string() })?;
        let kind = SymbolKind::from_str(kind_str);
        
        let file_id = doc.get_first(self.schema.file_id)
            .and_then(|v| v.as_u64())
            .ok_or(StorageError::InvalidFieldValue { field: "file_id".to_string(), reason: "missing from document".to_string() })?;
            
        let start_line = doc.get_first(self.schema.line_number)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
            
        let start_col = doc.get_first(self.schema.column)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;
            
        let end_line = start_line; // We don't store end_line separately
            
        let end_col = 0u16; // We don't store end_col separately
            
        let signature = doc.get_first(self.schema.signature)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
            
        let doc_comment = doc.get_first(self.schema.doc_comment)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
            
        let module_path = doc.get_first(self.schema.module_path)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
            
        // Check if symbol is public based on signature containing "pub "
        let is_public = signature.as_ref()
            .map(|sig| sig.contains("pub "))
            .unwrap_or(false);
            
        Ok(Symbol {
            id: SymbolId(symbol_id as u32),
            name: name.into(),
            kind,
            file_id: FileId(file_id as u32),
            range: Range {
                start_line,
                start_column: start_col,
                end_line,
                end_column: end_col,
            },
            signature: signature.map(|s| s.into()),
            doc_comment: doc_comment.map(|s| s.into()),
            module_path: module_path.map(|s| s.into()),
            visibility: if is_public { Visibility::Public } else { Visibility::Private },
        })
    }
    
    /// Get file info by path
    pub fn get_file_info(&self, path: &str) -> StorageResult<Option<(FileId, String)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "file_info"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.file_path, path),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
        ]);
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;
        
        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;
            
            let file_id = doc.get_first(self.schema.file_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| FileId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "file_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            let hash = doc.get_first(self.schema.file_hash)
                .and_then(|v| v.as_str())
                .ok_or(StorageError::InvalidFieldValue { field: "file_hash".to_string(), reason: "missing from document".to_string() })?
                .to_string();
                
            Ok(Some((file_id, hash)))
        } else {
            Ok(None)
        }
    }
    
    /// Get next file ID
    pub fn get_next_file_id(&self) -> StorageResult<u32> {
        let current = self.query_metadata(MetadataKey::FileCounter)?.unwrap_or(0) as u32;
        Ok(current + 1)
    }
    
    /// Get next symbol ID
    pub fn get_next_symbol_id(&self) -> StorageResult<u32> {
        let current = self.query_metadata(MetadataKey::SymbolCounter)?.unwrap_or(0) as u32;
        Ok(current + 1)
    }
    
    /// Delete a symbol
    pub fn delete_symbol(&self, id: SymbolId) -> StorageResult<()> {
        let mut writer_lock = match self.writer.lock() {
            Ok(lock) => lock,
            Err(poisoned) => {
                eprintln!("Warning: Recovering from poisoned writer mutex in delete_symbol");
                poisoned.into_inner()
            }
        };
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
            
        let term = Term::from_field_u64(self.schema.symbol_id, id.0 as u64);
        writer.delete_term(term);
        Ok(())
    }
    
    /// Delete relationships for a symbol
    pub fn delete_relationships_for_symbol(&self, id: SymbolId) -> StorageResult<()> {
        let mut writer_lock = match self.writer.lock() {
            Ok(lock) => lock,
            Err(poisoned) => {
                eprintln!("Warning: Recovering from poisoned writer mutex in delete_relationships");
                poisoned.into_inner()
            }
        };
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
            
        // Delete where from_symbol_id = id
        let from_term = Term::from_field_u64(self.schema.from_symbol_id, id.0 as u64);
        writer.delete_term(from_term);
        
        // Delete where to_symbol_id = id
        let to_term = Term::from_field_u64(self.schema.to_symbol_id, id.0 as u64);
        writer.delete_term(to_term);
        
        Ok(())
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
    
    /// Get relationships from a symbol
    pub fn get_relationships_from(&self, from_id: SymbolId, kind: RelationKind) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "relationship"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_u64(self.schema.from_symbol_id, from_id.0 as u64),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.relation_kind, &format!("{:?}", kind)),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
        ]);
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1000))?;
        let mut relationships = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            
            let to_id = doc.get_first(self.schema.to_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "to_symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            relationships.push((from_id, to_id, Relationship::new(kind)));
        }
        
        Ok(relationships)
    }
    
    /// Get relationships to a symbol
    pub fn get_relationships_to(&self, to_id: SymbolId, kind: RelationKind) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "relationship"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_u64(self.schema.to_symbol_id, to_id.0 as u64),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.relation_kind, &format!("{:?}", kind)),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
        ]);
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1000))?;
        let mut relationships = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            
            let from_id = doc.get_first(self.schema.from_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "from_symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            relationships.push((from_id, to_id, Relationship::new(kind)));
        }
        
        Ok(relationships)
    }
    
    /// Get file path by ID
    pub fn get_file_path(&self, file_id: FileId) -> StorageResult<Option<String>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "file_info"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_u64(self.schema.file_id, file_id.0 as u64),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
        ]);
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;
        
        if let Some((_score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<Document>(*doc_address)?;
            
            let path = doc.get_first(self.schema.file_path)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
                
            Ok(path)
        } else {
            Ok(None)
        }
    }
    
    /// Get the path where the index is stored
    /// 
    /// TODO: Potential use cases for this method:
    /// - Recreating the index if corrupted
    /// - Moving or copying the index to another location
    /// - Displaying index location in diagnostics or status commands
    /// - Cleaning up the entire index directory
    /// - Backing up the index data
    pub fn path(&self) -> &Path {
        &self.index_path
    }
    
    // Internal methods for storage operations (accessible within crate)
    
    /// Store a relationship between two symbols
    pub(crate) fn store_relationship(
        &self,
        from: SymbolId,
        to: SymbolId,
        rel: &Relationship,
    ) -> StorageResult<()> {
        let mut writer_lock = match self.writer.lock() {
            Ok(lock) => lock,
            Err(poisoned) => {
                eprintln!("Warning: Recovering from poisoned writer mutex in store_relationship");
                poisoned.into_inner()
            }
        };
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
        
        let mut doc = Document::new();
        doc.add_text(self.schema.doc_type, "relationship");
        doc.add_u64(self.schema.from_symbol_id, from.value() as u64);
        doc.add_u64(self.schema.to_symbol_id, to.value() as u64);
        doc.add_text(self.schema.relation_kind, &format!("{:?}", rel.kind));
        doc.add_f64(self.schema.relation_weight, rel.weight as f64);
        
        if let Some(ref metadata) = rel.metadata {
            if let Some(line) = metadata.line {
                doc.add_u64(self.schema.relation_line, line as u64);
            }
            if let Some(column) = metadata.column {
                doc.add_u64(self.schema.relation_column, column as u64);
            }
            if let Some(ref context) = metadata.context {
                doc.add_text(self.schema.relation_context, context.as_ref());
            }
        }
        
        writer.add_document(doc)?;
        Ok(())
    }
    
    /// Index a symbol from a Symbol struct
    pub fn index_symbol(&self, symbol: &crate::Symbol, file_path: &str) -> StorageResult<()> {
        self.add_document(
            symbol.id,
            &symbol.name,
            symbol.kind,
            symbol.file_id,
            file_path,
            symbol.range.start_line,
            symbol.range.start_column,
            symbol.doc_comment.as_ref().map(|s| s.as_ref()),
            symbol.signature.as_ref().map(|s| s.as_ref()),
            symbol.module_path.as_ref().map(|s| s.as_ref()).unwrap_or(""),
            None, // context not stored in Symbol struct
        )
    }
    
    /// Store file information
    pub(crate) fn store_file_info(
        &self,
        file_id: FileId,
        path: &str,
        hash: &str,
        timestamp: u64,
    ) -> StorageResult<()> {
        let mut writer_lock = match self.writer.lock() {
            Ok(lock) => lock,
            Err(poisoned) => {
                eprintln!("Warning: Recovering from poisoned writer mutex in store_file_info");
                poisoned.into_inner()
            }
        };
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
        
        let mut doc = Document::new();
        doc.add_text(self.schema.doc_type, "file_info");
        doc.add_u64(self.schema.file_id, file_id.value() as u64);
        doc.add_text(self.schema.file_path, path);
        doc.add_text(self.schema.file_hash, hash);
        doc.add_u64(self.schema.file_timestamp, timestamp);
        
        writer.add_document(doc)?;
        Ok(())
    }
    
    /// Store metadata (counters, etc.)
    pub(crate) fn store_metadata(
        &self,
        key: MetadataKey,
        value: u64,
    ) -> StorageResult<()> {
        let mut writer_lock = match self.writer.lock() {
            Ok(lock) => lock,
            Err(poisoned) => {
                eprintln!("Warning: Recovering from poisoned writer mutex in store_metadata");
                poisoned.into_inner()
            }
        };
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
        
        // First delete any existing metadata with this key
        let key_str = key.as_str();
        let term = Term::from_field_text(self.schema.meta_key, key_str);
        writer.delete_term(term);
        
        let mut doc = Document::new();
        doc.add_text(self.schema.doc_type, "metadata");
        doc.add_text(self.schema.meta_key, key_str);
        doc.add_u64(self.schema.meta_value, value);
        
        writer.add_document(doc)?;
        Ok(())
    }
    
    /// Query all relationships from the index
    #[allow(dead_code)]
    pub(crate) fn query_relationships(&self) -> StorageResult<Vec<(SymbolId, SymbolId, crate::Relationship)>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "relationship"),
            IndexRecordOption::Basic,
        );
        
        // Use a collector that retrieves all documents
        let collector = TopDocs::with_limit(1_000_000); // Adjust limit as needed
        let top_docs = searcher.search(&query, &collector)?;
        
        let mut relationships = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;
            
            let from_id = doc.get_first(self.schema.from_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "from_symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            let to_id = doc.get_first(self.schema.to_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "to_symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            let kind_str = doc.get_first(self.schema.relation_kind)
                .and_then(|v| v.as_str())
                .ok_or(StorageError::InvalidFieldValue { field: "relation_kind".to_string(), reason: "missing from document".to_string() })?;
                
            let weight = doc.get_first(self.schema.relation_weight)
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
            
            // Check for metadata
            let has_metadata = doc.get_first(self.schema.relation_line).is_some()
                || doc.get_first(self.schema.relation_column).is_some()
                || doc.get_first(self.schema.relation_context).is_some();
                
            if has_metadata {
                let mut metadata = RelationshipMetadata::new();
                
                if let Some(line) = doc.get_first(self.schema.relation_line).and_then(|v| v.as_u64()) {
                    metadata.line = Some(line as u32);
                }
                if let Some(column) = doc.get_first(self.schema.relation_column).and_then(|v| v.as_u64()) {
                    metadata.column = Some(column as u16);
                }
                if let Some(context) = doc.get_first(self.schema.relation_context).and_then(|v| v.as_str()) {
                    metadata.context = Some(context.into());
                }
                
                relationship = relationship.with_metadata(metadata);
            }
            
            relationships.push((from_id, to_id, relationship));
        }
        
        Ok(relationships)
    }
    
    /// Query all file information from the index
    #[allow(dead_code)]
    pub(crate) fn query_file_info(&self) -> StorageResult<Vec<(FileId, String, String, u64)>> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "file_info"),
            IndexRecordOption::Basic,
        );
        
        let collector = TopDocs::with_limit(100_000); // Adjust as needed
        let top_docs = searcher.search(&query, &collector)?;
        
        let mut files = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;
            
            let file_id = doc.get_first(self.schema.file_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| FileId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "file_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            let path = doc.get_first(self.schema.file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
                
            let hash = doc.get_first(self.schema.file_hash)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
                
            let timestamp = doc.get_first(self.schema.file_timestamp)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
                
            files.push((file_id, path, hash, timestamp));
        }
        
        Ok(files)
    }
    
    /// Count symbols in Tantivy index
    #[allow(dead_code)]
    pub(crate) fn count_symbol_documents(&self) -> StorageResult<u64> {
        let searcher = self.reader.searcher();
        let query = TermQuery::new(
            Term::from_field_text(self.schema.doc_type, "symbol"),
            IndexRecordOption::Basic,
        );
        
        let count = searcher.search(&query, &tantivy::collector::Count)?;
        Ok(count as u64)
    }
    
    /// DEPRECATED: This method is no longer needed with Tantivy-only architecture
    #[deprecated(note = "Use Tantivy queries directly instead of rebuilding IndexData")]
    #[allow(dead_code)]
    pub(crate) fn rebuild_index_data(&self) -> StorageResult<()> {
        // This method is kept temporarily for compatibility but does nothing
        Ok(())
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
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;
        
        if let Some((_score, doc_address)) = top_docs.into_iter().next() {
            let doc: Document = searcher.doc(doc_address)?;
            let value = doc.get_first(self.schema.meta_value)
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
    use tempfile::TempDir;
    
    #[test]
    fn test_document_index_creation() {
        let temp_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        
        assert_eq!(index.document_count().unwrap(), 0);
    }
    
    #[test]
    fn test_add_and_search_document() {
        let temp_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        
        // Start batch
        index.start_batch().unwrap();
        
        // Add a document
        let symbol_id = SymbolId::new(1).unwrap();
        let file_id = FileId::new(1).unwrap();
        index.add_document(
            symbol_id,
            "parse_json",
            SymbolKind::Function,
            file_id,
            "src/parser.rs",
            42,
            5,
            Some("Parse JSON string into a Value"),
            Some("fn parse_json(input: &str) -> StorageResult<Value, Error>"),
            "crate::parser",
            None,
        ).unwrap();
        
        // Commit batch
        index.commit_batch().unwrap();
        
        // Search for it
        let results = index.search("json", 10, None, None).unwrap();
        assert_eq!(results.len(), 1);
        
        let result = &results[0];
        assert_eq!(result.name, "parse_json");
        assert_eq!(result.line, 42);
        assert_eq!(result.file_path, "src/parser.rs");
    }
    
    #[test]
    fn test_fuzzy_search() {
        let temp_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        
        // Start batch
        index.start_batch().unwrap();
        
        let symbol_id = SymbolId::new(1).unwrap();
        let file_id = FileId::new(1).unwrap();
        index.add_document(
            symbol_id,
            "handle_request",
            SymbolKind::Function,
            file_id,
            "src/server.rs",
            100,
            0,
            Some("Handle incoming HTTP request"),
            None,
            "crate::server",
            None,
        ).unwrap();
        
        // Commit batch
        index.commit_batch().unwrap();
        
        // Search with typo - try searching for a single word with typo
        let results = index.search("handle", 10, None, None).unwrap();
        assert!(!results.is_empty(), "Should find exact match");
        
        // Now try with a small typo
        let results = index.search("handl", 10, None, None).unwrap();
        assert!(!results.is_empty(), "Should find with fuzzy search");
    }
    
    #[test]
    fn test_relationship_storage() {
        let temp_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        
        // Start batch
        index.start_batch().unwrap();
        
        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let rel = crate::Relationship::new(crate::RelationKind::Calls)
            .with_weight(0.8);
        
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
    fn test_file_info_storage() {
        let temp_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        
        // Start batch
        index.start_batch().unwrap();
        
        let file_id = crate::FileId::new(1).unwrap();
        index.store_file_info(file_id, "src/main.rs", "abc123", 1234567890).unwrap();
        
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
    fn test_metadata_storage() {
        let temp_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        
        // Start batch
        index.start_batch().unwrap();
        
        index.store_metadata(MetadataKey::FileCounter, 42).unwrap();
        index.store_metadata(MetadataKey::SymbolCounter, 100).unwrap();
        
        // Commit batch
        index.commit_batch().unwrap();
        
        // Query metadata
        assert_eq!(index.query_metadata(MetadataKey::FileCounter).unwrap(), Some(42));
        assert_eq!(index.query_metadata(MetadataKey::SymbolCounter).unwrap(), Some(100));
    }
}