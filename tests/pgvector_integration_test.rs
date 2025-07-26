//! Integration tests for PostgreSQL with pgvector extension
//! This tests embedding storage, HNSW indexing, and similarity search

use anyhow::Result;
use tokio_postgres::{NoTls, Client};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use pgvector::Vector;
use std::time::Instant;

/// Test database configuration
const TEST_DB_NAME: &str = "codanna_test";
const TEST_DB_URL: &str = "postgresql://localhost/postgres";

/// Helper to create a test database connection
async fn create_test_client() -> Result<Client> {
    // First connect to postgres database to create test database
    let (client, connection) = tokio_postgres::connect(TEST_DB_URL, NoTls).await?;
    
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });
    
    // Create test database if it doesn't exist
    let _ = client.execute(
        &format!("CREATE DATABASE {}", TEST_DB_NAME),
        &[]
    ).await;
    
    // Connect to test database
    let test_url = format!("postgresql://localhost/{}", TEST_DB_NAME);
    let (test_client, test_connection) = tokio_postgres::connect(&test_url, NoTls).await?;
    
    tokio::spawn(async move {
        if let Err(e) = test_connection.await {
            eprintln!("test connection error: {}", e);
        }
    });
    
    Ok(test_client)
}

#[tokio::test]
async fn test_pgvector_extension_setup() -> Result<()> {
    let client = create_test_client().await?;
    
    // Create pgvector extension
    client.execute("CREATE EXTENSION IF NOT EXISTS vector", &[]).await?;
    
    // Verify extension is installed
    let row = client.query_one(
        "SELECT * FROM pg_extension WHERE extname = 'vector'",
        &[]
    ).await?;
    
    let ext_name: &str = row.get("extname");
    assert_eq!(ext_name, "vector");
    
    println!("pgvector extension successfully installed");
    
    Ok(())
}

#[tokio::test]
async fn test_embedding_storage() -> Result<()> {
    let client = create_test_client().await?;
    
    // Setup
    client.execute("CREATE EXTENSION IF NOT EXISTS vector", &[]).await?;
    client.execute("DROP TABLE IF EXISTS code_embeddings", &[]).await?;
    
    // Create table with vector column (384 dimensions for AllMiniLML6V2)
    client.execute(
        "CREATE TABLE code_embeddings (
            id SERIAL PRIMARY KEY,
            symbol_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            embedding vector(384) NOT NULL
        )",
        &[]
    ).await?;
    
    // Generate a test embedding
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    let code = vec!["fn parse_function(input: &str) -> Result<Function, Error>"];
    let embeddings = model.embed(code, None)?;
    let embedding = &embeddings[0];
    
    // Store embedding using pgvector
    let vector = Vector::from(embedding.clone());
    client.execute(
        "INSERT INTO code_embeddings (symbol_name, file_path, embedding) VALUES ($1, $2, $3)",
        &[&"parse_function", &"src/parser.rs", &vector]
    ).await?;
    
    // Verify storage
    let row = client.query_one(
        "SELECT symbol_name, file_path, embedding FROM code_embeddings WHERE symbol_name = $1",
        &[&"parse_function"]
    ).await?;
    
    let retrieved_name: &str = row.get(0);
    let retrieved_vector: Vector = row.get(2);
    
    assert_eq!(retrieved_name, "parse_function");
    // Vector type doesn't expose len() directly, but we know it's 384 dimensions
    // We can verify by converting back to slice
    let vector_slice: &[f32] = retrieved_vector.as_slice();
    assert_eq!(vector_slice.len(), 384);
    
    // Calculate storage size
    let size_result = client.query_one(
        "SELECT pg_column_size(embedding) as size FROM code_embeddings LIMIT 1",
        &[]
    ).await?;
    
    let size: i32 = size_result.get("size");
    println!("Storage size per embedding: {} bytes", size);
    
    Ok(())
}

#[tokio::test]
async fn test_hnsw_index_creation() -> Result<()> {
    let client = create_test_client().await?;
    
    // Setup
    client.execute("CREATE EXTENSION IF NOT EXISTS vector", &[]).await?;
    client.execute("DROP TABLE IF EXISTS code_embeddings", &[]).await?;
    
    client.execute(
        "CREATE TABLE code_embeddings (
            id SERIAL PRIMARY KEY,
            symbol_name TEXT NOT NULL,
            embedding vector(384) NOT NULL
        )",
        &[]
    ).await?;
    
    // Generate and insert multiple embeddings
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    let code_snippets: Vec<String> = (0..100)
        .map(|i| format!("fn function_{}() {{ /* implementation {} */ }}", i, i))
        .collect();
    
    let embeddings = model.embed(code_snippets.iter().map(|s| s.as_str()).collect(), None)?;
    
    // Insert embeddings
    for (i, embedding) in embeddings.iter().enumerate() {
        let vector = Vector::from(embedding.clone());
        client.execute(
            "INSERT INTO code_embeddings (symbol_name, embedding) VALUES ($1, $2)",
            &[&format!("function_{}", i), &vector]
        ).await?;
    }
    
    // Create HNSW index
    let start = Instant::now();
    client.execute(
        "CREATE INDEX ON code_embeddings USING hnsw (embedding vector_cosine_ops)",
        &[]
    ).await?;
    let duration = start.elapsed();
    
    println!("HNSW index created in {:?} for 100 embeddings", duration);
    
    // Verify index exists
    let index_exists = client.query_one(
        "SELECT COUNT(*) FROM pg_indexes WHERE tablename = 'code_embeddings' AND indexdef LIKE '%hnsw%'",
        &[]
    ).await?;
    
    let count: i64 = index_exists.get(0);
    assert_eq!(count, 1);
    
    Ok(())
}

#[tokio::test]
async fn test_similarity_search() -> Result<()> {
    let client = create_test_client().await?;
    
    // Setup
    client.execute("CREATE EXTENSION IF NOT EXISTS vector", &[]).await?;
    client.execute("DROP TABLE IF EXISTS code_embeddings", &[]).await?;
    
    client.execute(
        "CREATE TABLE code_embeddings (
            id SERIAL PRIMARY KEY,
            symbol_name TEXT NOT NULL,
            embedding vector(384) NOT NULL
        )",
        &[]
    ).await?;
    
    // Generate embeddings for different code patterns
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    let code_snippets = vec![
        ("parse_string", "fn parse_string(input: &str) -> Result<String, ParseError>"),
        ("parse_str", "fn parse_str(s: &str) -> Result<String, Error>"),
        ("parse_number", "fn parse_number(input: &str) -> Result<f64, ParseError>"),
        ("drop_connection", "impl Drop for DatabaseConnection { fn drop(&mut self) { } }"),
        ("analyze_string", "fn analyze_string(text: &str) -> AnalysisResult"),
    ];
    
    // Insert embeddings
    for (name, code) in &code_snippets {
        let embeddings = model.embed(vec![*code], None)?;
        let vector = Vector::from(embeddings[0].clone());
        client.execute(
            "INSERT INTO code_embeddings (symbol_name, embedding) VALUES ($1, $2)",
            &[name, &vector]
        ).await?;
    }
    
    // Create HNSW index
    client.execute(
        "CREATE INDEX ON code_embeddings USING hnsw (embedding vector_cosine_ops)",
        &[]
    ).await?;
    
    // Search for similar functions to "parse_string"
    let query_embedding = model.embed(vec!["fn parse_string(input: &str) -> Result<String, ParseError>"], None)?;
    let query_vector = Vector::from(query_embedding[0].clone());
    let start = Instant::now();
    let results = client.query(
        "SELECT symbol_name, 1 - (embedding <=> $1) as similarity 
         FROM code_embeddings 
         ORDER BY embedding <=> $1 
         LIMIT 5",
        &[&query_vector]
    ).await?;
    let search_duration = start.elapsed();
    
    println!("Search completed in {:?}", search_duration);
    println!("\nTop 5 similar functions:");
    
    for row in &results {
        let name: &str = row.get(0);
        let similarity: f64 = row.get(1);
        println!("  {} - similarity: {:.4}", name, similarity);
    }
    
    // Verify results make sense
    let top_result: &str = results[0].get(0);
    assert_eq!(top_result, "parse_string"); // Should match itself
    
    let second_result: &str = results[1].get(0);
    assert!(second_result == "parse_str" || second_result == "parse_number"); // Similar parsing functions
    
    Ok(())
}

#[tokio::test]
async fn test_transactional_consistency() -> Result<()> {
    let mut client = create_test_client().await?;
    
    // Setup
    client.execute("CREATE EXTENSION IF NOT EXISTS vector", &[]).await?;
    client.execute("DROP TABLE IF EXISTS code_embeddings", &[]).await?;
    client.execute("DROP TABLE IF EXISTS symbols", &[]).await?;
    
    // Create related tables
    client.execute(
        "CREATE TABLE symbols (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            kind TEXT NOT NULL
        )",
        &[]
    ).await?;
    
    client.execute(
        "CREATE TABLE code_embeddings (
            symbol_id INTEGER PRIMARY KEY REFERENCES symbols(id) ON DELETE CASCADE,
            embedding vector(384) NOT NULL
        )",
        &[]
    ).await?;
    
    // Generate test embedding
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    let embeddings = model.embed(vec!["fn test_function() {}"], None)?;
    let embedding = &embeddings[0];
    
    // Test atomic transaction - both succeed
    let tx = client.transaction().await?;
    
    let symbol_id: i32 = tx.query_one(
        "INSERT INTO symbols (name, file_path, kind) VALUES ($1, $2, $3) RETURNING id",
        &[&"test_function", &"src/test.rs", &"function"]
    ).await?.get(0);
    
    let vector = Vector::from(embedding.clone());
    tx.execute(
        "INSERT INTO code_embeddings (symbol_id, embedding) VALUES ($1, $2)",
        &[&symbol_id, &vector]
    ).await?;
    
    tx.commit().await?;
    
    // Verify both were inserted
    let count: i64 = client.query_one("SELECT COUNT(*) FROM symbols", &[]).await?.get(0);
    assert_eq!(count, 1);
    
    let count: i64 = client.query_one("SELECT COUNT(*) FROM code_embeddings", &[]).await?.get(0);
    assert_eq!(count, 1);
    
    // Test rollback scenario
    let tx = client.transaction().await?;
    
    let symbol_id: i32 = tx.query_one(
        "INSERT INTO symbols (name, file_path, kind) VALUES ($1, $2, $3) RETURNING id",
        &[&"rollback_function", &"src/test.rs", &"function"]
    ).await?.get(0);
    
    let vector = Vector::from(embedding.clone());
    tx.execute(
        "INSERT INTO code_embeddings (symbol_id, embedding) VALUES ($1, $2)",
        &[&symbol_id, &vector]
    ).await?;
    
    // Rollback instead of commit
    tx.rollback().await?;
    
    // Verify nothing was added
    let count: i64 = client.query_one("SELECT COUNT(*) FROM symbols", &[]).await?.get(0);
    assert_eq!(count, 1); // Still just the first one
    
    println!("Transactional consistency verified");
    
    Ok(())
}

#[tokio::test]
async fn test_embedding_quantization_storage() -> Result<()> {
    let client = create_test_client().await?;
    
    // Setup
    client.execute("CREATE EXTENSION IF NOT EXISTS vector", &[]).await?;
    client.execute("DROP TABLE IF EXISTS quantized_embeddings", &[]).await?;
    
    // Create table with smaller vector for quantized embeddings
    // Using halfvec (16-bit floats) or storing as bytea for int8
    client.execute(
        "CREATE TABLE quantized_embeddings (
            id SERIAL PRIMARY KEY,
            symbol_name TEXT NOT NULL,
            embedding_int8 BYTEA NOT NULL,
            original_norm REAL NOT NULL
        )",
        &[]
    ).await?;
    
    // Generate test embedding
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    let embeddings = model.embed(vec!["fn parse_function(input: &str) -> Result<Function, Error>"], None)?;
    let embedding = &embeddings[0];
    
    // Quantize to int8
    let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    let quantized: Vec<i8> = embedding
        .iter()
        .map(|&f| ((f / norm * 127.0).round().clamp(-128.0, 127.0)) as i8)
        .collect();
    
    // Store quantized embedding as bytea
    let quantized_bytes: Vec<u8> = quantized.iter().map(|&i| i as u8).collect();
    client.execute(
        "INSERT INTO quantized_embeddings (symbol_name, embedding_int8, original_norm) VALUES ($1, $2, $3)",
        &[&"parse_function", &quantized_bytes.as_slice(), &norm]
    ).await?;
    
    // Check storage size
    let size_result = client.query_one(
        "SELECT pg_column_size(embedding_int8) as size FROM quantized_embeddings LIMIT 1",
        &[]
    ).await?;
    
    let size: i32 = size_result.get("size");
    println!("Quantized embedding storage size: {} bytes", size);
    assert!(size < 500); // Should be around 384 + overhead
    
    Ok(())
}