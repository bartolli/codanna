//! Sustained embedding benchmark using real text corpus.
//!
//! Downloads a public domain book and chunks it similar to doc comments.
//! Measures sustained throughput after warmup is complete.
//!
//! Run with: cargo test --test semantic_tests --features gpu-coreml sustained -- --nocapture --ignored

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::time::Instant;

/// Chunk text into doc-comment-sized pieces (50-500 chars, ~avg 150)
fn chunk_text(text: &str, target_chunks: usize) -> Vec<String> {
    let sentences: Vec<&str> = text
        .split(['.', '!', '?'])
        .filter(|s| s.len() > 20)
        .collect();

    let mut chunks = Vec::with_capacity(target_chunks);
    let mut current_chunk = String::new();

    for sentence in sentences {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            continue;
        }

        if current_chunk.len() + trimmed.len() > 400 {
            if current_chunk.len() > 50 {
                chunks.push(current_chunk.clone());
            }
            current_chunk.clear();
        }

        if !current_chunk.is_empty() {
            current_chunk.push_str(". ");
        }
        current_chunk.push_str(trimmed);

        if chunks.len() >= target_chunks {
            break;
        }
    }

    if !current_chunk.is_empty() && chunks.len() < target_chunks {
        chunks.push(current_chunk);
    }

    chunks
}

/// Load sample text - use a local file or embedded sample
fn load_corpus() -> String {
    // Try to load a real file first
    let paths = ["examples/rust/main.rs", "src/lib.rs", "README.md"];

    for path in paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            if content.len() > 10000 {
                return content;
            }
        }
    }

    // Fallback: generate substantial sample text
    let sample = r#"
        The Rust programming language helps you write faster, more reliable software.
        High-level ergonomics and low-level control are often at odds in programming language design.
        Rust challenges that conflict. Through balancing powerful technical capacity and a great developer experience.
        Rust gives you the option to control low-level details such as memory usage without all the hassle traditionally associated with such control.

        Rust's rich type system and ownership model guarantee memory-safety and thread-safety.
        They enable you to eliminate many classes of bugs at compile-time.
        Rust has great documentation, a friendly compiler with useful error messages, and top-tier tooling.
        An integrated package manager and build tool, smart multi-editor support with auto-completion and type inspections.
        An auto-formatter, and more.

        Rust is blazingly fast and memory-efficient: with no runtime or garbage collector.
        It can power performance-critical services, run on embedded devices, and easily integrate with other languages.
        Rust's rich type system and ownership model guarantee memory-safety and thread-safety.
        Enabling you to eliminate many classes of bugs at compile-time.

        Rust has excellent documentation with the Rust Book available online for free.
        The community is welcoming and helpful. There are many ways to get involved.
        From the official forums to local meetups and conferences.
        Companies around the world are using Rust in production for fast, low-resource, cross-platform solutions.
        Software you know and love, like Firefox, Dropbox, and Cloudflare, uses Rust.
        From startups to large corporations, from embedded devices to scalable web services, Rust is a great fit.
    "#;

    // Repeat to get enough text
    sample.repeat(100)
}

const TARGET_CHUNKS: usize = 2000;
const BATCH_SIZE: usize = 64;
const WARMUP_BATCHES: usize = 3;

fn run_benchmark(model: &mut TextEmbedding, chunks: &[String], provider_name: &str) {
    let total_embeddings = chunks.len();
    let batches: Vec<Vec<String>> = chunks.chunks(BATCH_SIZE).map(|c| c.to_vec()).collect();

    println!("\n=== {provider_name} Benchmark ===");
    println!("Total chunks: {total_embeddings}");
    println!("Batch size: {BATCH_SIZE}");
    println!("Total batches: {}", batches.len());
    println!(
        "Avg chunk length: {:.0} chars",
        chunks.iter().map(|c| c.len()).sum::<usize>() as f64 / chunks.len() as f64
    );

    // Warmup
    println!("\nWarming up ({WARMUP_BATCHES} batches)...");
    for batch in batches.iter().take(WARMUP_BATCHES) {
        let _ = model.embed(batch.clone(), None);
    }

    // Benchmark
    println!("Running sustained benchmark...");
    let start = Instant::now();
    let mut embedded_count = 0;

    for batch in &batches {
        let result = model.embed(batch.clone(), None).expect("Embedding failed");
        embedded_count += result.len();
    }

    let elapsed = start.elapsed();
    let throughput = embedded_count as f64 / elapsed.as_secs_f64();

    println!("\n--- {provider_name} Results ---");
    println!("Total embedded: {embedded_count}");
    println!("Total time: {:.2}s", elapsed.as_secs_f64());
    println!("Throughput: {throughput:.1} embeddings/second");
    println!(
        "Avg per embedding: {:.2}ms",
        elapsed.as_millis() as f64 / embedded_count as f64
    );
}

/// Small model benchmark (AllMiniLML6V2, ~23MB)
#[test]
#[ignore]
fn sustained_benchmark_cpu_small() {
    let corpus = load_corpus();
    let chunks = chunk_text(&corpus, TARGET_CHUNKS);
    println!("Loaded {} chunks from corpus", chunks.len());

    let mut model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
        .expect("Failed to create CPU model");

    run_benchmark(&mut model, &chunks, "CPU (AllMiniLML6V2)");
}

/// Large model benchmark (MultilingualE5Large, ~560M params, ~2.2GB)
#[test]
#[ignore]
fn sustained_benchmark_cpu_large() {
    let corpus = load_corpus();
    let chunks = chunk_text(&corpus, TARGET_CHUNKS);
    println!("Loaded {} chunks from corpus", chunks.len());

    let mut model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::MultilingualE5Large))
        .expect("Failed to create CPU model");

    run_benchmark(&mut model, &chunks, "CPU (E5-Large)");
}

/// Small model CoreML benchmark
#[test]
#[ignore]
#[cfg(feature = "gpu-coreml")]
fn sustained_benchmark_coreml_small() {
    use ort::execution_providers::CoreMLExecutionProvider;

    let corpus = load_corpus();
    let chunks = chunk_text(&corpus, TARGET_CHUNKS);
    println!("Loaded {} chunks from corpus", chunks.len());

    let coreml_ep = CoreMLExecutionProvider::default()
        .with_subgraphs(true)
        .build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_execution_providers(vec![coreml_ep]),
    )
    .expect("Failed to create CoreML model");

    run_benchmark(&mut model, &chunks, "CoreML (AllMiniLML6V2)");
}

/// Large model CoreML benchmark
#[test]
#[ignore]
#[cfg(feature = "gpu-coreml")]
fn sustained_benchmark_coreml_large() {
    use ort::execution_providers::CoreMLExecutionProvider;

    let corpus = load_corpus();
    let chunks = chunk_text(&corpus, TARGET_CHUNKS);
    println!("Loaded {} chunks from corpus", chunks.len());

    let coreml_ep = CoreMLExecutionProvider::default()
        .with_subgraphs(true)
        .build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::MultilingualE5Large)
            .with_execution_providers(vec![coreml_ep]),
    )
    .expect("Failed to create CoreML model");

    run_benchmark(&mut model, &chunks, "CoreML (E5-Large)");
}

/// Small model CUDA benchmark
#[test]
#[ignore]
#[cfg(feature = "gpu-cuda")]
fn sustained_benchmark_cuda_small() {
    use ort::execution_providers::CUDAExecutionProvider;

    let corpus = load_corpus();
    let chunks = chunk_text(&corpus, TARGET_CHUNKS);
    println!("Loaded {} chunks from corpus", chunks.len());

    let cuda_ep = CUDAExecutionProvider::default().build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_execution_providers(vec![cuda_ep]),
    )
    .expect("Failed to create CUDA model");

    run_benchmark(&mut model, &chunks, "CUDA (AllMiniLML6V2)");
}

/// Large model CUDA benchmark
#[test]
#[ignore]
#[cfg(feature = "gpu-cuda")]
fn sustained_benchmark_cuda_large() {
    use ort::execution_providers::CUDAExecutionProvider;

    let corpus = load_corpus();
    let chunks = chunk_text(&corpus, TARGET_CHUNKS);
    println!("Loaded {} chunks from corpus", chunks.len());

    let cuda_ep = CUDAExecutionProvider::default().build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::MultilingualE5Large)
            .with_execution_providers(vec![cuda_ep]),
    )
    .expect("Failed to create CUDA model");

    run_benchmark(&mut model, &chunks, "CUDA (E5-Large)");
}
