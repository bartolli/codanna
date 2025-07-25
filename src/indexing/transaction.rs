//! Transaction support for atomic indexing operations
//!
//! This module provides transactional guarantees for index updates,
//! ensuring that either all changes are committed or none are.
//!
//! With Tantivy-only architecture, transactions are handled by Tantivy's
//! writer commit system, so this is now a lightweight compatibility layer.

/// A transaction that can be committed or rolled back
/// 
/// In the Tantivy-only architecture, this is a lightweight wrapper
/// since Tantivy handles transactions internally through its writer.
#[derive(Debug)]
pub struct IndexTransaction {
    /// Whether this transaction has been committed or rolled back
    completed: bool,
}

impl IndexTransaction {
    /// Create a new transaction
    pub fn new(_data: &()) -> Self {
        Self {
            completed: false,
        }
    }
    
    /// Get the snapshot data for rollback (no longer applicable)
    #[deprecated(note = "Snapshot functionality is handled by Tantivy")]
    pub fn snapshot(&self) -> &() {
        &()
    }
    
    /// Mark transaction as completed
    pub fn complete(&mut self) {
        self.completed = true;
    }
    
    /// Check if transaction is still active
    pub fn is_active(&self) -> bool {
        !self.completed
    }
}

impl Drop for IndexTransaction {
    fn drop(&mut self) {
        if !self.completed {
            eprintln!("Warning: IndexTransaction dropped without explicit commit or rollback");
        }
    }
}

/// Transaction context for atomic file operations
/// 
/// With Tantivy, file operations are atomic within a batch
pub struct FileTransaction {
    file_id: Option<crate::FileId>,
    completed: bool,
}

impl FileTransaction {
    /// Create a new file transaction
    pub fn new() -> Self {
        Self {
            file_id: None,
            completed: false,
        }
    }
    
    /// Set the file ID for this transaction
    pub fn set_file_id(&mut self, file_id: crate::FileId) {
        self.file_id = Some(file_id);
    }
    
    /// Get the file ID if set
    pub fn file_id(&self) -> Option<crate::FileId> {
        self.file_id
    }
    
    /// Mark transaction as completed
    pub fn complete(&mut self) {
        self.completed = true;
    }
}