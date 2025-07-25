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
        TextFieldIndexing, TextOptions, Value, FAST, STORED, STRING,
    },
    Index, IndexReader, IndexWriter, IndexSettings, ReloadPolicy, Term,
    TantivyDocument as Document,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use crate::{SymbolId, SymbolKind};

/// Schema fields for the document index
#[derive(Debug)]
pub struct IndexSchema {
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
}

impl IndexSchema {
    /// Create the schema for indexing code documentation
    pub fn build() -> (Schema, IndexSchema) {
        let mut builder = SchemaBuilder::default();
        
        // Stored fields for retrieval
        let symbol_id = builder.add_u64_field("symbol_id", STORED | FAST);
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
        let context = builder.add_text_field("context", text_options);
        
        // String fields for filtering (using STRING for exact match)
        let module_path = builder.add_text_field("module_path", STRING | STORED);
        let kind = builder.add_text_field("kind", STRING | STORED);
        
        let schema = builder.build();
        let index_schema = IndexSchema {
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
    writer: Mutex<Option<IndexWriter<Document>>>,
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
    pub fn new(index_path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
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
            
        Ok(Self {
            index,
            reader,
            schema: index_schema,
            index_path,
            writer: Mutex::new(None),
        })
    }
    
    /// Start a batch operation for adding multiple documents
    pub fn start_batch(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer_lock = self.writer.lock().unwrap();
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
        file_path: &str,
        line: u32,
        column: u16,
        doc_comment: Option<&str>,
        signature: Option<&str>,
        module_path: &str,
        context: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer_lock = self.writer.lock().unwrap();
        let writer = writer_lock.as_mut()
            .ok_or("No active batch. Call start_batch() first")?;
        
        let mut doc = Document::new();
        doc.add_u64(self.schema.symbol_id, symbol_id.value() as u64);
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
    pub fn commit_batch(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer_lock = self.writer.lock().unwrap();
        if let Some(mut writer) = writer_lock.take() {
            writer.commit()?;
            // Reload the reader to see new documents
            self.reader.reload()?;
        }
        Ok(())
    }
    
    /// Remove documents for a specific file
    pub fn remove_file_documents(&self, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Use existing batch writer if available, otherwise create temporary one
        let mut writer_lock = self.writer.lock().unwrap();
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
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let searcher = self.reader.searcher();
        
        // Build the query
        let mut subqueries: Vec<Box<dyn Query>> = Vec::new();
        
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
                .ok_or("Invalid symbol_id")?;
                
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
    pub fn document_count(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let searcher = self.reader.searcher();
        Ok(searcher.num_docs())
    }
    
    /// Clear all documents from the index
    pub fn clear(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.index.writer::<Document>(50_000_000)?;
        writer.delete_all_documents()?;
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
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
        index.add_document(
            symbol_id,
            "parse_json",
            SymbolKind::Function,
            "src/parser.rs",
            42,
            5,
            Some("Parse JSON string into a Value"),
            Some("fn parse_json(input: &str) -> Result<Value, Error>"),
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
        index.add_document(
            symbol_id,
            "handle_request",
            SymbolKind::Function,
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
}