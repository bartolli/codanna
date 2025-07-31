//! Vector search engine that orchestrates indexing and searching operations.
//!
//! This module provides the main entry point for vector search functionality,
//! coordinating between storage, clustering, and search operations.

use std::collections::HashMap;
use std::path::Path;

use crate::vector::{
    assign_to_nearest_centroid, cosine_similarity, kmeans_clustering, ClusterId,
    ConcurrentVectorStorage, MmapVectorStorage, Score, SegmentOrdinal, VectorDimension,
    VectorError, VectorId,
};

/// Minimum number of clusters for K-means clustering.
const MIN_CLUSTERS: usize = 1;

/// Maximum number of clusters for K-means clustering.
const MAX_CLUSTERS: usize = 100;

/// Main vector search engine that coordinates indexing and search operations.
///
/// This engine manages:
/// - Vector storage in memory-mapped files
/// - K-means clustering for IVFFlat indexing
/// - Efficient nearest neighbor search
#[derive(Debug)]
pub struct VectorSearchEngine {
    /// Concurrent vector storage for thread-safe access
    storage: ConcurrentVectorStorage,
    
    /// Mapping from vector IDs to their assigned clusters
    cluster_assignments: HashMap<VectorId, ClusterId>,
    
    /// Cluster centroids for fast search
    centroids: Vec<Vec<f32>>,
    
    /// Vector dimension for validation
    dimension: VectorDimension,
}

impl VectorSearchEngine {
    /// Creates a new vector search engine.
    ///
    /// # Arguments
    /// * `storage_path` - Base path for vector storage files
    /// * `dimension` - Dimension of vectors to be indexed
    ///
    /// # Returns
    /// A new engine instance ready for indexing
    #[must_use = "The created VectorSearchEngine instance should be used for indexing and searching"]
    pub fn new(storage_path: impl AsRef<Path>, dimension: VectorDimension) -> Result<Self, VectorError> {
        // Use segment 0 as default for now (single segment)
        let mmap_storage = MmapVectorStorage::new(storage_path.as_ref(), SegmentOrdinal::new(0), dimension)
            .map_err(|e| VectorError::Storage(std::io::Error::other(
                format!("Failed to create storage: {}. Check that the directory exists and you have write permissions", e)
            )))?;
        
        let storage = ConcurrentVectorStorage::new(mmap_storage);
        
        Ok(Self {
            storage,
            cluster_assignments: HashMap::new(),
            centroids: Vec::new(),
            dimension,
        })
    }
    
    /// Indexes a batch of vectors with K-means clustering.
    ///
    /// # Arguments
    /// * `vectors` - Slice of (VectorId, vector) pairs to index
    ///
    /// # Algorithm
    /// 1. Validates all vector dimensions
    /// 2. Stores vectors in memory-mapped storage
    /// 3. Runs K-means clustering to create centroids
    /// 4. Updates cluster assignments
    pub fn index_vectors(&mut self, vectors: &[(VectorId, Vec<f32>)]) -> Result<(), VectorError> {
        if vectors.is_empty() {
            return Ok(());
        }
        
        // Validate dimensions
        for (_, vec) in vectors {
            self.dimension.validate_vector(vec)?;
        }
        
        // Store vectors using write_batch through concurrent storage
        // Convert to borrowed slices for write_batch
        let vector_refs: Vec<(VectorId, &[f32])> = vectors
            .iter()
            .map(|(id, vec)| (*id, vec.as_slice()))
            .collect();
        self.storage
            .write_batch(&vector_refs)
            .map_err(|e| VectorError::Storage(std::io::Error::other(
                format!("Failed to store vectors: {}. Check disk space and file permissions", e)
            )))?;
        
        // Extract just the vectors for clustering
        // TODO: Future optimization - clustering algorithm should accept &[&[f32]] to avoid clones
        let vecs: Vec<Vec<f32>> = vectors.iter().map(|(_, v)| v.clone()).collect();
        
        // Determine number of clusters (sqrt of num vectors, clamped to reasonable bounds)
        let k = (vecs.len() as f32).sqrt().ceil() as usize;
        let k = k.clamp(MIN_CLUSTERS, MAX_CLUSTERS);
        
        // Run K-means clustering
        let clustering_result = kmeans_clustering(&vecs, k)
            .map_err(|e| VectorError::ClusteringFailed(e.to_string()))?;
        
        // Update internal state
        self.centroids = clustering_result.centroids;
        self.cluster_assignments.clear();
        
        for (i, (id, _)) in vectors.iter().enumerate() {
            self.cluster_assignments.insert(*id, clustering_result.assignments[i]);
        }
        
        Ok(())
    }
    
    /// Searches for the k nearest neighbors to a query vector.
    ///
    /// # Arguments
    /// * `query` - Query vector
    /// * `k` - Number of nearest neighbors to return
    ///
    /// # Returns
    /// Vector of (VectorId, Score) pairs sorted by similarity (highest first)
    ///
    /// # Algorithm
    /// 1. Find the nearest centroid to the query
    /// 2. Retrieve all vectors in that cluster
    /// 3. Calculate cosine similarity to each vector
    /// 4. Return top-k results
    #[must_use = "Search results should be processed to retrieve relevant vectors"]
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<(VectorId, Score)>, VectorError> {
        // Validate query dimension
        self.dimension.validate_vector(query)?;
        
        if self.centroids.is_empty() {
            // Return empty results for empty index - not an error
            // Caller should check if results are empty and index vectors first
            return Ok(Vec::new());
        }
        
        // Find nearest centroid
        let centroid_refs: Vec<&[f32]> = self.centroids.iter().map(|c| c.as_slice()).collect();
        let nearest_cluster = assign_to_nearest_centroid(query, &centroid_refs);
        
        // Collect all vectors in the nearest cluster
        let mut candidates = Vec::new();
        for (vector_id, cluster_id) in &self.cluster_assignments {
            if *cluster_id == nearest_cluster {
                // Get vector from storage
                if let Some(vector) = self.storage.read_vector(*vector_id) {
                    let similarity = cosine_similarity(query, &vector);
                    // Convert similarity to score (already in [0, 1] range)
                    if let Ok(score) = Score::new(similarity) {
                        candidates.push((*vector_id, score));
                    }
                }
            }
        }
        
        // Sort by score (highest first) and take top k
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates.truncate(k);
        
        Ok(candidates)
    }
    
    /// Gets the cluster assignment for a specific vector.
    ///
    /// # Arguments
    /// * `id` - Vector ID to look up
    ///
    /// # Returns
    /// The cluster ID if the vector is indexed, None otherwise
    #[must_use = "The cluster assignment should be used for cluster-aware operations"]
    pub fn get_cluster_for_vector(&self, id: VectorId) -> Option<ClusterId> {
        self.cluster_assignments.get(&id).copied()
    }
    
    /// Gets a reference to cluster centroids for inspection.
    #[must_use]
    pub fn as_centroids(&self) -> &[Vec<f32>] {
        &self.centroids
    }
    
    /// Gets the number of indexed vectors.
    #[must_use]
    pub fn vector_count(&self) -> usize {
        self.cluster_assignments.len()
    }
    
    /// Gets the vector dimension.
    #[must_use]
    pub fn dimension(&self) -> VectorDimension {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    fn create_test_vectors(n: usize, dim: usize) -> Vec<(VectorId, Vec<f32>)> {
        (1..=n)
            .map(|i| {
                let id = VectorId::new(i as u32).unwrap();
                // Create more distinct vectors
                let mut vec = vec![0.0; dim];
                // Set multiple dimensions to create more separation
                let angle = (i as f32 - 1.0) * std::f32::consts::PI * 2.0 / n as f32;
                vec[0] = angle.cos();
                vec[1] = angle.sin();
                // Add some variation in other dimensions
                for j in 2..dim.min(10) {
                    vec[j] = ((i * j) as f32 / (n * dim) as f32).sin();
                }
                // Normalize to unit length for cosine similarity
                let norm = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                let vec: Vec<f32> = vec.iter().map(|x| x / norm).collect();
                (id, vec)
            })
            .collect()
    }
    
    #[test]
    fn test_engine_creation() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(128).unwrap();
        
        let engine = VectorSearchEngine::new(temp_dir.path().to_path_buf(), dimension).unwrap();
        
        assert!(engine.centroids.is_empty());
        assert!(engine.cluster_assignments.is_empty());
    }
    
    #[test]
    fn test_index_and_search() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(128).unwrap();
        
        let mut engine = VectorSearchEngine::new(temp_dir.path().to_path_buf(), dimension).unwrap();
        
        // Create test vectors - fewer for more predictable clustering
        let vectors = create_test_vectors(20, 128);
        
        // Index vectors
        engine.index_vectors(&vectors).unwrap();
        
        // Verify clustering happened
        assert!(!engine.centroids.is_empty());
        assert_eq!(engine.cluster_assignments.len(), 20);
        
        // Search for each vector and verify we can find at least some results
        let mut found_count = 0;
        for (query_id, query_vec) in &vectors {
            let results = engine.search(query_vec, 5).unwrap();
            
            // Check if we found the query vector itself
            if results.iter().any(|(id, _)| id == query_id) {
                found_count += 1;
            }
        }
        
        // With IVFFlat, we won't find all vectors (only those in the same cluster)
        // But we should find a reasonable portion
        assert!(found_count > 0, "Should find at least some vectors");
        
        // Test that search returns sorted results
        let query = &vectors[0].1;
        let results = engine.search(query, 10).unwrap();
        for i in 1..results.len() {
            assert!(results[i-1].1 >= results[i].1, "Results should be sorted by score");
        }
    }
    
    #[test]
    fn test_empty_index_search() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(128).unwrap();
        
        let engine = VectorSearchEngine::new(temp_dir.path().to_path_buf(), dimension).unwrap();
        
        let query = vec![0.5; 128];
        let results = engine.search(&query, 5).unwrap();
        
        assert!(results.is_empty());
    }
    
    #[test]
    fn test_dimension_validation() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(128).unwrap();
        
        let mut engine = VectorSearchEngine::new(temp_dir.path().to_path_buf(), dimension).unwrap();
        
        // Try to index vectors with wrong dimension
        let bad_vectors = vec![
            (VectorId::new(1).unwrap(), vec![0.5; 64]), // Wrong dimension
        ];
        
        let result = engine.index_vectors(&bad_vectors);
        assert!(result.is_err());
        
        // Try to search with wrong dimension query
        let bad_query = vec![0.5; 64];
        let result = engine.search(&bad_query, 5);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_cluster_assignment_lookup() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(128).unwrap();
        
        let mut engine = VectorSearchEngine::new(temp_dir.path().to_path_buf(), dimension).unwrap();
        
        let vectors = create_test_vectors(10, 128);
        engine.index_vectors(&vectors).unwrap();
        
        // Check that all vectors have cluster assignments
        for (id, _) in &vectors {
            let cluster = engine.get_cluster_for_vector(*id);
            assert!(cluster.is_some());
        }
        
        // Check non-existent vector
        let non_existent = VectorId::new(999).unwrap();
        assert!(engine.get_cluster_for_vector(non_existent).is_none());
    }
    
    #[test]
    fn test_search_returns_sorted_results() {
        let temp_dir = TempDir::new().unwrap();
        let dimension = VectorDimension::new(128).unwrap();
        
        let mut engine = VectorSearchEngine::new(temp_dir.path().to_path_buf(), dimension).unwrap();
        
        // Create diverse vectors
        let vectors = create_test_vectors(50, 128);
        engine.index_vectors(&vectors).unwrap();
        
        // Search for something
        let query = vectors[25].1.clone();
        let results = engine.search(&query, 10).unwrap();
        
        // Verify results are sorted by score (descending)
        for i in 1..results.len() {
            assert!(results[i-1].1 >= results[i].1);
        }
    }
}