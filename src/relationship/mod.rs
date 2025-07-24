use crate::types::SymbolId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationKind {
    Calls,
    CalledBy,
    Extends,
    ExtendedBy,
    Implements,
    ImplementedBy,
    Uses,
    UsedBy,
    Defines,
    DefinedIn,
    References,
    ReferencedBy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub kind: RelationKind,
    pub weight: f32,
    pub metadata: Option<RelationshipMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipMetadata {
    pub line: Option<u32>,
    pub column: Option<u16>,
    pub context: Option<Box<str>>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CompactRelationship {
    pub source_id: u32,
    pub target_id: u32,
    pub kind: u8,
    pub weight: f32,
    pub metadata_offset: u32,
}

impl Relationship {
    pub fn new(kind: RelationKind) -> Self {
        Self {
            kind,
            weight: 1.0,
            metadata: None,
        }
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_metadata(mut self, metadata: RelationshipMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn to_compact(&self) -> CompactRelationship {
        CompactRelationship {
            source_id: 0, // Will be set when adding to graph
            target_id: 0, // Will be set when adding to graph
            kind: self.kind as u8,
            weight: self.weight,
            metadata_offset: 0, // Would need metadata storage
        }
    }
}

impl RelationKind {
    pub fn inverse(&self) -> Self {
        match self {
            Self::Calls => Self::CalledBy,
            Self::CalledBy => Self::Calls,
            Self::Extends => Self::ExtendedBy,
            Self::ExtendedBy => Self::Extends,
            Self::Implements => Self::ImplementedBy,
            Self::ImplementedBy => Self::Implements,
            Self::Uses => Self::UsedBy,
            Self::UsedBy => Self::Uses,
            Self::Defines => Self::DefinedIn,
            Self::DefinedIn => Self::Defines,
            Self::References => Self::ReferencedBy,
            Self::ReferencedBy => Self::References,
        }
    }

    pub fn is_hierarchical(&self) -> bool {
        matches!(
            self,
            Self::Extends | Self::ExtendedBy | Self::Implements | Self::ImplementedBy
        )
    }

    pub fn is_usage(&self) -> bool {
        matches!(
            self,
            Self::Calls | Self::CalledBy | Self::Uses | Self::UsedBy | Self::References | Self::ReferencedBy
        )
    }
}

impl RelationshipMetadata {
    pub fn new() -> Self {
        Self {
            line: None,
            column: None,
            context: None,
        }
    }

    pub fn at_position(mut self, line: u32, column: u16) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_context(mut self, context: impl Into<Box<str>>) -> Self {
        self.context = Some(context.into());
        self
    }
}

pub struct RelationshipEdge {
    pub source: SymbolId,
    pub target: SymbolId,
    pub relationship: Relationship,
}

impl RelationshipEdge {
    pub fn new(source: SymbolId, target: SymbolId, relationship: Relationship) -> Self {
        Self {
            source,
            target,
            relationship,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_relationship_creation() {
        let rel = Relationship::new(RelationKind::Calls);
        assert_eq!(rel.kind, RelationKind::Calls);
        assert_eq!(rel.weight, 1.0);
        assert!(rel.metadata.is_none());
    }

    #[test]
    fn test_relationship_with_weight() {
        let rel = Relationship::new(RelationKind::Extends)
            .with_weight(0.8);
        assert_eq!(rel.weight, 0.8);
    }

    #[test]
    fn test_relationship_with_metadata() {
        let metadata = RelationshipMetadata::new()
            .at_position(10, 5)
            .with_context("inside main function");
            
        let rel = Relationship::new(RelationKind::Calls)
            .with_metadata(metadata);
            
        let meta = rel.metadata.unwrap();
        assert_eq!(meta.line, Some(10));
        assert_eq!(meta.column, Some(5));
        assert_eq!(meta.context.as_deref(), Some("inside main function"));
    }

    #[test]
    fn test_relation_kind_inverse() {
        assert_eq!(RelationKind::Calls.inverse(), RelationKind::CalledBy);
        assert_eq!(RelationKind::CalledBy.inverse(), RelationKind::Calls);
        assert_eq!(RelationKind::Extends.inverse(), RelationKind::ExtendedBy);
        assert_eq!(RelationKind::Implements.inverse(), RelationKind::ImplementedBy);
        assert_eq!(RelationKind::Uses.inverse(), RelationKind::UsedBy);
        assert_eq!(RelationKind::References.inverse(), RelationKind::ReferencedBy);
    }

    #[test]
    fn test_relation_kind_classification() {
        // Hierarchical relationships
        assert!(RelationKind::Extends.is_hierarchical());
        assert!(RelationKind::ExtendedBy.is_hierarchical());
        assert!(RelationKind::Implements.is_hierarchical());
        assert!(RelationKind::ImplementedBy.is_hierarchical());
        
        // Usage relationships
        assert!(RelationKind::Calls.is_usage());
        assert!(RelationKind::CalledBy.is_usage());
        assert!(RelationKind::Uses.is_usage());
        assert!(RelationKind::References.is_usage());
        
        // Not usage or hierarchical
        assert!(!RelationKind::Defines.is_usage());
        assert!(!RelationKind::Defines.is_hierarchical());
    }

    #[test]
    fn test_compact_relationship_size() {
        assert_eq!(mem::size_of::<CompactRelationship>(), 20);
    }

    #[test]
    fn test_relationship_edge() {
        let source = SymbolId::new(1).unwrap();
        let target = SymbolId::new(2).unwrap();
        let rel = Relationship::new(RelationKind::Calls);
        
        let edge = RelationshipEdge::new(source, target, rel);
        assert_eq!(edge.source, source);
        assert_eq!(edge.target, target);
        assert_eq!(edge.relationship.kind, RelationKind::Calls);
    }
}