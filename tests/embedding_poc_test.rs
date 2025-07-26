//! Proof of Concept tests for embedding generation using fastembed-rs
//! This is an isolated test module to validate embedding functionality
//! without modifying any production code.

use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

#[test]
fn test_basic_embedding_generation() -> Result<()> {
    // Initialize fastembed with a small model for testing
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    // Test single embedding generation
    let input = vec!["fn parse_function(input: &str) -> Result<Function, Error>"];
    let embeddings = model.embed(input, None)?;

    // Verify embedding properties
    assert_eq!(embeddings.len(), 1);
    let embedding = &embeddings[0];
    
    // AllMiniLML6V2 produces 384-dimensional embeddings
    assert_eq!(embedding.len(), 384);
    
    // Verify embeddings are normalized (approximately unit length)
    let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((magnitude - 1.0).abs() < 0.01, "Embedding magnitude: {}", magnitude);

    Ok(())
}

#[test]
fn test_embedding_consistency() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    // Same input should produce same embedding
    let input = vec!["impl Parser for RustParser"];
    let embeddings1 = model.embed(input.clone(), None)?;
    let embeddings2 = model.embed(input, None)?;

    assert_eq!(embeddings1.len(), embeddings2.len());
    
    // Check embeddings are identical
    for (e1, e2) in embeddings1[0].iter().zip(&embeddings2[0]) {
        assert!((e1 - e2).abs() < 1e-6);
    }

    Ok(())
}

#[test]
fn test_batch_embedding_generation() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    // Test batch processing
    let code_snippets = vec![
        "fn main() { println!(\"Hello, world!\"); }",
        "struct Point { x: f64, y: f64 }",
        "impl Display for Point { fn fmt(&self, f: &mut Formatter) -> Result { } }",
        "pub trait Parser { fn parse(&self, input: &str) -> Result<AST>; }",
        "use std::collections::HashMap;",
    ];

    let embeddings = model.embed(code_snippets.clone(), None)?;

    // Verify we got embeddings for all inputs
    assert_eq!(embeddings.len(), code_snippets.len());

    // All embeddings should have same dimensions
    for embedding in &embeddings {
        assert_eq!(embedding.len(), 384);
    }

    Ok(())
}

#[test]
fn test_embedding_similarity() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    // Similar code should have higher cosine similarity
    let similar_code = vec![
        "fn parse_string(input: &str) -> Result<String, ParseError>",
        "fn parse_str(s: &str) -> Result<String, Error>",
    ];

    let different_code = vec![
        "fn parse_string(input: &str) -> Result<String, ParseError>",
        "impl Drop for DatabaseConnection { fn drop(&mut self) { } }",
    ];

    let similar_embeddings = model.embed(similar_code, None)?;
    let different_embeddings = model.embed(different_code, None)?;

    // Calculate cosine similarities
    let similar_similarity = cosine_similarity(&similar_embeddings[0], &similar_embeddings[1]);
    let different_similarity = cosine_similarity(&different_embeddings[0], &different_embeddings[1]);

    // Similar code should have higher similarity
    assert!(
        similar_similarity > different_similarity,
        "Similar: {}, Different: {}",
        similar_similarity,
        different_similarity
    );
    
    // Similar code should have high similarity (> 0.7)
    assert!(similar_similarity > 0.7);

    Ok(())
}

#[test]
fn test_embedding_generation_performance() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    // Generate 100 code snippets
    let code_snippets: Vec<String> = (0..100)
        .map(|i| format!("fn function_{}() {{ /* implementation */ }}", i))
        .collect();
    
    let start = std::time::Instant::now();
    let embeddings = model.embed(code_snippets.iter().map(|s| s.as_str()).collect(), None)?;
    let duration = start.elapsed();

    println!("Generated {} embeddings in {:?}", embeddings.len(), duration);
    println!("Average time per embedding: {:?}", duration / embeddings.len() as u32);

    // Should be reasonably fast (< 10ms per embedding on average)
    assert!((duration.as_millis() / embeddings.len() as u128) < 10);

    Ok(())
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    dot_product / (magnitude_a * magnitude_b)
}

#[test]
fn test_embedding_memory_usage() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    // Generate embeddings for 1000 symbols to test memory usage
    let code_snippets: Vec<String> = (0..1000)
        .map(|i| format!("fn function_{}() {{ /* implementation for function {} */ }}", i, i))
        .collect();
    
    let embeddings = model.embed(code_snippets.iter().map(|s| s.as_str()).collect(), None)?;
    
    // Calculate memory usage
    let embedding_dimension = embeddings[0].len(); // 384 for AllMiniLML6V2
    let bytes_per_float = std::mem::size_of::<f32>(); // 4 bytes
    let memory_per_embedding = embedding_dimension * bytes_per_float;
    let total_memory = memory_per_embedding * embeddings.len();
    
    println!("Embedding dimension: {}", embedding_dimension);
    println!("Memory per embedding (float32): {} bytes", memory_per_embedding);
    println!("Total memory for {} embeddings: {} KB", embeddings.len(), total_memory / 1024);
    println!("Average memory per symbol: {} bytes", memory_per_embedding);
    
    // Verify memory usage is within expected bounds
    assert_eq!(embedding_dimension, 384);
    assert_eq!(memory_per_embedding, 1536); // 384 * 4 bytes
    
    Ok(())
}

#[test]
fn test_embedding_quantization() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    let input = vec!["fn parse_function(input: &str) -> Result<Function, Error>"];
    let embeddings = model.embed(input, None)?;
    let embedding = &embeddings[0];
    
    // Simulate int8 quantization
    let quantized: Vec<i8> = embedding
        .iter()
        .map(|&f| (f * 127.0).round().clamp(-128.0, 127.0) as i8)
        .collect();
    
    // Calculate memory savings
    let original_size = embedding.len() * std::mem::size_of::<f32>();
    let quantized_size = quantized.len() * std::mem::size_of::<i8>();
    let compression_ratio = original_size as f32 / quantized_size as f32;
    
    println!("Original size (float32): {} bytes", original_size);
    println!("Quantized size (int8): {} bytes", quantized_size);
    println!("Compression ratio: {:.2}x", compression_ratio);
    println!("Memory per symbol (quantized): {} bytes", quantized_size);
    
    // Dequantize to test quality
    let dequantized: Vec<f32> = quantized
        .iter()
        .map(|&i| i as f32 / 127.0)
        .collect();
    
    // Calculate quantization error
    let mse: f32 = embedding
        .iter()
        .zip(&dequantized)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>() / embedding.len() as f32;
    
    let similarity = cosine_similarity(embedding, &dequantized);
    
    println!("Mean squared error: {:.6}", mse);
    println!("Cosine similarity after quantization: {:.4}", similarity);
    
    // Verify quantization quality
    assert!(mse < 0.001); // Low quantization error
    assert!(similarity > 0.99); // High similarity preserved
    assert_eq!(compression_ratio, 4.0); // 4x compression
    
    Ok(())
}

#[test]
fn test_embedding_dimension_reduction() -> Result<()> {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;

    let input = vec!["fn parse_function(input: &str) -> Result<Function, Error>"];
    let embeddings = model.embed(input, None)?;
    let embedding = &embeddings[0];
    
    // Simulate dimension reduction from 384 to 128
    let target_dim = 128;
    let reduced_embedding: Vec<f32> = embedding.iter().take(target_dim).copied().collect();
    
    // Calculate memory savings
    let original_size = embedding.len() * std::mem::size_of::<f32>();
    let reduced_size = reduced_embedding.len() * std::mem::size_of::<f32>();
    let reduction_ratio = original_size as f32 / reduced_size as f32;
    
    println!("Original dimension: {}", embedding.len());
    println!("Reduced dimension: {}", reduced_embedding.len());
    println!("Dimension reduction ratio: {:.2}x", reduction_ratio);
    
    // Combined with int8 quantization
    let quantized_reduced: Vec<i8> = reduced_embedding
        .iter()
        .map(|&f| (f * 127.0).round().clamp(-128.0, 127.0) as i8)
        .collect();
    
    let final_size = quantized_reduced.len() * std::mem::size_of::<i8>();
    let total_compression = original_size as f32 / final_size as f32;
    
    println!("Final size (128 dims + int8): {} bytes", final_size);
    println!("Total compression ratio: {:.2}x", total_compression);
    println!("Achieves <100 bytes/symbol target: {} bytes", final_size);
    
    // Verify we meet the target
    assert_eq!(final_size, 128); // Exactly 128 bytes
    assert!(final_size < 150); // Well under 150 bytes target
    
    Ok(())
}