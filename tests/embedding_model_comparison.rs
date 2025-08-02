//! Compare different embedding models for code similarity
//! 
//! This test evaluates multiple embedding models to find the best one
//! for code documentation semantic search.

use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::time::Instant;

/// Test case for code similarity
struct CodeExample {
    name: &'static str,
    code1: &'static str,
    code2: &'static str,
    expected_similar: bool,
}

/// Model evaluation results
#[derive(Debug)]
#[allow(dead_code)]
struct ModelEvaluation {
    model_name: String,
    dimensions: usize,
    
    // Performance metrics
    avg_embedding_time_ms: f64,
    model_size_mb: f64,
    
    // Quality metrics
    similar_pairs_score: f32,
    different_pairs_score: f32,
    accuracy: f32,
}

#[test]
fn compare_embedding_models() -> Result<()> {
    // Define test cases
    let test_cases = vec![
        CodeExample {
            name: "Similar function implementations",
            code1: "Parse a string and return the parsed result or an error",
            code2: "Function that parses input string and returns parsed data or error",
            expected_similar: true,
        },
        CodeExample {
            name: "Same concept, different wording",
            code1: "Authenticate user with username and password",
            code2: "Verify user credentials for login",
            expected_similar: true,
        },
        CodeExample {
            name: "Error handling patterns",
            code1: "Handle database connection errors and retry with backoff",
            code2: "Retry failed database connections with exponential backoff",
            expected_similar: true,
        },
        CodeExample {
            name: "Different concepts",
            code1: "Calculate the factorial of a number recursively",
            code2: "Render HTML template with user data",
            expected_similar: false,
        },
        CodeExample {
            name: "Data structure vs algorithm",
            code1: "Binary tree node with left and right children",
            code2: "Sort array using quicksort algorithm",
            expected_similar: false,
        },
    ];
    
    // Models to evaluate
    let models = vec![
        EmbeddingModel::AllMiniLML6V2,
        // Note: These models might not be available in fastembed yet
        // We'll use what's available and document findings
    ];
    
    let mut results = Vec::new();
    
    for model_type in models {
        println!("\n=== Evaluating {:?} ===", model_type);
        let result = evaluate_model(model_type, &test_cases)?;
        println!("{:#?}", result);
        results.push(result);
    }
    
    // Compare results
    println!("\n=== Model Comparison Summary ===");
    for result in &results {
        println!(
            "{}: {} dims, {:.2}ms/embed, accuracy: {:.2}%",
            result.model_name,
            result.dimensions,
            result.avg_embedding_time_ms,
            result.accuracy * 100.0
        );
    }
    
    // Find best model
    let best_model = results
        .iter()
        .max_by(|a, b| a.accuracy.partial_cmp(&b.accuracy).unwrap())
        .unwrap();
    
    println!("\nBest model: {} with {:.2}% accuracy", best_model.model_name, best_model.accuracy * 100.0);
    
    Ok(())
}

fn evaluate_model(model_type: EmbeddingModel, test_cases: &[CodeExample]) -> Result<ModelEvaluation> {
    let start = Instant::now();
    
    // Get model info before moving
    let model_name = format!("{:?}", model_type);
    
    // Initialize model
    let mut model = TextEmbedding::try_new(
        InitOptions::new(model_type).with_show_download_progress(true),
    )?;
    
    let init_time = start.elapsed();
    println!("Model initialization: {:?}", init_time);
    
    // Test embedding generation
    let test_text = vec!["Test embedding"];
    let test_embedding = model.embed(test_text, None)?;
    let dimensions = test_embedding[0].len();
    
    // Evaluate on test cases
    let mut correct_predictions = 0;
    let mut similar_scores = Vec::new();
    let mut different_scores = Vec::new();
    let mut embedding_times = Vec::new();
    
    for test_case in test_cases {
        // Generate embeddings
        let embed_start = Instant::now();
        let embeddings = model.embed(vec![test_case.code1, test_case.code2], None)?;
        embedding_times.push(embed_start.elapsed().as_micros() as f64 / 1000.0);
        
        // Calculate similarity
        let similarity = cosine_similarity(&embeddings[0], &embeddings[1]);
        
        // Use 0.7 as threshold (can be tuned)
        let predicted_similar = similarity > 0.7;
        if predicted_similar == test_case.expected_similar {
            correct_predictions += 1;
        }
        
        if test_case.expected_similar {
            similar_scores.push(similarity);
        } else {
            different_scores.push(similarity);
        }
        
        println!(
            "{}: similarity={:.3}, expected={}, predicted={}, {}",
            test_case.name,
            similarity,
            test_case.expected_similar,
            predicted_similar,
            if predicted_similar == test_case.expected_similar { "✓" } else { "✗" }
        );
    }
    
    // Calculate metrics
    let accuracy = correct_predictions as f32 / test_cases.len() as f32;
    let avg_similar_score = similar_scores.iter().sum::<f32>() / similar_scores.len() as f32;
    let avg_different_score = different_scores.iter().sum::<f32>() / different_scores.len() as f32;
    let avg_embedding_time = embedding_times.iter().sum::<f64>() / embedding_times.len() as f64;
    
    // Estimate model size (very rough)
    let model_size_mb = match model_name.as_str() {
        "AllMiniLML6V2" => 90.0,
        _ => 100.0, // Default estimate
    };
    
    Ok(ModelEvaluation {
        model_name,
        dimensions,
        avg_embedding_time_ms: avg_embedding_time,
        model_size_mb,
        similar_pairs_score: avg_similar_score,
        different_pairs_score: avg_different_score,
        accuracy,
    })
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    dot_product / (magnitude_a * magnitude_b)
}

#[test]
fn test_code_specific_similarity() -> Result<()> {
    // Test with actual code documentation examples
    let code_docs = vec![
        (
            "Parse JSON data from a string and return a structured object",
            "Deserialize JSON string into a typed data structure",
            true, // Should be similar
        ),
        (
            "Establish database connection with retry logic",
            "Connect to database with automatic retry on failure",
            true,
        ),
        (
            "Calculate hash of file contents for integrity check",
            "Sort array elements in ascending order",
            false, // Different concepts
        ),
    ];
    
    // Use the default model for now
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    println!("\n=== Code Documentation Similarity Test ===");
    
    for (doc1, doc2, expected_similar) in code_docs {
        let embeddings = model.embed(vec![doc1, doc2], None)?;
        let similarity = cosine_similarity(&embeddings[0], &embeddings[1]);
        
        let threshold = 0.75; // Higher threshold for documentation
        let is_similar = similarity > threshold;
        
        println!(
            "Doc1: {}\nDoc2: {}\nSimilarity: {:.3}, Expected: {}, Got: {}, {}\n",
            doc1,
            doc2,
            similarity,
            expected_similar,
            is_similar,
            if is_similar == expected_similar { "✓" } else { "✗" }
        );
    }
    
    Ok(())
}

#[test]
fn test_similarity_thresholds() -> Result<()> {
    // Test to find optimal similarity thresholds
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
    )?;
    
    // Pairs with known relationships
    let test_pairs = vec![
        // Very similar (>0.9)
        ("Parse JSON string", "Parse JSON string", 1.0),
        
        // Similar concepts (0.7-0.9)
        ("Parse JSON data", "Deserialize JSON", 0.8),
        ("Handle errors gracefully", "Error handling with recovery", 0.75),
        
        // Related but different (0.5-0.7)
        ("Parse JSON", "Parse XML", 0.6),
        ("Database connection", "Network connection", 0.55),
        
        // Unrelated (<0.5)
        ("Parse JSON", "Calculate factorial", 0.3),
        ("User authentication", "Matrix multiplication", 0.2),
    ];
    
    println!("\n=== Similarity Threshold Analysis ===");
    
    for (text1, text2, expected_range) in test_pairs {
        let embeddings = model.embed(vec![text1, text2], None)?;
        let similarity = cosine_similarity(&embeddings[0], &embeddings[1]);
        
        println!(
            "{} <-> {}\nSimilarity: {:.3} (expected ~{:.1})\n",
            text1, text2, similarity, expected_range
        );
    }
    
    println!("Recommended thresholds:");
    println!("- Very similar: > 0.85");
    println!("- Similar: 0.70 - 0.85");
    println!("- Somewhat related: 0.50 - 0.70");
    println!("- Unrelated: < 0.50");
    
    Ok(())
}