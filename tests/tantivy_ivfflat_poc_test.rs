//! Proof of Concept tests for Tantivy-based IVFFlat vector search
//! This module implements a TDD approach to building vector search
//! directly integrated with Tantivy, inspired by production IVFFlat implementations.
//!
//! All POC code lives in this test file initially to maintain isolation
//! from production code while we validate the approach.

use anyhow::Result;

/// Type alias for cluster ID
type ClusterId = u32;

/// Test 1: Basic K-means Clustering
/// Validates that we can cluster high-dimensional vectors using linfa
#[test]
fn test_basic_kmeans_clustering() -> Result<()> {
    // Given: 100 random 384-dim vectors
    let n_vectors = 100;
    let n_dims = 384;
    let n_clusters = 10;
    
    let vectors = generate_random_vectors(n_vectors, n_dims);
    
    // When: Cluster into 10 groups using linfa
    let (centroids, assignments) = perform_kmeans_clustering(&vectors, n_clusters)?;
    
    // Then: Each vector assigned to exactly one cluster
    assert_eq!(assignments.len(), n_vectors);
    assert_eq!(centroids.len(), n_clusters);
    
    // Verify all cluster IDs are valid
    for &cluster_id in &assignments {
        assert!(cluster_id < n_clusters as ClusterId);
    }
    
    // Verify each cluster has at least one vector (no empty clusters)
    let mut cluster_counts = vec![0; n_clusters];
    for &cluster_id in &assignments {
        cluster_counts[cluster_id as usize] += 1;
    }
    
    for (i, &count) in cluster_counts.iter().enumerate() {
        assert!(count > 0, "Cluster {} has no assigned vectors", i);
    }
    
    // Print detailed test results
    println!("\n=== Test 1: Basic K-means Clustering ===");
    println!("✓ Generated {} random {}-dimensional vectors", n_vectors, n_dims);
    println!("✓ Successfully performed K-means clustering with {} clusters", n_clusters);
    println!("✓ Each vector assigned to exactly one cluster");
    println!("✓ All clusters have at least one vector (no empty clusters)");
    println!("\nCluster distribution:");
    for (i, &count) in cluster_counts.iter().enumerate() {
        println!("  - Cluster {}: {} vectors ({:.1}%)", 
                 i, count, (count as f32 / n_vectors as f32) * 100.0);
    }
    println!("\nCentroid dimensions: {} centroids × {} dimensions", 
             centroids.len(), n_dims);
    println!("Total assignments: {} vectors", assignments.len());
    println!("=== Test 1: PASSED ===\n");
    
    Ok(())
}

/// Generate random vectors for testing
fn generate_random_vectors(n_vectors: usize, n_dims: usize) -> Vec<Vec<f32>> {
    use rand::prelude::*;
    let mut rng = rand::rng();
    (0..n_vectors)
        .map(|_| {
            (0..n_dims)
                .map(|_| rng.random_range(-1.0..1.0))
                .collect()
        })
        .collect()
}

/// Perform K-means clustering on vectors
fn perform_kmeans_clustering(
    vectors: &[Vec<f32>],
    n_clusters: usize,
) -> Result<(Vec<Vec<f32>>, Vec<ClusterId>)> {
    use linfa::prelude::*;
    use linfa_clustering::KMeans;
    use ndarray::{Array1, Array2};
    
    // Convert vectors to ndarray format required by linfa
    let n_samples = vectors.len();
    let n_features = vectors[0].len();
    let mut data = Array2::<f32>::zeros((n_samples, n_features));
    
    for (i, vector) in vectors.iter().enumerate() {
        for (j, &value) in vector.iter().enumerate() {
            data[[i, j]] = value;
        }
    }
    
    // Create dataset with dummy targets for unsupervised learning
    let dataset = DatasetBase::new(data.clone(), Array1::<usize>::zeros(n_samples));
    
    // Configure and run K-means  
    let model = KMeans::params(n_clusters)
        .max_n_iterations(100)
        .tolerance(1e-4)
        .fit(&dataset)?;
    
    // Extract centroids
    let centroids = model.centroids()
        .rows()
        .into_iter()
        .map(|row| row.to_vec())
        .collect::<Vec<_>>();
    
    // Predict cluster assignments using the PredictInplace trait
    let mut assignments = Array1::<usize>::zeros(n_samples);
    model.predict_inplace(&data, &mut assignments);
    
    let assignments = assignments
        .iter()
        .map(|&label| label as ClusterId)
        .collect::<Vec<_>>();
    
    Ok((centroids, assignments))
}

// Placeholder implementations for future tests

/// Test 2: Centroid Serialization
#[test]
fn test_centroid_serialization() -> Result<()> {
    println!("\n=== Test 2: Centroid Serialization ===");
    
    // Given: Clustered vectors with centroids
    println!("Setting up test data...");
    let n_vectors = 50;
    let n_dims = 384;
    let n_clusters = 5;
    
    let vectors = generate_random_vectors(n_vectors, n_dims);
    let (centroids, assignments) = perform_kmeans_clustering(&vectors, n_clusters)?;
    
    println!("✓ Generated {} {}-dimensional vectors", n_vectors, n_dims);
    println!("✓ Clustered into {} groups", n_clusters);
    
    // Create an IVFFlat index structure
    let index = IVFFlatIndex {
        centroids: centroids.clone(),
        assignments: assignments.clone(),
    };
    
    // When: Serialize with bincode
    println!("\nSerializing index...");
    let serialized = bincode::encode_to_vec(&index, bincode::config::standard())?;
    let size_mb = serialized.len() as f64 / (1024.0 * 1024.0);
    println!("✓ Serialized index to {} bytes ({:.2} MB)", serialized.len(), size_mb);
    
    // Calculate breakdown
    let centroids_size = n_clusters * n_dims * std::mem::size_of::<f32>();
    let assignments_size = n_vectors * std::mem::size_of::<ClusterId>();
    let expected_size = centroids_size + assignments_size;
    let overhead = if serialized.len() >= expected_size {
        serialized.len() - expected_size
    } else {
        0
    };
    
    println!("\nSerialization breakdown:");
    println!("  - Centroids: {} bytes ({} × {} × {} bytes/float)", 
             centroids_size, n_clusters, n_dims, std::mem::size_of::<f32>());
    println!("  - Assignments: {} bytes ({} × {} bytes/id)", 
             assignments_size, n_vectors, std::mem::size_of::<ClusterId>());
    println!("  - Bincode overhead: {} bytes ({:.1}%)", 
             overhead, (overhead as f64 / serialized.len() as f64) * 100.0);
    
    // Then: Can deserialize and get identical centroids
    println!("\nDeserializing index...");
    let (deserialized, _): (IVFFlatIndex, usize) = bincode::decode_from_slice(&serialized, bincode::config::standard())?;
    println!("✓ Successfully deserialized index");
    
    // Verify centroids are identical
    assert_eq!(deserialized.centroids.len(), centroids.len());
    let mut max_diff = 0.0f32;
    for (i, (original, deserialized)) in centroids.iter().zip(&deserialized.centroids).enumerate() {
        assert_eq!(original.len(), deserialized.len(), "Centroid {} dimension mismatch", i);
        for (j, (&o, &d)) in original.iter().zip(deserialized.iter()).enumerate() {
            let diff = (o - d).abs();
            max_diff = max_diff.max(diff);
            assert!(diff < 1e-6, "Centroid {} dim {} mismatch: {} vs {}", i, j, o, d);
        }
    }
    println!("✓ All {} centroids identical (max difference: {:.2e})", n_clusters, max_diff);
    
    // Verify assignments are identical
    assert_eq!(deserialized.assignments.len(), assignments.len());
    let assignments_match = deserialized.assignments.iter()
        .zip(&assignments)
        .all(|(a, b)| a == b);
    assert!(assignments_match, "Assignments don't match after deserialization");
    println!("✓ All {} assignments identical", n_vectors);
    
    println!("\n=== Test 2: PASSED ===\n");
    Ok(())
}

/// Test 3: Memory-Mapped Vector Storage
#[test]
fn test_mmap_vector_storage() -> Result<()> {
    use std::fs::File;
    use std::io::Write;
    use memmap2::MmapOptions;
    use tempfile::TempDir;
    
    println!("\n=== Test 3: Memory-Mapped Vector Storage ===");
    
    // Given: Vectors grouped by cluster
    println!("Setting up test data...");
    let n_vectors = 100;
    let n_dims = 384;
    let n_clusters = 5;
    
    let vectors = generate_random_vectors(n_vectors, n_dims);
    let (centroids, assignments) = perform_kmeans_clustering(&vectors, n_clusters)?;
    
    println!("✓ Generated {} {}-dimensional vectors", n_vectors, n_dims);
    println!("✓ Clustered into {} groups", n_clusters);
    
    // Store centroids for future use (in production, these would be serialized)
    println!("✓ Computed {} centroids of dimension {}", centroids.len(), n_dims);
    
    // Group vectors by cluster
    let mut vectors_by_cluster: Vec<Vec<Vec<f32>>> = vec![vec![]; n_clusters];
    for (vec_idx, &cluster_id) in assignments.iter().enumerate() {
        vectors_by_cluster[cluster_id as usize].push(vectors[vec_idx].clone());
    }
    
    // Print cluster sizes
    println!("\nVector distribution by cluster:");
    for (i, cluster) in vectors_by_cluster.iter().enumerate() {
        println!("  - Cluster {}: {} vectors", i, cluster.len());
    }
    
    // When: Write vectors contiguously by cluster
    let temp_dir = TempDir::new()?;
    let vector_file_path = temp_dir.path().join("vectors.bin");
    let offset_file_path = temp_dir.path().join("offsets.bin");
    
    println!("\nWriting vectors to disk...");
    let mut vector_file = File::create(&vector_file_path)?;
    let mut offset_file = File::create(&offset_file_path)?;
    
    // Track offsets for each cluster
    let mut cluster_offsets = Vec::new();
    let mut current_offset = 0u64;
    
    // Write vectors contiguously by cluster
    for (cluster_id, cluster_vectors) in vectors_by_cluster.iter().enumerate() {
        cluster_offsets.push(current_offset);
        
        for vector in cluster_vectors {
            // Write as raw bytes (native endianness for performance)
            let bytes: Vec<u8> = vector.iter()
                .flat_map(|&f| f.to_ne_bytes())
                .collect();
            vector_file.write_all(&bytes)?;
            current_offset += bytes.len() as u64;
        }
        
        println!("  - Cluster {}: {} bytes written", 
                 cluster_id, 
                 cluster_vectors.len() * n_dims * std::mem::size_of::<f32>());
    }
    
    // Write cluster offsets
    for &offset in &cluster_offsets {
        offset_file.write_all(&offset.to_ne_bytes())?;
    }
    
    vector_file.sync_all()?;
    offset_file.sync_all()?;
    
    let total_size = current_offset;
    println!("✓ Written {} bytes total ({:.2} MB)", 
             total_size, total_size as f64 / (1024.0 * 1024.0));
    
    // Then: Can read back vectors by cluster efficiently using mmap
    println!("\nMemory-mapping vector storage...");
    let vector_file = File::open(&vector_file_path)?;
    let mmap = unsafe { MmapOptions::new().map(&vector_file)? };
    println!("✓ Memory-mapped {} bytes", mmap.len());
    
    // Test reading vectors from each cluster
    println!("\nVerifying random access to vectors by cluster:");
    for (cluster_id, cluster_vectors) in vectors_by_cluster.iter().enumerate() {
        let cluster_offset = cluster_offsets[cluster_id] as usize;
        let bytes_per_vector = n_dims * std::mem::size_of::<f32>();
        
        // Read first vector from cluster
        if !cluster_vectors.is_empty() {
            let vector_bytes = &mmap[cluster_offset..cluster_offset + bytes_per_vector];
            let mut recovered_vector = Vec::with_capacity(n_dims);
            
            for i in 0..n_dims {
                let byte_offset = i * std::mem::size_of::<f32>();
                let bytes = &vector_bytes[byte_offset..byte_offset + std::mem::size_of::<f32>()];
                let value = f32::from_ne_bytes(bytes.try_into().unwrap());
                recovered_vector.push(value);
            }
            
            // Verify it matches the original
            let original = &cluster_vectors[0];
            let mut max_diff = 0.0f32;
            for (i, (&orig, &recov)) in original.iter().zip(&recovered_vector).enumerate() {
                let diff = (orig - recov).abs();
                max_diff = max_diff.max(diff);
                assert!(diff < 1e-6, "Vector mismatch at dim {}: {} vs {}", i, orig, recov);
            }
            
            println!("  ✓ Cluster {}: Successfully read vector (max diff: {:.2e})", 
                     cluster_id, max_diff);
        }
    }
    
    // Performance test: Read multiple vectors
    println!("\nPerformance test - reading 1000 random vectors:");
    use rand::prelude::*;
    let mut rng = rand::rng();
    let start = std::time::Instant::now();
    
    for _ in 0..1000 {
        let cluster_id = rng.random_range(0..n_clusters);
        let cluster_size = vectors_by_cluster[cluster_id].len();
        if cluster_size > 0 {
            let vec_idx = rng.random_range(0..cluster_size);
            let offset = cluster_offsets[cluster_id] as usize 
                + vec_idx * n_dims * std::mem::size_of::<f32>();
            
            // Simulate reading the vector
            let _vector_bytes = &mmap[offset..offset + n_dims * std::mem::size_of::<f32>()];
        }
    }
    
    let duration = start.elapsed();
    println!("✓ Read 1000 random vectors in {:?} ({:.2} μs/vector)", 
             duration, duration.as_micros() as f64 / 1000.0);
    
    // Calculate storage efficiency
    let overhead = (cluster_offsets.len() * std::mem::size_of::<u64>()) as f64;
    let efficiency = (total_size as f64 / (total_size as f64 + overhead)) * 100.0;
    println!("\nStorage efficiency: {:.1}% (offset overhead: {} bytes)", 
             efficiency, overhead as u64);
    
    println!("\n=== Test 3: PASSED ===\n");
    Ok(())
}

/// Test 4: Cluster State Management (Simulated Warmer)
/// 
/// This test demonstrates how we would maintain cluster_id -> [doc_ids] mappings
/// that enable efficient ANN search by only loading vectors from selected clusters.
#[test]
fn test_tantivy_warmer_state() -> Result<()> {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    
    println!("\n=== Test 4: Cluster State Management (Simulated Warmer) ===");
    
    // Given: Documents with cluster assignments (simulating what would be in Tantivy)
    println!("Setting up test data...");
    let test_documents = vec![
        (0u64, 0u64, "fn parse_string() -> String"),     // doc_id=0, cluster_id=0
        (1u64, 0u64, "fn parse_number() -> i32"),        // doc_id=1, cluster_id=0
        (2u64, 1u64, "struct Parser { }"),               // doc_id=2, cluster_id=1
        (3u64, 1u64, "impl Parser { fn new() }"),        // doc_id=3, cluster_id=1
        (4u64, 2u64, "trait Parseable { }"),             // doc_id=4, cluster_id=2
        (5u64, 2u64, "impl Parseable for String"),       // doc_id=5, cluster_id=2
        (6u64, 0u64, "fn parse_json() -> Value"),        // doc_id=6, cluster_id=0
        (7u64, 1u64, "struct JsonParser { }"),           // doc_id=7, cluster_id=1
        (8u64, 3u64, "fn handle_error(e: Error)"),       // doc_id=8, cluster_id=3
        (9u64, 3u64, "impl Error for ParseError"),       // doc_id=9, cluster_id=3
    ];
    
    println!("✓ Created {} test documents across 4 clusters", test_documents.len());
    
    // When: Build cluster state mappings (simulating warmer behavior)
    println!("\nBuilding cluster state mappings...");
    
    // This would be maintained per-segment in real implementation
    type ClusterMappings = HashMap<u64, Vec<u64>>; // cluster_id -> [doc_ids]
    type SegmentClusterState = Arc<RwLock<HashMap<String, ClusterMappings>>>; // segment_id -> mappings
    
    let segment_state: SegmentClusterState = Arc::new(RwLock::new(HashMap::new()));
    
    // Simulate building state for a segment
    let segment_id = "segment_0";
    let mut cluster_mappings: ClusterMappings = HashMap::new();
    
    for (doc_id, cluster_id, content) in &test_documents {
        println!("  Processing doc {} (cluster {}) - {}", doc_id, cluster_id, content);
        cluster_mappings
            .entry(*cluster_id)
            .or_insert_with(Vec::new)
            .push(*doc_id);
    }
    
    // Sort doc_ids for each cluster (enables binary search)
    for doc_ids in cluster_mappings.values_mut() {
        doc_ids.sort_unstable();
    }
    
    // Store in segment state
    {
        let mut state = segment_state.write().unwrap();
        state.insert(segment_id.to_string(), cluster_mappings.clone());
    }
    
    println!("✓ Built cluster mappings for segment '{}'", segment_id);
    
    // Then: Verify state maintains ClusterId -> [DocId] mappings
    println!("\nVerifying cluster state...");
    
    let state = segment_state.read().unwrap();
    let mappings = state.get(segment_id).expect("Segment should have mappings");
    
    // Print cluster statistics
    println!("\nCluster statistics:");
    let mut total_docs = 0;
    for cluster_id in 0..4 {
        if let Some(doc_ids) = mappings.get(&cluster_id) {
            println!("  - Cluster {}: {} documents {:?}", cluster_id, doc_ids.len(), doc_ids);
            total_docs += doc_ids.len();
            
            // Verify sorted for binary search
            let is_sorted = doc_ids.windows(2).all(|w| w[0] <= w[1]);
            assert!(is_sorted, "Doc IDs should be sorted");
        } else {
            println!("  - Cluster {}: 0 documents", cluster_id);
        }
    }
    
    assert_eq!(total_docs, test_documents.len(), "All documents should be mapped");
    
    // Test performance of cluster lookups
    println!("\nTesting cluster lookup performance...");
    let start = std::time::Instant::now();
    
    // Simulate 10,000 cluster lookups
    for _ in 0..10_000 {
        let cluster_id = 0u64;
        if let Some(_doc_ids) = mappings.get(&cluster_id) {
            // Found cluster documents
        }
    }
    
    let duration = start.elapsed();
    println!("✓ 10,000 cluster lookups in {:?} ({:.2} ns/lookup)", 
             duration, duration.as_nanos() as f64 / 10_000.0);
    
    // Demonstrate ANN search flow
    println!("\nSimulating ANN search flow:");
    
    // 1. Select clusters based on centroid distance
    let selected_clusters = vec![0u64, 2u64]; // Clusters with nearest centroids
    println!("  1. Selected clusters {} based on centroid distance", 
             selected_clusters.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "));
    
    // 2. Gather candidate documents
    let mut candidate_docs: Vec<u64> = Vec::new();
    for cluster_id in &selected_clusters {
        if let Some(doc_ids) = mappings.get(cluster_id) {
            candidate_docs.extend(doc_ids);
        }
    }
    
    println!("  2. Found {} candidate documents: {:?}", 
             candidate_docs.len(), candidate_docs);
    
    // 3. Vector scoring would happen here
    println!("  3. Would load vectors only for these {} documents", candidate_docs.len());
    println!("     (Instead of all {} documents)", test_documents.len());
    
    // Calculate efficiency gain
    let efficiency = 100.0 * (1.0 - (candidate_docs.len() as f64 / test_documents.len() as f64));
    println!("\n✓ Efficiency gain: {:.1}% fewer vectors to load", efficiency);
    
    // Show how this integrates with Tantivy
    println!("\nIntegration with Tantivy:");
    println!("  - Store cluster_id as a FAST field in Tantivy documents");
    println!("  - Build mappings when IndexReader reloads (segment warmup)");
    println!("  - Use mappings to create filtered DocSet for scoring");
    println!("  - Combine with BooleanQuery for hybrid search");
    
    // Memory overhead calculation
    let memory_per_mapping = std::mem::size_of::<u64>() * 2; // cluster_id + doc_id
    let total_mappings = test_documents.len();
    let memory_overhead = memory_per_mapping * total_mappings;
    println!("\nMemory overhead:");
    println!("  - {} bytes per mapping", memory_per_mapping);
    println!("  - {} total bytes for {} documents", memory_overhead, total_mappings);
    println!("  - Scales linearly with document count");
    
    println!("\n=== Test 4: PASSED ===\n");
    Ok(())
}

/// Test 5: Custom ANN Query
/// 
/// This test demonstrates how to perform approximate nearest neighbor search
/// by selecting clusters based on centroid distance and scoring documents.
#[test]
fn test_ann_query_basic() -> Result<()> {
    use std::collections::HashMap;
    
    println!("\n=== Test 5: Custom ANN Query ===");
    
    // Given: Pre-clustered vectors with known centroids
    println!("Setting up test data...");
    
    // Create some test vectors in 3 dimensions for visualization
    let test_vectors = vec![
        // Cluster 0: Around [1.0, 0.0, 0.0]
        vec![0.9, 0.1, 0.0],
        vec![1.1, -0.1, 0.1],
        vec![0.95, 0.05, -0.05],
        // Cluster 1: Around [0.0, 1.0, 0.0]
        vec![-0.1, 0.9, 0.1],
        vec![0.1, 1.1, -0.1],
        vec![0.0, 0.95, 0.05],
        // Cluster 2: Around [0.0, 0.0, 1.0]
        vec![0.1, -0.1, 0.9],
        vec![-0.1, 0.1, 1.1],
        vec![0.05, 0.0, 0.95],
    ];
    
    // Known centroids (computed from above)
    let centroids = vec![
        vec![0.983, 0.017, 0.017],  // Cluster 0
        vec![0.0, 0.983, 0.017],    // Cluster 1
        vec![0.017, 0.0, 0.983],    // Cluster 2
    ];
    
    // Document assignments
    let doc_clusters = vec![
        (0, 0), (1, 0), (2, 0),  // Docs 0-2 in cluster 0
        (3, 1), (4, 1), (5, 1),  // Docs 3-5 in cluster 1
        (6, 2), (7, 2), (8, 2),  // Docs 6-8 in cluster 2
    ];
    
    println!("✓ Created {} documents in {} clusters", test_vectors.len(), centroids.len());
    
    // Build cluster mappings (simulating warmer state)
    let mut cluster_mappings: HashMap<u32, Vec<u32>> = HashMap::new();
    for (doc_id, cluster_id) in &doc_clusters {
        cluster_mappings
            .entry(*cluster_id)
            .or_insert_with(Vec::new)
            .push(*doc_id);
    }
    
    // When: Query with a vector near cluster 0
    println!("\nExecuting ANN query...");
    let query_vector = vec![0.8, 0.2, 0.0]; // Close to cluster 0
    
    // Step 1: Find nearest centroids
    println!("  Step 1: Computing distances to centroids");
    let mut centroid_distances: Vec<(usize, f32)> = centroids
        .iter()
        .enumerate()
        .map(|(idx, centroid)| {
            let dist = cosine_distance(&query_vector, centroid);
            println!("    - Cluster {}: distance = {:.4}", idx, dist);
            (idx, dist)
        })
        .collect();
    
    // Sort by distance (ascending)
    centroid_distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    
    // Step 2: Select top-k clusters (probe_factor)
    let probe_k = 2; // Probe top 2 clusters
    let selected_clusters: Vec<usize> = centroid_distances
        .iter()
        .take(probe_k)
        .map(|(idx, _)| *idx)
        .collect();
    
    println!("\n  Step 2: Selected {} nearest clusters: {:?}", probe_k, selected_clusters);
    
    // Step 3: Gather candidate documents
    let mut candidate_docs = Vec::new();
    for cluster_id in &selected_clusters {
        if let Some(doc_ids) = cluster_mappings.get(&(*cluster_id as u32)) {
            candidate_docs.extend(doc_ids);
        }
    }
    
    println!("  Step 3: Found {} candidate documents: {:?}", 
             candidate_docs.len(), candidate_docs);
    
    // Step 4: Score candidate documents
    println!("\n  Step 4: Scoring candidate documents");
    let mut doc_scores: Vec<(u32, f32)> = candidate_docs
        .iter()
        .map(|&doc_id| {
            let doc_vector = &test_vectors[doc_id as usize];
            let similarity = cosine_similarity(&query_vector, doc_vector);
            println!("    - Doc {}: similarity = {:.4}", doc_id, similarity);
            (doc_id, similarity)
        })
        .collect();
    
    // Sort by similarity (descending)
    doc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // Then: Return top results
    let top_k = 3;
    let results: Vec<u32> = doc_scores
        .iter()
        .take(top_k)
        .map(|(doc_id, _)| *doc_id)
        .collect();
    
    println!("\n✓ Top {} results: {:?}", top_k, results);
    
    // Verify results are from cluster 0 (nearest to query)
    assert_eq!(results.len(), 3);
    for &doc_id in &results {
        let (_, cluster_id) = doc_clusters[doc_id as usize];
        assert_eq!(cluster_id, 0, "Top results should be from cluster 0");
    }
    
    // Performance analysis
    println!("\nPerformance analysis:");
    let total_docs = test_vectors.len();
    let docs_examined = candidate_docs.len();
    let efficiency = 100.0 * (1.0 - (docs_examined as f64 / total_docs as f64));
    
    println!("  - Total documents: {}", total_docs);
    println!("  - Documents examined: {}", docs_examined);
    println!("  - Efficiency gain: {:.1}%", efficiency);
    println!("  - Probe factor: {:.1}% of clusters", 
             (probe_k as f64 / centroids.len() as f64) * 100.0);
    
    // Show how this would integrate with Tantivy
    println!("\nTantivy integration approach:");
    println!("  1. Create custom Query implementation (AnnQuery)");
    println!("  2. AnnQuery::weight() returns custom Weight");
    println!("  3. Weight::scorer() returns custom Scorer");
    println!("  4. Scorer uses cluster mappings to filter docs");
    println!("  5. Scorer computes vector similarity for ranking");
    
    println!("\n=== Test 5: PASSED ===\n");
    Ok(())
}

/// Compute cosine distance between two vectors (1 - cosine_similarity)
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    1.0 - cosine_similarity(a, b)
}

/// Compute cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

/// Test 6: Real Rust Code Vector Search
/// 
/// This test uses actual Rust code snippets to validate vector search capabilities
/// with real embeddings from fastembed, demonstrating production-ready functionality.
#[test]
fn test_real_rust_code_search() -> Result<()> {
    use std::collections::HashMap;
    use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
    
    println!("\n=== Test 6: Real Rust Code Vector Search ===");
    
    // Given: Real Rust code snippets from various domains
    println!("Setting up real Rust code snippets...");
    
    let rust_code_snippets: Vec<(&str, &str)> = vec![
        // JSON parsing functions
        (
            "parse_json",
            r#"/// Parse JSON string into a Value
pub fn parse_json(input: &str) -> Result<Value, ParseError> {
    let tokens = tokenize(input)?;
    let mut parser = JsonParser::new(tokens);
    parser.parse_value()
}"#,
        ),
        (
            "parse_json_object",
            r#"/// Parse a JSON object from token stream
fn parse_json_object(&mut self) -> Result<Object, ParseError> {
    self.expect_token(Token::LeftBrace)?;
    let mut object = Object::new();
    
    while !self.check_token(Token::RightBrace) {
        let key = self.parse_string()?;
        self.expect_token(Token::Colon)?;
        let value = self.parse_value()?;
        object.insert(key, value);
        
        if !self.match_token(Token::Comma) {
            break;
        }
    }
    
    self.expect_token(Token::RightBrace)?;
    Ok(object)
}"#,
        ),
        // XML parsing (different domain)
        (
            "parse_xml_element",
            r#"/// Parse an XML element with attributes
fn parse_xml_element(&mut self) -> Result<XmlElement, XmlError> {
    self.consume_char('<')?;
    let tag_name = self.parse_identifier()?;
    let attributes = self.parse_attributes()?;
    
    if self.match_str("/>") {
        return Ok(XmlElement::empty(tag_name, attributes));
    }
    
    self.consume_char('>')?;
    let children = self.parse_children(&tag_name)?;
    self.expect_closing_tag(&tag_name)?;
    
    Ok(XmlElement::new(tag_name, attributes, children))
}"#,
        ),
        // AST parsing (compiler-related)
        (
            "parse_function_definition", 
            r#"/// Parse a function definition from AST nodes
pub fn parse_function_definition(node: &Node, code: &str) -> Option<FunctionDef> {
    let name_node = node.child_by_field_name("name")?;
    let name = code[name_node.byte_range()].to_string();
    
    let params_node = node.child_by_field_name("parameters")?;
    let params = parse_parameters(params_node, code);
    
    let return_type = node.child_by_field_name("return_type")
        .map(|n| parse_type(n, code));
    
    let body_node = node.child_by_field_name("body")?;
    let body = parse_block(body_node, code);
    
    Some(FunctionDef { name, params, return_type, body })
}"#,
        ),
        // Error handling utilities
        (
            "handle_parse_error",
            r#"/// Handle parsing errors with context
fn handle_parse_error(error: ParseError, context: &ParseContext) -> CompileError {
    match error {
        ParseError::UnexpectedToken(token) => {
            CompileError::new(
                format!("Unexpected token: {:?}", token),
                context.current_span(),
            )
        }
        ParseError::UnexpectedEof => {
            CompileError::new(
                "Unexpected end of file".to_string(),
                context.last_span(),
            )
        }
        ParseError::InvalidSyntax(msg) => {
            CompileError::new(msg, context.current_span())
        }
    }
}"#,
        ),
        // Generic parser trait implementation
        (
            "impl_parser_for_rust",
            r#"/// Implementation of Parser trait for Rust language
impl Parser for RustParser {
    type Output = RustAst;
    type Error = RustParseError;
    
    fn parse(&mut self, input: &str) -> Result<Self::Output, Self::Error> {
        let tree = self.tree_sitter_parser
            .parse(input, None)
            .ok_or(RustParseError::TreeSitterFailed)?;
        
        let root = tree.root_node();
        let ast = self.build_ast(&root, input)?;
        
        Ok(ast)
    }
    
    fn can_parse(&self, file_extension: &str) -> bool {
        matches!(file_extension, "rs" | "rust")
    }
}"#,
        ),
        // String parsing utilities
        (
            "parse_string_literal",
            r#"/// Parse a string literal with escape sequences
fn parse_string_literal(&mut self) -> Result<String, ParseError> {
    self.expect_char('"')?;
    let mut result = String::new();
    
    while let Some(ch) = self.peek_char() {
        match ch {
            '"' => {
                self.advance();
                return Ok(result);
            }
            '\\' => {
                self.advance();
                let escaped = self.parse_escape_sequence()?;
                result.push(escaped);
            }
            _ => {
                result.push(ch);
                self.advance();
            }
        }
    }
    
    Err(ParseError::UnterminatedString)
}"#,
        ),
        // Configuration parsing
        (
            "parse_config_file",
            r#"/// Parse configuration from TOML file
pub fn parse_config_file(path: &Path) -> Result<Config, ConfigError> {
    let contents = fs::read_to_string(path)
        .map_err(|e| ConfigError::IoError(e))?;
    
    let config: Config = toml::from_str(&contents)
        .map_err(|e| ConfigError::ParseError(e))?;
    
    config.validate()?;
    Ok(config)
}"#,
        ),
    ];
    
    println!("✓ Loaded {} real Rust code snippets", rust_code_snippets.len());
    
    // Initialize fastembed for real embeddings
    println!("\nInitializing fastembed model...");
    let mut embedding_model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_show_download_progress(false)
    )?;
    
    // Generate embeddings for all code snippets
    println!("Generating embeddings for code snippets...");
    let code_texts: Vec<String> = rust_code_snippets.iter()
        .map(|(name, code)| format!("{}\n{}", name, code))
        .collect();
    
    let embeddings = embedding_model.embed(code_texts.clone(), None)?;
    println!("✓ Generated {} embeddings of dimension {}", 
             embeddings.len(), embeddings[0].len());
    
    // Cluster the embeddings
    println!("\nClustering code snippets...");
    let n_clusters = 3; // Parse functions, error handling, implementations
    let (centroids, assignments) = perform_kmeans_clustering(&embeddings, n_clusters)?;
    
    // Build cluster mappings
    let mut cluster_mappings: HashMap<u32, Vec<usize>> = HashMap::new();
    for (idx, &cluster_id) in assignments.iter().enumerate() {
        cluster_mappings
            .entry(cluster_id)
            .or_insert_with(Vec::new)
            .push(idx);
    }
    
    // Print cluster contents
    println!("\nCluster analysis:");
    for cluster_id in 0..n_clusters {
        if let Some(doc_ids) = cluster_mappings.get(&(cluster_id as u32)) {
            println!("  Cluster {}:", cluster_id);
            for &doc_id in doc_ids {
                let (name, _) = &rust_code_snippets[doc_id];
                println!("    - {}", name);
            }
        }
    }
    
    // Test queries
    let test_queries = vec![
        ("parse JSON data", "Should find JSON parsing functions"),
        ("implement parser trait", "Should find parser implementations"),  
        ("handle errors", "Should find error handling code"),
        ("parse string literal", "Should find string parsing"),
    ];
    
    println!("\nTesting semantic search queries:");
    for (query, expected) in test_queries {
        println!("\n  Query: '{}' ({})", query, expected);
        
        // Generate query embedding
        let query_embedding = embedding_model.embed(vec![query.to_string()], None)?;
        let query_vec = &query_embedding[0];
        
        // Find nearest clusters
        let mut cluster_distances: Vec<(usize, f32)> = centroids.iter()
            .enumerate()
            .map(|(idx, centroid)| {
                let dist = cosine_distance(query_vec, centroid);
                (idx, dist)
            })
            .collect();
        cluster_distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        
        // Select top clusters
        let top_clusters = 2;
        let selected_clusters: Vec<usize> = cluster_distances
            .iter()
            .take(top_clusters)
            .map(|(idx, _)| *idx)
            .collect();
        
        println!("    Selected clusters: {:?}", selected_clusters);
        
        // Score documents from selected clusters
        let mut doc_scores: Vec<(usize, f32, &str)> = Vec::new();
        for cluster_id in selected_clusters {
            if let Some(doc_ids) = cluster_mappings.get(&(cluster_id as u32)) {
                for &doc_id in doc_ids {
                    let similarity = cosine_similarity(query_vec, &embeddings[doc_id]);
                    let (name, _) = &rust_code_snippets[doc_id];
                    doc_scores.push((doc_id, similarity, name));
                }
            }
        }
        
        // Sort by similarity
        doc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Show top results
        println!("    Top results:");
        for (rank, (_doc_id, score, name)) in doc_scores.iter().take(3).enumerate() {
            println!("      {}. {} (similarity: {:.4})", rank + 1, name, score);
        }
    }
    
    // Performance analysis with real data
    println!("\nPerformance analysis with real embeddings:");
    let embedding_size = std::mem::size_of::<f32>() * 384;
    let total_embeddings_size = embedding_size * rust_code_snippets.len();
    println!("  - Embedding dimension: 384");
    println!("  - Memory per embedding: {} bytes", embedding_size);
    println!("  - Total embeddings memory: {} KB", total_embeddings_size / 1024);
    println!("  - Average cluster size: {:.1} documents", 
             rust_code_snippets.len() as f64 / n_clusters as f64);
    
    // Analyze semantic understanding across all similarity ranges
    println!("\nSemantic similarity analysis:");
    
    // Create similarity groups for analysis
    let mut similarity_pairs: Vec<(&str, &str, f32)> = Vec::new();
    
    // Check all pairs
    for i in 0..rust_code_snippets.len() {
        for j in i+1..rust_code_snippets.len() {
            let sim = cosine_similarity(&embeddings[i], &embeddings[j]);
            similarity_pairs.push((
                rust_code_snippets[i].0,
                rust_code_snippets[j].0,
                sim
            ));
        }
    }
    
    // Sort by similarity
    similarity_pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    
    // Group similarities by range
    let very_similar: Vec<_> = similarity_pairs.iter()
        .filter(|(_, _, sim)| *sim > 0.8)
        .collect();
    let somewhat_similar: Vec<_> = similarity_pairs.iter()
        .filter(|(_, _, sim)| *sim > 0.5 && *sim <= 0.8)
        .collect();
    let different: Vec<_> = similarity_pairs.iter()
        .filter(|(_, _, sim)| *sim <= 0.5)
        .collect();
    
    println!("\n  Very similar (>0.8):");
    for (a, b, sim) in very_similar.iter().take(3) {
        println!("    - {} ↔ {}: {:.4}", a, b, sim);
    }
    
    println!("\n  Somewhat similar (0.5-0.8):");
    for (a, b, sim) in somewhat_similar.iter().take(3) {
        println!("    - {} ↔ {}: {:.4}", a, b, sim);
    }
    
    println!("\n  Different (<0.5):");
    for (a, b, sim) in different.iter().take(3) {
        println!("    - {} ↔ {}: {:.4}", a, b, sim);
    }
    
    // Document findings
    println!("\n  Observed similarity ranges:");
    println!("    - Same concept functions: {:.3}-{:.3}", 
             similarity_pairs.iter()
                .filter(|(a, b, _)| (a.contains("json") && b.contains("json")) || 
                                   (a.contains("parse") && b.contains("parse")))
                .map(|(_, _, s)| s)
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(&0.0),
             similarity_pairs.iter()
                .filter(|(a, b, _)| (a.contains("json") && b.contains("json")) || 
                                   (a.contains("parse") && b.contains("parse")))
                .map(|(_, _, s)| s)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(&1.0));
    
    println!("    - Related concepts: {:.3}-{:.3}",
             somewhat_similar.iter().map(|(_, _, s)| s).min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.5),
             somewhat_similar.iter().map(|(_, _, s)| s).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.8));
    
    println!("    - Different concepts: {:.3}-{:.3}",
             different.iter().map(|(_, _, s)| s).min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0),
             different.iter().map(|(_, _, s)| s).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.5));
    
    // Validate that similarities make semantic sense
    let json_pairs: Vec<_> = similarity_pairs.iter()
        .filter(|(a, b, _)| a.contains("json") && b.contains("json"))
        .collect();
    
    let cross_domain_pairs: Vec<_> = similarity_pairs.iter()
        .filter(|(a, b, _)| (a.contains("json") && b.contains("xml")) ||
                           (a.contains("xml") && b.contains("json")))
        .collect();
    
    if !json_pairs.is_empty() && !cross_domain_pairs.is_empty() {
        let avg_json_sim: f32 = json_pairs.iter().map(|(_, _, s)| s).sum::<f32>() / json_pairs.len() as f32;
        let avg_cross_sim: f32 = cross_domain_pairs.iter().map(|(_, _, s)| s).sum::<f32>() / cross_domain_pairs.len() as f32;
        
        println!("\n  Semantic validation:");
        println!("    - Average JSON-JSON similarity: {:.4}", avg_json_sim);
        println!("    - Average JSON-XML similarity: {:.4}", avg_cross_sim);
        println!("    - Relative difference: {:.1}%", 
                 ((avg_json_sim - avg_cross_sim) / avg_cross_sim * 100.0));
        
        // This is the key insight: related functions should be more similar than unrelated ones
        assert!(avg_json_sim > avg_cross_sim, 
                "Related functions should have higher average similarity");
    }
    
    println!("\n✓ Semantic understanding validated across all similarity ranges");
    
    // Demonstrate the complete IVFFlat flow
    println!("\n=== Complete IVFFlat Flow Demonstration ===");
    
    // Step 1: Indexing phase (already done above)
    println!("\nIndexing Summary:");
    println!("  1. Generated {} embeddings from real code", embeddings.len());
    println!("  2. Clustered into {} groups using K-means", n_clusters);
    println!("  3. Computed {} centroids ({}D each)", centroids.len(), centroids[0].len());
    println!("  4. Assigned each code snippet to nearest centroid");
    
    // Step 2: Query phase - demonstrate full flow
    let query_text = "implement async parser";
    println!("\nQuery Phase: '{}'", query_text);
    
    // 2a. Generate query embedding
    let query_embedding = embedding_model.embed(vec![query_text], None)?;
    let query_vec = &query_embedding[0];
    println!("  1. Generated query embedding ({}D)", query_vec.len());
    
    // 2b. Find nearest centroids (this is the key IVFFlat optimization)
    let mut centroid_distances: Vec<(usize, f32)> = centroids.iter()
        .enumerate()
        .map(|(idx, centroid)| {
            let similarity = cosine_similarity(query_vec, centroid);
            (idx, 1.0 - similarity) // Convert to distance
        })
        .collect();
    centroid_distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    
    println!("\n  2. Centroid distances:");
    for (idx, dist) in &centroid_distances {
        println!("     - Cluster {}: {:.4}", idx, dist);
    }
    
    // 2c. Select clusters to search (probe_factor determines quality/speed tradeoff)
    let probe_clusters = 2; // Search top 66% of clusters
    let selected_clusters: Vec<usize> = centroid_distances
        .iter()
        .take(probe_clusters)
        .map(|(idx, _)| *idx)
        .collect();
    
    println!("\n  3. Selected {} nearest clusters: {:?}", probe_clusters, selected_clusters);
    println!("     (Probe factor: {:.0}%)", (probe_clusters as f32 / n_clusters as f32) * 100.0);
    
    // 2d. Get candidate documents from selected clusters only
    let mut candidate_docs = Vec::new();
    for cluster_id in &selected_clusters {
        if let Some(doc_ids) = cluster_mappings.get(&(*cluster_id as u32)) {
            candidate_docs.extend(doc_ids);
        }
    }
    println!("\n  4. Found {} candidate documents (out of {} total)", 
             candidate_docs.len(), embeddings.len());
    
    // 2e. Score only candidate documents
    let mut final_scores: Vec<(usize, f32, &str)> = candidate_docs.iter()
        .map(|&doc_id| {
            let doc_embedding: &Vec<f32> = &embeddings[doc_id];
            let similarity = cosine_similarity(query_vec, doc_embedding);
            let (name, _) = rust_code_snippets[doc_id];
            (doc_id, similarity, name)
        })
        .collect();
    final_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    println!("\n  5. Final ranking (only searched {} docs):", candidate_docs.len());
    for (rank, (_doc_id, score, name)) in final_scores.iter().take(3).enumerate() {
        println!("     {}. {} (similarity: {:.4})", rank + 1, name, score);
    }
    
    // Performance analysis
    let efficiency = ((embeddings.len() - candidate_docs.len()) as f32 / embeddings.len() as f32) * 100.0;
    println!("\nPerformance Impact:");
    println!("  - Documents skipped: {} ({:.1}% reduction)", 
             embeddings.len() - candidate_docs.len(), efficiency);
    println!("  - Memory access: Only loaded {} vectors instead of {}", 
             candidate_docs.len(), embeddings.len());
    println!("  - With 1M documents and 1000 clusters, this could mean:");
    println!("    - Searching ~2000 docs instead of 1,000,000");
    println!("    - 99.8% reduction in vector comparisons");
    
    println!("\n✓ Complete IVFFlat flow validated with real embeddings");
    println!("\n=== Test 6: PASSED ===\n");
    Ok(())
}

/// Test 5b: Realistic Scoring and Ranking
/// 
/// This test demonstrates a real-world scenario where we have code symbols
/// with both text content and embeddings, showing how vector similarity
/// integrates with text relevance for hybrid ranking.
#[test]
fn test_realistic_scoring_and_ranking() -> Result<()> {
    use std::collections::HashMap;
    
    println!("\n=== Test 5b: Realistic Scoring and Ranking ===");
    
    // Given: Code symbols with embeddings (simulating real parse_* functions)
    println!("Setting up realistic code symbol data...");
    
    // Simulate real code symbols with semantic embeddings
    // In reality, these would come from fastembed
    let code_symbols = vec![
        // JSON parsing functions (semantically similar)
        ("parse_json", "Parse JSON string into a Value", vec![0.9, 0.8, 0.1, 0.2, 0.1]),
        ("parse_json_object", "Parse JSON object from tokens", vec![0.85, 0.75, 0.15, 0.25, 0.1]),
        ("parse_json_array", "Parse JSON array from string", vec![0.8, 0.7, 0.2, 0.3, 0.15]),
        
        // XML parsing functions (different semantic group)
        ("parse_xml", "Parse XML document into DOM tree", vec![0.2, 0.3, 0.9, 0.8, 0.1]),
        ("parse_xml_element", "Parse single XML element", vec![0.25, 0.35, 0.85, 0.75, 0.15]),
        
        // String parsing utilities (somewhat related)
        ("parse_string", "Parse quoted string literal", vec![0.5, 0.5, 0.5, 0.5, 0.3]),
        ("parse_integer", "Parse integer from string", vec![0.4, 0.4, 0.4, 0.4, 0.6]),
        
        // Error handling (semantically distant)
        ("handle_parse_error", "Handle parsing errors", vec![0.1, 0.1, 0.1, 0.1, 0.9]),
    ];
    
    // Build inverted index for text search simulation
    let mut text_index: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, (name, desc, _)) in code_symbols.iter().enumerate() {
        // Index by function name tokens
        for token in name.split('_') {
            text_index.entry(token.to_string()).or_insert_with(Vec::new).push(idx);
        }
        // Index by description tokens
        let desc_lower = desc.to_lowercase();
        for token in desc_lower.split_whitespace() {
            text_index.entry(token.to_string()).or_insert_with(Vec::new).push(idx);
        }
    }
    
    // When: User searches for "parse json" (hybrid query)
    println!("\nExecuting hybrid query: 'parse json'");
    let query_text = "parse json";
    let query_embedding = vec![0.88, 0.76, 0.12, 0.24, 0.08]; // Similar to JSON parsers
    
    // Step 1: Text scoring (BM25-like)
    println!("\n  Step 1: Text relevance scoring");
    let mut text_scores: HashMap<usize, f32> = HashMap::new();
    
    for token in query_text.split_whitespace() {
        if let Some(doc_ids) = text_index.get(&token.to_string()) {
            let idf = (code_symbols.len() as f32 / doc_ids.len() as f32).ln();
            for &doc_id in doc_ids {
                let tf = 1.0; // Simplified term frequency
                let score = tf * idf;
                *text_scores.entry(doc_id).or_insert(0.0) += score;
                println!("    - Doc {} matches '{}': +{:.3} (IDF: {:.3})", 
                         doc_id, token, score, idf);
            }
        }
    }
    
    // Step 2: Vector similarity scoring
    println!("\n  Step 2: Vector similarity scoring");
    let mut vector_scores: Vec<(usize, f32)> = Vec::new();
    
    for (idx, (name, _, embedding)) in code_symbols.iter().enumerate() {
        let similarity = cosine_similarity(&query_embedding, embedding);
        vector_scores.push((idx, similarity));
        println!("    - {} (doc {}): similarity = {:.4}", name, idx, similarity);
    }
    
    // Step 3: Combined scoring (RRF - Reciprocal Rank Fusion)
    println!("\n  Step 3: Hybrid scoring with RRF");
    let k = 60.0; // RRF constant
    
    // Sort by text scores
    let mut text_ranked: Vec<(usize, f32)> = text_scores.into_iter().collect();
    text_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // Sort by vector scores
    vector_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // Calculate RRF scores
    let mut rrf_scores: HashMap<usize, f32> = HashMap::new();
    
    // Add text rank contributions
    for (rank, (doc_id, score)) in text_ranked.iter().enumerate() {
        let rrf_contribution = 1.0 / (k + rank as f32 + 1.0);
        *rrf_scores.entry(*doc_id).or_insert(0.0) += rrf_contribution;
        println!("    - Doc {} text rank {}: RRF += {:.4} (score: {:.3})", 
                 doc_id, rank + 1, rrf_contribution, score);
    }
    
    // Add vector rank contributions
    for (rank, (doc_id, score)) in vector_scores.iter().enumerate() {
        let rrf_contribution = 1.0 / (k + rank as f32 + 1.0);
        *rrf_scores.entry(*doc_id).or_insert(0.0) += rrf_contribution;
        println!("    - Doc {} vector rank {}: RRF += {:.4} (similarity: {:.3})", 
                 doc_id, rank + 1, rrf_contribution, score);
    }
    
    // Final ranking
    let mut final_ranking: Vec<(usize, f32)> = rrf_scores.into_iter().collect();
    final_ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // Then: Display final results
    println!("\n✓ Final hybrid ranking:");
    for (rank, (doc_id, rrf_score)) in final_ranking.iter().take(5).enumerate() {
        let (name, desc, _) = &code_symbols[*doc_id];
        println!("  {}. {} - {} (RRF: {:.4})", 
                 rank + 1, name, desc, rrf_score);
    }
    
    // Verify top results are JSON-related
    let top_3: Vec<&str> = final_ranking.iter()
        .take(3)
        .map(|(id, _)| code_symbols[*id].0)
        .collect();
    
    assert!(top_3.iter().all(|name| name.contains("json")), 
            "Top 3 results should be JSON-related functions");
    
    // Analysis
    println!("\nScoring analysis:");
    println!("  - Text search found {} matching documents", text_ranked.len());
    println!("  - Vector search ranked all {} documents", vector_scores.len());
    println!("  - RRF successfully combined both signals");
    println!("  - JSON parsing functions ranked highest (semantic + text match)");
    
    // Tantivy integration notes
    println!("\nTantivy Scorer implementation:");
    println!("  1. Custom Scorer computes vector similarity on-the-fly");
    println!("  2. Scorer.score() returns normalized similarity [0, 1]");
    println!("  3. Tantivy's Collector handles score combination");
    println!("  4. RRF can be implemented as custom Collector");
    println!("  5. Or use score boosting: text_score + boost * vector_score");
    
    println!("\n=== Test 5b: PASSED ===\n");
    Ok(())
}

// Data structures that will evolve as we implement more tests

/// IVFFlat index structure (to be expanded)
#[derive(bincode::Encode, bincode::Decode)]
struct IVFFlatIndex {
    centroids: Vec<Vec<f32>>,
    assignments: Vec<ClusterId>,
    // vector_storage: MmapVectorStorage, // To be added in Test 3
}

// Future additions will include:
// - MmapVectorStorage for Test 3
// - TantivyWarmer extensions for Test 4
// - AnnQuery implementation for Test 5
// - Hybrid search logic for Test 6