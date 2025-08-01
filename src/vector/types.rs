//! Type-safe wrappers and core types for vector search functionality.
//!
//! This module provides newtypes and error types following the project's
//! strict type safety guidelines. All types implement necessary traits
//! for ergonomic usage while preventing primitive obsession.

use std::num::NonZeroU32;
use thiserror::Error;

/// Standard vector dimension for code embeddings (all-MiniLM-L6-v2 model).
pub const VECTOR_DIMENSION_384: usize = 384;

/// Type-safe wrapper for vector IDs.
///
/// Uses `NonZeroU32` internally for space optimization and to ensure
/// vector IDs are never zero (which could indicate uninitialized state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorId(NonZeroU32);

impl VectorId {
    /// Creates a new `VectorId` from a non-zero u32.
    ///
    /// Returns `None` if the provided ID is zero.
    #[must_use]
    pub fn new(id: u32) -> Option<Self> {
        NonZeroU32::new(id).map(Self)
    }

    /// Creates a new `VectorId` from a non-zero u32, panicking if zero.
    ///
    /// # Panics
    /// Panics if `id` is zero. Use `new()` for fallible construction.
    #[must_use]
    pub fn new_unchecked(id: u32) -> Self {
        Self(NonZeroU32::new(id).expect("VectorId cannot be zero"))
    }

    /// Returns the underlying u32 value.
    #[must_use]
    pub fn get(&self) -> u32 {
        self.0.get()
    }

    /// Converts to little-endian bytes for storage.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 4] {
        self.0.get().to_le_bytes()
    }

    /// Creates from little-endian bytes.
    ///
    /// Returns `None` if the bytes represent zero.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 4]) -> Option<Self> {
        let id = u32::from_le_bytes(bytes);
        Self::new(id)
    }
}

/// Type-safe wrapper for cluster IDs in IVFFlat indexing.
///
/// Clusters are identified by non-zero IDs to prevent confusion
/// with uninitialized or error states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClusterId(NonZeroU32);

impl ClusterId {
    /// Creates a new `ClusterId` from a non-zero u32.
    ///
    /// Returns `None` if the provided ID is zero.
    #[must_use]
    pub fn new(id: u32) -> Option<Self> {
        NonZeroU32::new(id).map(Self)
    }

    /// Creates a new `ClusterId` from a non-zero u32, panicking if zero.
    ///
    /// # Panics
    /// Panics if `id` is zero. Use `new()` for fallible construction.
    #[must_use]
    pub fn new_unchecked(id: u32) -> Self {
        Self(NonZeroU32::new(id).expect("ClusterId cannot be zero"))
    }

    /// Returns the underlying u32 value.
    #[must_use]
    pub fn get(&self) -> u32 {
        self.0.get()
    }

    /// Converts to little-endian bytes for storage.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 4] {
        self.0.get().to_le_bytes()
    }

    /// Creates from little-endian bytes.
    ///
    /// Returns `None` if the bytes represent zero.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 4]) -> Option<Self> {
        let id = u32::from_le_bytes(bytes);
        Self::new(id)
    }
}

/// Type-safe wrapper for Tantivy segment ordinals.
///
/// Segment ordinals can be zero (for the first segment), so we use
/// a plain u32 rather than NonZeroU32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SegmentOrdinal(u32);

impl SegmentOrdinal {
    /// Creates a new `SegmentOrdinal`.
    #[must_use]
    pub const fn new(ordinal: u32) -> Self {
        Self(ordinal)
    }

    /// Returns the underlying u32 value.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.0
    }

    /// Converts to little-endian bytes for storage.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    /// Creates from little-endian bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 4]) -> Self {
        Self(u32::from_le_bytes(bytes))
    }
}

impl std::fmt::Display for SegmentOrdinal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe wrapper for similarity scores.
///
/// Scores are normalized to the range [0.0, 1.0] where:
/// - 1.0 indicates perfect similarity
/// - 0.0 indicates no similarity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score(f32);

impl Score {
    /// Creates a new `Score` with validation.
    ///
    /// Returns an error if the score is not in the range [0.0, 1.0] or is NaN.
    pub fn new(value: f32) -> Result<Self, VectorError> {
        if value.is_nan() {
            return Err(VectorError::InvalidScore {
                value,
                reason: "Score cannot be NaN",
            });
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(VectorError::InvalidScore {
                value,
                reason: "Score must be in range [0.0, 1.0]",
            });
        }
        Ok(Self(value))
    }

    /// Creates a score of 0.0 (no similarity).
    #[must_use]
    pub const fn zero() -> Self {
        Self(0.0)
    }

    /// Creates a score of 1.0 (perfect similarity).
    #[must_use]
    pub const fn one() -> Self {
        Self(1.0)
    }

    /// Returns the underlying f32 value.
    #[must_use]
    pub fn get(&self) -> f32 {
        self.0
    }

    /// Combines two scores using weighted average.
    ///
    /// # Arguments
    /// * `other` - The other score to combine with
    /// * `weight` - Weight for this score (0.0 to 1.0). The other score gets weight (1.0 - weight).
    ///
    /// # Errors
    /// Returns an error if weight is not in [0.0, 1.0] or is NaN.
    pub fn weighted_combine(&self, other: Score, weight: f32) -> Result<Self, VectorError> {
        if weight.is_nan() || !(0.0..=1.0).contains(&weight) {
            return Err(VectorError::InvalidWeight {
                value: weight,
                reason: "Weight must be in range [0.0, 1.0] and not NaN",
            });
        }
        Ok(Self(self.0 * weight + other.0 * (1.0 - weight)))
    }
}

impl Eq for Score {}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Score {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.partial_cmp(&other.0)
            .expect("Score values should never be NaN")
    }
}

/// Type-safe wrapper for vector dimensions.
///
/// Ensures compile-time or runtime validation of vector dimensions
/// to prevent dimension mismatches during operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorDimension(usize);

impl VectorDimension {
    /// Creates a new `VectorDimension` with validation.
    ///
    /// Returns an error if the dimension is zero.
    pub fn new(dim: usize) -> Result<Self, VectorError> {
        if dim == 0 {
            return Err(VectorError::InvalidDimension {
                dimension: 0,
                reason: "Vector dimension cannot be zero",
            });
        }
        Ok(Self(dim))
    }

    /// Creates a standard 384-dimensional vector dimension.
    #[must_use]
    pub const fn dimension_384() -> Self {
        Self(VECTOR_DIMENSION_384)
    }

    /// Returns the underlying dimension value.
    #[must_use]
    pub const fn get(&self) -> usize {
        self.0
    }

    /// Validates that a vector has the expected dimension.
    pub fn validate_vector(&self, vector: &[f32]) -> Result<(), VectorError> {
        if vector.len() != self.0 {
            return Err(VectorError::DimensionMismatch {
                expected: self.0,
                actual: vector.len(),
            });
        }
        Ok(())
    }
}

/// Errors that can occur during vector operations.
///
/// All error messages include actionable suggestions for resolution.
#[derive(Error, Debug)]
pub enum VectorError {
    #[error("Vector dimension mismatch: expected {expected}, got {actual}\nSuggestion: Ensure all vectors use the same embedding model")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("Invalid vector dimension: {dimension}\nReason: {reason}")]
    InvalidDimension { dimension: usize, reason: &'static str },

    #[error("Invalid score value: {value}\nReason: {reason}")]
    InvalidScore { value: f32, reason: &'static str },

    #[error("Cache warming failed: {0}\nSuggestion: Check disk space and permissions for cache directory")]
    CacheWarming(String),

    #[error("Invalid cluster ID: {0}\nSuggestion: Ensure clustering has been performed before assigning vectors")]
    InvalidClusterId(u32),

    #[error("Storage error: {0}\nSuggestion: Check disk space and file permissions")]
    Storage(#[from] std::io::Error),

    #[error("Embedding generation failed: {0}\nSuggestion: Verify the embedding model is properly initialized")]
    EmbeddingFailed(String),

    #[error("Clustering failed: {0}\nSuggestion: Ensure sufficient vectors are available for clustering (minimum: k clusters)")]
    ClusteringFailed(String),

    #[error("Serialization error: {0}\nSuggestion: Check that vector data is valid and not corrupted")]
    Serialization(String),

    #[error("Vector not found: ID {0}\nSuggestion: Verify the vector was properly indexed")]
    VectorNotFound(u32),
    #[error("Invalid weight value: {value}\nReason: {reason}")]
    InvalidWeight { value: f32, reason: &'static str },

    #[error("Invalid storage version: expected {expected}, got {actual}\nSuggestion: Migrate the storage format or use a compatible version")]
    VersionMismatch { expected: u32, actual: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_id_construction() {
        // Valid construction
        let id = VectorId::new(42).unwrap();
        assert_eq!(id.get(), 42);

        // Invalid construction (zero)
        assert!(VectorId::new(0).is_none());

        // Unchecked construction
        let id = VectorId::new_unchecked(100);
        assert_eq!(id.get(), 100);
    }

    #[test]
    #[should_panic(expected = "VectorId cannot be zero")]
    fn test_vector_id_unchecked_panic() {
        let _ = VectorId::new_unchecked(0);
    }

    #[test]
    fn test_vector_id_serialization() {
        let id = VectorId::new(12345).unwrap();
        let bytes = id.to_bytes();
        let deserialized = VectorId::from_bytes(bytes).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_cluster_id_construction() {
        // Valid construction
        let id = ClusterId::new(1).unwrap();
        assert_eq!(id.get(), 1);

        // Invalid construction (zero)
        assert!(ClusterId::new(0).is_none());
    }

    #[test]
    fn test_segment_ordinal() {
        let seg = SegmentOrdinal::new(0);
        assert_eq!(seg.get(), 0);

        let seg2 = SegmentOrdinal::new(42);
        assert_eq!(seg2.get(), 42);

        // Test ordering
        assert!(seg < seg2);
    }

    #[test]
    fn test_score_validation() {
        // Valid scores
        let score = Score::new(0.5).unwrap();
        assert_eq!(score.get(), 0.5);

        let zero = Score::zero();
        assert_eq!(zero.get(), 0.0);

        let one = Score::one();
        assert_eq!(one.get(), 1.0);

        // Invalid scores
        assert!(Score::new(-0.1).is_err());
        assert!(Score::new(1.1).is_err());
        assert!(Score::new(f32::NAN).is_err());
    }

    #[test]
    fn test_score_combining() {
        let score1 = Score::new(0.8).unwrap();
        let score2 = Score::new(0.6).unwrap();

        let combined = score1.weighted_combine(score2, 0.7).unwrap();
        assert!((combined.get() - 0.74).abs() < f32::EPSILON);
    }

    #[test]
    fn test_vector_dimension() {
        let dim = VectorDimension::new(384).unwrap();
        assert_eq!(dim.get(), 384);

        let standard = VectorDimension::dimension_384();
        assert_eq!(standard.get(), 384);

        // Invalid dimension
        assert!(VectorDimension::new(0).is_err());

        // Validation
        let vec = vec![0.1; 384];
        assert!(dim.validate_vector(&vec).is_ok());

        let wrong_vec = vec![0.1; 100];
        assert!(dim.validate_vector(&wrong_vec).is_err());
    }
}