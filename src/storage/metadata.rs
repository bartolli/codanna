//! Metadata tracking for index state and data sources

use crate::IndexResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Current emission-semantics version. Bump in the same commit as any
/// parser or pipeline change that alters persisted row semantics (the
/// commits that force a full-heal note in release notes). Compared
/// against the stored stamp before an existing index is read or
/// extended; a mismatch forces a full rebuild -- an incremental pass
/// over rows from another version leaves a silent hybrid.
pub const EMISSION_SEMANTICS_VERSION: u32 = 3;

/// Short commit of the binary, `-dirty` when it was built from a modified
/// tree. `None` when built without a work tree (release tarballs), which
/// distinguishes a packaged build from a local one.
///
/// Descriptive only. Unlike [`EMISSION_SEMANTICS_VERSION`] this never gates
/// a read: binaries are rebuilt constantly during development and an index
/// stays valid across them.
pub fn builder_commit() -> Option<&'static str> {
    option_env!("CODANNA_GIT_COMMIT")
}

/// Metadata about the index state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Version of the index format
    pub version: u32,

    /// Current data source
    pub data_source: DataSource,

    /// Number of symbols in the index
    pub symbol_count: u32,

    /// Number of files in the index
    pub file_count: u32,

    /// Last modification timestamp
    pub last_modified: u64,

    /// Directories that were indexed (canonicalized paths)
    /// Used to detect config changes and auto-sync on load
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_paths: Option<Vec<PathBuf>>,

    /// Emission-semantics version of the binary that built this index.
    /// Stamped at save; absent on pre-gate indexes, and absence reads
    /// as a mismatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emission_version: Option<u32>,

    /// Commit of the binary that last wrote this index, `-dirty` when that
    /// binary came from a modified tree. Two binaries reporting the same
    /// `--version` between releases are told apart by this field and by
    /// nothing else.
    ///
    /// Descriptive: no read path compares it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder_commit: Option<String>,
}

/// Describes where the index data came from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    /// Loaded from Tantivy index
    Tantivy {
        path: PathBuf,
        doc_count: u64,
        timestamp: u64,
    },

    /// Fresh index (not loaded)
    Fresh,
}

impl Default for IndexMetadata {
    fn default() -> Self {
        Self {
            version: 1,
            data_source: DataSource::Fresh,
            symbol_count: 0,
            file_count: 0,
            last_modified: crate::indexing::get_utc_timestamp(),
            indexed_paths: None,
            emission_version: None,
            builder_commit: None,
        }
    }
}

impl IndexMetadata {
    /// Create new metadata for a fresh index
    pub fn new() -> Self {
        Self::default()
    }

    /// Update counts from the indexer
    pub fn update_counts(&mut self, symbol_count: u32, file_count: u32) {
        self.symbol_count = symbol_count;
        self.file_count = file_count;
        self.last_modified = crate::indexing::get_utc_timestamp();
    }

    /// Update indexed paths from the indexer
    pub fn update_indexed_paths(&mut self, paths: Vec<PathBuf>) {
        self.indexed_paths = Some(paths);
        self.last_modified = crate::indexing::get_utc_timestamp();
    }

    /// Save metadata to file
    pub fn save(&self, base_path: &Path) -> IndexResult<()> {
        let metadata_path = base_path.join("index.meta");
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            crate::IndexError::General(format!("Failed to serialize metadata: {e}"))
        })?;

        fs::write(&metadata_path, json).map_err(|e| crate::IndexError::FileWrite {
            path: metadata_path,
            source: e,
        })?;

        Ok(())
    }

    /// Load metadata from file
    pub fn load(base_path: &Path) -> IndexResult<Self> {
        let metadata_path = base_path.join("index.meta");

        if !metadata_path.exists() {
            return Ok(Self::new());
        }

        let json = fs::read_to_string(&metadata_path).map_err(|e| crate::IndexError::FileRead {
            path: metadata_path.clone(),
            source: e,
        })?;

        serde_json::from_str(&json)
            .map_err(|e| crate::IndexError::General(format!("Failed to parse metadata: {e}")))
    }

    /// Display source information to the user
    pub fn display_source(&self) {
        match &self.data_source {
            DataSource::Tantivy {
                path, doc_count, ..
            } => {
                eprintln!(
                    "Loaded from Tantivy index: {} ({} documents)",
                    path.display(),
                    doc_count
                );
            }
            DataSource::Fresh => {
                eprintln!("Created fresh index");
            }
        }
        eprintln!(
            "Index contains {} symbols from {} files",
            self.symbol_count, self.file_count
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emission_version_round_trips() {
        let mut meta = IndexMetadata::new();
        meta.emission_version = Some(EMISSION_SEMANTICS_VERSION);
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: IndexMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.emission_version, Some(EMISSION_SEMANTICS_VERSION));
    }

    #[test]
    fn pre_gate_metadata_reads_as_unstamped() {
        // A 0.9.23-era index.meta: no emission_version field.
        let legacy = r#"{
            "version": 1,
            "data_source": "Fresh",
            "symbol_count": 42,
            "file_count": 7,
            "last_modified": 0
        }"#;
        let meta: IndexMetadata = serde_json::from_str(legacy).expect("legacy parse");
        assert_eq!(meta.emission_version, None);
    }

    #[test]
    fn unstamped_metadata_serializes_without_field() {
        let meta = IndexMetadata::new();
        let json = serde_json::to_string(&meta).expect("serialize");
        assert!(!json.contains("emission_version"));
        assert!(!json.contains("builder_commit"));
    }

    // The stamp exists to separate two binaries reporting the same
    // `--version`, so it has to survive the round trip verbatim -- including
    // the `-dirty` suffix, which is what says the commit does not identify
    // the code that ran.
    #[test]
    fn builder_commit_round_trips_including_dirty_suffix() {
        let mut meta = IndexMetadata::new();
        meta.builder_commit = Some("1f48208-dirty".to_string());
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: IndexMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.builder_commit.as_deref(), Some("1f48208-dirty"));
    }

    // Indexes written before the stamp existed stay readable: unlike
    // emission_version, absence here carries no verdict.
    #[test]
    fn metadata_without_builder_commit_reads_as_unknown() {
        let prior = r#"{
            "version": 1,
            "data_source": "Fresh",
            "symbol_count": 42,
            "file_count": 7,
            "last_modified": 0,
            "emission_version": 3
        }"#;
        let meta: IndexMetadata = serde_json::from_str(prior).expect("parse");
        assert_eq!(meta.builder_commit, None);
        assert_eq!(meta.emission_version, Some(3));
    }
}
