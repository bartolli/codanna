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
use crate::vector::{VectorId, ClusterId, VectorSearchEngine, SegmentOrdinal, EmbeddingGenerator};
use super::{StorageError, StorageResult, MetadataKey};
use serde::{Serialize, Deserialize};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use tantivy::DocId;

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
    
    // Vector search fields
    pub cluster_id: Field,
    pub vector_id: Field,
    pub has_vector: Field,
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
        
        // Vector search fields
        let cluster_id = builder.add_u64_field("cluster_id", FAST | STORED);
        let vector_id = builder.add_u64_field("vector_id", FAST | STORED);
        let has_vector = builder.add_u64_field("has_vector", FAST | STORED); // Using u64 as bool for FAST field
        
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
            cluster_id,
            vector_id,
            has_vector,
        };
        
        (schema, index_schema)
    }
}

/// Metadata for tracking vector-related information per document
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMetadata {
    /// The vector ID associated with this document (maps to SymbolId)
    pub vector_id: Option<VectorId>,
    /// The cluster assignment for this vector
    pub cluster_id: Option<ClusterId>,
    /// Version of the embedding model used to generate this vector
    pub embedding_version: u32,
}

/// Internal representation for JSON serialization
#[derive(Serialize, Deserialize)]
struct VectorMetadataJson {
    vector_id: Option<u32>,
    cluster_id: Option<u32>,
    embedding_version: u32,
}

impl VectorMetadata {
    /// Creates a new VectorMetadata with no vector assignment
    pub fn new(embedding_version: u32) -> Self {
        Self {
            vector_id: None,
            cluster_id: None,
            embedding_version,
        }
    }
    
    /// Creates VectorMetadata with full vector information
    pub fn with_vector(vector_id: VectorId, cluster_id: ClusterId, embedding_version: u32) -> Self {
        Self {
            vector_id: Some(vector_id),
            cluster_id: Some(cluster_id),
            embedding_version,
        }
    }
    
    /// Checks if this document has an associated vector
    pub fn has_vector(&self) -> bool {
        self.vector_id.is_some()
    }
    
    /// Serializes the metadata to a JSON string for storage in Tantivy
    pub fn to_json(&self) -> StorageResult<String> {
        let json_repr = VectorMetadataJson {
            vector_id: self.vector_id.map(|id| id.get()),
            cluster_id: self.cluster_id.map(|id| id.get()),
            embedding_version: self.embedding_version,
        };
        serde_json::to_string(&json_repr)
            .map_err(|e| StorageError::Serialization(format!("Failed to serialize VectorMetadata: {}", e)))
    }
    
    /// Deserializes metadata from a JSON string
    pub fn from_json(json: &str) -> StorageResult<Self> {
        let json_repr: VectorMetadataJson = serde_json::from_str(json)
            .map_err(|e| StorageError::Serialization(format!("Failed to deserialize VectorMetadata: {}", e)))?;
        
        Ok(Self {
            vector_id: json_repr.vector_id.and_then(VectorId::new),
            cluster_id: json_repr.cluster_id.and_then(ClusterId::new),
            embedding_version: json_repr.embedding_version,
        })
    }
}

/// Cache for cluster assignments to enable efficient vector search
/// 
/// This cache maintains mappings from cluster IDs to document IDs within each segment,
/// enabling the vector search to quickly find relevant documents without scanning
/// all vectors. The cache is rebuilt when the index reader generation changes.
#[derive(Debug, Clone)]
struct ClusterCache {
    /// The reader generation this cache was built for
    generation: u64,
    /// Mappings per segment: SegmentOrdinal -> (ClusterId -> [DocId])
    segment_mappings: HashMap<SegmentOrdinal, HashMap<ClusterId, Vec<DocId>>>,
}

impl ClusterCache {
    /// Creates a new empty cache
    fn new(generation: u64) -> Self {
        Self {
            generation,
            segment_mappings: HashMap::new(),
        }
    }
    
    /// Checks if the cache is valid for the given generation
    fn is_valid_for_generation(&self, generation: u64) -> bool {
        self.generation == generation
    }
    
    /// Adds a document to the cache
    fn add_document(&mut self, segment_ord: SegmentOrdinal, cluster_id: ClusterId, doc_id: DocId) {
        self.segment_mappings
            .entry(segment_ord)
            .or_default()
            .entry(cluster_id)
            .or_default()
            .push(doc_id);
    }
    
    /// Gets all documents in a cluster for a specific segment
    fn get_documents(&self, segment_ord: SegmentOrdinal, cluster_id: ClusterId) -> Option<&[DocId]> {
        self.segment_mappings
            .get(&segment_ord)
            .and_then(|clusters| clusters.get(&cluster_id))
            .map(|docs| docs.as_slice())
    }
    
    /// Sorts all document lists for efficient searching
    fn sort_all(&mut self) {
        for segment_map in self.segment_mappings.values_mut() {
            for doc_list in segment_map.values_mut() {
                doc_list.sort_unstable();
            }
        }
    }
    
    /// Gets the total number of cached documents
    #[cfg(test)]
    fn total_documents(&self) -> usize {
        self.segment_mappings
            .values()
            .flat_map(|clusters| clusters.values())
            .map(|docs| docs.len())
            .sum()
    }
    
    /// Gets all unique cluster IDs across all segments
    fn all_cluster_ids(&self) -> Vec<ClusterId> {
        let mut cluster_ids: Vec<ClusterId> = self.segment_mappings
            .values()
            .flat_map(|clusters| clusters.keys())
            .copied()
            .collect();
        cluster_ids.sort_unstable_by_key(|id| id.get());
        cluster_ids.dedup();
        cluster_ids
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
    /// Optional path for vector storage files
    vector_storage_path: Option<PathBuf>,
    /// Optional vector search engine for semantic search
    vector_engine: Option<Arc<Mutex<VectorSearchEngine>>>,
    /// Cache for cluster assignments (protected by RwLock for concurrent reads)
    cluster_cache: Arc<RwLock<Option<ClusterCache>>>,
    /// Optional embedding generator for vector search
    embedding_generator: Option<Arc<dyn EmbeddingGenerator>>,
    /// Symbols pending vector processing (SymbolId, symbol_text)
    pub(crate) pending_embeddings: Mutex<Vec<(SymbolId, String)>>,
}

impl std::fmt::Debug for DocumentIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentIndex")
            .field("index_path", &self.index_path)
            .field("schema", &self.schema)
            .field("vector_storage_path", &self.vector_storage_path)
            .field("has_vector_engine", &self.vector_engine.is_some())
            .field("has_embedding_generator", &self.embedding_generator.is_some())
            .field("has_cluster_cache", &self.cluster_cache.read().unwrap().is_some())
            .field("pending_embeddings_count", &self.pending_embeddings.lock().unwrap().len())
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
            vector_storage_path: None,
            vector_engine: None,
            cluster_cache: Arc::new(RwLock::new(None)),
            embedding_generator: None,
            pending_embeddings: Mutex::new(Vec::new()),
        })
    }
    
    /// Enable vector search support with the given engine and storage path
    pub fn with_vector_support(
        mut self, 
        vector_engine: Arc<Mutex<VectorSearchEngine>>,
        vector_storage_path: impl AsRef<Path>
    ) -> Self {
        self.vector_storage_path = Some(vector_storage_path.as_ref().to_path_buf());
        self.vector_engine = Some(vector_engine);
        self
    }
    
    /// Set the embedding generator for vector search
    pub fn with_embedding_generator(mut self, generator: Arc<dyn EmbeddingGenerator>) -> Self {
        self.embedding_generator = Some(generator);
        self
    }
    
    /// Check if vector search is enabled
    pub fn has_vector_support(&self) -> bool {
        self.vector_engine.is_some()
    }
    
    /// Get the vector storage path if configured
    pub fn vector_storage_path(&self) -> Option<&Path> {
        self.vector_storage_path.as_deref()
    }
    
    /// Get a reference to the vector engine if configured
    pub fn vector_engine(&self) -> Option<&Arc<Mutex<VectorSearchEngine>>> {
        self.vector_engine.as_ref()
    }
    
    /// Process pending embeddings after a successful Tantivy commit
    fn post_commit_vector_processing(&self) -> StorageResult<()> {
        // Get pending embeddings
        let pending_embeddings = {
            let mut pending = self.pending_embeddings.lock()
                .map_err(|_| StorageError::LockPoisoned)?;
            std::mem::take(&mut *pending)
        };
        
        if pending_embeddings.is_empty() {
            return Ok(());
        }
        
        // Get references to engine and generator
        let vector_engine = self.vector_engine.as_ref()
            .ok_or_else(|| StorageError::General("Vector engine not configured".to_string()))?;
        let embedding_generator = self.embedding_generator.as_ref()
            .ok_or_else(|| StorageError::General("Embedding generator not configured".to_string()))?;
        
        // Extract texts for embedding generation
        let texts: Vec<&str> = pending_embeddings
            .iter()
            .map(|(_, text)| text.as_str())
            .collect();
        
        // Generate embeddings
        let embeddings = embedding_generator.generate_embeddings(&texts)
            .map_err(|e| StorageError::General(format!("Embedding generation failed: {}", e)))?;
        
        if embeddings.len() != pending_embeddings.len() {
            return Err(StorageError::General(format!(
                "Embedding count mismatch: expected {}, got {}",
                pending_embeddings.len(),
                embeddings.len()
            )));
        }
        
        // Create vector IDs and embeddings pairs
        let mut vectors = Vec::with_capacity(pending_embeddings.len());
        for (i, (symbol_id, _)) in pending_embeddings.iter().enumerate() {
            // Convert SymbolId to VectorId (both wrap u32)
            if let Some(vector_id) = crate::vector::VectorId::new(symbol_id.value()) {
                vectors.push((vector_id, embeddings[i].clone()));
            }
        }
        
        // Index vectors in the engine
        let mut engine = vector_engine.lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        engine.index_vectors(&vectors)
            .map_err(|e| StorageError::General(format!("Vector indexing failed: {}", e)))?;
        
        // Now we need to mark documents as having vectors
        // Since we can't do this in the same transaction, we'll need to do it separately
        // Store the pending updates for later processing
        drop(engine); // Release the lock
        
        // Call update_cluster_assignments to sync the cluster IDs
        // This will be done in a separate batch to avoid writer conflicts
        
        Ok(())
    }
    
    
    /// Build or rebuild the cluster cache from current segments
    /// This should be called after commits when vector support is enabled
    fn build_cluster_cache(&self) -> StorageResult<()> {
        if !self.has_vector_support() {
            return Ok(());
        }
        
        let searcher = self.reader.searcher();
        let generation = searcher.segment_readers().len() as u64; // Simple generation tracking
        
        // Check if cache is already valid
        {
            let cache = self.cluster_cache.read().map_err(|_| StorageError::LockPoisoned)?;
            if let Some(ref existing_cache) = *cache {
                if existing_cache.is_valid_for_generation(generation) {
                    return Ok(());
                }
            }
        }
        
        // Build new cache
        let mut new_cache = ClusterCache::new(generation);
        
        // Iterate through all segments
        for (segment_ord, segment_reader) in searcher.segment_readers().iter().enumerate() {
            let segment_ord = SegmentOrdinal::new(segment_ord as u32);
            
            // Get the fast fields for this segment
            let fast_fields = segment_reader.fast_fields();
            let cluster_id_reader = fast_fields.u64("cluster_id")?
                .first_or_default_col(0);
            let has_vector_reader = fast_fields.u64("has_vector")?
                .first_or_default_col(0);
            
            // Scan all documents in the segment
            for doc_id in 0..segment_reader.num_docs() {
                // Check if document has a vector
                let has_vector_val = has_vector_reader.get_val(doc_id);
                if has_vector_val == 1 {
                    // Get cluster assignment
                    let cluster_val = cluster_id_reader.get_val(doc_id);
                    if let Some(cluster_id) = ClusterId::new(cluster_val as u32) {
                        new_cache.add_document(segment_ord, cluster_id, doc_id);
                    }
                }
            }
        }
        
        // Sort all document lists for efficient searching
        new_cache.sort_all();
        
        // Store the new cache
        let mut cache = self.cluster_cache.write().map_err(|_| StorageError::LockPoisoned)?;
        *cache = Some(new_cache);
        
        Ok(())
    }
    
    /// Get documents in a specific cluster for a segment
    pub fn get_cluster_documents(&self, segment_ord: SegmentOrdinal, cluster_id: ClusterId) -> StorageResult<Vec<DocId>> {
        let cache = self.cluster_cache.read().map_err(|_| StorageError::LockPoisoned)?;
        
        if let Some(ref cluster_cache) = *cache {
            Ok(cluster_cache.get_documents(segment_ord, cluster_id)
                .map(|docs| docs.to_vec())
                .unwrap_or_default())
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Get all unique cluster IDs in the index
    pub fn get_all_cluster_ids(&self) -> StorageResult<Vec<ClusterId>> {
        let cache = self.cluster_cache.read().map_err(|_| StorageError::LockPoisoned)?;
        
        if let Some(ref cluster_cache) = *cache {
            Ok(cluster_cache.all_cluster_ids())
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Warm the cluster cache by forcing a rebuild
    /// This is useful after major index changes or reader reloads
    pub fn warm_cluster_cache(&self) -> StorageResult<()> {
        if !self.has_vector_support() {
            return Ok(());
        }
        
        // Force cache invalidation by setting an invalid generation
        {
            let mut cache = self.cluster_cache.write().map_err(|_| StorageError::LockPoisoned)?;
            *cache = None; // Clear existing cache to force rebuild
        }
        
        // Rebuild the cache
        self.build_cluster_cache()
    }
    
    /// Get current cache generation for monitoring
    pub fn get_cache_generation(&self) -> StorageResult<Option<u64>> {
        let cache = self.cluster_cache.read().map_err(|_| StorageError::LockPoisoned)?;
        Ok(cache.as_ref().map(|c| c.generation))
    }
    
    /// Reload the reader and warm caches
    /// This ensures the index is ready for high-performance queries
    pub fn reload_and_warm(&self) -> StorageResult<()> {
        // Reload the reader to see latest changes
        self.reader.reload()?;
        
        // Warm the cluster cache if vector support is enabled
        if self.has_vector_support() {
            self.warm_cluster_cache()?;
        }
        
        Ok(())
    }
    
    /// Update documents with cluster assignments from the vector engine
    /// This should be called after vector processing to sync cluster IDs
    pub fn update_cluster_assignments(&self) -> StorageResult<()> {
        if !self.has_vector_support() {
            return Ok(());
        }
        
        let vector_engine = self.vector_engine.as_ref()
            .ok_or_else(|| StorageError::General("Vector engine not configured".to_string()))?;
        
        let engine = vector_engine.lock()
            .map_err(|_| StorageError::LockPoisoned)?;
        
        // Get all vectors that have cluster assignments
        let cluster_assignments = engine.get_all_cluster_assignments();
        
        drop(engine); // Release the lock before we start updating
        
        // Convert VectorIds to SymbolIds
        let mut updates_needed = Vec::new();
        for (vector_id, cluster_id) in cluster_assignments {
            // Convert VectorId back to SymbolId
            if let Some(symbol_id) = SymbolId::new(vector_id.get()) {
                updates_needed.push((symbol_id, cluster_id));
            }
        }
        
        if updates_needed.is_empty() {
            return Ok(());
        }
        
        // Get the current searcher
        let searcher = self.reader.searcher();
        
        // Perform batch update
        self.start_batch()?;
        let mut writer_lock = self.writer.lock().map_err(|_| StorageError::LockPoisoned)?;
        let writer = writer_lock.as_mut()
            .ok_or(StorageError::NoActiveBatch)?;
        
        for (symbol_id, cluster_id) in updates_needed {
            // Find the document by symbol_id
            let symbol_id_term = Term::from_field_u64(self.schema.symbol_id, symbol_id.value() as u64);
            let query = TermQuery::new(symbol_id_term.clone(), IndexRecordOption::Basic);
            
            // Search for the document
            let top_docs = searcher.search(&query, &TopDocs::with_limit(1))?;
            
            if let Some((_score, doc_address)) = top_docs.first() {
                // Retrieve the stored document
                if let Ok(old_doc) = searcher.doc::<Document>(*doc_address) {
                    // Delete old document
                    writer.delete_term(symbol_id_term);
                    
                    // Create new document with updated cluster_id
                    let mut new_doc = Document::new();
                    
                    // Copy all fields except cluster_id and has_vector
                    for (field, value) in old_doc.field_values() {
                        if field != self.schema.cluster_id && field != self.schema.has_vector {
                            new_doc.add_field_value(field, value.clone());
                        }
                    }
                    
                    // Add updated vector fields
                    new_doc.add_u64(self.schema.cluster_id, cluster_id.get() as u64);
                    new_doc.add_u64(self.schema.has_vector, 1);
                    
                    writer.add_document(new_doc)?;
                }
            }
        }
        
        drop(writer_lock);
        self.commit_batch()?;
        
        Ok(())
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
        
        // Add default vector fields - these will be updated later if vectors are generated
        if self.has_vector_support() {
            doc.add_u64(self.schema.cluster_id, 0); // 0 means not yet assigned
            doc.add_u64(self.schema.vector_id, symbol_id.value() as u64);
            doc.add_u64(self.schema.has_vector, 0); // Will be set to 1 after vector processing
        }
        
        writer.add_document(doc)?;
        
        // Track symbol for vector embedding if vector support is enabled
        if self.has_vector_support() && self.embedding_generator.is_some() {
            // Create symbol text representation for embedding
            let symbol_text = format!(
                "{} {:?} {}",
                name,
                kind,
                signature.unwrap_or("")
            );
            
            let mut pending = self.pending_embeddings.lock()
                .map_err(|_| StorageError::LockPoisoned)?;
            pending.push((symbol_id, symbol_text));
        }
        
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
            
            // Process pending vector embeddings if enabled
            if self.has_vector_support() && self.embedding_generator.is_some() {
                self.post_commit_vector_processing()?;
            }
            
            // Build cluster cache if vector support is enabled
            self.build_cluster_cache()?;
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
            // Note: We don't commit here - that happens at batch end
        } else {
            // Create temporary writer for single operation
            drop(writer_lock); // Release lock before creating new writer
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
        
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.schema.name,
                self.schema.doc_comment,
                self.schema.signature,
                self.schema.context,
            ],
        );
        let main_query = query_parser.parse_query(query_str)?;

        // Fuzzy query for typo tolerance on the name field
        let term = Term::from_field_text(self.schema.name, query_str);
        let fuzzy_query = FuzzyTermQuery::new(term, 1, true);

        // All queries will be collected here.
        let mut all_clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        // The text search part: must match either the main query or the fuzzy query.
        all_clauses.push((
            Occur::Must,
            Box::new(BooleanQuery::new(vec![
                (Occur::Should, main_query),
                (Occur::Should, Box::new(fuzzy_query)),
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
            let term = Term::from_field_text(self.schema.kind, &format!("{:?}", kind));
            all_clauses.push((Occur::Must, Box::new(TermQuery::new(term, IndexRecordOption::Basic))));
        }
        
        if let Some(module) = module_filter {
            let term = Term::from_field_text(self.schema.module_path, module);
            all_clauses.push((Occur::Must, Box::new(TermQuery::new(term, IndexRecordOption::Basic))));
        }
        
        let final_query = BooleanQuery::new(all_clauses);
        
        let top_docs = searcher.search(&final_query, &TopDocs::with_limit(limit))?;
        
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
        
        // Use a QueryParser to correctly handle tokenization of the symbol name.
        let query_parser = QueryParser::for_index(&self.index, vec![self.schema.name]);
        let query = match query_parser.parse_query(name) {
            Ok(q) => q,
            Err(_) => {
                // If parsing fails (e.g., special characters), fall back to a simple term query.
                Box::new(TermQuery::new(
                    Term::from_field_text(self.schema.name, name),
                    IndexRecordOption::Basic,
                ))
            }
        };

        // Combine with a filter to ensure we only get symbol documents.
        let final_query = BooleanQuery::new(vec![
            (Occur::Must, query),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "symbol"),
                IndexRecordOption::Basic,
            ))),
        ]);
        
        let top_docs = searcher.search(&final_query, &TopDocs::with_limit(100))?;
        let mut symbols = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            symbols.push(self.document_to_symbol(&doc)?);
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
        let all_query = tantivy::query::AllQuery;

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
    
    /// Get all relationships of a specific kind
    pub fn get_all_relationships_by_kind(&self, kind: RelationKind) -> StorageResult<Vec<(SymbolId, SymbolId, Relationship)>> {
        let searcher = self.reader.searcher();
        let query = BooleanQuery::from(vec![
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.doc_type, "relationship"),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
            (Occur::Must, Box::new(TermQuery::new(
                Term::from_field_text(self.schema.relation_kind, &format!("{:?}", kind)),
                IndexRecordOption::Basic,
            )) as Box<dyn Query>),
        ]);
        
        let top_docs = searcher.search(&query, &TopDocs::with_limit(10000))?;
        let mut relationships = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<Document>(doc_address)?;
            
            let from_id = doc.get_first(self.schema.from_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "from_symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
            let to_id = doc.get_first(self.schema.to_symbol_id)
                .and_then(|v| v.as_u64())
                .and_then(|id| SymbolId::new(id as u32))
                .ok_or(StorageError::InvalidFieldValue { field: "to_symbol_id".to_string(), reason: "not a valid u32".to_string() })?;
                
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
        assert!(!index.has_vector_support());
        assert!(index.vector_storage_path().is_none());
        assert!(index.vector_engine().is_none());
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
    
    #[test]
    fn test_vector_metadata_creation() {
        // Test creating metadata without vector
        let metadata = VectorMetadata::new(1);
        assert_eq!(metadata.vector_id, None);
        assert_eq!(metadata.cluster_id, None);
        assert_eq!(metadata.embedding_version, 1);
        assert!(!metadata.has_vector());
        
        // Test creating metadata with vector
        let vector_id = VectorId::new(42).unwrap();
        let cluster_id = ClusterId::new(5).unwrap();
        let metadata_with_vector = VectorMetadata::with_vector(vector_id, cluster_id, 2);
        assert_eq!(metadata_with_vector.vector_id, Some(vector_id));
        assert_eq!(metadata_with_vector.cluster_id, Some(cluster_id));
        assert_eq!(metadata_with_vector.embedding_version, 2);
        assert!(metadata_with_vector.has_vector());
    }
    
    #[test]
    fn test_vector_metadata_serialization() {
        // Test serialization of metadata without vector
        let metadata = VectorMetadata::new(1);
        let json = metadata.to_json().unwrap();
        let deserialized = VectorMetadata::from_json(&json).unwrap();
        assert_eq!(metadata, deserialized);
        
        // Test serialization of metadata with vector
        let vector_id = VectorId::new(123).unwrap();
        let cluster_id = ClusterId::new(7).unwrap();
        let metadata_with_vector = VectorMetadata::with_vector(vector_id, cluster_id, 3);
        let json = metadata_with_vector.to_json().unwrap();
        let deserialized = VectorMetadata::from_json(&json).unwrap();
        assert_eq!(metadata_with_vector, deserialized);
        
        // Verify JSON structure
        assert!(json.contains("\"vector_id\""));
        assert!(json.contains("\"cluster_id\""));
        assert!(json.contains("\"embedding_version\":3"));
    }
    
    #[test]
    fn test_vector_metadata_deserialization_error() {
        // Test invalid JSON
        let invalid_json = "{ invalid json }";
        let result = VectorMetadata::from_json(invalid_json);
        assert!(result.is_err());
        match result {
            Err(StorageError::Serialization(msg)) => {
                assert!(msg.contains("Failed to deserialize VectorMetadata"));
            }
            _ => panic!("Expected Serialization error"),
        }
    }
    
    #[test]
    fn test_vector_metadata_tantivy_roundtrip() {
        use tantivy::schema::{SchemaBuilder, STORED, TEXT};
        use tantivy::{doc, Index, TantivyDocument};
        
        // Create a simple schema with a metadata field
        let mut schema_builder = SchemaBuilder::default();
        let metadata_field = schema_builder.add_text_field("vector_metadata", TEXT | STORED);
        let schema = schema_builder.build();
        
        // Create an in-memory index
        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer(50_000_000).unwrap();
        
        // Create test metadata
        let vector_id = VectorId::new(999).unwrap();
        let cluster_id = ClusterId::new(42).unwrap();
        let metadata = VectorMetadata::with_vector(vector_id, cluster_id, 5);
        
        // Serialize and store in document
        let json = metadata.to_json().unwrap();
        let doc = doc!(metadata_field => json.clone());
        index_writer.add_document(doc).unwrap();
        index_writer.commit().unwrap();
        
        // Read back from index
        let reader = index.reader().unwrap();
        let searcher = reader.searcher();
        let doc: TantivyDocument = searcher.doc(tantivy::DocAddress::new(0, 0)).unwrap();
        
        // Deserialize and verify
        let stored_json = doc.get_first(metadata_field)
            .and_then(|v| v.as_str())
            .unwrap();
        let retrieved_metadata = VectorMetadata::from_json(stored_json).unwrap();
        
        assert_eq!(metadata, retrieved_metadata);
        assert_eq!(json, stored_json);
    }
    
    #[test]
    fn test_document_index_with_vector_support() {
        use crate::vector::{VectorSearchEngine, VectorDimension};
        
        let temp_dir = TempDir::new().unwrap();
        let vector_dir = temp_dir.path().join("vectors");
        
        // Create vector engine
        let vector_engine = VectorSearchEngine::new(&vector_dir, VectorDimension::new(384).unwrap())
            .unwrap();
        let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
        
        // Create index with vector support
        let index = DocumentIndex::new(temp_dir.path())
            .unwrap()
            .with_vector_support(vector_engine_arc.clone(), &vector_dir);
        
        // Verify vector support is enabled
        assert!(index.has_vector_support());
        assert_eq!(index.vector_storage_path(), Some(vector_dir.as_ref()));
        assert!(index.vector_engine().is_some());
        
        // Verify the engine reference is the same
        let engine_ref = index.vector_engine().unwrap();
        assert!(Arc::ptr_eq(engine_ref, &vector_engine_arc));
    }
    
    #[test]
    fn test_document_index_operations_with_and_without_vectors() {
        let temp_dir = TempDir::new().unwrap();
        
        // Test 1: Create index without vector support
        let index_no_vectors = DocumentIndex::new(temp_dir.path().join("no_vectors")).unwrap();
        
        // Basic operations should work
        index_no_vectors.start_batch().unwrap();
        index_no_vectors.add_document(
            SymbolId::new(1).unwrap(),
            "test_func",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            "test.rs",
            1,
            1,
            None,
            None,
            "test",
            None,
        ).unwrap();
        index_no_vectors.commit_batch().unwrap();
        
        let results = index_no_vectors.search("test_func", 10, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!index_no_vectors.has_vector_support());
        
        // Test 2: Create index with vector support
        use crate::vector::{VectorSearchEngine, VectorDimension};
        
        let vector_dir = temp_dir.path().join("vectors");
        let vector_engine = VectorSearchEngine::new(&vector_dir, VectorDimension::new(384).unwrap())
            .unwrap();
        let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
        
        let index_with_vectors = DocumentIndex::new(temp_dir.path().join("with_vectors"))
            .unwrap()
            .with_vector_support(vector_engine_arc, &vector_dir);
        
        // Same operations should work with vector support
        index_with_vectors.start_batch().unwrap();
        index_with_vectors.add_document(
            SymbolId::new(2).unwrap(),
            "vector_func",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            "test.rs",
            10,
            1,
            None,
            None,
            "test",
            None,
        ).unwrap();
        index_with_vectors.commit_batch().unwrap();
        
        let results = index_with_vectors.search("vector_func", 10, None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(index_with_vectors.has_vector_support());
    }
    
    #[test]
    fn test_document_index_debug_impl() {
        use crate::vector::{VectorSearchEngine, VectorDimension};
        
        let temp_dir = TempDir::new().unwrap();
        
        // Test Debug without vector support
        let index = DocumentIndex::new(temp_dir.path()).unwrap();
        let debug_str = format!("{:?}", index);
        assert!(debug_str.contains("DocumentIndex"));
        assert!(debug_str.contains("index_path"));
        assert!(debug_str.contains("vector_storage_path: None"));
        assert!(debug_str.contains("has_vector_engine: false"));
        
        // Test Debug with vector support
        let vector_dir = temp_dir.path().join("vectors");
        let vector_engine = VectorSearchEngine::new(&vector_dir, VectorDimension::new(384).unwrap())
            .unwrap();
        let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
        
        let index_with_vectors = DocumentIndex::new(temp_dir.path())
            .unwrap()
            .with_vector_support(vector_engine_arc, &vector_dir);
            
        let debug_str = format!("{:?}", index_with_vectors);
        assert!(debug_str.contains("DocumentIndex"));
        assert!(debug_str.contains("has_vector_engine: true"));
        assert!(debug_str.contains(&format!("vector_storage_path: Some(\"{}\")", vector_dir.display())));
    }
    
    #[test]
    fn test_cluster_cache_operations() {
        use crate::vector::{VectorSearchEngine, VectorDimension};
        use std::time::Instant;
        
        let temp_dir = TempDir::new().unwrap();
        let vector_dir = temp_dir.path().join("vectors");
        
        // Create vector engine
        let vector_engine = VectorSearchEngine::new(&vector_dir, VectorDimension::new(384).unwrap())
            .unwrap();
        let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
        
        // Create index with vector support
        let index = DocumentIndex::new(temp_dir.path())
            .unwrap()
            .with_vector_support(vector_engine_arc, &vector_dir);
        
        // Start batch and add documents with cluster assignments
        index.start_batch().unwrap();
        
        // Add documents to different clusters
        let test_docs = vec![
            (1, "parse_string", 0),    // cluster 0
            (2, "parse_number", 0),    // cluster 0  
            (3, "parse_json", 0),      // cluster 0
            (4, "Parser", 1),          // cluster 1
            (5, "JsonParser", 1),      // cluster 1
            (6, "handle_error", 2),    // cluster 2
            (7, "ParseError", 2),      // cluster 2
        ];
        
        for (id, name, cluster) in &test_docs {
            let mut doc = Document::new();
            doc.add_text(index.schema.doc_type, "symbol");
            doc.add_u64(index.schema.symbol_id, *id);
            doc.add_text(index.schema.name, name);
            doc.add_text(index.schema.kind, "function");
            doc.add_u64(index.schema.file_id, 1);
            doc.add_text(index.schema.file_path, "test.rs");
            doc.add_u64(index.schema.line_number, (*id * 10) as u64);
            doc.add_u64(index.schema.column, 1);
            doc.add_text(index.schema.module_path, "test");
            
            // Add vector fields - cluster IDs need to be non-zero
            doc.add_u64(index.schema.cluster_id, *cluster + 1); // Make 1-based
            doc.add_u64(index.schema.vector_id, *id);
            doc.add_u64(index.schema.has_vector, 1);
            
            index.writer.lock().unwrap()
                .as_mut().unwrap()
                .add_document(doc).unwrap();
        }
        
        // Commit and trigger cache building
        index.commit_batch().unwrap();
        
        // Verify cache was built
        {
            let cache = index.cluster_cache.read().unwrap();
            assert!(cache.is_some(), "Cache should be built after commit");
            
            let cluster_cache = cache.as_ref().unwrap();
            println!("Total documents in cache: {}", cluster_cache.total_documents());
            
            // Check cluster IDs
            let cluster_ids = cluster_cache.all_cluster_ids();
            println!("Found cluster IDs: {:?}", cluster_ids.iter().map(|id| id.get()).collect::<Vec<_>>());
            
            // Debug: print mappings
            for (_seg_ord, clusters) in &cluster_cache.segment_mappings {
                println!("Segment {}: clusters={:?}", _seg_ord.get(), 
                    clusters.keys().map(|c| c.get()).collect::<Vec<_>>());
                for (cluster_id, docs) in clusters {
                    println!("  Cluster {}: {} docs", cluster_id.get(), docs.len());
                }
            }
        }
        
        // Test document lookups - need to collect from all segments
        let all_cluster_ids = index.get_all_cluster_ids().unwrap();
        assert_eq!(all_cluster_ids.len(), 3);
        
        // Count total documents per cluster across all segments
        let mut cluster1_total = 0;
        let mut cluster2_total = 0;
        let mut cluster3_total = 0;
        
        {
            let cache = index.cluster_cache.read().unwrap();
            let cluster_cache = cache.as_ref().unwrap();
            
            for (_seg_ord, clusters) in &cluster_cache.segment_mappings {
                if let Some(docs) = clusters.get(&ClusterId::new(1).unwrap()) {
                    cluster1_total += docs.len();
                }
                if let Some(docs) = clusters.get(&ClusterId::new(2).unwrap()) {
                    cluster2_total += docs.len();
                }
                if let Some(docs) = clusters.get(&ClusterId::new(3).unwrap()) {
                    cluster3_total += docs.len();
                }
            }
        }
        
        assert_eq!(cluster1_total, 3);
        assert_eq!(cluster2_total, 2);
        assert_eq!(cluster3_total, 2);
        
        // Test performance of lookups (should be <10μs)
        let cluster_id = ClusterId::new(1).unwrap();
        let segment_ord = SegmentOrdinal::new(0);
        let start = Instant::now();
        for _ in 0..1000 {
            let _ = index.get_cluster_documents(segment_ord, cluster_id).unwrap();
        }
        let elapsed = start.elapsed();
        let per_lookup = elapsed / 1000;
        println!("Cluster lookup performance: {:?} per lookup", per_lookup);
        assert!(per_lookup.as_micros() < 10, "Lookup should be <10μs, was {:?}", per_lookup);
        
        // Test cache invalidation - add more documents
        index.start_batch().unwrap();
        
        let mut doc = Document::new();
        doc.add_text(index.schema.doc_type, "symbol");
        doc.add_u64(index.schema.symbol_id, 8);
        doc.add_text(index.schema.name, "new_function");
        doc.add_text(index.schema.kind, "function");
        doc.add_u64(index.schema.file_id, 1);
        doc.add_text(index.schema.file_path, "test.rs");
        doc.add_u64(index.schema.line_number, 80);
        doc.add_u64(index.schema.column, 1);
        doc.add_text(index.schema.module_path, "test");
        doc.add_u64(index.schema.cluster_id, 1); // cluster_id must be non-zero
        doc.add_u64(index.schema.vector_id, 8);
        doc.add_u64(index.schema.has_vector, 1);
        
        index.writer.lock().unwrap()
            .as_mut().unwrap()
            .add_document(doc).unwrap();
        
        index.commit_batch().unwrap();
        
        // Verify cache was rebuilt
        {
            let cache = index.cluster_cache.read().unwrap();
            let cluster_cache = cache.as_ref().unwrap();
            assert_eq!(cluster_cache.total_documents(), 8);
        }
        
        // Count documents again after update
        {
            let cache = index.cluster_cache.read().unwrap();
            let cluster_cache = cache.as_ref().unwrap();
            
            let mut cluster1_updated = 0;
            for (_seg_ord, clusters) in &cluster_cache.segment_mappings {
                if let Some(docs) = clusters.get(&ClusterId::new(1).unwrap()) {
                    cluster1_updated += docs.len();
                }
            }
            assert_eq!(cluster1_updated, 4); // Should have one more
        }
    }
    
    #[test]
    fn test_cluster_cache_unit_operations() {
        // Test ClusterCache directly
        let mut cache = ClusterCache::new(1);
        
        let seg0 = SegmentOrdinal::new(0);
        let seg1 = SegmentOrdinal::new(1);
        let cluster0 = ClusterId::new(1).unwrap();
        let cluster1 = ClusterId::new(2).unwrap();
        
        // Add documents
        cache.add_document(seg0, cluster0, 0);
        cache.add_document(seg0, cluster0, 2);
        cache.add_document(seg0, cluster0, 1);
        cache.add_document(seg0, cluster1, 3);
        cache.add_document(seg1, cluster0, 0);
        
        // Sort
        cache.sort_all();
        
        // Test retrieval
        let docs = cache.get_documents(seg0, cluster0).unwrap();
        assert_eq!(docs, &[0, 1, 2]); // Should be sorted
        
        let docs = cache.get_documents(seg0, cluster1).unwrap();
        assert_eq!(docs, &[3]);
        
        let docs = cache.get_documents(seg1, cluster0).unwrap();
        assert_eq!(docs, &[0]);
        
        // Test missing lookups
        assert!(cache.get_documents(seg1, cluster1).is_none());
        
        // Test metadata
        assert_eq!(cache.total_documents(), 5);
        assert_eq!(cache.all_cluster_ids().len(), 2);
        
        // Test generation check
        assert!(cache.is_valid_for_generation(1));
        assert!(!cache.is_valid_for_generation(2));
    }
}