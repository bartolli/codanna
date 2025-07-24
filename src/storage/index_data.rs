//! Simple serializable index data structure
//! 
//! This is just plain data - no custom serialization needed!

use serde::{Serialize, Deserialize};
use crate::{Symbol, SymbolId, Relationship, FileId};
use std::collections::HashMap;

/// Plain data structure that can be serialized/deserialized
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IndexData {
    pub symbols: Vec<Symbol>,
    pub relationships: Vec<(SymbolId, SymbolId, Relationship)>,
    pub file_map: HashMap<String, FileId>,
    pub file_counter: u32,
}

impl IndexData {
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            relationships: Vec::new(),
            file_map: HashMap::new(),
            file_counter: 1,
        }
    }
}

impl Default for IndexData {
    fn default() -> Self {
        Self::new()
    }
}