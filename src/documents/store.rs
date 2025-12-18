//! Document storage with tantivy metadata and vector embeddings.
//!
//! This module provides the main storage interface for document chunks,
//! combining tantivy for metadata/filtering with mmap vectors for semantic search.

use std::collections::HashMap;

/// Progress updates during document indexing.
#[derive(Debug, Clone)]
pub enum IndexProgress<'a> {
    /// Processing a file (chunking, metadata extraction)
    ProcessingFile {
        current: usize,
        total: usize,
        path: &'a Path,
    },
    /// Generating embeddings for chunks
    GeneratingEmbeddings { current: usize, total: usize },
}

/// Default batch size for embedding generation.
/// Smaller batches reduce memory pressure and provide smoother progress.
const EMBEDDING_BATCH_SIZE: usize = 64;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::directory::error::OpenDirectoryError;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::Value;
use tantivy::{
    Index, IndexReader, IndexSettings, IndexWriter, ReloadPolicy, TantivyDocument as Document, Term,
};
use thiserror::Error;

use super::chunker::{Chunker, HybridChunker, RawChunk};
use super::config::{ChunkingConfig, CollectionConfig};
use super::schema::DocumentSchema;
use super::types::{ChunkId, CollectionId, FileState};
use crate::indexing::file_info::{calculate_hash, get_utc_timestamp};
use crate::vector::{
    ClusterId, EmbeddingGenerator, MmapVectorStorage, SegmentOrdinal, VectorDimension, VectorId,
    VectorStorageError, cosine_similarity, kmeans_clustering,
};

/// Errors from document storage operations.
#[derive(Error, Debug)]
pub enum DocumentStoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    #[error("Directory error: {0}")]
    Directory(#[from] OpenDirectoryError),

    #[error("Vector storage error: {0}")]
    VectorStorage(#[from] VectorStorageError),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Lock poisoned")]
    LockPoisoned,
}

/// Result type for document store operations.
pub type StoreResult<T> = Result<T, DocumentStoreError>;

/// Statistics from an indexing operation.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Number of files processed.
    pub files_processed: usize,
    /// Number of files skipped (unchanged).
    pub files_skipped: usize,
    /// Number of chunks created.
    pub chunks_created: usize,
    /// Number of chunks removed (from changed/deleted files).
    pub chunks_removed: usize,
}

/// Query parameters for document search.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Search text to embed and match.
    pub text: String,
    /// Filter by collection name.
    pub collection: Option<String>,
    /// Filter by source document path.
    pub document: Option<PathBuf>,
    /// Maximum results to return.
    pub limit: usize,
    /// Preview configuration (KWIC, highlighting, etc.).
    pub preview_config: Option<super::config::SearchConfig>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            collection: None,
            document: None,
            limit: 10,
            preview_config: None,
        }
    }
}

/// Extract a KWIC (Keyword In Context) preview centered on the first keyword match.
/// Expands boundaries to word edges to avoid cutting words mid-character.
fn extract_kwic_preview(content: &str, query: &str, window_chars: usize) -> String {
    // Find first keyword match (case-insensitive)
    let content_lower = content.to_lowercase();
    let query_lower = query.to_lowercase();

    // Try to find any word from the query
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let mut best_match_pos: Option<usize> = None;

    for word in &query_words {
        if word.len() < 2 {
            continue; // Skip very short words
        }
        if let Some(pos) = content_lower.find(word) {
            // Prefer earlier matches
            if best_match_pos.is_none() || pos < best_match_pos.unwrap() {
                best_match_pos = Some(pos);
            }
        }
    }

    // If no match found, just return from start
    let match_pos = best_match_pos.unwrap_or(0);

    // Calculate window boundaries (character-based)
    let half_window = window_chars / 2;
    let chars: Vec<char> = content.chars().collect();
    let total_chars = chars.len();

    // Find char position from byte position
    let char_pos = content[..match_pos.min(content.len())]
        .chars()
        .count()
        .min(total_chars);

    let mut start_char = char_pos.saturating_sub(half_window);
    let mut end_char = (char_pos + half_window).min(total_chars);

    // Expand start to word boundary (find previous whitespace)
    if start_char > 0 {
        while start_char > 0 && !chars[start_char - 1].is_whitespace() {
            start_char -= 1;
        }
    }

    // Expand end to word boundary (find next whitespace)
    if end_char < total_chars {
        while end_char < total_chars && !chars[end_char].is_whitespace() {
            end_char += 1;
        }
    }

    // Build preview
    let mut preview = String::new();

    if start_char > 0 {
        preview.push_str("...");
    }

    preview.extend(chars[start_char..end_char].iter());

    if end_char < total_chars {
        preview.push_str("...");
    }

    preview
}

/// Dual highlighting markers for both humans and LLMs.
/// - ANSI bold cyan: renders as color in terminals
/// - Text markers >>..<<: parseable pattern for LLMs
const HIGHLIGHT_START: &str = "\x1b[1;36m>>";
const HIGHLIGHT_END: &str = "<<\x1b[0m";

/// Highlight keywords with dual markers (ANSI + text).
/// Merges adjacent keywords: ">>word1<< >>word2<<" becomes ">>word1 word2<<"
fn highlight_keywords(text: &str, query: &str) -> String {
    let query_words: Vec<&str> = query.split_whitespace().collect();
    let text_lower = text.to_lowercase();

    // Collect all match ranges (start, end)
    let mut matches: Vec<(usize, usize)> = Vec::new();

    for word in &query_words {
        if word.len() < 2 {
            continue;
        }
        let word_lower = word.to_lowercase();
        let mut search_start = 0;

        while let Some(rel_pos) = text_lower[search_start..].find(&word_lower) {
            let start = search_start + rel_pos;
            let end = start + word.len();
            matches.push((start, end));
            search_start = end;
        }
    }

    if matches.is_empty() {
        return text.to_string();
    }

    // Sort by start position
    matches.sort_by_key(|m| m.0);

    // Merge overlapping or adjacent ranges (adjacent = spaces/tabs only, not newlines)
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in matches {
        if let Some(last) = merged.last_mut() {
            // Check if adjacent: only spaces/tabs between (no newlines)
            let between = &text[last.1..start];
            let is_adjacent = start <= last.1 || between.chars().all(|c| c == ' ' || c == '\t');
            if is_adjacent {
                // Merge: extend the previous range
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    // Build result with highlights
    let mut result = String::new();
    let mut offset = 0;

    for (start, end) in merged {
        result.push_str(&text[offset..start]);
        result.push_str(HIGHLIGHT_START);
        result.push_str(&text[start..end]);
        result.push_str(HIGHLIGHT_END);
        offset = end;
    }

    result.push_str(&text[offset..]);
    result
}

/// Generate preview from full content based on config.
fn generate_preview(content: &str, query: &str, config: &super::config::SearchConfig) -> String {
    use super::config::PreviewMode;

    let preview = match config.preview_mode {
        PreviewMode::Full => content.to_string(),
        PreviewMode::Kwic => extract_kwic_preview(content, query, config.preview_chars),
    };

    if config.highlight {
        highlight_keywords(&preview, query)
    } else {
        preview
    }
}

/// A search result with chunk metadata and similarity score.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    /// Chunk identifier.
    pub chunk_id: ChunkId,
    /// Collection this chunk belongs to.
    pub collection: String,
    /// Source file path.
    pub source_path: PathBuf,
    /// Heading hierarchy for context.
    pub heading_context: Vec<String>,
    /// Content preview (first ~200 chars).
    pub content_preview: String,
    /// Byte range in source file.
    pub byte_range: (usize, usize),
    /// Similarity score (0.0 - 1.0).
    pub similarity: f32,
}

/// Document store combining tantivy metadata with vector embeddings.
pub struct DocumentStore {
    /// Base path for all storage files.
    base_path: PathBuf,

    /// Tantivy index for chunk metadata.
    index: Index,

    /// Index reader for queries.
    reader: IndexReader,

    /// Schema fields for documents.
    schema: DocumentSchema,

    /// Index writer (lazily created).
    writer: Mutex<Option<IndexWriter<Document>>>,

    /// Vector storage for chunk embeddings.
    vector_storage: Option<MmapVectorStorage>,

    /// Cluster assignments for IVFFlat search.
    cluster_assignments: HashMap<VectorId, ClusterId>,

    /// Cluster centroids.
    centroids: Vec<Vec<f32>>,

    /// File states for change detection.
    file_states: HashMap<PathBuf, FileState>,

    /// Collection name to ID mapping.
    collection_ids: HashMap<String, CollectionId>,

    /// Next chunk ID counter.
    next_chunk_id: u64,

    /// Chunker implementation.
    chunker: Box<dyn Chunker>,

    /// Embedding generator (optional).
    embedding_generator: Option<Box<dyn EmbeddingGenerator>>,

    /// Vector dimension.
    dimension: VectorDimension,

    /// Tantivy heap size in bytes.
    heap_size: usize,
}

impl std::fmt::Debug for DocumentStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentStore")
            .field("base_path", &self.base_path)
            .field("has_vector_storage", &self.vector_storage.is_some())
            .field(
                "has_embedding_generator",
                &self.embedding_generator.is_some(),
            )
            .field("file_states_count", &self.file_states.len())
            .field("collection_count", &self.collection_ids.len())
            .field("next_chunk_id", &self.next_chunk_id)
            .finish()
    }
}

impl DocumentStore {
    /// Create or open a document store.
    ///
    /// # Arguments
    /// * `base_path` - Directory for all storage files (tantivy index, vectors, state)
    /// * `dimension` - Vector dimension for embeddings
    pub fn new(base_path: impl AsRef<Path>, dimension: VectorDimension) -> StoreResult<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base_path)?;

        let index_path = base_path.join("tantivy");
        std::fs::create_dir_all(&index_path)?;

        let (tantivy_schema, document_schema) = DocumentSchema::build();

        // Create or open tantivy index
        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(&index_path)?
        } else {
            let dir = MmapDirectory::open(&index_path)?;
            Index::create(dir, tantivy_schema, IndexSettings::default())?
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        // If opening existing index, reload to get latest segments
        if index_path.join("meta.json").exists() {
            reader.reload()?;
        }

        // Load persisted state if available
        let state_path = base_path.join("state.json");
        let (file_states, collection_ids, next_chunk_id) = if state_path.exists() {
            Self::load_state(&state_path)?
        } else {
            (HashMap::new(), HashMap::new(), 1)
        };

        Ok(Self {
            base_path,
            index,
            reader,
            schema: document_schema,
            writer: Mutex::new(None),
            vector_storage: None,
            cluster_assignments: HashMap::new(),
            centroids: Vec::new(),
            file_states,
            collection_ids,
            next_chunk_id,
            chunker: Box::new(HybridChunker::new()),
            embedding_generator: None,
            dimension,
            heap_size: 50_000_000, // 50MB default
        })
    }

    /// Enable embedding generation for semantic search.
    pub fn with_embeddings(mut self, generator: Box<dyn EmbeddingGenerator>) -> StoreResult<Self> {
        // Initialize vector storage
        let vector_path = self.base_path.join("vectors");
        std::fs::create_dir_all(&vector_path)?;

        let vector_storage = MmapVectorStorage::open_or_create(
            &vector_path,
            SegmentOrdinal::new(0),
            self.dimension,
        )?;

        self.vector_storage = Some(vector_storage);
        self.embedding_generator = Some(generator);

        // Load cluster data if available
        self.load_cluster_data()?;

        Ok(self)
    }

    /// Count the number of files that would be indexed for a collection.
    ///
    /// Useful for progress bar setup before indexing.
    pub fn count_collection_files(&self, config: &CollectionConfig) -> StoreResult<usize> {
        let files = self.collect_files(config)?;
        Ok(files.len())
    }

    /// Index documents from a collection configuration.
    ///
    /// Only processes files that have changed since last index.
    pub fn index_collection(
        &mut self,
        name: &str,
        config: &CollectionConfig,
        chunking_config: &ChunkingConfig,
    ) -> StoreResult<IndexStats> {
        self.index_collection_with_progress(name, config, chunking_config, |_| {})
    }

    /// Index documents from a collection with progress callback.
    ///
    /// Progress is reported in two phases:
    /// 1. `ProcessingFile` - for each file being chunked and indexed
    /// 2. `GeneratingEmbeddings` - for each batch of embeddings generated
    ///
    /// Embeddings are generated in batches to reduce memory pressure.
    pub fn index_collection_with_progress<F>(
        &mut self,
        name: &str,
        config: &CollectionConfig,
        chunking_config: &ChunkingConfig,
        mut on_progress: F,
    ) -> StoreResult<IndexStats>
    where
        F: FnMut(IndexProgress<'_>),
    {
        let mut stats = IndexStats::default();

        // Ensure collection has an ID
        let _collection_id = self.get_or_create_collection_id(name);

        // Collect files to process
        let files = self.collect_files(config)?;

        // Detect changes
        let (changed, unchanged, removed) = self.detect_changes(&files, name);

        stats.files_skipped = unchanged.len();

        // Remove chunks from deleted/changed files
        for path in removed.iter().chain(changed.iter()) {
            if let Some(state) = self.file_states.get(path) {
                stats.chunks_removed += state.chunk_ids.len();
                self.delete_chunks_by_file(path, name)?;
            }
        }

        // Phase 1: Process files (chunking and metadata)
        let mut pending_embeddings: Vec<(ChunkId, String)> = Vec::new();
        let total_files = changed.len();

        for (idx, path) in changed.iter().enumerate() {
            // Report file processing progress
            on_progress(IndexProgress::ProcessingFile {
                current: idx + 1,
                total: total_files,
                path,
            });

            let content = std::fs::read_to_string(path)?;
            let raw_chunks = self.chunker.chunk(&content, chunking_config);

            let mut chunk_ids = Vec::new();

            for raw_chunk in raw_chunks {
                let chunk_id = self.allocate_chunk_id();
                chunk_ids.push(chunk_id);

                // Store chunk metadata in tantivy
                self.store_chunk(chunk_id, name, path, &raw_chunk, &content)?;

                // Queue for embedding
                pending_embeddings.push((chunk_id, raw_chunk.content.clone()));

                stats.chunks_created += 1;
            }

            // Update file state
            let file_state = FileState {
                path: path.clone(),
                collection: name.to_string(),
                content_hash: calculate_hash(&content),
                chunk_ids,
                last_indexed: get_utc_timestamp(),
            };
            self.file_states.insert(path.clone(), file_state);

            stats.files_processed += 1;
        }

        // Commit tantivy changes
        self.commit()?;

        // Phase 2: Generate embeddings in batches
        if !pending_embeddings.is_empty() {
            self.process_embeddings_batched(&pending_embeddings, &mut on_progress)?;
        }

        // Remove file states for deleted files
        for path in &removed {
            self.file_states.remove(path);
        }

        // Persist state
        self.save_state()?;

        Ok(stats)
    }

    /// Re-index a single file.
    ///
    /// Used by the file watcher when a document changes. Looks up the file's
    /// collection from stored state and re-indexes with the provided config.
    ///
    /// Returns the number of chunks created, or None if file wasn't indexed.
    pub fn reindex_file(
        &mut self,
        path: &Path,
        chunking_config: &ChunkingConfig,
    ) -> StoreResult<Option<usize>> {
        // Look up collection from file state
        let collection = match self.file_states.get(path) {
            Some(state) => state.collection.clone(),
            None => return Ok(None), // File not in index
        };

        // Delete existing chunks
        self.delete_chunks_by_file(path, &collection)?;

        // Read file content
        let content = std::fs::read_to_string(path)?;

        // Chunk the content
        let raw_chunks = self.chunker.chunk(&content, chunking_config);
        let mut chunk_ids = Vec::new();
        let mut pending_embeddings: Vec<(ChunkId, String)> = Vec::new();

        for raw_chunk in raw_chunks {
            let chunk_id = self.allocate_chunk_id();
            chunk_ids.push(chunk_id);

            // Store chunk metadata in tantivy
            self.store_chunk(chunk_id, &collection, path, &raw_chunk, &content)?;

            // Queue for embedding
            pending_embeddings.push((chunk_id, raw_chunk.content.clone()));
        }

        // Commit tantivy changes
        self.commit()?;

        // Generate embeddings
        let chunks_created = pending_embeddings.len();
        if !pending_embeddings.is_empty() {
            self.process_embeddings_batched(&pending_embeddings, &mut |_| {})?;
        }

        // Update file state
        let file_state = FileState {
            path: path.to_path_buf(),
            collection,
            content_hash: calculate_hash(&content),
            chunk_ids,
            last_indexed: get_utc_timestamp(),
        };
        self.file_states.insert(path.to_path_buf(), file_state);

        // Persist state
        self.save_state()?;

        Ok(Some(chunks_created))
    }

    /// Remove a file from the index.
    ///
    /// Used by the file watcher when a document is deleted.
    /// Returns true if the file was in the index.
    pub fn remove_file(&mut self, path: &Path) -> StoreResult<bool> {
        let Some(state) = self.file_states.remove(path) else {
            return Ok(false);
        };

        // Delete chunks from tantivy
        self.delete_chunks_by_file(path, &state.collection)?;
        self.commit()?;

        // Persist state
        self.save_state()?;

        Ok(true)
    }

    /// Get the collection name for a file, if indexed.
    pub fn get_file_collection(&self, path: &Path) -> Option<&str> {
        self.file_states.get(path).map(|s| s.collection.as_str())
    }

    /// Get all indexed file paths.
    pub fn get_indexed_paths(&self) -> Vec<PathBuf> {
        self.file_states.keys().cloned().collect()
    }

    /// Clear all file states to force full re-indexing.
    ///
    /// Used by `--force` flag to treat all files as new.
    pub fn clear_file_states(&mut self) {
        self.file_states.clear();
    }

    /// Search for chunks matching a query.
    pub fn search(&mut self, query: SearchQuery) -> StoreResult<Vec<SearchResult>> {
        if query.text.is_empty() {
            return Ok(Vec::new());
        }

        // Get candidate chunks based on filters
        let candidates = self.get_filtered_candidates(&query)?;

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // If no embeddings, return candidates by tantivy score order
        let Some(ref generator) = self.embedding_generator else {
            return self.enrich_results(candidates, &query);
        };

        // Generate query embedding
        let query_embeddings = generator
            .generate_embeddings(&[query.text.as_str()])
            .map_err(|e| DocumentStoreError::Embedding(e.to_string()))?;

        let query_vec = query_embeddings
            .into_iter()
            .next()
            .ok_or_else(|| DocumentStoreError::Embedding("No embedding generated".to_string()))?;

        // Score candidates by vector similarity
        let mut scored_candidates = self.score_by_similarity(&candidates, &query_vec)?;

        // Sort by similarity (highest first) and limit
        scored_candidates
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored_candidates.truncate(query.limit);

        // Enrich with full metadata and KWIC preview
        self.build_search_results(scored_candidates, &query)
    }

    /// Delete all chunks from a collection.
    pub fn delete_collection(&mut self, name: &str) -> StoreResult<usize> {
        let searcher = self.reader.searcher();

        // Find all chunks in collection
        let term = Term::from_field_text(self.schema.collection_name, name);
        let query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

        let top_docs = searcher.search(&query, &TopDocs::with_limit(100_000))?;
        let count = top_docs.len();

        // Delete from tantivy
        {
            let mut writer_guard = self
                .writer
                .lock()
                .map_err(|_| DocumentStoreError::LockPoisoned)?;
            let writer = self.ensure_writer(&mut writer_guard)?;

            let term = Term::from_field_text(self.schema.collection_name, name);
            writer.delete_term(term);
            writer.commit()?;
        }

        self.reader.reload()?;

        // Remove file states for this collection
        // Note: This is a simplification - in a full implementation we'd track collection per file
        self.file_states.retain(|_, state| {
            !state.chunk_ids.iter().any(|_id| {
                // TODO: Check if chunk belongs to this collection
                true
            })
        });

        // Remove collection ID
        self.collection_ids.remove(name);

        self.save_state()?;

        Ok(count)
    }

    /// Get statistics about a collection.
    pub fn collection_stats(&self, name: &str) -> StoreResult<CollectionStats> {
        let searcher = self.reader.searcher();

        let term = Term::from_field_text(self.schema.collection_name, name);
        let query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

        let count = searcher.search(&query, &tantivy::collector::Count)?;

        // Count unique files
        let file_count = self
            .file_states
            .values()
            .filter(|s| !s.chunk_ids.is_empty())
            .count();

        Ok(CollectionStats {
            name: name.to_string(),
            chunk_count: count,
            file_count,
        })
    }

    /// List all collections.
    pub fn list_collections(&self) -> Vec<String> {
        self.collection_ids.keys().cloned().collect()
    }

    // Private helper methods

    fn allocate_chunk_id(&mut self) -> ChunkId {
        let id = self.next_chunk_id;
        self.next_chunk_id += 1;
        ChunkId::from_u32(id as u32).unwrap_or_else(|| {
            // Wrap around if we hit zero
            self.next_chunk_id = 2;
            ChunkId::from_u32(1).expect("1 is not zero")
        })
    }

    fn get_or_create_collection_id(&mut self, name: &str) -> CollectionId {
        if let Some(&id) = self.collection_ids.get(name) {
            return id;
        }

        let id = CollectionId::from_u32((self.collection_ids.len() + 1) as u32)
            .expect("collection ID should be valid (non-zero)");
        self.collection_ids.insert(name.to_string(), id);
        id
    }

    fn collect_files(&self, config: &CollectionConfig) -> StoreResult<Vec<PathBuf>> {
        let mut files = Vec::new();
        let patterns = config.effective_patterns();

        for base_path in &config.paths {
            if !base_path.exists() {
                continue;
            }

            if base_path.is_file() {
                files.push(base_path.clone());
                continue;
            }

            // Use glob patterns
            for pattern in &patterns {
                let full_pattern = base_path.join(pattern);
                let pattern_str = full_pattern.to_string_lossy();

                for path in glob::glob(&pattern_str)
                    .map_err(|e| DocumentStoreError::Index(format!("Invalid glob pattern: {e}")))?
                    .flatten()
                {
                    if path.is_file() {
                        files.push(path);
                    }
                }
            }
        }

        Ok(files)
    }

    fn detect_changes(
        &self,
        files: &[PathBuf],
        _collection: &str,
    ) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
        let mut changed = Vec::new();
        let mut unchanged = Vec::new();
        let mut removed: Vec<PathBuf> = Vec::new();

        let current_files: std::collections::HashSet<_> = files.iter().collect();

        // Find changed and unchanged files
        for path in files {
            if let Some(state) = self.file_states.get(path) {
                // Check if file content changed
                if let Ok(content) = std::fs::read_to_string(path) {
                    let current_hash = calculate_hash(&content);
                    if current_hash == state.content_hash {
                        unchanged.push(path.clone());
                    } else {
                        changed.push(path.clone());
                    }
                } else {
                    // File unreadable, treat as removed
                    removed.push(path.clone());
                }
            } else {
                // New file
                changed.push(path.clone());
            }
        }

        // Find removed files
        for path in self.file_states.keys() {
            if !current_files.contains(path) {
                removed.push(path.clone());
            }
        }

        (changed, unchanged, removed)
    }

    fn store_chunk(
        &mut self,
        chunk_id: ChunkId,
        collection: &str,
        source_path: &Path,
        raw_chunk: &RawChunk,
        _full_content: &str,
    ) -> StoreResult<()> {
        let mut writer_guard = self
            .writer
            .lock()
            .map_err(|_| DocumentStoreError::LockPoisoned)?;
        let writer = self.ensure_writer(&mut writer_guard)?;

        let mut doc = Document::new();

        // Document type discriminator
        doc.add_text(self.schema.doc_type, "chunk");

        // Chunk ID
        doc.add_u64(self.schema.chunk_id, chunk_id.get() as u64);

        // Collection name
        doc.add_text(self.schema.collection_name, collection);

        // Source path
        doc.add_text(
            self.schema.source_path,
            source_path.to_string_lossy().as_ref(),
        );

        // Heading context as JSON array
        let heading_json =
            serde_json::to_string(&raw_chunk.heading_context).unwrap_or_else(|_| "[]".to_string());
        doc.add_text(self.schema.heading_context, &heading_json);

        // Full content
        doc.add_text(self.schema.content, &raw_chunk.content);

        // Content preview (first ~200 chars)
        let preview: String = raw_chunk.content.chars().take(200).collect();
        doc.add_text(self.schema.content_preview, &preview);

        // Byte offsets
        doc.add_u64(self.schema.byte_start, raw_chunk.byte_range.0 as u64);
        doc.add_u64(self.schema.byte_end, raw_chunk.byte_range.1 as u64);

        // Character count
        doc.add_u64(self.schema.char_count, raw_chunk.char_count() as u64);

        // Indexed timestamp
        doc.add_u64(self.schema.indexed_at, get_utc_timestamp());

        writer.add_document(doc)?;

        Ok(())
    }

    fn delete_chunks_by_file(&mut self, path: &Path, _collection: &str) -> StoreResult<()> {
        let mut writer_guard = self
            .writer
            .lock()
            .map_err(|_| DocumentStoreError::LockPoisoned)?;
        let writer = self.ensure_writer(&mut writer_guard)?;

        let term = Term::from_field_text(self.schema.source_path, path.to_string_lossy().as_ref());
        writer.delete_term(term);

        Ok(())
    }

    fn ensure_writer<'a>(
        &self,
        writer_guard: &'a mut Option<IndexWriter<Document>>,
    ) -> StoreResult<&'a mut IndexWriter<Document>> {
        if writer_guard.is_none() {
            *writer_guard = Some(self.index.writer(self.heap_size)?);
        }
        Ok(writer_guard.as_mut().unwrap())
    }

    fn commit(&mut self) -> StoreResult<()> {
        let mut writer_guard = self
            .writer
            .lock()
            .map_err(|_| DocumentStoreError::LockPoisoned)?;

        if let Some(ref mut writer) = *writer_guard {
            writer.commit()?;
        }

        self.reader.reload()?;

        Ok(())
    }

    /// Process embeddings in batches with progress reporting.
    ///
    /// Batching reduces memory pressure and provides smoother progress updates.
    fn process_embeddings_batched<F>(
        &mut self,
        chunks: &[(ChunkId, String)],
        on_progress: &mut F,
    ) -> StoreResult<()>
    where
        F: FnMut(IndexProgress<'_>),
    {
        let Some(ref generator) = self.embedding_generator else {
            return Ok(());
        };

        let Some(ref mut vector_storage) = self.vector_storage else {
            return Ok(());
        };

        let total_chunks = chunks.len();
        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(total_chunks);
        let mut processed = 0;

        // Process in batches
        for batch in chunks.chunks(EMBEDDING_BATCH_SIZE) {
            // Extract texts for this batch
            let texts: Vec<&str> = batch.iter().map(|(_, text)| text.as_str()).collect();

            // Generate embeddings for batch
            let embeddings = generator
                .generate_embeddings(&texts)
                .map_err(|e| DocumentStoreError::Embedding(e.to_string()))?;

            // Store vectors immediately (releases memory pressure)
            let vector_pairs: Vec<(VectorId, &[f32])> = batch
                .iter()
                .zip(embeddings.iter())
                .filter_map(|((chunk_id, _), embedding)| {
                    VectorId::new(chunk_id.get()).map(|vid| (vid, embedding.as_slice()))
                })
                .collect();

            vector_storage.write_batch(&vector_pairs)?;

            // Keep embeddings for clustering
            all_embeddings.extend(embeddings);

            processed += batch.len();

            // Report progress
            on_progress(IndexProgress::GeneratingEmbeddings {
                current: processed,
                total: total_chunks,
            });
        }

        // Update clustering with all embeddings
        self.update_clustering(&all_embeddings, chunks)?;

        Ok(())
    }

    fn update_clustering(
        &mut self,
        embeddings: &[Vec<f32>],
        chunks: &[(ChunkId, String)],
    ) -> StoreResult<()> {
        if embeddings.is_empty() {
            return Ok(());
        }

        // For now, do full re-clustering (incremental clustering would be more efficient)
        let k = ((embeddings.len() as f32).sqrt().ceil() as usize).clamp(1, 100);

        let clustering_result = kmeans_clustering(embeddings, k)
            .map_err(|e| DocumentStoreError::Index(format!("Clustering failed: {e}")))?;

        self.centroids = clustering_result.centroids;

        // Update assignments
        for (i, (chunk_id, _)) in chunks.iter().enumerate() {
            if let Some(vid) = VectorId::new(chunk_id.get()) {
                self.cluster_assignments
                    .insert(vid, clustering_result.assignments[i]);
            }
        }

        // Save cluster data
        self.save_cluster_data()?;

        Ok(())
    }

    fn get_filtered_candidates(&self, query: &SearchQuery) -> StoreResult<Vec<ChunkId>> {
        let searcher = self.reader.searcher();

        // Build filter query
        let mut subqueries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        // Always filter for chunks (not metadata)
        let doc_type_term = Term::from_field_text(self.schema.doc_type, "chunk");
        subqueries.push((
            Occur::Must,
            Box::new(TermQuery::new(
                doc_type_term,
                tantivy::schema::IndexRecordOption::Basic,
            )),
        ));

        // Collection filter
        if let Some(ref collection) = query.collection {
            let term = Term::from_field_text(self.schema.collection_name, collection);
            subqueries.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    term,
                    tantivy::schema::IndexRecordOption::Basic,
                )),
            ));
        }

        // Document filter
        if let Some(ref doc_path) = query.document {
            let term =
                Term::from_field_text(self.schema.source_path, doc_path.to_string_lossy().as_ref());
            subqueries.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    term,
                    tantivy::schema::IndexRecordOption::Basic,
                )),
            ));
        }

        let filter_query = BooleanQuery::new(subqueries);

        // Execute query
        let top_docs = searcher.search(&filter_query, &TopDocs::with_limit(10_000))?;

        // Extract chunk IDs
        let mut chunk_ids = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: Document = searcher.doc(doc_address)?;
            if let Some(id_value) = doc.get_first(self.schema.chunk_id) {
                if let Some(id) = id_value.as_u64() {
                    if let Some(chunk_id) = ChunkId::from_u32(id as u32) {
                        chunk_ids.push(chunk_id);
                    }
                }
            }
        }

        Ok(chunk_ids)
    }

    fn score_by_similarity(
        &mut self,
        candidates: &[ChunkId],
        query_vec: &[f32],
    ) -> StoreResult<Vec<(ChunkId, f32)>> {
        let Some(ref mut vector_storage) = self.vector_storage else {
            // No vectors, return with zero scores
            return Ok(candidates.iter().map(|&id| (id, 0.0)).collect());
        };

        let mut scored = Vec::new();

        for &chunk_id in candidates {
            if let Some(vid) = VectorId::new(chunk_id.get()) {
                if let Some(chunk_vec) = vector_storage.read_vector(vid) {
                    let similarity = cosine_similarity(query_vec, &chunk_vec);
                    scored.push((chunk_id, similarity));
                }
            }
        }

        Ok(scored)
    }

    fn enrich_results(
        &self,
        candidates: Vec<ChunkId>,
        query: &SearchQuery,
    ) -> StoreResult<Vec<SearchResult>> {
        let chunk_ids: Vec<(ChunkId, f32)> = candidates
            .into_iter()
            .take(query.limit)
            .map(|id| (id, 0.0))
            .collect();

        self.build_search_results(chunk_ids, query)
    }

    fn build_search_results(
        &self,
        scored: Vec<(ChunkId, f32)>,
        query: &SearchQuery,
    ) -> StoreResult<Vec<SearchResult>> {
        let searcher = self.reader.searcher();
        let mut results = Vec::new();

        // Get preview config (use defaults if not provided)
        let default_config = super::config::SearchConfig::default();
        let preview_config = query.preview_config.as_ref().unwrap_or(&default_config);

        for (chunk_id, similarity) in scored {
            // Find document by chunk_id
            let term = Term::from_field_u64(self.schema.chunk_id, chunk_id.get() as u64);
            let tantivy_query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

            let top_docs = searcher.search(&tantivy_query, &TopDocs::with_limit(1))?;

            if let Some((_score, doc_address)) = top_docs.first() {
                let doc: Document = searcher.doc(*doc_address)?;

                let collection = doc
                    .get_first(self.schema.collection_name)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let source_path = doc
                    .get_first(self.schema.source_path)
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_default();

                let heading_json = doc
                    .get_first(self.schema.heading_context)
                    .and_then(|v| v.as_str())
                    .unwrap_or("[]");

                let heading_context: Vec<String> =
                    serde_json::from_str(heading_json).unwrap_or_default();

                // Get full content for KWIC extraction
                let full_content = doc
                    .get_first(self.schema.content)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Generate preview with KWIC and highlighting
                let content_preview = generate_preview(full_content, &query.text, preview_config);

                let byte_start = doc
                    .get_first(self.schema.byte_start)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;

                let byte_end = doc
                    .get_first(self.schema.byte_end)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;

                results.push(SearchResult {
                    chunk_id,
                    collection,
                    source_path,
                    heading_context,
                    content_preview,
                    byte_range: (byte_start, byte_end),
                    similarity,
                });
            }
        }

        Ok(results)
    }

    #[allow(clippy::type_complexity)]
    fn load_state(
        path: &Path,
    ) -> StoreResult<(
        HashMap<PathBuf, FileState>,
        HashMap<String, CollectionId>,
        u64,
    )> {
        let content = std::fs::read_to_string(path)?;
        let state: PersistedState = serde_json::from_str(&content)
            .map_err(|e| DocumentStoreError::Index(format!("Failed to parse state: {e}")))?;

        let file_states = state
            .file_states
            .into_iter()
            .map(|(k, v)| (PathBuf::from(k), v))
            .collect();

        let collection_ids = state
            .collection_ids
            .into_iter()
            .filter_map(|(name, id)| CollectionId::from_u32(id).map(|cid| (name, cid)))
            .collect();

        Ok((file_states, collection_ids, state.next_chunk_id))
    }

    fn save_state(&self) -> StoreResult<()> {
        let state = PersistedState {
            file_states: self
                .file_states
                .iter()
                .map(|(k, v)| (k.to_string_lossy().to_string(), v.clone()))
                .collect(),
            collection_ids: self
                .collection_ids
                .iter()
                .map(|(name, id)| (name.clone(), id.get()))
                .collect(),
            next_chunk_id: self.next_chunk_id,
        };

        let content = serde_json::to_string_pretty(&state)
            .map_err(|e| DocumentStoreError::Index(format!("Failed to serialize state: {e}")))?;

        let state_path = self.base_path.join("state.json");
        std::fs::write(state_path, content)?;

        Ok(())
    }

    fn load_cluster_data(&mut self) -> StoreResult<()> {
        let cluster_path = self.base_path.join("clusters.json");

        if !cluster_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(cluster_path)?;
        let data: ClusterData = serde_json::from_str(&content)
            .map_err(|e| DocumentStoreError::Index(format!("Failed to parse clusters: {e}")))?;

        self.centroids = data.centroids;
        self.cluster_assignments = data
            .assignments
            .into_iter()
            .filter_map(|(id, cluster)| {
                let vid = VectorId::new(id)?;
                let cid = ClusterId::new(cluster)?;
                Some((vid, cid))
            })
            .collect();

        Ok(())
    }

    fn save_cluster_data(&self) -> StoreResult<()> {
        let data = ClusterData {
            centroids: self.centroids.clone(),
            assignments: self
                .cluster_assignments
                .iter()
                .map(|(vid, cid)| (vid.get(), cid.get()))
                .collect(),
        };

        let content = serde_json::to_string_pretty(&data)
            .map_err(|e| DocumentStoreError::Index(format!("Failed to serialize clusters: {e}")))?;

        let cluster_path = self.base_path.join("clusters.json");
        std::fs::write(cluster_path, content)?;

        Ok(())
    }
}

/// Statistics about a collection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectionStats {
    /// Collection name.
    pub name: String,
    /// Number of chunks indexed.
    pub chunk_count: usize,
    /// Number of files indexed.
    pub file_count: usize,
}

/// Persisted state for the document store.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedState {
    file_states: HashMap<String, FileState>,
    collection_ids: HashMap<String, u32>,
    next_chunk_id: u64,
}

/// Persisted cluster data.
#[derive(serde::Serialize, serde::Deserialize)]
struct ClusterData {
    centroids: Vec<Vec<f32>>,
    assignments: HashMap<u32, u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_dimension() -> VectorDimension {
        VectorDimension::new(4).unwrap()
    }

    #[test]
    fn test_document_store_creation() {
        let temp_dir = TempDir::new().unwrap();
        let store = DocumentStore::new(temp_dir.path(), test_dimension());
        assert!(store.is_ok());
    }

    #[test]
    fn test_collection_id_allocation() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = DocumentStore::new(temp_dir.path(), test_dimension()).unwrap();

        let id1 = store.get_or_create_collection_id("test-collection");
        let id2 = store.get_or_create_collection_id("test-collection");
        let id3 = store.get_or_create_collection_id("another-collection");

        // Same name should return same ID
        assert_eq!(id1.get(), id2.get());

        // Different name should return different ID
        assert_ne!(id1.get(), id3.get());
    }

    #[test]
    fn test_chunk_id_allocation() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = DocumentStore::new(temp_dir.path(), test_dimension()).unwrap();

        let id1 = store.allocate_chunk_id();
        let id2 = store.allocate_chunk_id();
        let id3 = store.allocate_chunk_id();

        // IDs should be unique and sequential
        assert_ne!(id1.get(), id2.get());
        assert_ne!(id2.get(), id3.get());
        assert_eq!(id2.get(), id1.get() + 1);
        assert_eq!(id3.get(), id2.get() + 1);
    }

    #[test]
    fn test_state_persistence() {
        let temp_dir = TempDir::new().unwrap();

        // Create store and allocate some IDs
        {
            let mut store = DocumentStore::new(temp_dir.path(), test_dimension()).unwrap();
            store.get_or_create_collection_id("persist-test");
            let _id1 = store.allocate_chunk_id();
            let _id2 = store.allocate_chunk_id();
            store.save_state().unwrap();
        }

        // Reopen and verify state
        {
            let store = DocumentStore::new(temp_dir.path(), test_dimension()).unwrap();
            assert!(store.collection_ids.contains_key("persist-test"));
            assert!(store.next_chunk_id > 2);
        }
    }

    #[test]
    fn test_list_collections() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = DocumentStore::new(temp_dir.path(), test_dimension()).unwrap();

        store.get_or_create_collection_id("alpha");
        store.get_or_create_collection_id("beta");
        store.get_or_create_collection_id("gamma");

        let collections = store.list_collections();
        assert_eq!(collections.len(), 3);
        assert!(collections.contains(&"alpha".to_string()));
        assert!(collections.contains(&"beta".to_string()));
        assert!(collections.contains(&"gamma".to_string()));
    }
}
