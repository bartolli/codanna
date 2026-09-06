use codanna::vector::{MmapVectorStorage, SegmentOrdinal, VectorDimension, VectorId};
use std::time::Instant;

fn check_batches(count: u32) {
    let dir = tempfile::tempdir().unwrap();
    let segment = SegmentOrdinal::new(0);
    let mut storage =
        MmapVectorStorage::new(dir.path(), segment, VectorDimension::dimension_384()).unwrap();
    let vectors: Vec<_> = (1..=count)
        .map(|id| (VectorId::new(id).unwrap(), vec![id as f32; 384]))
        .collect();
    let batch: Vec<_> = vectors.iter().map(|(id, v)| (*id, v.as_slice())).collect();
    let middle = batch.len() / 2;

    let start = Instant::now();
    storage.write_batch(&batch[..middle]).unwrap();
    let first_write = start.elapsed();
    assert_eq!(storage.read_all_vectors().unwrap(), vectors[..middle]);
    let start = Instant::now();
    storage.write_batch(&batch[middle..]).unwrap();
    println!(
        "{count} vectors, 384 dimensions: {:?} writing",
        first_write + start.elapsed()
    );

    let mut reopened = MmapVectorStorage::open(dir.path(), segment).unwrap();
    assert_eq!(reopened.vector_count(), count as usize);
    assert_eq!(reopened.read_all_vectors().unwrap(), vectors);
    assert_eq!(
        std::fs::metadata(dir.path().join("segment_0.vec"))
            .unwrap()
            .len(),
        16 + u64::from(count) * (4 + 384 * 4)
    );
}

#[test]
fn appended_batches_are_readable_after_reopen() {
    check_batches(32);
}

#[test]
#[ignore = "measures a large synthetic embedding batch; run explicitly for write throughput"]
fn large_embedding_batch_roundtrips() {
    check_batches(10_000);
}
