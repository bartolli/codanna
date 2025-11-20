//! Resolution index persistence for project resolvers
//!
//! Implements Sprint 1 Epic D: Resolution Index & Watch
//! Persists resolution mappings to `.codanna/index/resolvers/` with SHA validation

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{ResolutionError, ResolutionResult, Sha256Hash};

/// Version of the resolution index schema
pub const RESOLUTION_INDEX_VERSION: &str = "1.0";

/// Resolution index schema v1 for TypeScript
///
/// Stored at `.codanna/index/resolvers/typescript_resolution.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionIndex {
    /// Schema version for forward compatibility
    pub version: String,

    /// SHA-256 hashes of config files for invalidation detection
    pub hashes: HashMap<PathBuf, String>,

    /// File pattern to config file mappings (e.g., "src/**/*.ts" -> "/path/tsconfig.json")
    pub mappings: HashMap<String, PathBuf>,

    /// Compiled resolution rules per config file
    pub rules: HashMap<PathBuf, ResolutionRules>,
}

/// Resolution rules extracted from a config file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionRules {
    /// Base URL for path resolution
    pub base_url: Option<String>,

    /// Path alias mappings (e.g., "@app/*" -> ["src/app/*"])
    pub paths: HashMap<String, Vec<String>>,
}

impl Default for ResolutionIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ResolutionIndex {
    /// Create a new resolution index with current schema version
    pub fn new() -> Self {
        Self {
            version: RESOLUTION_INDEX_VERSION.to_string(),
            hashes: HashMap::new(),
            mappings: HashMap::new(),
            rules: HashMap::new(),
        }
    }

    /// Check if the index needs rebuilding based on SHA comparison
    pub fn needs_rebuild(&self, config_path: &Path, current_sha: &Sha256Hash) -> bool {
        self.hashes
            .get(config_path)
            .map(|stored_sha| stored_sha != current_sha.as_str())
            .unwrap_or(true)
    }

    /// Update the SHA for a config file
    pub fn update_sha(&mut self, config_path: &Path, sha: &Sha256Hash) {
        self.hashes
            .insert(config_path.to_path_buf(), sha.as_str().to_string());
    }

    /// Add a file mapping to a config
    pub fn add_mapping(&mut self, pattern: &str, config_path: &Path) {
        self.mappings
            .insert(pattern.to_string(), config_path.to_path_buf());
    }

    /// Set resolution rules for a config
    pub fn set_rules(&mut self, config_path: &Path, rules: ResolutionRules) {
        self.rules.insert(config_path.to_path_buf(), rules);
    }

    /// Get the config file for a source file using longest-prefix match
    pub fn get_config_for_file(&self, file_path: &Path) -> Option<&PathBuf> {
        // Canonicalize file path to resolve symlinks and make absolute
        // Fall back to original path if file doesn't exist (for tests with non-existent paths)
        let resolved_file = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        let file_str = resolved_file.to_str()?;

        // Find all matching patterns
        let mut matches: Vec<(&String, &PathBuf)> = self
            .mappings
            .iter()
            .filter(|(pattern, _)| {
                // Simple glob matching (for MVP, just check prefix)
                // Strip any **/*.ext pattern to get the base directory
                let pattern_prefix = if let Some(pos) = pattern.find("/**/*") {
                    &pattern[..pos]
                } else {
                    pattern.trim_end_matches('/')
                };

                // Try canonicalizing the pattern prefix to handle symlinks
                // Fall back to non-canonicalized if it doesn't exist
                let pattern_path = std::path::Path::new(pattern_prefix);
                let canon_pattern = pattern_path
                    .canonicalize()
                    .unwrap_or_else(|_| pattern_path.to_path_buf());

                // Convert to string, return false if conversion fails
                let Some(canon_pattern_str) = canon_pattern.to_str() else {
                    return false;
                };

                file_str.starts_with(canon_pattern_str)
            })
            .collect();

        // Sort by pattern length (longest first)
        matches.sort_by_key(|(pattern, _)| -(pattern.len() as i32));

        matches.first().map(|(_, config)| *config)
    }
}

/// Persistence manager for resolution indices
pub struct ResolutionPersistence {
    /// Base directory for resolver indices
    base_dir: PathBuf,
}

impl ResolutionPersistence {
    /// Create a new persistence manager
    pub fn new(codanna_dir: &Path) -> Self {
        Self {
            base_dir: codanna_dir.join("index").join("resolvers"),
        }
    }

    /// Get the index file path for a language
    fn index_path(&self, language_id: &str) -> PathBuf {
        self.base_dir.join(format!("{language_id}_resolution.json"))
    }

    /// Load resolution index for a language
    pub fn load(&self, language_id: &str) -> ResolutionResult<ResolutionIndex> {
        let path = self.index_path(language_id);

        if !path.exists() {
            return Ok(ResolutionIndex::new());
        }

        let content = fs::read_to_string(&path).map_err(|e| ResolutionError::IoError {
            path: path.clone(),
            cause: e.to_string(),
        })?;

        let index: ResolutionIndex =
            serde_json::from_str(&content).map_err(|e| ResolutionError::ParseError {
                message: format!("Failed to parse resolution index: {e}"),
            })?;

        // Validate version compatibility
        if index.version != RESOLUTION_INDEX_VERSION {
            return Err(ResolutionError::ParseError {
                message: format!(
                    "Incompatible index version: expected {}, got {}",
                    RESOLUTION_INDEX_VERSION, index.version
                ),
            });
        }

        Ok(index)
    }

    /// Save resolution index for a language
    pub fn save(&self, language_id: &str, index: &ResolutionIndex) -> ResolutionResult<()> {
        // Ensure directory exists
        fs::create_dir_all(&self.base_dir).map_err(|e| ResolutionError::IoError {
            path: self.base_dir.clone(),
            cause: e.to_string(),
        })?;

        let path = self.index_path(language_id);
        let content =
            serde_json::to_string_pretty(index).map_err(|e| ResolutionError::ParseError {
                message: format!("Failed to serialize resolution index: {e}"),
            })?;

        fs::write(&path, content).map_err(|e| ResolutionError::IoError {
            path,
            cause: e.to_string(),
        })?;

        Ok(())
    }

    /// Clear resolution index for a language
    pub fn clear(&self, language_id: &str) -> ResolutionResult<()> {
        let path = self.index_path(language_id);

        if path.exists() {
            fs::remove_file(&path).map_err(|e| ResolutionError::IoError {
                path,
                cause: e.to_string(),
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LanguageConfig, Settings};
    use crate::parsing::typescript::tsconfig::read_tsconfig;
    use tempfile::TempDir;

    /// Create test settings with TypeScript config files
    fn create_test_settings_with_tsconfigs() -> Settings {
        let mut settings = Settings::default();

        // Add TypeScript language config with real tsconfig paths
        let ts_config = LanguageConfig {
            enabled: true,
            config_files: vec![
                PathBuf::from("examples/typescript/tsconfig.json"),
                PathBuf::from("examples/typescript/packages/web/tsconfig.json"),
            ],
            extensions: vec![".ts".to_string(), ".tsx".to_string()],
            parser_options: Default::default(),
        };
        settings
            .languages
            .insert("typescript".to_string(), ts_config);
        settings
    }

    #[test]
    fn test_resolution_index_with_settings() {
        // NOTE: Requires examples/typescript/src/components/Button.ts to exist for file mapping test
        let settings = create_test_settings_with_tsconfigs();
        let ts_configs = settings
            .languages
            .get("typescript")
            .expect("Should have TypeScript config")
            .config_files
            .clone();

        if !ts_configs.iter().all(|p| p.exists()) {
            println!("Skipping test - example tsconfig files not found");
            return;
        }

        let mut index = ResolutionIndex::new();

        // Process each config from settings
        for config_path in &ts_configs {
            // Compute real SHA
            let sha = crate::project_resolver::sha::compute_file_sha(config_path)
                .expect("Should compute SHA");

            // Test rebuild detection
            assert!(index.needs_rebuild(config_path, &sha));
            index.update_sha(config_path, &sha);
            assert!(!index.needs_rebuild(config_path, &sha));

            // Load actual config
            let config = read_tsconfig(config_path).expect("Should parse tsconfig from settings");

            // Add rules from real config
            index.set_rules(
                config_path,
                ResolutionRules {
                    base_url: config.compilerOptions.baseUrl,
                    paths: config.compilerOptions.paths,
                },
            );
        }

        // Add file mappings based on settings config locations
        index.add_mapping("examples/typescript/src/**/*.ts", &ts_configs[0]);
        index.add_mapping("examples/typescript/packages/web/**/*.ts", &ts_configs[1]);

        // Test file-to-config resolution
        assert_eq!(
            index.get_config_for_file(&PathBuf::from(
                "examples/typescript/src/components/Button.ts"
            )),
            Some(&ts_configs[0])
        );
    }

    #[test]
    fn test_persistence_with_settings() {
        // NOTE: Requires examples/typescript/src/main.ts to exist for file mapping test
        let settings = create_test_settings_with_tsconfigs();
        let ts_configs = &settings
            .languages
            .get("typescript")
            .expect("Should have TypeScript config")
            .config_files;

        if ts_configs.is_empty() || !ts_configs[0].exists() {
            println!("Skipping test - no tsconfig in settings");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let persistence = ResolutionPersistence::new(temp_dir.path());

        let mut index = ResolutionIndex::new();

        // Use first config from settings
        let config_path = &ts_configs[0];
        let sha = crate::project_resolver::sha::compute_file_sha(config_path)
            .expect("Should compute SHA");
        let config = read_tsconfig(config_path).expect("Should parse tsconfig");

        // Store data in index
        index.update_sha(config_path, &sha);
        index.add_mapping("examples/typescript/**/*.ts", config_path);
        index.set_rules(
            config_path,
            ResolutionRules {
                base_url: config.compilerOptions.baseUrl,
                paths: config.compilerOptions.paths,
            },
        );

        // Save and load
        persistence.save("typescript", &index).unwrap();
        let loaded = persistence.load("typescript").unwrap();

        // Verify basic structure
        assert_eq!(loaded.version, RESOLUTION_INDEX_VERSION);
        assert_eq!(loaded.hashes.len(), 1);
        assert_eq!(loaded.mappings.len(), 1);
        assert_eq!(loaded.rules.len(), 1);

        // Verify SHA persisted correctly
        assert!(
            !loaded.needs_rebuild(config_path, &sha),
            "SHA should match after load"
        );

        // Verify mapping persisted correctly
        assert_eq!(
            loaded.get_config_for_file(&PathBuf::from("examples/typescript/src/main.ts")),
            Some(config_path)
        );

        // Verify rules exist for the config
        assert!(
            loaded.rules.contains_key(config_path),
            "Rules should exist for config"
        );
    }
}
