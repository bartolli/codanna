//! Configuration types for document chunking and collections.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level configuration for the documents feature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentsConfig {
    /// Whether document indexing is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Default chunking configuration (applies to all collections unless overridden).
    #[serde(default)]
    pub defaults: ChunkingConfig,

    /// Search result display configuration.
    #[serde(default)]
    pub search: SearchConfig,

    /// Named collections of documents.
    #[serde(default)]
    pub collections: HashMap<String, CollectionConfig>,
}

/// Configuration for search result display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Preview mode: "full" shows entire chunk, "kwic" centers on keyword.
    #[serde(default)]
    pub preview_mode: PreviewMode,

    /// Number of characters to show in preview (for kwic mode).
    #[serde(default = "default_preview_chars")]
    pub preview_chars: usize,

    /// Whether to highlight matching keywords in preview.
    #[serde(default = "default_highlight")]
    pub highlight: bool,
}

fn default_preview_chars() -> usize {
    600
}

fn default_highlight() -> bool {
    true
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            preview_mode: PreviewMode::default(),
            preview_chars: default_preview_chars(),
            highlight: default_highlight(),
        }
    }
}

/// Preview mode for search results.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PreviewMode {
    /// Show entire chunk content.
    Full,
    /// Keyword In Context: center preview window around first match.
    #[default]
    Kwic,
}

/// Configuration for a single document collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionConfig {
    /// Paths to include (directories or individual files).
    #[serde(default)]
    pub paths: Vec<PathBuf>,

    /// Glob patterns for file matching (e.g., "**/*.md").
    #[serde(default)]
    pub patterns: Vec<String>,

    /// Chunking strategy (overrides defaults).
    pub strategy: Option<ChunkingStrategy>,

    /// Minimum chunk size in characters (overrides defaults).
    pub min_chunk_chars: Option<usize>,

    /// Maximum chunk size in characters (overrides defaults).
    pub max_chunk_chars: Option<usize>,

    /// Overlap between chunks in characters (overrides defaults).
    pub overlap_chars: Option<usize>,
}

impl CollectionConfig {
    /// Merge with default config to get effective chunking settings.
    pub fn effective_chunking(&self, defaults: &ChunkingConfig) -> ChunkingConfig {
        ChunkingConfig {
            strategy: self.strategy.clone().unwrap_or(defaults.strategy.clone()),
            min_chunk_chars: self.min_chunk_chars.unwrap_or(defaults.min_chunk_chars),
            max_chunk_chars: self.max_chunk_chars.unwrap_or(defaults.max_chunk_chars),
            overlap_chars: self.overlap_chars.unwrap_or(defaults.overlap_chars),
        }
    }

    /// Get default patterns if none specified.
    pub fn effective_patterns(&self) -> Vec<String> {
        if self.patterns.is_empty() {
            vec!["**/*.md".to_string(), "**/*.txt".to_string()]
        } else {
            self.patterns.clone()
        }
    }
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            patterns: vec!["**/*.md".to_string()],
            strategy: None,
            min_chunk_chars: None,
            max_chunk_chars: None,
            overlap_chars: None,
        }
    }
}

/// Configuration for document chunking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingConfig {
    /// Chunking strategy to use.
    #[serde(default)]
    pub strategy: ChunkingStrategy,

    /// Minimum chunk size in characters. Smaller chunks are merged.
    #[serde(default = "default_min_chunk_chars")]
    pub min_chunk_chars: usize,

    /// Maximum chunk size in characters. Larger chunks are split.
    #[serde(default = "default_max_chunk_chars")]
    pub max_chunk_chars: usize,

    /// Overlap between adjacent chunks in characters.
    #[serde(default = "default_overlap_chars")]
    pub overlap_chars: usize,
}

fn default_min_chunk_chars() -> usize {
    200
}

fn default_max_chunk_chars() -> usize {
    1500
}

fn default_overlap_chars() -> usize {
    100
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            strategy: ChunkingStrategy::default(),
            min_chunk_chars: default_min_chunk_chars(),
            max_chunk_chars: default_max_chunk_chars(),
            overlap_chars: default_overlap_chars(),
        }
    }
}

impl ChunkingConfig {
    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), String> {
        if self.min_chunk_chars >= self.max_chunk_chars {
            return Err(format!(
                "min_chunk_chars ({}) must be less than max_chunk_chars ({})",
                self.min_chunk_chars, self.max_chunk_chars
            ));
        }

        if self.overlap_chars >= self.min_chunk_chars {
            return Err(format!(
                "overlap_chars ({}) should be less than min_chunk_chars ({})",
                self.overlap_chars, self.min_chunk_chars
            ));
        }

        Ok(())
    }
}

/// Strategy for splitting documents into chunks.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChunkingStrategy {
    /// Hybrid strategy: paragraph-based with size constraints.
    /// Splits on double newlines, merges small chunks, splits large chunks with overlap.
    #[default]
    Hybrid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking_config_defaults() {
        let config = ChunkingConfig::default();
        assert_eq!(config.min_chunk_chars, 200);
        assert_eq!(config.max_chunk_chars, 1500);
        assert_eq!(config.overlap_chars, 100);
        assert_eq!(config.strategy, ChunkingStrategy::Hybrid);
    }

    #[test]
    fn test_chunking_config_validation() {
        let mut config = ChunkingConfig::default();

        // Valid config
        assert!(config.validate().is_ok());

        // Invalid: min >= max
        config.min_chunk_chars = 2000;
        assert!(config.validate().is_err());

        // Invalid: overlap >= min
        config.min_chunk_chars = 200;
        config.overlap_chars = 300;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_collection_effective_chunking() {
        let defaults = ChunkingConfig::default();

        // No overrides
        let collection = CollectionConfig::default();
        let effective = collection.effective_chunking(&defaults);
        assert_eq!(effective.max_chunk_chars, 1500);

        // With override
        let collection = CollectionConfig {
            max_chunk_chars: Some(2000),
            ..Default::default()
        };
        let effective = collection.effective_chunking(&defaults);
        assert_eq!(effective.max_chunk_chars, 2000);
        assert_eq!(effective.min_chunk_chars, 200); // Still default
    }

    #[test]
    fn test_documents_config_default() {
        let config = DocumentsConfig::default();
        assert!(!config.enabled);
        assert!(config.collections.is_empty());
    }
}
