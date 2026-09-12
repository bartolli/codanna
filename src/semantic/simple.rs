//! Simple semantic search implementation for documentation comments

use crate::SymbolId;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Error type for semantic search operations
#[derive(Debug, thiserror::Error)]
pub enum SemanticSearchError {
    #[error("Failed to initialize embedding model: {0}")]
    ModelInitError(String),

    #[error("Failed to generate embedding: {0}")]
    EmbeddingError(String),

    #[error("No embeddings available for search")]
    NoEmbeddings,

    #[error("Storage error: {message}\nSuggestion: {suggestion}")]
    StorageError { message: String, suggestion: String },

    #[error("Dimension mismatch: expected {expected}, got {actual}\nSuggestion: {suggestion}")]
    DimensionMismatch {
        expected: usize,
        actual: usize,
        suggestion: String,
    },

    #[error("Invalid ID: {id}\nSuggestion: {suggestion}")]
    InvalidId { id: u32, suggestion: String },

    #[error(
        "Embedding pool exhausted: no instance available after {waited:?} (pool size {pool_size})\nSuggestion: An embedder is wedged or leaked. Sample the process to capture stacks, then restart the run."
    )]
    PoolExhausted {
        pool_size: usize,
        waited: std::time::Duration,
    },
}

/// Advanced semantic search engine for documentation analysis
///
/// This implementation uses state-of-the-art embeddings to find
/// semantically similar documentation across the entire codebase,
/// enabling natural language queries for code discovery.
/// Updated: Final test - embedding cleanup working correctly!
pub struct SimpleSemanticSearch {
    /// Embeddings indexed by symbol ID
    embeddings: HashMap<SymbolId, Vec<f32>>,

    /// Language mapping for each symbol (for language-filtered search)
    symbol_languages: HashMap<SymbolId, String>,

    /// The embedding model for query-time embedding (None in remote mode — caller
    /// must use `search_with_embedding` and provide the query vector externally).
    model: Option<Mutex<TextEmbedding>>,

    /// Model dimensions for validation
    dimensions: usize,

    /// Metadata for tracking model info and timestamps
    metadata: Option<crate::semantic::SemanticMetadata>,
}

impl std::fmt::Debug for SimpleSemanticSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleSemanticSearch")
            .field("embeddings_count", &self.embeddings.len())
            .field("dimensions", &self.dimensions)
            .field("model", &"<TextEmbedding>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl SimpleSemanticSearch {
    /// Create a new semantic search instance using default model (AllMiniLML6V2).
    ///
    /// For multilingual support, use `from_model_name` with "MultilingualE5Small".
    pub fn new() -> Result<Self, SemanticSearchError> {
        Self::with_model(EmbeddingModel::AllMiniLML6V2)
    }

    /// Create a semantic search instance from a model name string.
    ///
    /// # Arguments
    /// * `model_name` - Name of the model (e.g., "MultilingualE5Small", "AllMiniLML6V2")
    ///
    /// # Supported Models
    /// - `AllMiniLML6V2` - English-only, 384 dimensions (default)
    /// - `MultilingualE5Small` - 94 languages, 384 dimensions (recommended for multilingual)
    /// - `MultilingualE5Base` - 94 languages, 768 dimensions
    /// - `MultilingualE5Large` - 94 languages, 1024 dimensions
    /// - And many more (see `parse_embedding_model` documentation)
    ///
    /// # Example
    /// ```ignore
    /// let search = SimpleSemanticSearch::from_model_name("MultilingualE5Small")?;
    /// ```
    pub fn from_model_name(model_name: &str) -> Result<Self, SemanticSearchError> {
        let model = crate::vector::parse_embedding_model(model_name)
            .map_err(|e| SemanticSearchError::ModelInitError(format!("Invalid model name: {e}")))?;
        Self::with_model(model)
    }

    /// Create with a specific model enum.
    pub fn with_model(model: EmbeddingModel) -> Result<Self, SemanticSearchError> {
        let cache_dir = crate::init::models_dir();
        let model_name = crate::vector::model_to_string(&model);

        // Check if models directory has any content (indicating cached models)
        let has_cached_models = cache_dir.exists()
            && cache_dir
                .read_dir()
                .is_ok_and(|mut entries| entries.any(|_| true));

        // Inform user what's happening
        if has_cached_models {
            eprintln!("Loading embedding model '{model_name}' from cache...");
        } else {
            eprintln!("Downloading embedding model '{model_name}' (first time only)...");
        }

        let mut text_model = TextEmbedding::try_new(
            InitOptions::new(model)
                .with_cache_dir(cache_dir)
                .with_show_download_progress(true),
        )
        .map_err(|e| {
            SemanticSearchError::ModelInitError(format!(
                "Failed to initialize model '{model_name}': {e}"
            ))
        })?;

        let test_embedding = text_model
            .embed(vec!["test"], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
        let dimensions = test_embedding.into_iter().next().unwrap().len();

        let metadata = crate::semantic::SemanticMetadata::new(model_name.clone(), dimensions, 0);

        Ok(Self {
            embeddings: HashMap::new(),
            symbol_languages: HashMap::new(),
            model: Some(Mutex::new(text_model)),
            dimensions,
            metadata: Some(metadata),
        })
    }

    pub fn index_doc_comment(
        &mut self,
        symbol_id: SymbolId,
        doc: &str,
    ) -> Result<(), SemanticSearchError> {
        if doc.trim().is_empty() {
            return Ok(());
        }

        let model = self.model.as_ref().ok_or_else(|| {
            SemanticSearchError::ModelInitError(
                "No local model — use EmbeddingBackend to generate embeddings in remote mode"
                    .to_string(),
            )
        })?;
        let embeddings = model
            .lock()
            .unwrap()
            .embed(vec![doc], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;

        let embedding = embeddings.into_iter().next().unwrap();

        if embedding.len() != self.dimensions {
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimensions,
                embedding.len()
            )));
        }

        self.embeddings.insert(symbol_id, embedding);
        Ok(())
    }

    pub fn index_doc_comment_with_language(
        &mut self,
        symbol_id: SymbolId,
        doc: &str,
        language: &str,
    ) -> Result<(), SemanticSearchError> {
        self.index_doc_comment(symbol_id, doc)?;

        if self.embeddings.contains_key(&symbol_id) {
            self.symbol_languages
                .insert(symbol_id, language.to_string());
        }

        Ok(())
    }

    /// Store pre-generated embeddings produced by an `EmbeddingBackend`.
    pub fn store_embeddings(&mut self, items: Vec<(SymbolId, Vec<f32>, String)>) -> usize {
        let mut count = 0;
        let mut dropped = 0usize;
        for (symbol_id, embedding, language) in items {
            if embedding.len() == self.dimensions {
                self.embeddings.insert(symbol_id, embedding);
                self.symbol_languages.insert(symbol_id, language);
                count += 1;
            } else {
                dropped += 1;
            }
        }
        if dropped > 0 {
            tracing::warn!(
                target: "semantic",
                "store_embeddings dropped {dropped} embeddings due to dimension mismatch \
                 (index={}, received=?). Re-index with --force to fix.",
                self.dimensions
            );
        }
        count
    }

    pub fn search_with_embedding(
        &self,
        query_embedding: &[f32],
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        if self.embeddings.is_empty() {
            return Err(SemanticSearchError::NoEmbeddings);
        }
        if query_embedding.len() != self.dimensions {
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Query embedding dimension {} does not match index dimension {}",
                query_embedding.len(),
                self.dimensions
            )));
        }
        let mut similarities: Vec<(SymbolId, f32)> = self
            .embeddings
            .iter()
            .filter_map(|(id, emb)| {
                let sim = cosine_similarity(query_embedding, emb);
                if sim >= threshold {
                    Some((*id, sim))
                } else {
                    None
                }
            })
            .collect();
        similarities.sort_by(|a, b| b.1.total_cmp(&a.1));
        similarities.truncate(limit);
        Ok(similarities)
    }

    pub fn search_with_embedding_threshold(
        &self,
        query_embedding: &[f32],
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        self.search_with_embedding(query_embedding, limit, threshold)
    }

    pub fn search_with_embedding_and_language(
        &self,
        query_embedding: &[f32],
        limit: usize,
        language: Option<&str>,
    ) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        if self.embeddings.is_empty() {
            return Err(SemanticSearchError::NoEmbeddings);
        }
        if query_embedding.len() != self.dimensions {
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Query embedding dimension {} does not match index dimension {}",
                query_embedding.len(),
                self.dimensions
            )));
        }
        let candidates: Vec<(&SymbolId, &Vec<f32>)> = if let Some(lang) = language {
            self.embeddings
                .iter()
                .filter(|(id, _)| self.symbol_languages.get(id).is_some_and(|l| l == lang))
                .collect()
        } else {
            self.embeddings.iter().collect()
        };
        let mut similarities: Vec<(SymbolId, f32)> = candidates
            .into_iter()
            .map(|(id, emb)| (*id, cosine_similarity(query_embedding, emb)))
            .collect();
        similarities.sort_by(|a, b| b.1.total_cmp(&a.1));
        similarities.truncate(limit);
        Ok(similarities)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        if self.embeddings.is_empty() {
            return Err(SemanticSearchError::NoEmbeddings);
        }

        let model = self.model.as_ref().ok_or_else(|| {
            SemanticSearchError::ModelInitError(
                "No local model available — use search_with_embedding() in remote mode".to_string(),
            )
        })?;

        let query_embeddings = model
            .lock()
            .unwrap()
            .embed(vec![query], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
        let query_embedding = query_embeddings.into_iter().next().unwrap();

        let mut similarities: Vec<(SymbolId, f32)> = self
            .embeddings
            .iter()
            .map(|(id, embedding)| {
                let similarity = cosine_similarity(&query_embedding, embedding);
                (*id, similarity)
            })
            .collect();

        similarities.sort_by(|a, b| b.1.total_cmp(&a.1));
        similarities.truncate(limit);
        Ok(similarities)
    }

    pub fn search_with_language(
        &self,
        query: &str,
        limit: usize,
        language: Option<&str>,
    ) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        if self.embeddings.is_empty() {
            return Err(SemanticSearchError::NoEmbeddings);
        }

        let model = self.model.as_ref().ok_or_else(|| {
            SemanticSearchError::ModelInitError(
                "No local model available — use search_with_embedding() in remote mode".to_string(),
            )
        })?;

        let query_embeddings = model
            .lock()
            .unwrap()
            .embed(vec![query], None)
            .map_err(|e| SemanticSearchError::EmbeddingError(e.to_string()))?;
        let query_embedding = query_embeddings.into_iter().next().unwrap();

        let filtered_embeddings: Vec<(&SymbolId, &Vec<f32>)> = if let Some(lang) = language {
            self.embeddings
                .iter()
                .filter(|(id, _)| {
                    self.symbol_languages
                        .get(id)
                        .is_some_and(|symbol_lang| symbol_lang == lang)
                })
                .collect()
        } else {
            self.embeddings.iter().collect()
        };

        let mut similarities: Vec<(SymbolId, f32)> = filtered_embeddings
            .into_iter()
            .map(|(id, embedding)| {
                let similarity = cosine_similarity(&query_embedding, embedding);
                (*id, similarity)
            })
            .collect();

        similarities.sort_by(|a, b| b.1.total_cmp(&a.1));
        similarities.truncate(limit);
        Ok(similarities)
    }

    pub fn search_with_threshold(
        &self,
        query: &str,
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<(SymbolId, f32)>, SemanticSearchError> {
        let results = self.search(query, limit)?;
        Ok(results
            .into_iter()
            .filter(|(_, score)| *score >= threshold)
            .collect())
    }

    pub fn has_local_model(&self) -> bool {
        self.model.is_some()
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    pub fn is_remote_index(&self) -> bool {
        self.metadata.as_ref().is_some_and(|m| m.is_remote())
    }

    pub fn embedding_count(&self) -> usize {
        self.embeddings.len()
    }

    pub fn clear(&mut self) {
        self.embeddings.clear();
        self.symbol_languages.clear();
    }

    pub fn remove_embeddings(&mut self, symbol_ids: &[SymbolId]) {
        for id in symbol_ids {
            self.embeddings.remove(id);
            self.symbol_languages.remove(id);
        }
    }

    pub fn metadata(&self) -> Option<&crate::semantic::SemanticMetadata> {
        self.metadata.as_ref()
    }

    pub fn save(&self, path: &Path) -> Result<(), SemanticSearchError> {
        use crate::semantic::{SemanticMetadata, SemanticVectorStorage};
        use crate::vector::VectorDimension;

        std::fs::create_dir_all(path).map_err(|e| SemanticSearchError::StorageError {
            message: format!("Failed to create semantic directory: {e}"),
            suggestion: "Check directory permissions".to_string(),
        })?;

        let (model_name, is_remote_backend) = if let Some(ref meta) = self.metadata {
            (meta.model_name.clone(), meta.is_remote())
        } else {
            ("AllMiniLML6V2".to_string(), self.model.is_none())
        };

        let metadata = if is_remote_backend {
            SemanticMetadata::new_remote(model_name, self.dimensions, self.embeddings.len())
        } else {
            SemanticMetadata::new(model_name, self.dimensions, self.embeddings.len())
        };

        let dimension = VectorDimension::new(self.dimensions).map_err(|e| {
            SemanticSearchError::StorageError {
                message: format!("Invalid dimension: {e}"),
                suggestion: "Dimension must be between 1 and 4096".to_string(),
            }
        })?;

        clear_stale_staging(path);
        static STAGING_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let staging_dir = path.join(format!(
            ".staging-{}-{}",
            std::process::id(),
            STAGING_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&staging_dir).map_err(|e| SemanticSearchError::StorageError {
            message: format!("Failed to create staging dir: {e}"),
            suggestion: "Check directory permissions".to_string(),
        })?;

        let mut storage = SemanticVectorStorage::new(&staging_dir, dimension)?;
        let embeddings: Vec<(SymbolId, Vec<f32>)> = self
            .embeddings
            .iter()
            .map(|(id, embedding)| (*id, embedding.clone()))
            .collect();
        storage.save_batch(&embeddings)?;
        drop(storage);

        let staged_vec = staging_dir.join("segment_0.vec");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&staged_vec)
            .and_then(|f| f.sync_all())
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Failed to sync staged vector file: {e}"),
                suggestion: "Check disk space and staged-file write access".to_string(),
            })?;
        std::fs::rename(&staged_vec, path.join("segment_0.vec")).map_err(|e| {
            SemanticSearchError::StorageError {
                message: format!("Failed to swap vector file into place: {e}"),
                suggestion: "Check directory permissions".to_string(),
            }
        })?;

        metadata.save_staged(&staging_dir, path)?;

        let languages_path = path.join("languages.json");
        let languages_map: HashMap<u32, String> = self
            .symbol_languages
            .iter()
            .map(|(id, lang)| (id.to_u32(), lang.clone()))
            .collect();
        let languages_json = serde_json::to_string(&languages_map).map_err(|e| {
            SemanticSearchError::StorageError {
                message: format!("Failed to serialize language mappings: {e}"),
                suggestion: "This is likely a bug in the code".to_string(),
            }
        })?;
        let languages_tmp = staging_dir.join("languages.json.tmp");
        std::fs::write(&languages_tmp, languages_json).map_err(|e| {
            SemanticSearchError::StorageError {
                message: format!("Failed to write language mappings: {e}"),
                suggestion: "Check disk space and file permissions".to_string(),
            }
        })?;
        std::fs::rename(&languages_tmp, &languages_path).map_err(|e| {
            SemanticSearchError::StorageError {
                message: format!("Failed to swap language mappings into place: {e}"),
                suggestion: "Check directory permissions".to_string(),
            }
        })?;

        let _ = std::fs::remove_dir_all(&staging_dir);
        Ok(())
    }

    pub fn new_empty(dimensions: usize, model_name: &str) -> Self {
        let metadata =
            crate::semantic::SemanticMetadata::new_remote(model_name.to_string(), dimensions, 0);
        Self {
            embeddings: HashMap::new(),
            symbol_languages: HashMap::new(),
            model: None,
            dimensions,
            metadata: Some(metadata),
        }
    }

    fn load_symbol_languages(
        path: &Path,
    ) -> Result<HashMap<SymbolId, String>, SemanticSearchError> {
        let languages_path = path.join("languages.json");
        if !languages_path.exists() {
            return Ok(HashMap::new());
        }
        let languages_json = std::fs::read_to_string(&languages_path).map_err(|e| {
            SemanticSearchError::StorageError {
                message: format!("Failed to read language mappings: {e}"),
                suggestion: "Language mappings file may be corrupted".to_string(),
            }
        })?;
        let languages_map: HashMap<u32, String> =
            serde_json::from_str(&languages_json).map_err(|e| {
                SemanticSearchError::StorageError {
                    message: format!("Failed to parse language mappings: {e}"),
                    suggestion: "Try rebuilding the semantic index".to_string(),
                }
            })?;
        Ok(languages_map
            .into_iter()
            .filter_map(|(id, lang)| SymbolId::new(id).map(|sid| (sid, lang)))
            .collect())
    }

    pub fn load_remote(path: &Path) -> Result<Self, SemanticSearchError> {
        use crate::semantic::{SemanticMetadata, SemanticVectorStorage};

        let metadata = SemanticMetadata::load(path)?;
        let mut storage = SemanticVectorStorage::open(path)?;

        if storage.dimension().get() != metadata.dimension {
            return Err(SemanticSearchError::DimensionMismatch {
                expected: metadata.dimension,
                actual: storage.dimension().get(),
                suggestion: format!(
                    "Remote index was built with {}-dimensional embeddings but storage has {}. Re-index with: codanna index <path> --force",
                    metadata.dimension,
                    storage.dimension().get()
                ),
            });
        }

        let embeddings_vec = storage.load_all()?;
        let mut embeddings = HashMap::with_capacity(embeddings_vec.len());
        for (id, embedding) in embeddings_vec {
            embeddings.insert(id, embedding);
        }

        let symbol_languages = Self::load_symbol_languages(path)?;

        Ok(Self {
            embeddings,
            symbol_languages,
            model: None,
            dimensions: metadata.dimension,
            metadata: Some(metadata),
        })
    }

    pub fn load(path: &Path) -> Result<Self, SemanticSearchError> {
        use crate::semantic::{SemanticMetadata, SemanticVectorStorage};

        let metadata = SemanticMetadata::load(path)?;
        if metadata.is_remote() {
            return Self::load_remote(path);
        }

        let model = crate::vector::parse_embedding_model(&metadata.model_name)
            .map_err(|e| SemanticSearchError::StorageError {
                message: format!("Invalid model in metadata: {e}"),
                suggestion: format!(
                    "The index was created with model '{}' which is not supported. Consider re-indexing with a supported model.",
                    metadata.model_name
                ),
            })?;

        let mut storage = SemanticVectorStorage::open(path)?;
        if storage.dimension().get() != metadata.dimension {
            return Err(SemanticSearchError::DimensionMismatch {
                expected: metadata.dimension,
                actual: storage.dimension().get(),
                suggestion: format!(
                    "Index was created with a {}-dimension model. Re-index with: codanna index <path> --force",
                    storage.dimension().get()
                ),
            });
        }

        let embeddings_vec = storage.load_all()?;
        if embeddings_vec.len() != metadata.embedding_count {
            eprintln!(
                "WARNING: Expected {} embeddings but found {}",
                metadata.embedding_count,
                embeddings_vec.len()
            );
        }

        let mut embeddings = HashMap::with_capacity(embeddings_vec.len());
        for (id, embedding) in embeddings_vec {
            embeddings.insert(id, embedding);
        }

        let text_model = TextEmbedding::try_new(
            InitOptions::new(model)
                .with_cache_dir(crate::init::models_dir())
                .with_show_download_progress(false),
        )
        .map_err(|e| {
            SemanticSearchError::ModelInitError(format!(
                "Failed to load model '{}': {}",
                metadata.model_name, e
            ))
        })?;

        let symbol_languages = Self::load_symbol_languages(path)?;

        Ok(Self {
            embeddings,
            symbol_languages,
            model: Some(Mutex::new(text_model)),
            dimensions: metadata.dimension,
            metadata: Some(metadata),
        })
    }
}

/// Calculate cosine similarity between two vectors.
///
/// Semantic ranking must never emit a non-orderable score. Malformed direct
/// callers or legacy in-memory vectors containing NaN/±infinity therefore
/// degrade to zero similarity instead of propagating NaN into sorting.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if !dot_product.is_finite()
        || !magnitude_a.is_finite()
        || !magnitude_b.is_finite()
        || magnitude_a == 0.0
        || magnitude_b == 0.0
    {
        return 0.0;
    }

    let score = dot_product / (magnitude_a * magnitude_b);
    if score.is_finite() { score } else { 0.0 }
}

fn clear_stale_staging(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let dead = match name.strip_prefix(".staging") {
            Some("") => true,
            Some(suffix) => {
                let pid = suffix
                    .strip_prefix('-')
                    .and_then(|rest| rest.split('-').next())
                    .and_then(|p| p.parse::<u32>().ok());
                match pid {
                    Some(pid) => !crate::io::process::pid_is_alive(pid),
                    None => true,
                }
            }
            None => false,
        };
        if dead {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_saves_do_not_destroy_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let mut handles = Vec::new();
        for seed in 1..=2u32 {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let mut search = SimpleSemanticSearch::new_empty(8, "test-remote");
                let items: Vec<(SymbolId, Vec<f32>, String)> = (1..=16u32)
                    .map(|i| {
                        let id = SymbolId::new(seed * 100 + i).unwrap();
                        let v: Vec<f32> = (0..8).map(|d| (seed * 100 + i + d) as f32).collect();
                        (id, v, "rust".to_string())
                    })
                    .collect();
                search.store_embeddings(items);

                barrier.wait();
                let mut errors = Vec::new();
                for _ in 0..50 {
                    if let Err(e) = search.save(&path) {
                        errors.push(e.to_string());
                    }
                }
                errors
            }));
        }

        let mut all_errors = Vec::new();
        for h in handles {
            all_errors.extend(h.join().expect("saver thread panicked"));
        }
        assert!(
            all_errors.is_empty(),
            "concurrent saves must not destroy each other's staging:\n{all_errors:#?}"
        );

        let loaded = SimpleSemanticSearch::load(&path).unwrap();
        assert_eq!(loaded.embedding_count(), 16, "the promoted generation is complete");
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored for semantic tests"]
    fn test_save_survives_stale_staging_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let mut search = SimpleSemanticSearch::new().unwrap();
        search.index_doc_comment(SymbolId::new(1).unwrap(), "Parse JSON data").unwrap();
        search.save(dir.path()).unwrap();
        let staging = dir.path().join(".staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("segment_0.vec"), b"garbage from a dead save").unwrap();
        let loaded = SimpleSemanticSearch::load(dir.path()).unwrap();
        assert_eq!(loaded.embedding_count(), 1);
        search.index_doc_comment(SymbolId::new(2).unwrap(), "Connect to database").unwrap();
        search.save(dir.path()).unwrap();
        assert!(!staging.exists());
        let reloaded = SimpleSemanticSearch::load(dir.path()).unwrap();
        assert_eq!(reloaded.embedding_count(), 2);
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored for semantic tests"]
    fn test_remove_embeddings() {
        let mut search = SimpleSemanticSearch::new().unwrap();
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        search.index_doc_comment(id1, "Parse JSON data from file").unwrap();
        search.index_doc_comment(id2, "Connect to database server").unwrap();
        search.index_doc_comment(id3, "Calculate hash of string").unwrap();
        assert_eq!(search.embedding_count(), 3);
        search.remove_embeddings(&[id1, id3]);
        assert_eq!(search.embedding_count(), 1);
        let results = search.search("database connection", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, id2);
        let json_results = search.search_with_threshold("parse JSON", 10, 0.6).unwrap();
        assert!(json_results.is_empty(), "Should not find removed JSON parsing doc");
        let hash_results = search.search_with_threshold("calculate hash", 10, 0.6).unwrap();
        assert!(hash_results.is_empty(), "Should not find removed hash doc");
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored for semantic tests"]
    fn test_save_and_load() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let mut search = match SimpleSemanticSearch::new() {
            Ok(s) => s,
            Err(_) => {
                eprintln!("Skipping test: FastEmbed model not available");
                return;
            }
        };
        search.index_doc_comment(SymbolId::new(1).unwrap(), "This function parses JSON data").unwrap();
        search.index_doc_comment(SymbolId::new(2).unwrap(), "Authenticates a user with credentials").unwrap();
        let original_count = search.embedding_count();
        search.save(temp_dir.path()).unwrap();
        let loaded = SimpleSemanticSearch::load(temp_dir.path()).unwrap();
        assert_eq!(loaded.embedding_count(), original_count);
        let results = loaded.search("parse JSON", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, SymbolId::new(1).unwrap());
    }

    #[test]
    fn test_load_missing_file() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let result = SimpleSemanticSearch::load(temp_dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            SemanticSearchError::StorageError { .. } => {}
            _ => panic!("Expected StorageError"),
        }
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored for semantic tests"]
    fn test_semantic_search_basic() {
        let mut search = match SimpleSemanticSearch::new() {
            Ok(s) => s,
            Err(_) => {
                eprintln!("Skipping test: FastEmbed model not available");
                return;
            }
        };
        let id1 = SymbolId::new(1).unwrap();
        let id2 = SymbolId::new(2).unwrap();
        let id3 = SymbolId::new(3).unwrap();
        search.index_doc_comment(id1, "Parse JSON data from a string").unwrap();
        search.index_doc_comment(id2, "Serialize data structure to JSON").unwrap();
        search.index_doc_comment(id3, "Calculate factorial of a number").unwrap();
        let results = search.search("parse JSON", 3).unwrap();
        assert!(results[0].1 > 0.7);
        assert!(results[1].1 > 0.5);
        assert!(results[2].1 < 0.3);
        assert_eq!(results[0].0, id1);
    }

    #[test]
    #[ignore = "Downloads 86MB model - run with --ignored for semantic tests"]
    fn test_similarity_threshold() {
        let mut search = match SimpleSemanticSearch::new() {
            Ok(s) => s,
            Err(_) => {
                eprintln!("Skipping test: FastEmbed model not available");
                return;
            }
        };
        search.index_doc_comment(SymbolId::new(1).unwrap(), "Authentication and authorization").unwrap();
        search.index_doc_comment(SymbolId::new(2).unwrap(), "User login and authentication").unwrap();
        search.index_doc_comment(SymbolId::new(3).unwrap(), "Matrix multiplication algorithm").unwrap();
        let results = search.search_with_threshold("user authentication", 10, 0.5).unwrap();
        assert_eq!(results.len(), 2);
        for (_, score) in &results {
            assert!(*score >= 0.5);
        }
    }

    #[test]
    fn test_cosine_similarity() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v1, &v2) - 1.0).abs() < 0.001);
        let v3 = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&v1, &v3) - 0.0).abs() < 0.001);
        let v4 = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v1, &v4) - (-1.0)).abs() < 0.001);
    }

    #[test]
    fn hardening_cosine_similarity_never_returns_nonfinite_score() {
        let finite = [1.0, 0.0];
        for malformed in [[f32::NAN, 1.0], [f32::INFINITY, 1.0], [f32::NEG_INFINITY, 1.0]] {
            let score = cosine_similarity(&finite, &malformed);
            assert!(score.is_finite());
            assert_eq!(score, 0.0);
        }
    }

    #[test]
    fn hardening_semantic_search_handles_nonfinite_direct_query_without_panic() {
        let mut search = SimpleSemanticSearch::new_empty(2, "test-remote");
        search.store_embeddings(vec![(
            SymbolId::new(1).unwrap(),
            vec![1.0, 0.0],
            "rust".to_string(),
        )]);

        let results = search
            .search_with_embedding(&[f32::NAN, 1.0], 10, -1.0)
            .expect("malformed direct query must degrade instead of panicking");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, 0.0);
    }
}
