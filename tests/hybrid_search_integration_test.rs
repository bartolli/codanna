//! Test 3: Hybrid Search Integration
//! 
//! Validates the integration of text and vector search using Reciprocal Rank Fusion (RRF).
//! This test ensures that hybrid search effectively combines traditional text matching
//! with semantic vector similarity to provide superior search results.
//!
//! Success criteria:
//! - Text-dominant queries rank exact matches highest
//! - Semantic queries find conceptually related code
//! - RRF scoring prevents single-source dominance
//! - Query latency remains under 20ms

use anyhow::Result;
use thiserror::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use std::num::NonZeroU32;
use tempfile::TempDir;

// Production imports
use codanna::Symbol;
use codanna::storage::{DocumentIndex, SearchResult};
use codanna::types::{SymbolKind, SymbolId, FileId, Range};
use codanna::Visibility;
// use codanna::indexing::SimpleIndexer; // Not needed since we're creating mock symbols directly

// ============================================================================
// Constants
// ============================================================================

const EMBEDDING_DIMENSION: usize = 384;
const DEFAULT_SEARCH_LIMIT: usize = 10;
const CONCURRENT_SEARCH_COUNT: usize = 10;
const LATENCY_P95_TARGET_MS: f32 = 20.0;
const LATENCY_P99_TARGET_MS: f32 = 50.0;
const RRF_DEFAULT_K: f32 = 60.0;
const MIN_JSON_RELATED_IN_TOP5: usize = 2;
const MOCK_EMBEDDING_BASE_VALUE: f32 = 0.1;

// ============================================================================
// Domain-Specific Newtypes
// ============================================================================

/// Type-safe wrapper for search scores (must be between 0.0 and 1.0)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Score(f32);

impl Score {
    /// Create a new score, returns error if not in valid range [0.0, 1.0]
    #[must_use]
    pub fn new(value: f32) -> Result<Self, HybridSearchError> {
        if value < 0.0 || value > 1.0 {
            return Err(HybridSearchError::InvalidScore { value });
        }
        Ok(Score(value))
    }
    
    /// Create a score without validation (for internal use only)
    #[must_use]
    fn new_unchecked(value: f32) -> Self {
        Score(value)
    }
    
    /// Get the inner value
    #[must_use]
    pub fn get(&self) -> f32 {
        self.0
    }
}

/// Type-safe wrapper for vector dimensions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorDimension(NonZeroU32);

impl VectorDimension {
    /// Create a new vector dimension
    #[must_use]
    pub fn new(value: u32) -> Result<Self, HybridSearchError> {
        NonZeroU32::new(value)
            .map(VectorDimension)
            .ok_or(HybridSearchError::InvalidDimension { value })
    }
    
    /// Get the inner value as usize
    #[must_use]
    pub fn get(&self) -> usize {
        self.0.get() as usize
    }
}

impl Default for VectorDimension {
    fn default() -> Self {
        VectorDimension(NonZeroU32::new(EMBEDDING_DIMENSION as u32).unwrap())
    }
}

// ============================================================================
// Mock Vector Search Components
// ============================================================================

/// Mock embedding generator for testing
struct MockEmbeddingGenerator {
    dimension: VectorDimension,
}

impl MockEmbeddingGenerator {
    #[must_use]
    fn new() -> Self {
        Self { dimension: VectorDimension::default() }
    }
    
    fn generate(&self, text: &str) -> Result<Vec<f32>, HybridSearchError> {
        // Create deterministic embeddings based on text content
        let mut embedding = vec![MOCK_EMBEDDING_BASE_VALUE; self.dimension.get()];
        
        // Add variation based on content
        if text.contains("parse") || text.contains("Parse") {
            embedding[0] = 0.9;
            embedding[1] = 0.8;
        }
        if text.contains("json") || text.contains("JSON") {
            embedding[2] = 0.85;
            embedding[3] = 0.75;
        }
        if text.contains("error") || text.contains("Error") {
            embedding[4] = 0.8;
            embedding[5] = 0.7;
        }
        if text.contains("async") {
            embedding[6] = 0.9;
            embedding[7] = 0.85;
        }
        
        Ok(embedding)
    }
}

/// Mock vector search result
#[derive(Debug, Clone)]
struct VectorSearchResult {
    pub symbol_id: SymbolId,
    pub score: Score,
}

/// Mock vector search engine
struct VectorSearchEngine {
    _document_index: Arc<DocumentIndex>,
    _mock_embedder: MockEmbeddingGenerator,
    _vector_path: PathBuf,
}

impl VectorSearchEngine {
    #[must_use]
    fn new(document_index: Arc<DocumentIndex>, vector_path: PathBuf) -> Self {
        Self {
            _document_index: document_index,
            _mock_embedder: MockEmbeddingGenerator::new(),
            _vector_path: vector_path,
        }
    }
    
    fn build_index(&self) -> Result<()> {
        // Mock implementation - nothing to build for testing
        Ok(())
    }
    
    fn search(&self, query: &str, limit: usize) -> Result<Vec<VectorSearchResult>> {
        // Generate mock vector search results based on query
        let mut results = Vec::new();
        
        // Simulate semantic search results
        if query.contains("parse") || query.contains("json") || query.contains("JSON") || query.contains("parsing") {
            results.push(VectorSearchResult {
                symbol_id: SymbolId(101), // parse_json
                score: Score::new_unchecked(0.95),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(102), // parse_json_object
                score: Score::new_unchecked(0.85),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(201), // parse_xml
                score: Score::new_unchecked(0.65),
            });
            // Add ParseError which has JSON error variant
            results.push(VectorSearchResult {
                symbol_id: SymbolId(302), // ParseError enum
                score: Score::new_unchecked(0.60),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(303), // JsonParser struct
                score: Score::new_unchecked(0.75),
            });
        }
        
        if query.contains("error") {
            results.push(VectorSearchResult {
                symbol_id: SymbolId(301), // handle_parse_error
                score: Score::new_unchecked(0.90),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(302), // ParseError
                score: Score::new_unchecked(0.80),
            });
        }
        
        if query.contains("async") {
            results.push(VectorSearchResult {
                symbol_id: SymbolId(401), // parse_async
                score: Score::new_unchecked(0.92),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(402), // fetch_and_parse
                score: Score::new_unchecked(0.88),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(501), // parse_with_retry
                score: Score::new_unchecked(0.85),
            });
            results.push(VectorSearchResult {
                symbol_id: SymbolId(502), // handle_retry_error
                score: Score::new_unchecked(0.78),
            });
        }
        
        // Special handling for mixed queries
        if query.contains("async") && query.contains("error") {
            results.push(VectorSearchResult {
                symbol_id: SymbolId(601), // handle_retry_error
                score: Score::new_unchecked(0.98),
            });
        }
        
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(limit);
        
        Ok(results)
    }
}

// ============================================================================
// Hybrid Search Error Types
// ============================================================================

#[derive(Error, Debug)]
pub enum HybridSearchError {
    #[error("No results from text search\nSuggestion: Check if the index is properly built and the query terms exist in the codebase")]
    NoTextResults {
        query: String,
        #[source]
        cause: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    
    #[error("No results from vector search\nSuggestion: Verify that vector embeddings have been generated for the indexed symbols")]
    NoVectorResults {
        query: String,
        #[source]
        cause: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    
    #[error("Invalid RRF constant: {value} (must be positive)\nSuggestion: Use a positive value, typically 60.0 for standard RRF")]
    InvalidRrfConstant { value: f32 },
    
    #[error("Invalid score value: {value} (must be between 0.0 and 1.0)\nSuggestion: Ensure score normalization is applied correctly")]
    InvalidScore { value: f32 },
    
    #[error("Invalid vector dimension: {value} (must be positive)\nSuggestion: Use a standard embedding dimension like 384 or 768")]
    InvalidDimension { value: u32 },
    
    #[error("Search timeout exceeded after {timeout_ms}ms\nSuggestion: Reduce query complexity or increase timeout limit")]
    SearchTimeout {
        query: String,
        timeout_ms: u64,
    },
    
    #[error("Indexing error: {message}\nSuggestion: Check file permissions and available disk space")]
    Indexing {
        message: String,
        #[source]
        cause: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    
    #[error("IO error")]
    Io(#[from] std::io::Error),
}

// ============================================================================
// Type-Safe RRF Implementation
// ============================================================================

/// Type-safe wrapper for RRF constant (must be positive)
#[derive(Debug, Clone, Copy)]
pub struct RrfConstant(f32);

impl RrfConstant {
    /// Create a new RRF constant, returns error if not positive
    pub fn new(value: f32) -> Result<Self, HybridSearchError> {
        if value <= 0.0 {
            return Err(HybridSearchError::InvalidRrfConstant { value });
        }
        Ok(RrfConstant(value))
    }
    
    /// Get the inner value
    pub fn get(&self) -> f32 {
        self.0
    }
}

impl Default for RrfConstant {
    fn default() -> Self {
        RrfConstant(RRF_DEFAULT_K)
    }
}

// ============================================================================
// Hybrid Search Result Types
// ============================================================================

#[derive(Debug, Clone)]
pub struct HybridSearchResult<'a> {
    pub symbol_id: SymbolId,
    pub file_path: &'a Path,
    pub symbol_name: &'a str,
    pub text_score: Option<Score>,
    pub vector_score: Option<Score>,
    pub rrf_score: Score,
}

/// Trait for scoring hybrid search results
pub trait HybridScorer: Send + Sync {
    fn score(&self, text_results: &[(SymbolId, Score)], 
             vector_results: &[(SymbolId, Score)]) -> Vec<(SymbolId, Score)>;
}

/// RRF implementation of hybrid scoring
pub struct RrfScorer {
    k: RrfConstant,
}

impl RrfScorer {
    #[must_use]
    pub fn new(k: RrfConstant) -> Self {
        RrfScorer { k }
    }
}

impl HybridScorer for RrfScorer {
    fn score(&self, text_results: &[(SymbolId, Score)], 
             vector_results: &[(SymbolId, Score)]) -> Vec<(SymbolId, Score)> {
        let mut rrf_scores: HashMap<SymbolId, f32> = HashMap::new();
        let k = self.k.get();
        
        // Add text result contributions
        for (rank, (symbol_id, _score)) in text_results.iter().enumerate() {
            let rrf_contribution = 1.0 / (k + rank as f32 + 1.0);
            *rrf_scores.entry(*symbol_id).or_insert(0.0) += rrf_contribution;
        }
        
        // Add vector result contributions
        for (rank, (symbol_id, _score)) in vector_results.iter().enumerate() {
            let rrf_contribution = 1.0 / (k + rank as f32 + 1.0);
            *rrf_scores.entry(*symbol_id).or_insert(0.0) += rrf_contribution;
        }
        
        // Sort by RRF score and convert to Score type
        let mut final_ranking: Vec<(SymbolId, f32)> = rrf_scores.into_iter().collect();
        final_ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Convert to Score type - RRF scores can be > 1.0, so normalize
        final_ranking.into_iter()
            .map(|(id, rrf)| (id, Score::new_unchecked(rrf.min(1.0))))
            .collect()
    }
}

// ============================================================================
// Test Fixture Creation
// ============================================================================

fn create_test_fixtures(temp_dir: &Path) -> Result<Vec<PathBuf>> {
    use std::fs;
    
    let mut files = Vec::new();
    
    // Create diverse Rust code patterns for testing
    
    // 1. Parser implementations (for "parse" queries)
    let parser_code = r#"use serde_json::Value;

pub fn parse_json(input: &str) -> Result<Value, ParseError> {
    serde_json::from_str(input).map_err(|e| ParseError::Json(e))
}

pub fn parse_json_object(tokens: &[Token]) -> Result<Object, ParseError> {
    // Parse JSON object from token stream
    let mut object = Object::new();
    // Implementation details...
    Ok(object)
}

pub fn parse_xml(input: &str) -> Result<XmlDocument, ParseError> {
    // Parse XML document into DOM tree
    XmlParser::new(input).parse()
}

impl Parser for JsonParser {
    fn parse(&self, input: &str) -> Result<Ast, ParseError> {
        // Main parsing implementation
        self.parse_value(input)
    }
}"#;
    
    let parser_path = temp_dir.join("parsers.rs");
    fs::write(&parser_path, parser_code)?;
    files.push(parser_path);
    
    // 2. Error handling patterns (for "error handling" queries)
    let error_code = r#"use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Invalid syntax at position {position}")]
    InvalidSyntax { position: usize },
    
    #[error("Unexpected token: {0}")]
    UnexpectedToken(String),
}

pub fn handle_parse_error(error: ParseError) -> Response {
    match error {
        ParseError::Json(e) => {
            log::error!("JSON error: {}", e);
            Response::bad_request("Invalid JSON")
        }
        ParseError::InvalidSyntax { position } => {
            Response::bad_request(&format!("Syntax error at {}", position))
        }
        _ => Response::internal_error("Parse failed")
    }
}

impl From<std::io::Error> for ParseError {
    fn from(error: std::io::Error) -> Self {
        ParseError::Io(error)
    }
}"#;
    
    let error_path = temp_dir.join("errors.rs");
    fs::write(&error_path, error_code)?;
    files.push(error_path);
    
    // 3. Async patterns (for "async function" queries)
    let async_code = r#"use tokio::io::AsyncReadExt;

pub async fn parse_async(reader: impl AsyncRead) -> Result<Document, Error> {
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).await?;
    parse_document(&buffer)
}

pub async fn fetch_and_parse(url: &str) -> Result<Data, Error> {
    let response = reqwest::get(url).await?;
    let text = response.text().await?;
    parse_json(&text)
}

async fn process_stream(stream: impl Stream<Item = String>) -> Vec<ParseResult> {
    stream
        .map(|line| parse_line(&line))
        .collect()
        .await
}"#;
    
    let async_path = temp_dir.join("async_parsing.rs");
    fs::write(&async_path, async_code)?;
    files.push(async_path);
    
    // 4. Mixed patterns for complex queries
    let mixed_code = r#"use futures::future::join_all;

pub struct AsyncParser {
    runtime: Runtime,
}

impl AsyncParser {
    pub async fn parse_with_retry(&self, input: &str) -> Result<Value, ParseError> {
        let mut attempts = 0;
        loop {
            match self.try_parse(input).await {
                Ok(value) => return Ok(value),
                Err(e) if attempts < 3 => {
                    attempts += 1;
                    self.handle_retry_error(e, attempts).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }
    
    async fn handle_retry_error(&self, error: ParseError, attempt: u32) -> Result<(), ParseError> {
        log::warn!("Parse attempt {} failed: {}", attempt, error);
        tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
        Ok(())
    }
}"#;
    
    let mixed_path = temp_dir.join("async_error_handling.rs");
    fs::write(&mixed_path, mixed_code)?;
    files.push(mixed_path);
    
    Ok(files)
}

// ============================================================================
// Test Environment and Configuration
// ============================================================================

/// Test environment containing all necessary components for hybrid search testing
struct TestEnvironment {
    document_index: Arc<DocumentIndex>,
    vector_engine: Arc<VectorSearchEngine>,
    scorer: Arc<RrfScorer>,
    _temp_dir: TempDir, // Keep temp dir alive
}

impl TestEnvironment {
    /// Execute a hybrid search and return ranked results
    fn hybrid_search(&self, query: &str, limit: usize) -> Result<Vec<(SymbolId, Score)>> {
        // Text search
        let text_results = self.document_index.search(query, limit, None, None)?;
        
        // Normalize text scores to [0, 1] range
        let max_score = text_results.iter()
            .map(|r| r.score)
            .fold(0.0f32, |a, b| a.max(b));
        
        let text_scored: Vec<(SymbolId, Score)> = if max_score > 0.0 {
            text_results.into_iter()
                .map(|r| Ok((r.symbol_id, Score::new(r.score / max_score)?)))
                .collect::<Result<Vec<_>, HybridSearchError>>()?
        } else {
            Vec::new()
        };
        
        // Vector search  
        let vector_results = self.vector_engine.search(query, limit)?;
        let vector_scored: Vec<(SymbolId, Score)> = vector_results.into_iter()
            .map(|r| (r.symbol_id, r.score))
            .collect();
        
        // Hybrid scoring
        Ok(self.scorer.score(&text_scored, &vector_scored))
    }
}

// ============================================================================
// Main Hybrid Search Test
// ============================================================================

#[test]
fn test_hybrid_text_vector_search() -> Result<()> {
    println!("\n=== Test 3: Hybrid Search Integration ===");
    
    // Setup test environment
    let temp_dir = TempDir::new()?;
    let temp_path = temp_dir.path();
    
    println!("Creating test fixtures with diverse code patterns...");
    let test_files = create_test_fixtures(temp_path)?;
    println!("  Created {} test files", test_files.len());
    
    // Initialize search engines
    println!("\nInitializing hybrid search components...");
    let index_dir = temp_path.join("index");
    std::fs::create_dir_all(&index_dir)?;
    
    let document_index = Arc::new(DocumentIndex::new(index_dir.clone()).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    })?);
    // Mock embedding generator is integrated into VectorSearchEngine
    
    let vector_engine = Arc::new(VectorSearchEngine::new(
        document_index.clone(),
        index_dir.join("vectors"),
    ));
    
    // Index test files
    println!("\nIndexing test fixtures...");
    let start = Instant::now();
    
    // Start batch for indexing
    document_index.start_batch()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    
    // Create and index mock symbols
    create_and_index_mock_symbols(&document_index, &test_files)?;
    
    // Commit the batch
    document_index.commit_batch()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    
    // Build vector index
    vector_engine.build_index()?;
    
    let indexing_duration = start.elapsed();
    println!("  Indexing completed in {:?}", indexing_duration);
    
    // Create test environment
    let test_env = TestEnvironment {
        document_index,
        vector_engine,
        scorer: Arc::new(RrfScorer::new(RrfConstant::default())),
        _temp_dir: temp_dir,
    };
    
    // Test scenarios
    println!("\n=== Running Hybrid Search Scenarios ===");
    
    // Scenario 1: Text-dominant query (exact function name)
    test_text_dominant_query(&test_env)?;
    
    // Scenario 2: Semantic query (conceptual search)
    test_semantic_query(&test_env)?;
    
    // Scenario 3: Mixed query (combining patterns)
    test_mixed_query(&test_env)?;
    
    // Scenario 4: Score distribution analysis
    test_score_distribution(&test_env)?;
    
    // Scenario 5: Performance under concurrent load
    test_concurrent_performance(&test_env)?;
    
    println!("\n✓ All hybrid search scenarios passed!");
    println!("\n=== Test 3: PASSED ===\n");
    Ok(())
}

// ============================================================================
// Test Scenario Implementations
// ============================================================================

fn test_text_dominant_query(env: &TestEnvironment) -> Result<()> {
    println!("\n--- Scenario 1: Text-Dominant Query ---");
    println!("Query: 'parse_json' (exact function name)");
    
    let start = Instant::now();
    
    // Text search - use correct signature
    let hybrid_results = env.hybrid_search("parse_json", DEFAULT_SEARCH_LIMIT)?;
    
    let duration = start.elapsed();
    
    // Verify exact match ranks first
    assert!(!hybrid_results.is_empty(), "Should have results");
    
    // Get symbol details for verification
    let top_symbol = env.document_index.find_symbol_by_id(hybrid_results[0].0)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?
        .expect("Top result should exist");
    
    assert_eq!(top_symbol.name.as_ref(), "parse_json", 
              "Exact match should rank first");
    
    println!("  ✓ Exact match ranked first");
    println!("  ✓ Query completed in {:?}", duration);
    
    Ok(())
}

fn test_semantic_query(env: &TestEnvironment) -> Result<()> {
    println!("\n--- Scenario 2: Semantic Query ---");
    println!("Query: 'JSON parsing' (conceptual search)");
    
    let start = Instant::now();
    
    // Execute searches
    let text_results = env.document_index.search("JSON parsing", DEFAULT_SEARCH_LIMIT, None, None)?;
    let vector_results = env.vector_engine.search("JSON parsing", DEFAULT_SEARCH_LIMIT)?;
    
    // Debug: print text and vector results
    println!("  Text results: {} items", text_results.len());
    for result in text_results.iter().take(3) {
        println!("    SymbolId({}) - score: {:.3}", result.symbol_id.0, result.score);
    }
    println!("  Vector results: {} items", vector_results.len());
    for result in vector_results.iter().take(3) {
        println!("    SymbolId({}) - score: {:.3}", result.symbol_id.0, result.score.get());
    }
    
    // Execute hybrid search
    let hybrid_results = env.hybrid_search("JSON parsing", DEFAULT_SEARCH_LIMIT)?;
    let duration = start.elapsed();
    
    println!("  Hybrid results: {} items", hybrid_results.len());
    
    // Verify semantic relevance
    let json_related_count = count_json_related_symbols(env, &hybrid_results)?;
    
    assert!(json_related_count >= MIN_JSON_RELATED_IN_TOP5, 
           "Top 5 results should include at least {} JSON-related symbols, found {}", 
           MIN_JSON_RELATED_IN_TOP5, json_related_count);
    
    println!("  ✓ Found {} JSON-related symbols in top 5", json_related_count);
    println!("  ✓ Query completed in {:?}", duration);
    
    Ok(())
}

fn test_mixed_query(env: &TestEnvironment) -> Result<()> {
    println!("\n--- Scenario 3: Mixed Query ---");
    println!("Query: 'async error handling' (pattern combination)");
    
    let start = Instant::now();
    
    // Execute hybrid search
    let hybrid_results = env.hybrid_search("async error handling", DEFAULT_SEARCH_LIMIT)?;
    let duration = start.elapsed();
    
    // Verify mixed pattern matching
    assert!(!hybrid_results.is_empty(), "Should have results for mixed query");
    
    // Check if we found the async error handling code
    let found_mixed_pattern = find_mixed_pattern_symbols(env, &hybrid_results)?;
    
    assert!(found_mixed_pattern, 
           "Should find async error handling patterns in top results");
    
    println!("  ✓ Found mixed pattern functions");
    println!("  ✓ Query completed in {:?}", duration);
    
    Ok(())
}

fn test_score_distribution(env: &TestEnvironment) -> Result<()> {
    println!("\n--- Scenario 4: Score Distribution Analysis ---");
    println!("Analyzing RRF score distribution for balanced ranking");
    
    // Execute a query with expected overlap
    let query = "parse";
    let text_results = env.document_index.search(query, 20, None, None)?;
    let vector_results = env.vector_engine.search(query, 20)?;
    
    // Analyze score distribution
    let distribution = analyze_score_distribution(&text_results, &vector_results);
    
    println!("  Results from text only: {}", distribution.text_only);
    println!("  Results from vector only: {}", distribution.vector_only);
    println!("  Results from both sources: {}", distribution.both_sources);
    
    // Verify RRF prevents single-source dominance
    assert!(distribution.both_sources > 0, "Should have some overlap between sources");
    assert!(distribution.text_only + distribution.vector_only > 0, 
           "Should have unique results from each source");
    
    println!("  ✓ RRF successfully balances multiple sources");
    
    Ok(())
}

fn test_concurrent_performance(env: &TestEnvironment) -> Result<()> {
    println!("\n--- Scenario 5: Concurrent Performance ---");
    println!("Testing hybrid search under concurrent load");
    
    use std::thread;
    use std::sync::Mutex;
    
    let queries = vec![
        "parse json",
        "error handling",
        "async function",
        "parse_xml",
        "handle error",
    ];
    
    let latencies = Arc::new(Mutex::new(Vec::new()));
    let mut handles = vec![];
    
    // Clone Arc references for thread safety
    let env_index = env.document_index.clone();
    let env_vector = env.vector_engine.clone();
    let env_scorer = env.scorer.clone();
    
    // Launch concurrent searches
    for i in 0..CONCURRENT_SEARCH_COUNT {
        let query = queries[i % queries.len()].to_string();
        let text_index = env_index.clone();
        let vector_engine = env_vector.clone();
        let scorer = env_scorer.clone();
        let latencies = latencies.clone();
        
        let handle = thread::spawn(move || -> Result<()> {
            let start = Instant::now();
            
            // Execute hybrid search
            let text_results = text_index.search(&query, DEFAULT_SEARCH_LIMIT, None, None)?;
            let vector_results = vector_engine.search(&query, DEFAULT_SEARCH_LIMIT)?;
            
            // Normalize text scores
            let max_score = text_results.iter()
                .map(|r| r.score)
                .fold(0.0f32, |a, b| a.max(b));
            
            let text_scored: Vec<(SymbolId, Score)> = if max_score > 0.0 {
                text_results.into_iter()
                    .map(|r| Ok((r.symbol_id, Score::new(r.score / max_score)?)))
                    .collect::<Result<Vec<_>, HybridSearchError>>()?
            } else {
                Vec::new()
            };
            
            let vector_scored: Vec<(SymbolId, Score)> = vector_results.into_iter()
                .map(|r| (r.symbol_id, r.score))
                .collect();
            
            let _hybrid_results = scorer.score(&text_scored, &vector_scored);
            
            let duration = start.elapsed();
            latencies.lock().unwrap().push(duration.as_millis() as f32);
            
            Ok(())
        });
        
        handles.push(handle);
    }
    
    // Wait for all searches to complete
    for handle in handles {
        handle.join().unwrap()?;
    }
    
    // Analyze latencies
    let latency_stats = analyze_latencies(&latencies.lock().unwrap());
    
    println!("  Latency percentiles:");
    println!("    p50: {:.1}ms", latency_stats.p50);
    println!("    p95: {:.1}ms", latency_stats.p95);
    println!("    p99: {:.1}ms", latency_stats.p99);
    
    // Verify performance targets
    assert!(latency_stats.p95 < LATENCY_P95_TARGET_MS, 
           "p95 latency should be under {}ms", LATENCY_P95_TARGET_MS);
    assert!(latency_stats.p99 < LATENCY_P99_TARGET_MS, 
           "p99 latency should be under {}ms", LATENCY_P99_TARGET_MS);
    
    println!("  ✓ Performance targets met under concurrent load");
    
    Ok(())
}

// Helper function to create and index mock symbols
fn create_and_index_mock_symbols(index: &Arc<DocumentIndex>, test_files: &[PathBuf]) -> Result<()> {
    
    // Define mock symbols that match our test queries
    let symbols = vec![
        // Parser functions
        Symbol {
            id: SymbolId(101),
            name: "parse_json".into(),
            kind: SymbolKind::Function,
            file_id: FileId(1),
            range: Range { start_line: 3, start_column: 1, end_line: 5, end_column: 1 },
            signature: Some("fn parse_json(input: &str) -> Result<Value, ParseError>".into()),
            doc_comment: Some("/// Parse JSON from string".into()),
            module_path: Some("parsers".into()),
            visibility: Visibility::Public,
        },
        Symbol {
            id: SymbolId(102),
            name: "parse_json_object".into(),
            kind: SymbolKind::Function,
            file_id: FileId(1),
            range: Range { start_line: 7, start_column: 1, end_line: 12, end_column: 1 },
            signature: Some("fn parse_json_object(tokens: &[Token]) -> Result<Object, ParseError>".into()),
            doc_comment: None,
            module_path: Some("parsers".into()),
            visibility: Visibility::Public,
        },
        Symbol {
            id: SymbolId(201),
            name: "parse_xml".into(),
            kind: SymbolKind::Function,
            file_id: FileId(1),
            range: Range { start_line: 14, start_column: 1, end_line: 17, end_column: 1 },
            signature: Some("fn parse_xml(input: &str) -> Result<XmlDocument, ParseError>".into()),
            doc_comment: None,
            module_path: Some("parsers".into()),
            visibility: Visibility::Public,
        },
        // Error handling
        Symbol {
            id: SymbolId(301),
            name: "handle_parse_error".into(),
            kind: SymbolKind::Function,
            file_id: FileId(2),
            range: Range { start_line: 12, start_column: 1, end_line: 24, end_column: 1 },
            signature: Some("fn handle_parse_error(error: ParseError) -> Response".into()),
            doc_comment: None,
            module_path: Some("errors".into()),
            visibility: Visibility::Public,
        },
        Symbol {
            id: SymbolId(302),
            name: "ParseError".into(),
            kind: SymbolKind::Enum,
            file_id: FileId(2),
            range: Range { start_line: 3, start_column: 1, end_line: 10, end_column: 1 },
            signature: Some("enum ParseError".into()),
            doc_comment: Some("/// Error types including JSON parsing errors".into()),
            module_path: Some("errors".into()),
            visibility: Visibility::Public,
        },
        Symbol {
            id: SymbolId(303),
            name: "JsonParser".into(),
            kind: SymbolKind::Struct,
            file_id: FileId(1),
            range: Range { start_line: 19, start_column: 1, end_line: 23, end_column: 1 },
            signature: Some("impl Parser for JsonParser".into()),
            doc_comment: None,
            module_path: Some("parsers".into()),
            visibility: Visibility::Public,
        },
        // Async functions
        Symbol {
            id: SymbolId(401),
            name: "parse_async".into(),
            kind: SymbolKind::Function,
            file_id: FileId(3),
            range: Range { start_line: 3, start_column: 1, end_line: 7, end_column: 1 },
            signature: Some("async fn parse_async(reader: impl AsyncRead) -> Result<Document, Error>".into()),
            doc_comment: None,
            module_path: Some("async_parsing".into()),
            visibility: Visibility::Public,
        },
        Symbol {
            id: SymbolId(402),
            name: "fetch_and_parse".into(),
            kind: SymbolKind::Function,
            file_id: FileId(3),
            range: Range { start_line: 9, start_column: 1, end_line: 13, end_column: 1 },
            signature: Some("async fn fetch_and_parse(url: &str) -> Result<Data, Error>".into()),
            doc_comment: None,
            module_path: Some("async_parsing".into()),
            visibility: Visibility::Public,
        },
        // Mixed patterns
        Symbol {
            id: SymbolId(501),
            name: "parse_with_retry".into(),
            kind: SymbolKind::Method,
            file_id: FileId(4),
            range: Range { start_line: 8, start_column: 1, end_line: 20, end_column: 1 },
            signature: Some("async fn parse_with_retry(&self, input: &str) -> Result<Value, ParseError>".into()),
            doc_comment: None,
            module_path: Some("async_error_handling".into()),
            visibility: Visibility::Public,
        },
        Symbol {
            id: SymbolId(502),
            name: "handle_retry_error".into(),
            kind: SymbolKind::Method,
            file_id: FileId(4),
            range: Range { start_line: 22, start_column: 1, end_line: 26, end_column: 1 },
            signature: Some("async fn handle_retry_error(&self, error: ParseError, attempt: u32) -> Result<(), ParseError>".into()),
            doc_comment: None,
            module_path: Some("async_error_handling".into()),
            visibility: Visibility::Private,
        },
        Symbol {
            id: SymbolId(601),
            name: "handle_retry_error".into(),
            kind: SymbolKind::Method,
            file_id: FileId(4),
            range: Range { start_line: 22, start_column: 1, end_line: 26, end_column: 1 },
            signature: Some("async fn handle_retry_error(&self, error: ParseError, attempt: u32) -> Result<(), ParseError>".into()),
            doc_comment: None,
            module_path: Some("async_error_handling".into()),
            visibility: Visibility::Private,
        },
    ];
    
    // Index each symbol
    for (i, symbol) in symbols.into_iter().enumerate() {
        // Use the appropriate file path based on the module
        let file_path = match symbol.module_path.as_deref() {
            Some("parsers") => &test_files[0],
            Some("errors") => &test_files[1],
            Some("async_parsing") => &test_files[2],
            Some("async_error_handling") => &test_files[3],
            _ => &test_files[0],
        };
        let file_path_str = file_path.to_string_lossy();
        index.index_symbol(&symbol, &file_path_str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, 
                format!("Failed to index symbol {}: {}", i, e)))?;
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rrf_constant_validation() {
        // Valid constant
        let valid = RrfConstant::new(60.0);
        assert!(valid.is_ok());
        assert_eq!(valid.unwrap().get(), 60.0);
        
        // Invalid constants
        assert!(RrfConstant::new(0.0).is_err());
        assert!(RrfConstant::new(-1.0).is_err());
        
        // Default
        assert_eq!(RrfConstant::default().get(), 60.0);
    }
    
    #[test]
    fn test_rrf_scoring_logic() {
        let scorer = RrfScorer::new(RrfConstant::default());
        
        // Test with simple results
        let text_results = vec![
            (SymbolId(1), Score::new_unchecked(0.9)),
            (SymbolId(2), Score::new_unchecked(0.8)),
            (SymbolId(3), Score::new_unchecked(0.7)),
        ];
        
        let vector_results = vec![
            (SymbolId(2), Score::new_unchecked(0.95)),
            (SymbolId(3), Score::new_unchecked(0.85)),
            (SymbolId(4), Score::new_unchecked(0.75)),
        ];
        
        let hybrid = scorer.score(&text_results, &vector_results);
        
        // Symbol 2 and 3 should rank higher due to appearing in both
        assert_eq!(hybrid.len(), 4); // Total unique symbols
        
        // Verify RRF scoring math
        let k = RRF_DEFAULT_K;
        let expected_score_2 = 1.0 / (k + 2.0) + 1.0 / (k + 1.0); // rank 2 in text, rank 1 in vector
        let expected_score_3 = 1.0 / (k + 3.0) + 1.0 / (k + 2.0); // rank 3 in text, rank 2 in vector
        
        // Find scores for symbols 2 and 3
        let score_2 = hybrid.iter().find(|(id, _)| id.0 == 2).unwrap().1.get();
        let score_3 = hybrid.iter().find(|(id, _)| id.0 == 3).unwrap().1.get();
        
        assert!((score_2 - expected_score_2).abs() < 0.0001);
        assert!((score_3 - expected_score_3).abs() < 0.0001);
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Count JSON-related symbols in the top 5 results
fn count_json_related_symbols(env: &TestEnvironment, results: &[(SymbolId, Score)]) -> Result<usize> {
    let mut count = 0;
    for (symbol_id, score) in results.iter().take(5) {
        if let Some(symbol) = env.document_index.find_symbol_by_id(*symbol_id)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))? {
            println!("    SymbolId({}) '{}' - RRF score: {:.3}", 
                    symbol_id.0, symbol.name.as_ref(), score.get());
            if is_json_related(symbol.name.as_ref()) {
                count += 1;
            }
        } else {
            println!("    SymbolId({}) - NOT FOUND in index", symbol_id.0);
        }
    }
    Ok(count)
}

/// Check if a symbol name is JSON-related
fn is_json_related(name: &str) -> bool {
    name.contains("json") || name.contains("Json") || name.contains("JSON")
}

/// Check if results contain async error handling patterns
fn find_mixed_pattern_symbols(env: &TestEnvironment, results: &[(SymbolId, Score)]) -> Result<bool> {
    for (symbol_id, _score) in results.iter().take(5) {
        if let Some(symbol) = env.document_index.find_symbol_by_id(*symbol_id)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))? {
            if symbol.name.as_ref().contains("retry") || symbol.name.as_ref().contains("handle_retry") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Score distribution analysis results
struct ScoreDistribution {
    text_only: usize,
    vector_only: usize,
    both_sources: usize,
}

/// Analyze the distribution of results between text and vector sources
fn analyze_score_distribution(text_results: &[SearchResult], 
                             vector_results: &[VectorSearchResult]) -> ScoreDistribution {
    let text_ids: std::collections::HashSet<_> = text_results.iter()
        .map(|r| r.symbol_id)
        .collect();
    let vector_ids: std::collections::HashSet<_> = vector_results.iter()
        .map(|r| r.symbol_id)
        .collect();
    
    let mut text_only = 0;
    let mut vector_only = 0;
    let mut both_sources = 0;
    
    for id in &text_ids {
        if vector_ids.contains(id) {
            both_sources += 1;
        } else {
            text_only += 1;
        }
    }
    
    for id in &vector_ids {
        if !text_ids.contains(id) {
            vector_only += 1;
        }
    }
    
    ScoreDistribution {
        text_only,
        vector_only,
        both_sources,
    }
}

/// Latency statistics
struct LatencyStats {
    p50: f32,
    p95: f32,
    p99: f32,
}

/// Calculate latency percentiles
fn analyze_latencies(latencies: &[f32]) -> LatencyStats {
    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    
    LatencyStats {
        p50: sorted[sorted.len() / 2],
        p95: sorted[(sorted.len() * 95) / 100],
        p99: sorted[(sorted.len() * 99) / 100],
    }
}