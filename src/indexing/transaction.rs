//! Transaction support for atomic indexing operations
//!
//! This module provides transactional guarantees for index updates,
//! ensuring that either all changes are committed or none are.

use crate::{IndexData, FileId, SymbolId};

/// A transaction that can be committed or rolled back
pub struct IndexTransaction {
    /// Snapshot of data before transaction started
    snapshot: IndexData,
    /// Whether this transaction has been committed or rolled back
    completed: bool,
}

impl IndexTransaction {
    /// Create a new transaction with a snapshot of current data
    pub fn new(current_data: &IndexData) -> Self {
        Self {
            snapshot: current_data.clone(),
            completed: false,
        }
    }
    
    /// Get the snapshot data for rollback
    pub fn snapshot(&self) -> &IndexData {
        &self.snapshot
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
pub struct FileTransaction {
    pub file_id: FileId,
    pub path: String,
    pub old_symbols: Vec<SymbolId>,
    pub transaction: IndexTransaction,
}

impl FileTransaction {
    pub fn new(file_id: FileId, path: String, old_symbols: Vec<SymbolId>, data: &IndexData) -> Self {
        Self {
            file_id,
            path,
            old_symbols,
            transaction: IndexTransaction::new(data),
        }
    }
}