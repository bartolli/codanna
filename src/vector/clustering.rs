//! K-means clustering implementation for IVFFlat vector indexing.
//!
//! This module provides a pure Rust implementation of K-means clustering
//! optimized for code embedding vectors. It uses cosine similarity as the
//! distance metric and K-means++ for intelligent centroid initialization.
//!
//! # Algorithm Details
//! - Distance metric: Cosine similarity (not Euclidean)
//! - Initialization: K-means++ for better convergence
//! - Max iterations: 100
//! - Convergence tolerance: 1e-4
//!
//! # Performance Characteristics
//! - O(n * k * d * iterations) time complexity
//! - O(k * d) space for centroids
//! - Parallelizable assignment step

use crate::vector::types::{ClusterId, VectorError};
use rand::Rng;
use thiserror::Error;

/// Maximum number of iterations for K-means clustering.
const MAX_ITERATIONS: usize = 100;

/// Convergence tolerance for centroid updates.
const CONVERGENCE_TOLERANCE: f32 = 1e-4;

/// Epsilon for floating-point comparisons.
const EPSILON: f32 = 1e-10;

/// Result of K-means clustering operation.
#[derive(Debug, Clone, PartialEq)]
pub struct KMeansResult {
    /// Cluster centroids, each a vector of the same dimension as input vectors.
    pub centroids: Vec<Vec<f32>>,
    
    /// Cluster assignment for each input vector.
    pub assignments: Vec<ClusterId>,
    
    /// Number of iterations until convergence.
    pub iterations: usize,
}

/// Errors that can occur during clustering operations.
#[derive(Error, Debug)]
pub enum ClusteringError {
    #[error("Empty vector set provided for clustering\nSuggestion: Ensure vectors are generated before clustering")]
    EmptyVectorSet,
    
    #[error("Invalid cluster count: {0}\nSuggestion: Use k between 1 and the number of vectors")]
    InvalidClusterCount(usize),
    
    #[error("Dimension mismatch in vectors\nSuggestion: Ensure all vectors come from the same embedding model")]
    DimensionMismatch,
    
    #[error("Failed to initialize centroids\nSuggestion: Check that vectors contain valid floating-point values")]
    InitializationFailed,
    
    #[error("Clustering did not converge after {0} iterations\nSuggestion: Consider increasing max iterations or adjusting convergence tolerance")]
    ConvergenceFailed(usize),
    
    #[error("Vector operation error: {0}")]
    VectorError(#[from] VectorError),
}

/// Performs K-means clustering on a set of vectors using cosine similarity.
///
/// # Arguments
/// * `vectors` - Input vectors to cluster (must be non-empty and same dimension)
/// * `k` - Number of clusters (must be >= 1 and <= number of vectors)
///
/// # Returns
/// * `KMeansResult` containing centroids, assignments, and iteration count
///
/// # Algorithm
/// 1. Initialize centroids using K-means++ method
/// 2. Iterate until convergence or max iterations:
///    - Assign each vector to nearest centroid (by cosine similarity)
///    - Update centroids as mean of assigned vectors
///    - Check convergence based on centroid movement
#[must_use = "clustering results should be used or the computation is wasted"]
pub fn kmeans_clustering(
    vectors: &[Vec<f32>],
    k: usize,
) -> Result<KMeansResult, ClusteringError> {
    // Validate inputs
    if vectors.is_empty() {
        return Err(ClusteringError::EmptyVectorSet);
    }
    
    if k == 0 || k > vectors.len() {
        return Err(ClusteringError::InvalidClusterCount(k));
    }
    
    // Ensure all vectors have the same dimension
    let dimension = vectors[0].len();
    if vectors.iter().any(|v| v.len() != dimension) {
        return Err(ClusteringError::DimensionMismatch);
    }
    
    // Initialize centroids using K-means++
    let mut centroids = initialize_centroids_kmeans_plus_plus(vectors, k)?;
    let mut assignments = vec![ClusterId::new_unchecked(1); vectors.len()];
    let mut iterations = 0;
    
    // Main K-means loop
    loop {
        iterations += 1;
        
        // Assignment step: assign each vector to nearest centroid
        let centroid_refs: Vec<&[f32]> = centroids.iter().map(|c| c.as_slice()).collect();
        let new_assignments: Vec<ClusterId> = vectors
            .iter()
            .map(|vector| assign_to_nearest_centroid(vector, &centroid_refs))
            .collect();
        
        // Check for convergence (no assignment changes)
        let converged = new_assignments == assignments;
        assignments = new_assignments;
        
        if converged || iterations >= MAX_ITERATIONS {
            break;
        }
        
        // Update step: recompute centroids
        let new_centroids = update_centroids(vectors, &assignments, k)?;
        
        // Check centroid convergence
        let centroid_movement = calculate_centroid_movement(&centroids, &new_centroids);
        centroids = new_centroids;
        
        if centroid_movement < CONVERGENCE_TOLERANCE {
            break;
        }
    }
    
    if iterations >= MAX_ITERATIONS {
        // Note: We still return results even if not fully converged
        eprintln!("Warning: K-means did not fully converge after {} iterations", MAX_ITERATIONS);
    }
    
    Ok(KMeansResult {
        centroids,
        assignments,
        iterations,
    })
}

/// Assigns a vector to the nearest centroid based on cosine similarity.
///
/// # Arguments
/// * `vector` - The vector to assign
/// * `centroids` - Current cluster centroids
///
/// # Returns
/// * `ClusterId` of the nearest centroid
pub fn assign_to_nearest_centroid(vector: &[f32], centroids: &[&[f32]]) -> ClusterId {
    let mut best_similarity = f32::NEG_INFINITY;
    let mut best_cluster = 0;
    
    for (i, centroid) in centroids.iter().enumerate() {
        let similarity = cosine_similarity(vector, centroid);
        if similarity > best_similarity {
            best_similarity = similarity;
            best_cluster = i;
        }
    }
    
    // ClusterId is 1-indexed, so add 1
    ClusterId::new_unchecked((best_cluster + 1) as u32)
}

/// Updates centroids as the mean of their assigned vectors.
///
/// # Arguments
/// * `vectors` - All input vectors
/// * `assignments` - Current cluster assignments
/// * `k` - Number of clusters
///
/// # Returns
/// * Updated centroids
fn update_centroids(
    vectors: &[Vec<f32>],
    assignments: &[ClusterId],
    k: usize,
) -> Result<Vec<Vec<f32>>, ClusteringError> {
    let dimension = vectors[0].len();
    let mut new_centroids = vec![vec![0.0; dimension]; k];
    let mut cluster_sizes = vec![0usize; k];
    
    // Sum vectors for each cluster
    for (vector, &cluster_id) in vectors.iter().zip(assignments.iter()) {
        let cluster_idx = (cluster_id.get() - 1) as usize;
        
        for (i, &value) in vector.iter().enumerate() {
            new_centroids[cluster_idx][i] += value;
        }
        cluster_sizes[cluster_idx] += 1;
    }
    
    // Compute means and normalize
    for (centroid, &size) in new_centroids.iter_mut().zip(cluster_sizes.iter()) {
        if size == 0 {
            // Empty cluster: reinitialize to a random vector
            let random_idx = rand::rng().random_range(0..vectors.len());
            *centroid = normalize_vector_copy(&vectors[random_idx]);
        } else {
            // Compute mean
            for value in centroid.iter_mut() {
                *value /= size as f32;
            }
            
            // Normalize for cosine similarity
            normalize_vector(centroid);
        }
    }
    
    Ok(new_centroids)
}

/// Computes cosine similarity between two vectors.
///
/// # Arguments
/// * `a` - First vector
/// * `b` - Second vector
///
/// # Returns
/// * Cosine similarity in range [-1, 1], where 1 is most similar
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "Vectors must have same dimension");
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

/// Initializes centroids using the K-means++ algorithm.
///
/// K-means++ selects initial centroids that are far apart, leading to
/// better convergence properties than random initialization.
fn initialize_centroids_kmeans_plus_plus(
    vectors: &[Vec<f32>],
    k: usize,
) -> Result<Vec<Vec<f32>>, ClusteringError> {
    let mut rng = rand::rng();
    let mut centroids = Vec::with_capacity(k);
    
    // Choose first centroid randomly
    let first_idx = rng.random_range(0..vectors.len());
    centroids.push(normalize_vector_copy(&vectors[first_idx]));
    
    // Choose remaining centroids
    for _ in 1..k {
        // Calculate distances to nearest centroid for each vector
        let mut distances = vec![0.0f32; vectors.len()];
        let mut total_distance = 0.0f32;
        
        for (i, vector) in vectors.iter().enumerate() {
            let mut min_distance = f32::MAX;
            
            for centroid in &centroids {
                // Use cosine distance (1 - similarity)
                let distance = 1.0 - cosine_similarity(vector, centroid);
                min_distance = min_distance.min(distance);
            }
            
            // Square the distance for K-means++ probability distribution
            distances[i] = min_distance * min_distance;
            total_distance += distances[i];
        }
        
        if total_distance < EPSILON {
            // All points are coincident with existing centroids
            // Stop early to prevent infinite loop
            break;
        }
        
        // Choose next centroid with probability proportional to squared distance
        let mut cumulative = 0.0;
        let target = rng.random::<f32>() * total_distance;
        let mut added = false;
        
        for (i, &distance) in distances.iter().enumerate() {
            cumulative += distance;
            if cumulative >= target {
                centroids.push(normalize_vector_copy(&vectors[i]));
                added = true;
                break;
            }
        }
        
        // Fallback: add the last vector if rounding errors prevent selection
        if !added && centroids.len() < k {
            centroids.push(normalize_vector_copy(&vectors[vectors.len() - 1]));
        }
    }
    
    if centroids.len() != k {
        return Err(ClusteringError::InitializationFailed);
    }
    
    Ok(centroids)
}

/// Calculates the total movement of centroids between iterations.
fn calculate_centroid_movement(old: &[Vec<f32>], new: &[Vec<f32>]) -> f32 {
    old.iter()
        .zip(new.iter())
        .map(|(old_c, new_c)| {
            // Use cosine distance as movement metric
            1.0 - cosine_similarity(old_c, new_c)
        })
        .sum::<f32>()
        / old.len() as f32
}

/// Normalizes a vector in-place to unit length.
fn normalize_vector(vector: &mut [f32]) {
    let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
    // If norm is too small, leave vector as-is (effectively zero vector)
}

/// Creates a normalized copy of a vector.
fn normalize_vector_copy(vector: &[f32]) -> Vec<f32> {
    let mut normalized = vector.to_vec();
    normalize_vector(&mut normalized);
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cosine_similarity() {
        // Identical vectors
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < f32::EPSILON);
        
        // Orthogonal vectors
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < f32::EPSILON);
        
        // Opposite vectors
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        assert!((cosine_similarity(&a, &b) - (-1.0)).abs() < f32::EPSILON);
        
        // Zero vector
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![0.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
    
    #[test]
    fn test_assign_to_nearest_centroid() {
        let centroids = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        
        // Vector closest to first centroid
        let vector = vec![0.9, 0.1, 0.0];
        let centroid_refs: Vec<&[f32]> = centroids.iter().map(|c| c.as_slice()).collect();
        let cluster = assign_to_nearest_centroid(&vector, &centroid_refs);
        assert_eq!(cluster.get(), 1);
        
        // Vector closest to second centroid
        let vector = vec![0.1, 0.9, 0.1];
        let cluster = assign_to_nearest_centroid(&vector, &centroid_refs);
        assert_eq!(cluster.get(), 2);
        
        // Vector closest to third centroid
        let vector = vec![0.0, 0.1, 0.9];
        let cluster = assign_to_nearest_centroid(&vector, &centroid_refs);
        assert_eq!(cluster.get(), 3);
    }
    
    #[test]
    fn test_kmeans_clustering_basic() {
        // Create simple test data with clear clusters
        let vectors = vec![
            // Cluster 1: mostly x-axis
            vec![1.0, 0.1, 0.0],
            vec![0.9, 0.2, 0.1],
            vec![1.1, 0.0, 0.2],
            // Cluster 2: mostly y-axis
            vec![0.1, 1.0, 0.0],
            vec![0.2, 0.9, 0.1],
            vec![0.0, 1.1, 0.2],
            // Cluster 3: mostly z-axis
            vec![0.0, 0.1, 1.0],
            vec![0.1, 0.2, 0.9],
            vec![0.2, 0.0, 1.1],
        ];
        
        let result = kmeans_clustering(&vectors, 3).unwrap();
        
        assert_eq!(result.centroids.len(), 3);
        assert_eq!(result.assignments.len(), 9);
        assert!(result.iterations <= MAX_ITERATIONS);
        
        // Verify that similar vectors are in the same cluster
        // (First 3 should be in one cluster, next 3 in another, last 3 in the third)
        let cluster1 = result.assignments[0];
        assert_eq!(result.assignments[1], cluster1);
        assert_eq!(result.assignments[2], cluster1);
        
        let cluster2 = result.assignments[3];
        assert_eq!(result.assignments[4], cluster2);
        assert_eq!(result.assignments[5], cluster2);
        
        let cluster3 = result.assignments[6];
        assert_eq!(result.assignments[7], cluster3);
        assert_eq!(result.assignments[8], cluster3);
    }
    
    #[test]
    fn test_kmeans_edge_cases() {
        // Empty vectors
        let vectors: Vec<Vec<f32>> = vec![];
        assert!(matches!(
            kmeans_clustering(&vectors, 1),
            Err(ClusteringError::EmptyVectorSet)
        ));
        
        // Invalid k (zero)
        let vectors = vec![vec![1.0, 2.0]];
        assert!(matches!(
            kmeans_clustering(&vectors, 0),
            Err(ClusteringError::InvalidClusterCount(0))
        ));
        
        // Invalid k (too large)
        let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
        assert!(matches!(
            kmeans_clustering(&vectors, 3),
            Err(ClusteringError::InvalidClusterCount(3))
        ));
        
        // Dimension mismatch
        let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0, 5.0]];
        assert!(matches!(
            kmeans_clustering(&vectors, 1),
            Err(ClusteringError::DimensionMismatch)
        ));
    }
    
    #[test]
    fn test_single_cluster() {
        let vectors = vec![
            vec![1.0, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 9.0],
        ];
        
        let result = kmeans_clustering(&vectors, 1).unwrap();
        
        assert_eq!(result.centroids.len(), 1);
        assert_eq!(result.assignments.len(), 3);
        
        // All vectors should be assigned to the same cluster
        let cluster = result.assignments[0];
        assert!(result.assignments.iter().all(|&c| c == cluster));
    }
    
    #[test]
    fn test_normalize_vector() {
        let mut vector = vec![3.0, 4.0];
        normalize_vector(&mut vector);
        
        // Should be unit length
        let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < f32::EPSILON);
        
        // Check values
        assert!((vector[0] - 0.6).abs() < f32::EPSILON);
        assert!((vector[1] - 0.8).abs() < f32::EPSILON);
    }
}