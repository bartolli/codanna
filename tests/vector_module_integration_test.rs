//! Integration tests for the vector module public API.
//!
//! This test file verifies that the vector module can be used correctly
//! from external code, testing the complete public API surface.

use codanna::vector::{
    ClusterId, ConcurrentVectorStorage, MmapVectorStorage, Score, SegmentOrdinal, VectorDimension,
    VectorError, VectorId, VectorStorageError, VECTOR_DIMENSION_384,
};
use tempfile::TempDir;

#[test]
fn test_vector_id_public_api() {
    // Test construction methods
    let id = VectorId::new(42).expect("Should create valid VectorId");
    assert_eq!(id.get(), 42);

    // Test zero handling
    assert!(VectorId::new(0).is_none(), "Zero should not be valid");

    // Test unchecked construction
    let id_unchecked = VectorId::new_unchecked(100);
    assert_eq!(id_unchecked.get(), 100);

    // Test serialization
    let bytes = id.to_bytes();
    let deserialized = VectorId::from_bytes(bytes).expect("Should deserialize");
    assert_eq!(id, deserialized);

    // Test that IDs are comparable
    let id2 = VectorId::new(43).expect("Should create valid VectorId");
    assert_ne!(id, id2);
}

#[test]
fn test_cluster_id_public_api() {
    // Test construction
    let cluster = ClusterId::new(1).expect("Should create valid ClusterId");
    assert_eq!(cluster.get(), 1);

    // Test zero is invalid
    assert!(ClusterId::new(0).is_none(), "Zero cluster ID should be invalid");

    // Test serialization round-trip
    let bytes = cluster.to_bytes();
    let deserialized = ClusterId::from_bytes(bytes).expect("Should deserialize");
    assert_eq!(cluster, deserialized);
}

#[test]
fn test_segment_ordinal_public_api() {
    // Segment ordinals can be zero (first segment)
    let seg0 = SegmentOrdinal::new(0);
    assert_eq!(seg0.get(), 0);

    let seg42 = SegmentOrdinal::new(42);
    assert_eq!(seg42.get(), 42);

    // Test ordering
    assert!(seg0 < seg42);

    // Test display formatting
    assert_eq!(format!("{}", seg42), "42");

    // Test serialization
    let bytes = seg42.to_bytes();
    let deserialized = SegmentOrdinal::from_bytes(bytes);
    assert_eq!(seg42, deserialized);
}

#[test]
fn test_score_public_api() {
    // Test valid score creation
    let score = Score::new(0.75).expect("Should create valid score");
    assert_eq!(score.get(), 0.75);

    // Test predefined scores
    assert_eq!(Score::zero().get(), 0.0);
    assert_eq!(Score::one().get(), 1.0);

    // Test invalid scores
    assert!(matches!(
        Score::new(-0.1),
        Err(VectorError::InvalidScore { .. })
    ));
    assert!(matches!(
        Score::new(1.5),
        Err(VectorError::InvalidScore { .. })
    ));
    assert!(matches!(
        Score::new(f32::NAN),
        Err(VectorError::InvalidScore { .. })
    ));

    // Test score combination
    let score1 = Score::new(0.8).expect("Valid score");
    let score2 = Score::new(0.6).expect("Valid score");
    let combined = score1.weighted_combine(score2, 0.7).expect("Valid weight");
    // 0.8 * 0.7 + 0.6 * 0.3 = 0.56 + 0.18 = 0.74
    assert!((combined.get() - 0.74).abs() < 0.001);

    // Test ordering
    assert!(score2 < score1);
}

#[test]
fn test_vector_dimension_public_api() {
    // Test standard dimension
    let dim = VectorDimension::dimension_384();
    assert_eq!(dim.get(), VECTOR_DIMENSION_384);
    assert_eq!(dim.get(), 384);

    // Test custom dimension
    let custom_dim = VectorDimension::new(256).expect("Should create valid dimension");
    assert_eq!(custom_dim.get(), 256);

    // Test zero dimension is invalid
    assert!(matches!(
        VectorDimension::new(0),
        Err(VectorError::InvalidDimension { .. })
    ));

    // Test vector validation
    let vec_correct = vec![0.1; 384];
    assert!(dim.validate_vector(&vec_correct).is_ok());

    let vec_wrong = vec![0.1; 100];
    assert!(matches!(
        dim.validate_vector(&vec_wrong),
        Err(VectorError::DimensionMismatch { expected: 384, actual: 100 })
    ));
}

#[test]
fn test_mmap_vector_storage_basic_operations() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let segment = SegmentOrdinal::new(0);
    let dimension = VectorDimension::new(4).expect("Valid dimension");

    // Test creation - use open_or_create which handles initialization
    let mut storage = MmapVectorStorage::open_or_create(&temp_dir, segment, dimension)
        .expect("Should create storage");
    assert_eq!(storage.vector_count(), 0);
    assert_eq!(storage.dimension(), dimension);

    // Test writing vectors
    let vectors = vec![
        (VectorId::new(1).unwrap(), vec![1.0, 2.0, 3.0, 4.0]),
        (VectorId::new(2).unwrap(), vec![5.0, 6.0, 7.0, 8.0]),
        (VectorId::new(3).unwrap(), vec![9.0, 10.0, 11.0, 12.0]),
    ];

    let vector_refs: Vec<(VectorId, &[f32])> = vectors.iter()
        .map(|(id, vec)| (*id, vec.as_slice()))
        .collect();
    storage.write_batch(&vector_refs).expect("Should write vectors");
    assert_eq!(storage.vector_count(), 3);

    // Test reading vectors
    for (id, expected_vec) in &vectors {
        let read_vec = storage
            .read_vector(*id)
            .expect("Should find vector");
        assert_eq!(&read_vec, expected_vec);
    }

    // Test non-existent vector
    assert!(storage.read_vector(VectorId::new(999).unwrap()).is_none());

    // Test reading all vectors
    let all_vectors = storage.read_all_vectors().expect("Should read all");
    assert_eq!(all_vectors.len(), 3);
    assert_eq!(all_vectors, vectors);
}

#[test]
fn test_mmap_vector_storage_persistence() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let segment = SegmentOrdinal::new(1);
    let dimension = VectorDimension::new(3).expect("Valid dimension");

    // Write vectors in one instance
    {
        let mut storage = MmapVectorStorage::open_or_create(&temp_dir, segment, dimension)
            .expect("Should create storage");

        let vectors = vec![
            (VectorId::new(10).unwrap(), vec![1.1, 2.2, 3.3]),
            (VectorId::new(20).unwrap(), vec![4.4, 5.5, 6.6]),
        ];

        let vector_refs: Vec<(VectorId, &[f32])> = vectors.iter()
            .map(|(id, vec)| (*id, vec.as_slice()))
            .collect();
        storage.write_batch(&vector_refs).expect("Should write");
        assert_eq!(storage.vector_count(), 2);
    }

    // Read vectors in another instance
    {
        let mut storage = MmapVectorStorage::open(&temp_dir, segment)
            .expect("Should open existing storage");

        assert_eq!(storage.vector_count(), 2);
        assert_eq!(storage.dimension().get(), 3);

        let vec1 = storage.read_vector(VectorId::new(10).unwrap()).unwrap();
        assert_eq!(vec1, vec![1.1, 2.2, 3.3]);

        let vec2 = storage.read_vector(VectorId::new(20).unwrap()).unwrap();
        assert_eq!(vec2, vec![4.4, 5.5, 6.6]);
    }
}

#[test]
fn test_concurrent_vector_storage() {
    use std::sync::Arc;
    use std::thread;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let segment = SegmentOrdinal::new(0);
    let dimension = VectorDimension::new(2).expect("Valid dimension");

    let mut base_storage = MmapVectorStorage::open_or_create(&temp_dir, segment, dimension)
        .expect("Should create storage");

    // Write initial data
    let initial_vectors = vec![
        (VectorId::new(1).unwrap(), vec![1.0, 2.0]),
        (VectorId::new(2).unwrap(), vec![3.0, 4.0]),
    ];
    let initial_vector_refs: Vec<(VectorId, &[f32])> = initial_vectors.iter()
        .map(|(id, vec)| (*id, vec.as_slice()))
        .collect();
    base_storage.write_batch(&initial_vector_refs).expect("Should write");

    // Create concurrent storage
    let concurrent_storage = Arc::new(ConcurrentVectorStorage::new(base_storage));

    // Test concurrent reads
    let mut handles = vec![];
    for i in 1..=2 {
        let storage = Arc::clone(&concurrent_storage);
        let handle = thread::spawn(move || {
            let vec = storage
                .read_vector(VectorId::new(i).unwrap())
                .expect("Should read vector");
            assert_eq!(vec.len(), 2);
            vec
        });
        handles.push(handle);
    }

    // Wait for all reads to complete
    let results: Vec<Vec<f32>> = handles
        .into_iter()
        .map(|h| h.join().expect("Thread should succeed"))
        .collect();

    assert_eq!(results[0], vec![1.0, 2.0]);
    assert_eq!(results[1], vec![3.0, 4.0]);

    // Test concurrent write
    let new_vectors = vec![(VectorId::new(3).unwrap(), vec![5.0, 6.0])];
    let new_vector_refs: Vec<(VectorId, &[f32])> = new_vectors.iter()
        .map(|(id, vec)| (*id, vec.as_slice()))
        .collect();
    concurrent_storage
        .write_batch(&new_vector_refs)
        .expect("Should write concurrently");

    // Verify the write
    let vec3 = concurrent_storage
        .read_vector(VectorId::new(3).unwrap())
        .expect("Should find new vector");
    assert_eq!(vec3, vec![5.0, 6.0]);
}

#[test]
fn test_vector_storage_error_handling() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let segment = SegmentOrdinal::new(0);
    let dimension = VectorDimension::new(4).expect("Valid dimension");

    let mut storage = MmapVectorStorage::open_or_create(&temp_dir, segment, dimension)
        .expect("Should create storage");

    // Test dimension mismatch
    let wrong_dim_vectors = vec![(VectorId::new(1).unwrap(), vec![1.0, 2.0])]; // Wrong size
    let wrong_dim_refs: Vec<(VectorId, &[f32])> = wrong_dim_vectors.iter()
        .map(|(id, vec)| (*id, vec.as_slice()))
        .collect();

    let result = storage.write_batch(&wrong_dim_refs);
    assert!(matches!(
        result,
        Err(VectorStorageError::Vector(VectorError::DimensionMismatch { .. }))
    ));

    // Test opening non-existent storage
    let result = MmapVectorStorage::open(&temp_dir, SegmentOrdinal::new(99));
    assert!(matches!(result, Err(VectorStorageError::Io(_))));
}

#[test]
fn test_real_world_usage_pattern() {
    // This test simulates a realistic usage pattern for integrating
    // vector search into the codebase intelligence system
    
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    
    // Simulate multiple segments with different vector counts
    let segments = vec![
        (SegmentOrdinal::new(0), 100),
        (SegmentOrdinal::new(1), 50),
        (SegmentOrdinal::new(2), 75),
    ];
    
    // Use standard embedding dimension
    let dimension = VectorDimension::dimension_384();
    
    // Create and populate storages for each segment
    for (segment, vector_count) in &segments {
        let mut storage = MmapVectorStorage::open_or_create(&temp_dir, *segment, dimension)
            .expect("Should create storage");
        
        // Generate dummy embeddings
        let mut vectors = Vec::new();
        for i in 1..=*vector_count {
            let id = VectorId::new(i as u32).unwrap();
            let embedding = vec![i as f32 / 100.0; dimension.get()];
            vectors.push((id, embedding));
        }
        
        let vector_refs: Vec<(VectorId, &[f32])> = vectors.iter()
            .map(|(id, vec)| (*id, vec.as_slice()))
            .collect();
        storage.write_batch(&vector_refs).expect("Should write batch");
    }
    
    // Verify we can open and read from all segments
    for (segment, expected_count) in &segments {
        let mut storage = MmapVectorStorage::open(&temp_dir, *segment)
            .expect("Should open existing storage");
        
        assert_eq!(storage.vector_count(), *expected_count);
        assert_eq!(storage.dimension(), dimension);
        
        // Read a sample vector
        let sample_id = VectorId::new(1).unwrap();
        let vector = storage.read_vector(sample_id).expect("Should find vector");
        assert_eq!(vector.len(), dimension.get());
    }
}

#[test]
fn test_vector_error_types() {
    // Test the various error types are properly exposed and usable
    
    // DimensionMismatch
    let dim = VectorDimension::new(10).unwrap();
    let result = dim.validate_vector(&vec![1.0; 5]);
    assert!(matches!(
        result,
        Err(VectorError::DimensionMismatch { expected: 10, actual: 5 })
    ));
    
    // InvalidScore
    let score_result = Score::new(2.0);
    assert!(matches!(
        score_result,
        Err(VectorError::InvalidScore { .. })
    ));
    
    // InvalidDimension
    let dim_result = VectorDimension::new(0);
    assert!(matches!(
        dim_result,
        Err(VectorError::InvalidDimension { dimension: 0, .. })
    ));
    
    // Storage error (from IO)
    let bad_path = "/nonexistent/path/vectors";
    let result = MmapVectorStorage::open(bad_path, SegmentOrdinal::new(0));
    assert!(matches!(result, Err(VectorStorageError::Io(_))));
}