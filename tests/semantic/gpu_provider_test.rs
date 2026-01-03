//! Test GPU execution provider integration with fastembed.
//!
//! Run with: cargo test --test gpu_provider_test --features gpu-coreml

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

#[test]
fn test_cpu_embedding_baseline() {
    // CPU baseline - should always work
    let mut model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
        .expect("Failed to create CPU model");

    let embeddings = model
        .embed(vec!["hello world"], None)
        .expect("Failed to embed");

    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].len(), 384); // AllMiniLML6V2 dimension
    println!("CPU embedding works: {} dimensions", embeddings[0].len());
}

#[test]
#[cfg(feature = "gpu-coreml")]
fn test_coreml_execution_provider() {
    use ort::execution_providers::CoreMLExecutionProvider;

    // Create with CoreML execution provider
    let coreml_ep = CoreMLExecutionProvider::default()
        .with_subgraphs(true) // Enable subgraph partitioning
        .build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_execution_providers(vec![coreml_ep]),
    )
    .expect("Failed to create CoreML model");

    let embeddings = model
        .embed(vec!["hello world"], None)
        .expect("Failed to embed with CoreML");

    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].len(), 384);
    println!("CoreML embedding works: {} dimensions", embeddings[0].len());
}

#[test]
#[cfg(feature = "gpu-cuda")]
fn test_cuda_execution_provider() {
    use ort::execution_providers::CUDAExecutionProvider;

    let cuda_ep = CUDAExecutionProvider::default().build();

    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_execution_providers(vec![cuda_ep]),
    )
    .expect("Failed to create CUDA model");

    let embeddings = model
        .embed(vec!["hello world"], None)
        .expect("Failed to embed with CUDA");

    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].len(), 384);
    println!("CUDA embedding works: {} dimensions", embeddings[0].len());
}
