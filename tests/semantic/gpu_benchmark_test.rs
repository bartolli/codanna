//! Benchmark CPU vs GPU embedding performance.
//!
//! Run with: cargo test --test semantic_tests --features gpu-coreml benchmark -- --nocapture --ignored

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::time::Instant;

const BATCH_SIZE: usize = 100;
const ITERATIONS: usize = 5;

fn generate_test_docs() -> Vec<String> {
    (0..BATCH_SIZE)
        .map(|i| format!(
            "This is test document number {i}. It contains some sample text for embedding generation benchmarks. \
             The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet."
        ))
        .collect()
}

#[test]
#[ignore] // Run explicitly with --ignored
fn benchmark_cpu_embedding() {
    let mut model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
        .expect("Failed to create CPU model");

    let docs = generate_test_docs();

    // Warmup
    let _ = model.embed(vec!["warmup"], None);

    let mut total_time = std::time::Duration::ZERO;

    for i in 0..ITERATIONS {
        let start = Instant::now();
        let embeddings = model.embed(docs.clone(), None).expect("Failed to embed");
        let elapsed = start.elapsed();
        total_time += elapsed;

        println!(
            "CPU iteration {}: {} embeddings in {:?} ({:.1} emb/s)",
            i + 1,
            embeddings.len(),
            elapsed,
            embeddings.len() as f64 / elapsed.as_secs_f64()
        );
    }

    let avg = total_time / ITERATIONS as u32;
    println!(
        "\nCPU Average: {:?} for {} embeddings ({:.1} emb/s)",
        avg,
        BATCH_SIZE,
        BATCH_SIZE as f64 / avg.as_secs_f64()
    );
}

#[test]
#[ignore]
#[cfg(feature = "gpu-coreml")]
fn benchmark_coreml_embedding() {
    use ort::execution_providers::CoreMLExecutionProvider;

    let coreml_ep = CoreMLExecutionProvider::default()
        .with_subgraphs(true)
        .build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_execution_providers(vec![coreml_ep]),
    )
    .expect("Failed to create CoreML model");

    let docs = generate_test_docs();

    // Warmup (important for GPU - first run compiles kernels)
    let _ = model.embed(vec!["warmup"], None);

    let mut total_time = std::time::Duration::ZERO;

    for i in 0..ITERATIONS {
        let start = Instant::now();
        let embeddings = model.embed(docs.clone(), None).expect("Failed to embed");
        let elapsed = start.elapsed();
        total_time += elapsed;

        println!(
            "CoreML iteration {}: {} embeddings in {:?} ({:.1} emb/s)",
            i + 1,
            embeddings.len(),
            elapsed,
            embeddings.len() as f64 / elapsed.as_secs_f64()
        );
    }

    let avg = total_time / ITERATIONS as u32;
    println!(
        "\nCoreML Average: {:?} for {} embeddings ({:.1} emb/s)",
        avg,
        BATCH_SIZE,
        BATCH_SIZE as f64 / avg.as_secs_f64()
    );
}
