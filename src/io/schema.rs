//! Unified output schema for consistent CLI and MCP output
//!
//! Zero-cost abstractions for different output shapes while maintaining
//! type safety and avoiding allocations in hot paths.

use crate::io::ExitCode;
use crate::symbol::Symbol;
use crate::types::{SymbolId, SymbolKind};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

/// Unified output structure that adapts to different data shapes
///
/// Uses borrowed types to avoid allocations when piping output
#[derive(Debug, Clone, Serialize)]
pub struct UnifiedOutput<'a, T> {
    /// Status of the operation
    pub status: OutputStatus,

    /// Type of entities in the output
    pub entity_type: EntityType,

    /// Count of items in the output
    pub count: usize,

    /// The actual data in various shapes
    #[serde(flatten)]
    pub data: OutputData<'a, T>,

    /// Optional metadata about the output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<OutputMetadata<'a>>,

    /// Optional AI guidance message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance: Option<Cow<'a, str>>,

    /// Exit code for Unix compliance
    #[serde(skip)]
    pub exit_code: ExitCode,
}

/// Status of the operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStatus {
    Success,
    NotFound,
    PartialSuccess,
    Error,
}

/// Type of entities being output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Symbol,
    Function,
    Class,
    Trait,
    Interface,
    Module,
    Variable,
    SearchResult,
    Impact,
    IndexInfo,
    Mixed,
}

/// Different shapes of output data
///
/// Uses generic lifetime 'a to borrow strings without allocation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputData<'a, T> {
    /// Simple list of items (most common case)
    Items { items: Vec<T> },

    /// Items grouped by category (e.g., by SymbolKind)
    Grouped {
        groups: HashMap<Cow<'a, str>, Vec<T>>,
    },

    /// Items with rich context (e.g., semantic_search_with_context)
    Contextual { results: Vec<ContextualItem<'a, T>> },

    /// Ranked search results with scores
    Ranked { results: Vec<RankedItem<'a, T>> },

    /// Single item result
    Single { item: T },

    /// Empty result
    Empty,
}

/// Item with additional context information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualItem<'a, T> {
    /// The main item
    pub item: T,

    /// Additional context as key-value pairs
    /// Uses Cow to avoid allocation when borrowing static strings
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub context: HashMap<Cow<'a, str>, serde_json::Value>,

    /// Optional relationships to other items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationships: Option<ItemRelationships<'a>>,
}

/// Ranked item with score and optional metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedItem<'a, T> {
    /// The main item
    pub item: T,

    /// Relevance score (0.0 to 1.0 for similarity, unbounded for other scores)
    pub score: f32,

    /// Optional rank position (1-based)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,

    /// Additional metadata as key-value pairs
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<Cow<'a, str>, Cow<'a, str>>,
}

/// Relationships between items (zero-cost when not used)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemRelationships<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub called_by: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub implemented_by: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extended_by: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub used_by: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub defines: Vec<RelatedItem<'a>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub impacts: Vec<RelatedItem<'a>>,
}

/// A related item with minimal information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedItem<'a> {
    pub id: SymbolId,
    pub name: Cow<'a, str>,
    pub kind: SymbolKind,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<Cow<'a, str>>,
}

/// Optional metadata about the output
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputMetadata<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<Cow<'a, str>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<Cow<'a, str>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing_ms: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,

    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<Cow<'a, str>, serde_json::Value>,
}

// Conversion traits for zero-cost transformations

/// Convert types into UnifiedOutput without allocation
pub trait IntoUnifiedOutput<'a> {
    /// Convert self into UnifiedOutput
    /// Uses lifetime 'a to borrow strings without allocation
    fn into_unified(self, entity_type: EntityType) -> UnifiedOutput<'a, Self>
    where
        Self: Sized;
}

/// Builder for UnifiedOutput with ergonomic API
pub struct UnifiedOutputBuilder<'a, T> {
    status: OutputStatus,
    entity_type: EntityType,
    data: OutputData<'a, T>,
    metadata: Option<OutputMetadata<'a>>,
    guidance: Option<Cow<'a, str>>,
}

impl<'a, T> UnifiedOutputBuilder<'a, T> {
    /// Create a new builder with items
    pub fn items(items: Vec<T>, entity_type: EntityType) -> Self {
        Self {
            status: if items.is_empty() {
                OutputStatus::NotFound
            } else {
                OutputStatus::Success
            },
            entity_type,
            data: OutputData::Items { items },
            metadata: None,
            guidance: None,
        }
    }

    /// Create a new builder with grouped items
    pub fn grouped(groups: HashMap<Cow<'a, str>, Vec<T>>, entity_type: EntityType) -> Self {
        let count: usize = groups.values().map(|v| v.len()).sum();
        Self {
            status: if count == 0 {
                OutputStatus::NotFound
            } else {
                OutputStatus::Success
            },
            entity_type,
            data: OutputData::Grouped { groups },
            metadata: None,
            guidance: None,
        }
    }

    /// Create a new builder with ranked results
    pub fn ranked(results: Vec<RankedItem<'a, T>>, entity_type: EntityType) -> Self {
        Self {
            status: if results.is_empty() {
                OutputStatus::NotFound
            } else {
                OutputStatus::Success
            },
            entity_type,
            data: OutputData::Ranked { results },
            metadata: None,
            guidance: None,
        }
    }

    /// Create a new builder with contextual results
    pub fn contextual(results: Vec<ContextualItem<'a, T>>, entity_type: EntityType) -> Self {
        Self {
            status: if results.is_empty() {
                OutputStatus::NotFound
            } else {
                OutputStatus::Success
            },
            entity_type,
            data: OutputData::Contextual { results },
            metadata: None,
            guidance: None,
        }
    }

    /// Add metadata to the output
    pub fn with_metadata(mut self, metadata: OutputMetadata<'a>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Add guidance message
    pub fn with_guidance(mut self, guidance: impl Into<Cow<'a, str>>) -> Self {
        self.guidance = Some(guidance.into());
        self
    }

    /// Override the status
    pub fn with_status(mut self, status: OutputStatus) -> Self {
        self.status = status;
        self
    }

    /// Build the UnifiedOutput
    pub fn build(self) -> UnifiedOutput<'a, T> {
        let count = match &self.data {
            OutputData::Items { items } => items.len(),
            OutputData::Grouped { groups } => groups.values().map(|v| v.len()).sum(),
            OutputData::Contextual { results } => results.len(),
            OutputData::Ranked { results } => results.len(),
            OutputData::Single { .. } => 1,
            OutputData::Empty => 0,
        };

        let exit_code = match self.status {
            OutputStatus::Success => ExitCode::Success,
            OutputStatus::NotFound => ExitCode::NotFound,
            OutputStatus::PartialSuccess => ExitCode::Success,
            OutputStatus::Error => ExitCode::GeneralError,
        };

        UnifiedOutput {
            status: self.status,
            entity_type: self.entity_type,
            count,
            data: self.data,
            metadata: self.metadata,
            guidance: self.guidance,
            exit_code,
        }
    }
}

// Helper functions for common patterns

/// Create output for a simple list of symbols
pub fn symbol_list_output(symbols: Vec<Symbol>) -> UnifiedOutput<'static, Symbol> {
    UnifiedOutputBuilder::items(symbols, EntityType::Symbol).build()
}

/// Create output for search results with scores
pub fn search_results_output<'a, T>(
    results: Vec<(T, f32)>,
    query: &'a str,
) -> UnifiedOutput<'a, T> {
    let ranked: Vec<RankedItem<'a, T>> = results
        .into_iter()
        .enumerate()
        .map(|(i, (item, score))| RankedItem {
            item,
            score,
            rank: Some(i + 1),
            metadata: HashMap::new(),
        })
        .collect();

    UnifiedOutputBuilder::ranked(ranked, EntityType::SearchResult)
        .with_metadata(OutputMetadata {
            query: Some(Cow::Borrowed(query)),
            ..Default::default()
        })
        .build()
}

/// Create output for impact analysis grouped by kind
pub fn impact_analysis_output<'a>(
    symbols_by_kind: HashMap<SymbolKind, Vec<Symbol>>,
) -> UnifiedOutput<'a, Symbol> {
    let groups: HashMap<Cow<'a, str>, Vec<Symbol>> = symbols_by_kind
        .into_iter()
        .map(|(kind, symbols)| {
            let kind_str = format!("{kind:?}");
            (Cow::Owned(kind_str), symbols)
        })
        .collect();

    UnifiedOutputBuilder::grouped(groups, EntityType::Impact).build()
}

// Display implementation for text output

impl<'a, T: fmt::Display> fmt::Display for UnifiedOutput<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.data {
            OutputData::Items { items } => {
                for item in items {
                    writeln!(f, "{item}")?;
                }
            }
            OutputData::Grouped { groups } => {
                for (group_name, items) in groups {
                    writeln!(f, "\n{} ({}):", group_name, items.len())?;
                    for item in items {
                        writeln!(f, "  {item}")?;
                    }
                }
            }
            OutputData::Contextual { results } => {
                for result in results {
                    let item = &result.item;
                    writeln!(f, "{item}")?;
                    if !result.context.is_empty() {
                        writeln!(f, "  Context:")?;
                        for (key, value) in &result.context {
                            writeln!(f, "    {key}: {value}")?;
                        }
                    }
                }
            }
            OutputData::Ranked { results } => {
                for result in results {
                    if let Some(rank) = result.rank {
                        write!(f, "{rank}. ")?;
                    }
                    let item = &result.item;
                    write!(f, "{item}")?;
                    let score = result.score;
                    writeln!(f, " (score: {score:.3})")?;
                }
            }
            OutputData::Single { item } => {
                writeln!(f, "{item}")?;
            }
            OutputData::Empty => {
                writeln!(f, "No results found")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_cost_builder() {
        let symbols: Vec<Symbol> = vec![];
        let output = UnifiedOutputBuilder::items(symbols, EntityType::Symbol).build();

        assert_eq!(output.status, OutputStatus::NotFound);
        assert_eq!(output.count, 0);
        assert_eq!(output.exit_code, ExitCode::NotFound);
    }

    #[test]
    fn test_ranked_output() {
        let results = vec![("item1", 0.95), ("item2", 0.85)];

        let output = search_results_output(results, "test query");
        assert_eq!(output.count, 2);
        assert_eq!(output.entity_type, EntityType::SearchResult);

        if let OutputData::Ranked { results } = output.data {
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].score, 0.95);
            assert_eq!(results[0].rank, Some(1));
        } else {
            panic!("Expected Ranked variant");
        }
    }
}
