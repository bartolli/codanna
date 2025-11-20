//! Compare embedding-only search vs embedding+reranking
//!
//! This test validates whether reranking improves search result quality
//! for code documentation queries.

use anyhow::Result;
use fastembed::{
    EmbeddingModel, InitOptions, RerankInitOptions, RerankerModel, TextEmbedding, TextRerank,
};
use std::time::Instant;

/// Get a unique cache directory for each test to avoid conflicts
fn get_test_cache_dir(test_name: &str) -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir();
    temp_dir.join(format!(
        "codanna_test_rerank_{}_{}",
        test_name,
        std::process::id()
    ))
}

/// Test query with expected best match
struct QueryTest {
    query: &'static str,
    documents: Vec<&'static str>,
    expected_best_index: usize, // Which doc should rank #1
}

#[test]
#[ignore = "Downloads models (~200MB) - run with --ignored for reranking tests"]
fn test_reranking_improves_results() -> Result<()> {
    let cache_dir = get_test_cache_dir("reranking_test");

    println!("\n=== Available Reranker Models ===");
    println!("1. BGERerankerBase - General English/Chinese");
    println!("2. BGERerankerV2M3 - Multilingual general");
    println!("3. JINARerankerV1TurboEn - Fast English (code-focused)");
    println!("4. JINARerankerV2BaseMultiligual - Multilingual (code-focused)");

    // Realistic code documentation queries
    let test_cases = [
        QueryTest {
            query: "authenticate user with credentials",
            documents: vec![
                "User authentication service - handles login, logout, and session management",
                "Parse command-line arguments and validate input flags",
                "Database connection pool with automatic retry logic",
                "Verify user credentials and generate authentication tokens",
            ],
            expected_best_index: 3, // "Verify user credentials" is most relevant
        },
        QueryTest {
            query: "parse JSON data",
            documents: vec![
                "Calculate hash of file contents for integrity verification",
                "Deserialize JSON string into typed data structure with validation",
                "Render HTML template with dynamic data interpolation",
                "Parse configuration file and extract settings",
            ],
            expected_best_index: 1, // "Deserialize JSON" is most relevant
        },
        QueryTest {
            query: "error handling with retry",
            documents: vec![
                "Sort array elements in ascending order using quicksort",
                "Handle database connection errors and retry with exponential backoff",
                "Calculate the factorial of a number recursively",
                "Format date string according to locale-specific patterns",
            ],
            expected_best_index: 1, // "Handle database connection errors" is most relevant
        },
    ];

    println!("\n=== Reranking Quality Test ===\n");

    // Initialize embedding model (cached)
    println!("Loading embedding model (AllMiniLML6V2)...");
    let mut embedding_model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache_dir.clone())
            .with_show_download_progress(true),
    )?;

    // Calculate embedding-only results ONCE
    let mut embedding_only_correct = 0;
    let mut embedding_results = Vec::new();

    println!("\n--- Testing Embedding-Only (Baseline) ---");
    for (test_num, test) in test_cases.iter().enumerate() {
        println!("\nTest {}: \"{}\"", test_num + 1, test.query);

        let start = Instant::now();
        let query_embedding = embedding_model.embed(vec![test.query], None)?;
        let doc_embeddings = embedding_model.embed(test.documents.clone(), None)?;

        let mut scores: Vec<(usize, f32)> = doc_embeddings
            .iter()
            .enumerate()
            .map(|(idx, doc_emb)| {
                let score = cosine_similarity(&query_embedding[0], doc_emb);
                (idx, score)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let time = start.elapsed();

        println!("  Results ({time:?}):");
        for (rank, (idx, score)) in scores.iter().take(4).enumerate() {
            println!(
                "    {}. Doc {} (score: {:.3}): {}",
                rank + 1,
                idx,
                score,
                test.documents[*idx].chars().take(60).collect::<String>()
            );
        }

        let correct = scores[0].0 == test.expected_best_index;
        if correct {
            embedding_only_correct += 1;
            println!("  ✓ Correct");
        } else {
            println!(
                "  ✗ Wrong (expected Doc {}, got Doc {})",
                test.expected_best_index, scores[0].0
            );
        }

        embedding_results.push((scores, time));
    }

    // Test each reranker model
    let reranker_models = vec![
        (RerankerModel::BGERerankerBase, "BGERerankerBase"),
        (RerankerModel::BGERerankerV2M3, "BGERerankerV2M3"),
        (
            RerankerModel::JINARerankerV1TurboEn,
            "JINARerankerV1TurboEn",
        ),
        (
            RerankerModel::JINARerankerV2BaseMultiligual,
            "JINARerankerV2BaseMultiligual",
        ),
    ];

    let mut model_accuracies = Vec::new();

    for (model_type, model_name) in reranker_models {
        println!("\n\n--- Testing with {model_name} ---");
        println!("Loading reranker model (cached if available)...");

        let mut reranker = TextRerank::try_new(
            RerankInitOptions::new(model_type)
                .with_cache_dir(cache_dir.clone())
                .with_show_download_progress(true),
        )?;

        let mut correct = 0;

        for (test_num, test) in test_cases.iter().enumerate() {
            println!("\nTest {}: \"{}\"", test_num + 1, test.query);

            let start = Instant::now();
            let results = reranker.rerank(test.query, test.documents.clone(), false, None)?;
            let time = start.elapsed();

            println!("  Results ({time:?}):");
            for (rank, result) in results.iter().take(4).enumerate() {
                println!(
                    "    {}. Doc {} (score: {:.3}): {}",
                    rank + 1,
                    result.index,
                    result.score,
                    test.documents[result.index]
                        .chars()
                        .take(60)
                        .collect::<String>()
                );
            }

            if results[0].index == test.expected_best_index {
                correct += 1;
                println!("  ✓ Correct");
            } else {
                println!(
                    "  ✗ Wrong (expected Doc {}, got Doc {})",
                    test.expected_best_index, results[0].index
                );
            }
        }

        let accuracy = (correct as f32 / test_cases.len() as f32) * 100.0;
        model_accuracies.push((model_name, correct, accuracy));
    }

    // Final comparison
    println!("\n\n=== FINAL RESULTS ===");
    println!(
        "\nEmbedding-only (Baseline): {}/{} ({:.1}%)",
        embedding_only_correct,
        test_cases.len(),
        (embedding_only_correct as f32 / test_cases.len() as f32) * 100.0
    );

    println!("\nReranker Models:");
    for (name, correct, accuracy) in &model_accuracies {
        let vs_baseline = *correct - embedding_only_correct;
        let symbol = if vs_baseline > 0 {
            "↑"
        } else if vs_baseline < 0 {
            "↓"
        } else {
            "="
        };
        println!(
            "  {}: {}/{} ({:.1}%) {} {}{} vs baseline",
            name,
            correct,
            test_cases.len(),
            accuracy,
            symbol,
            if vs_baseline > 0 { "+" } else { "" },
            vs_baseline
        );
    }

    // Find best model
    let best_reranker = model_accuracies
        .iter()
        .max_by(|a, b| a.1.cmp(&b.1))
        .unwrap();

    println!("\n=== CONCLUSION ===");
    if best_reranker.1 > embedding_only_correct {
        println!("✅ Best reranker: {} improves results!", best_reranker.0);
        println!(
            "   Accuracy: {:.1}% (embedding: {:.1}%)",
            best_reranker.2,
            (embedding_only_correct as f32 / test_cases.len() as f32) * 100.0
        );
        println!(
            "   → RECOMMEND: Implement reranking with {}",
            best_reranker.0
        );
    } else if best_reranker.1 == embedding_only_correct {
        println!("⚠️  Reranking doesn't improve results");
        println!(
            "   Best model: {} matches baseline ({:.1}%)",
            best_reranker.0, best_reranker.2
        );
        println!("   → Consider cost/benefit of added latency");
    } else {
        println!("❌ Reranking makes results worse");
        println!(
            "   Best model: {} = {:.1}% (baseline: {:.1}%)",
            best_reranker.0,
            best_reranker.2,
            (embedding_only_correct as f32 / test_cases.len() as f32) * 100.0
        );
        println!("   → DO NOT implement reranking");
    }

    Ok(())
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    dot_product / (magnitude_a * magnitude_b)
}
