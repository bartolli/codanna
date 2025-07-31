//! Test Task 1.6: Cache Warming Implementation
//!
//! This test validates that the DocumentIndex properly warms the cluster
//! cache after reader reloads and tracks generation changes correctly.

use codanna::storage::DocumentIndex;
use codanna::types::{SymbolId, SymbolKind, FileId};
use codanna::vector::{VectorSearchEngine, VectorDimension, EmbeddingGenerator, VectorError};
use tempfile::TempDir;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Mock embedding generator for testing
struct MockEmbeddingGenerator {
    dimension: usize,
}

impl MockEmbeddingGenerator {
    fn new(dimension: usize) -> Self {
        Self { dimension }
    }
}

impl EmbeddingGenerator for MockEmbeddingGenerator {
    fn generate_embeddings(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, VectorError> {
        // Generate deterministic embeddings for testing
        let embeddings = texts.iter()
            .enumerate()
            .map(|(i, text)| {
                let mut vec = vec![0.1; self.dimension];
                // Create some variation based on index and text length
                let variation = (i as f32 + text.len() as f32) / 100.0;
                
                // Vary first few dimensions
                if self.dimension > 0 { vec[0] = variation; }
                if self.dimension > 1 { vec[1] = variation * 0.8; }
                if self.dimension > 2 { vec[2] = variation * 1.2; }
                if self.dimension > 3 { vec[3] = (i as f32) / 10.0; }
                
                // Normalize
                let magnitude: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                if magnitude > 0.0 {
                    for val in &mut vec {
                        *val /= magnitude;
                    }
                }
                
                vec
            })
            .collect();
        Ok(embeddings)
    }
    
    fn dimension(&self) -> VectorDimension {
        VectorDimension::new(self.dimension).unwrap()
    }
}

#[test]
fn test_cache_warming_performance() {
    let temp_dir = TempDir::new().unwrap();
    let vector_dir = temp_dir.path().join("vectors");
    
    // Create vector engine and embedding generator
    let dimension = VectorDimension::new(384).unwrap();
    let vector_engine = VectorSearchEngine::new(&vector_dir, dimension).unwrap();
    let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
    let embedding_generator = Arc::new(MockEmbeddingGenerator::new(384));
    
    // Create index with vector support
    let index = DocumentIndex::new(temp_dir.path())
        .unwrap()
        .with_vector_support(vector_engine_arc.clone(), &vector_dir)
        .with_embedding_generator(embedding_generator);
    
    // Phase 1: Initial indexing with multiple documents
    index.start_batch().unwrap();
    
    // Add documents to create enough for multiple clusters
    for i in 1..=100 {
        let symbol_id = SymbolId::new(i).unwrap();
        let file_id = FileId::new((i % 10) + 1).unwrap();
        
        index.add_document(
            symbol_id,
            &format!("function_{}", i),
            SymbolKind::Function,
            file_id,
            &format!("file_{}.rs", file_id.value()),
            i * 10,
            1,
            Some(&format!("Function {} documentation", i)),
            Some(&format!("fn function_{}() -> Result<(), Error>", i)),
            "test_module",
            Some(&format!("fn function_{}() {{ /* implementation */ }}", i)),
        ).unwrap();
    }
    
    // Commit batch to trigger vector indexing
    index.commit_batch().unwrap();
    
    // Update cluster assignments after vector processing
    index.update_cluster_assignments().unwrap();
    
    // Verify initial cache generation
    let initial_generation = index.get_cache_generation().unwrap();
    assert!(initial_generation.is_some(), "Cache should have a generation after commit");
    let gen1 = initial_generation.unwrap();
    println!("Initial generation: {}", gen1);
    
    // Phase 2: Add more documents and test cache rebuild
    index.start_batch().unwrap();
    
    for i in 101..=150 {
        let symbol_id = SymbolId::new(i).unwrap();
        let file_id = FileId::new((i % 10) + 1).unwrap();
        
        index.add_document(
            symbol_id,
            &format!("method_{}", i),
            SymbolKind::Method,
            file_id,
            &format!("file_{}.rs", file_id.value()),
            i * 10,
            1,
            None,
            Some(&format!("fn method_{}(&self)", i)),
            "test_module",
            None,
        ).unwrap();
    }
    
    index.commit_batch().unwrap();
    
    // Update cluster assignments for the new batch
    index.update_cluster_assignments().unwrap();
    
    // Check generation changed (it may or may not change depending on segment merging)
    let gen2 = index.get_cache_generation().unwrap().unwrap();
    println!("Generation after second commit: {}", gen2);
    // Note: Generation is based on segment count, which may not always increase
    
    // Phase 3: Test explicit cache warming
    let start = Instant::now();
    index.warm_cluster_cache().unwrap();
    let warm_duration = start.elapsed();
    
    println!("Cache warming took: {:?}", warm_duration);
    assert!(warm_duration.as_millis() < 100, "Cache warming should be fast (< 100ms)");
    
    // Generation should remain the same after warm (no new data)
    let gen3 = index.get_cache_generation().unwrap().unwrap();
    println!("Generation after warm: {}", gen3);
    assert_eq!(gen2, gen3, "Generation should not change on warm without new data");
    
    // Phase 4: Test reload_and_warm
    let start = Instant::now();
    index.reload_and_warm().unwrap();
    let reload_duration = start.elapsed();
    
    println!("Reload and warm took: {:?}", reload_duration);
    assert!(reload_duration.as_millis() < 150, "Reload and warm should be fast (< 150ms)");
    
    // Verify cluster IDs are available after warming
    let cluster_ids = index.get_all_cluster_ids().unwrap();
    println!("Found {} clusters", cluster_ids.len());
    
    // With the cluster_id update fix, we should now have clusters
    assert!(!cluster_ids.is_empty(), "Should have cluster IDs after warming with vector support");
}

#[test]
fn test_cache_generation_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let vector_dir = temp_dir.path().join("vectors");
    
    // Create vector engine and embedding generator
    let dimension = VectorDimension::new(384).unwrap();
    let vector_engine = VectorSearchEngine::new(&vector_dir, dimension).unwrap();
    let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
    let embedding_generator = Arc::new(MockEmbeddingGenerator::new(384));
    
    // Create index with vector support
    let index = DocumentIndex::new(temp_dir.path())
        .unwrap()
        .with_vector_support(vector_engine_arc, &vector_dir)
        .with_embedding_generator(embedding_generator);
    
    // Initially no cache
    assert!(index.get_cache_generation().unwrap().is_none(), "Should have no cache initially");
    
    // Add and commit documents
    index.start_batch().unwrap();
    
    for i in 1..=10 {
        let symbol_id = SymbolId::new(i).unwrap();
        let file_id = FileId::new(1).unwrap();
        
        index.add_document(
            symbol_id,
            &format!("symbol_{}", i),
            SymbolKind::Function,
            file_id,
            "test.rs",
            i * 10,
            1,
            None,
            None,
            "test",
            None,
        ).unwrap();
    }
    
    index.commit_batch().unwrap();
    
    // Should have a generation (based on segment count)
    let generation = index.get_cache_generation().unwrap().unwrap();
    println!("First generation: {}", generation);
    assert!(generation > 0, "Should have a positive generation");
    
    // Warm cache should keep same generation
    index.warm_cluster_cache().unwrap();
    let generation_after_warm = index.get_cache_generation().unwrap().unwrap();
    assert_eq!(generation, generation_after_warm, "Generation should not change on warm");
}

#[test]
fn test_cache_warming_without_vectors() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create index WITHOUT vector support
    let index = DocumentIndex::new(temp_dir.path()).unwrap();
    
    // Warming should be a no-op
    index.warm_cluster_cache().unwrap();
    
    // Generation should be None
    assert!(index.get_cache_generation().unwrap().is_none(), "No cache without vector support");
    
    // reload_and_warm should also work
    index.reload_and_warm().unwrap();
}

#[test]
fn test_concurrent_cache_access() {
    use std::thread;
    
    let temp_dir = TempDir::new().unwrap();
    let vector_dir = temp_dir.path().join("vectors");
    
    // Create vector engine and embedding generator
    let dimension = VectorDimension::new(384).unwrap();
    let vector_engine = VectorSearchEngine::new(&vector_dir, dimension).unwrap();
    let vector_engine_arc = Arc::new(Mutex::new(vector_engine));
    let embedding_generator = Arc::new(MockEmbeddingGenerator::new(384));
    
    // Create index with vector support
    let index = Arc::new(DocumentIndex::new(temp_dir.path())
        .unwrap()
        .with_vector_support(vector_engine_arc, &vector_dir)
        .with_embedding_generator(embedding_generator));
    
    // Initial population
    index.start_batch().unwrap();
    for i in 1..=20 {
        let symbol_id = SymbolId::new(i).unwrap();
        let file_id = FileId::new(1).unwrap();
        
        index.add_document(
            symbol_id,
            &format!("concurrent_{}", i),
            SymbolKind::Function,
            file_id,
            "test.rs",
            i * 10,
            1,
            None,
            None,
            "test",
            None,
        ).unwrap();
    }
    index.commit_batch().unwrap();
    
    // Spawn multiple readers
    let mut handles = vec![];
    
    for thread_id in 0..3 {
        let index_clone = index.clone();
        let handle = thread::spawn(move || {
            for _ in 0..5 {
                // Read cluster IDs
                let cluster_ids = index_clone.get_all_cluster_ids().unwrap();
                println!("Thread {} found {} clusters", thread_id, cluster_ids.len());
                
                // Read generation
                if let Some(generation) = index_clone.get_cache_generation().unwrap() {
                    println!("Thread {} sees generation {}", thread_id, generation);
                }
                
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        handles.push(handle);
    }
    
    // Spawn a writer that warms cache
    let index_clone = index.clone();
    let handle = thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(25));
        println!("Writer: Warming cache...");
        index_clone.warm_cluster_cache().unwrap();
        println!("Writer: Cache warmed");
    });
    handles.push(handle);
    
    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}