//! Configuration module for the codebase intelligence system.
//! 
//! This module provides a layered configuration system that supports:
//! - Default values
//! - TOML configuration file
//! - Environment variable overrides
//! - CLI argument overrides
//! 
//! # Environment Variables
//! 
//! Environment variables must be prefixed with `CI_` and use double underscores
//! to separate nested levels:
//! - `CI_INDEXING__PARALLEL_THREADS=8` sets `indexing.parallel_threads`
//! - `CI_MCP__PORT=9999` sets `mcp.port`
//! - `CI_INDEXING__INCLUDE_TESTS=false` sets `indexing.include_tests`

use figment::{Figment, providers::{Format, Toml, Env, Serialized}};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Settings {
    /// Version of the configuration schema
    #[serde(default = "default_version")]
    pub version: u32,
    
    /// Path to the index directory
    #[serde(default = "default_index_path")]
    pub index_path: PathBuf,
    
    /// Workspace root directory (where .codanna is located)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,
    
    /// Global debug mode
    #[serde(default = "default_false")]
    pub debug: bool,
    
    /// Indexing configuration
    #[serde(default)]
    pub indexing: IndexingConfig,
    
    /// Language-specific settings
    #[serde(default)]
    pub languages: HashMap<String, LanguageConfig>,
    
    /// MCP server settings
    #[serde(default)]
    pub mcp: McpConfig,
    
    /// Semantic search settings
    #[serde(default)]
    pub semantic_search: SemanticSearchConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IndexingConfig {
    /// Number of parallel threads for indexing
    #[serde(default = "default_parallel_threads")]
    pub parallel_threads: usize,
    
    /// Project root directory (defaults to workspace root)
    /// Used for gitignore resolution and module path calculation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,
    
    /// Patterns to ignore during indexing
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    
    
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LanguageConfig {
    /// Whether this language is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// File extensions for this language
    #[serde(default)]
    pub extensions: Vec<String>,
    
    /// Additional parser options
    #[serde(default)]
    pub parser_options: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpConfig {
    /// Port for the MCP server
    #[serde(default = "default_mcp_port")]
    pub port: u16,
    
    /// Maximum context size in bytes
    #[serde(default = "default_max_context_size")]
    pub max_context_size: usize,
    
    /// Enable debug logging
    #[serde(default = "default_false")]
    pub debug: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SemanticSearchConfig {
    /// Enable semantic search
    #[serde(default = "default_false")]
    pub enabled: bool,
    
    /// Model to use for embeddings
    #[serde(default = "default_embedding_model")]
    pub model: String,
    
    /// Similarity threshold for search results
    #[serde(default = "default_similarity_threshold")]
    pub threshold: f32,
}

// Default value functions
fn default_version() -> u32 { 1 }
fn default_index_path() -> PathBuf { PathBuf::from(".codanna/index") }
fn default_parallel_threads() -> usize { num_cpus::get() }
fn default_true() -> bool { true }
fn default_false() -> bool { false }
fn default_mcp_port() -> u16 { 7777 }
fn default_max_context_size() -> usize { 100_000 }
fn default_embedding_model() -> String { "AllMiniLML6V2".to_string() }
fn default_similarity_threshold() -> f32 { 0.6 }

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: default_version(),
            index_path: default_index_path(),
            workspace_root: None,
            debug: false,
            indexing: IndexingConfig::default(),
            languages: default_languages(),
            mcp: McpConfig::default(),
            semantic_search: SemanticSearchConfig::default(),
        }
    }
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            parallel_threads: default_parallel_threads(),
            project_root: None,
            ignore_patterns: vec![
                "target/**".to_string(),
                "node_modules/**".to_string(),
                ".git/**".to_string(),
                "*.generated.*".to_string(),
            ],
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            port: default_mcp_port(),
            max_context_size: default_max_context_size(),
            debug: false,
        }
    }
}

impl Default for SemanticSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_embedding_model(),
            threshold: default_similarity_threshold(),
        }
    }
}

fn default_languages() -> HashMap<String, LanguageConfig> {
    let mut langs = HashMap::new();
    
    // Rust configuration
    langs.insert("rust".to_string(), LanguageConfig {
        enabled: true,
        extensions: vec!["rs".to_string()],
        parser_options: HashMap::new(),
    });
    
    // Python configuration
    langs.insert("python".to_string(), LanguageConfig {
        enabled: false,
        extensions: vec!["py".to_string(), "pyi".to_string()],
        parser_options: HashMap::new(),
    });
    
    // TypeScript/JavaScript configuration
    langs.insert("typescript".to_string(), LanguageConfig {
        enabled: false,
        extensions: vec!["ts".to_string(), "tsx".to_string(), "js".to_string(), "jsx".to_string()],
        parser_options: HashMap::new(),
    });
    
    langs
}

impl Settings {
    /// Load configuration from all sources
    pub fn load() -> Result<Self, Box<figment::Error>> {
        // Try to find the workspace root by looking for .codanna directory
        let config_path = Self::find_workspace_config()
            .unwrap_or_else(|| PathBuf::from(".codanna/settings.toml"));
        
        Figment::new()
            // Start with defaults
            .merge(Serialized::defaults(Settings::default()))
            // Layer in config file if it exists
            .merge(Toml::file(config_path))
            // Layer in environment variables with CI_ prefix
            // Use double underscore (__) to separate nested levels
            // Single underscore (_) remains as is within field names
            .merge(
                Env::prefixed("CI_")
                    .map(|key| {
                        key.as_str()
                            .to_lowercase()
                            .replace("__", ".")  // Double underscore becomes dot
                            .into()
                    })
            )
            // Extract into Settings struct
            .extract()
            .map_err(Box::new)
            .map(|mut settings: Settings| {
                // If workspace_root is not set in config, detect it
                if settings.workspace_root.is_none() {
                    settings.workspace_root = Self::workspace_root();
                }
                settings
            })
    }
    
    /// Find the workspace root by looking for .codanna directory
    /// Searches from current directory up to root
    fn find_workspace_config() -> Option<PathBuf> {
        let current = std::env::current_dir().ok()?;
        
        for ancestor in current.ancestors() {
            let config_dir = ancestor.join(".codanna");
            if config_dir.exists() && config_dir.is_dir() {
                return Some(config_dir.join("settings.toml"));
            }
        }
        
        None
    }
    
    /// Check if configuration is properly initialized
    pub fn check_init() -> Result<(), String> {
        // Try to find workspace config
        let config_path = if let Some(path) = Self::find_workspace_config() {
            path
        } else {
            // No workspace found, check current directory
            PathBuf::from(".codanna/settings.toml")
        };
        
        // Check if settings.toml exists
        if !config_path.exists() {
            return Err("No configuration file found".to_string());
        }
        
        // Try to parse the config file to check if it's valid
        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                if let Err(e) = toml::from_str::<Settings>(&content) {
                    return Err(format!("Configuration file is corrupted: {}\nRun 'codanna init --force' to regenerate.", e));
                }
            }
            Err(e) => {
                return Err(format!("Cannot read configuration file: {}", e));
            }
        }
        
        Ok(())
    }
    
    /// Get the workspace root directory (where .codanna is located)
    pub fn workspace_root() -> Option<PathBuf> {
        let current = std::env::current_dir().ok()?;
        
        for ancestor in current.ancestors() {
            let config_dir = ancestor.join(".codanna");
            if config_dir.exists() && config_dir.is_dir() {
                return Some(ancestor.to_path_buf());
            }
        }
        
        None
    }
    
    /// Load configuration from a specific file
    pub fn load_from(path: impl AsRef<std::path::Path>) -> Result<Self, Box<figment::Error>> {
        Figment::new()
            .merge(Serialized::defaults(Settings::default()))
            .merge(Toml::file(path))
            .merge(Env::prefixed("CI_").split("_"))
            .extract()
            .map_err(Box::new)
    }
    
    /// Save current configuration to file
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.as_ref().parent().ok_or("Invalid path")?;
        std::fs::create_dir_all(parent)?;
        
        let toml_string = toml::to_string_pretty(self)?;
        std::fs::write(path, toml_string)?;
        
        Ok(())
    }
    
    /// Create a default settings file
    pub fn init_config_file(force: bool) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let config_path = PathBuf::from(".codanna/settings.toml");
        
        if !force && config_path.exists() {
            return Err("Configuration file already exists. Use --force to overwrite".into());
        }
        
        // Create settings with detected workspace root
        let mut settings = Settings::default();
        
        // Set workspace root to current directory
        if let Ok(current_dir) = std::env::current_dir() {
            settings.workspace_root = Some(current_dir);
        }
        
        settings.save(&config_path)?;
        if force && config_path.exists() {
            println!("Overwrote configuration at: {}", config_path.display());
        } else {
            println!("Created default configuration at: {}", config_path.display());
        }
        
        // Create default .codannaignore file
        Self::create_default_ignore_file(force)?;
        
        Ok(config_path)
    }
    
    /// Create a default .codannaignore file with helpful patterns
    fn create_default_ignore_file(force: bool) -> Result<(), Box<dyn std::error::Error>> {
        let ignore_path = PathBuf::from(".codannaignore");
        
        if !force && ignore_path.exists() {
            println!("Found existing .codannaignore file");
            return Ok(());
        }
        
        let default_content = r#"# Codanna ignore patterns (gitignore syntax)
# https://git-scm.com/docs/gitignore
#
# This file tells codanna which files to exclude from indexing.
# Each line specifies a pattern. Patterns follow the same rules as .gitignore.

# Build artifacts
target/
build/
dist/
*.o
*.so
*.dylib
*.exe
*.dll

# Test files (uncomment to exclude tests from indexing)
# tests/
# *_test.rs
# *.test.js
# *.spec.ts
# test_*.py

# Temporary files
*.tmp
*.temp
*.bak
*.swp
*.swo
*~
.DS_Store

# Codanna's own directory
.codanna/

# Dependency directories
node_modules/
vendor/
.venv/
venv/
__pycache__/
*.egg-info/
.cargo/

# IDE and editor directories
.idea/
.vscode/
*.iml
.project
.classpath
.settings/

# Documentation (uncomment if you don't want to index docs)
# docs/
# *.md

# Generated files
*.generated.*
*.auto.*
*_pb2.py
*.pb.go

# Version control
.git/
.svn/
.hg/

# Example of including specific files from ignored directories:
# !target/doc/
# !vendor/specific-file.rs
"#;
        
        std::fs::write(&ignore_path, default_content)?;
        
        if force && ignore_path.exists() {
            println!("Overwrote .codannaignore file");
        } else {
            println!("Created default .codannaignore file");
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    
    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.version, 1);
        assert_eq!(settings.index_path, PathBuf::from(".codanna/index"));
        assert!(settings.indexing.parallel_threads > 0);
        assert!(settings.languages.contains_key("rust"));
    }
    
    #[test]
    fn test_load_from_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        
        let toml_content = r#"
version = 2

[indexing]
parallel_threads = 4
ignore_patterns = ["custom/**"]
include_tests = false

[mcp]
port = 8888
debug = true

[languages.rust]
enabled = false
"#;
        
        fs::write(&config_path, toml_content).unwrap();
        
        let settings = Settings::load_from(&config_path).unwrap();
        assert_eq!(settings.version, 2);
        assert_eq!(settings.indexing.parallel_threads, 4);
        assert_eq!(settings.indexing.ignore_patterns, vec!["custom/**"]);
        // Default ignore patterns should be replaced by custom ones
        assert_eq!(settings.indexing.ignore_patterns.len(), 1);
        assert_eq!(settings.mcp.port, 8888);
        assert!(settings.mcp.debug);
        assert!(!settings.languages["rust"].enabled);
    }
    
    #[test]
    fn test_save_settings() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        
        let mut settings = Settings::default();
        settings.indexing.parallel_threads = 2;
        settings.mcp.port = 9999;
        
        settings.save(&config_path).unwrap();
        
        let loaded = Settings::load_from(&config_path).unwrap();
        assert_eq!(loaded.indexing.parallel_threads, 2);
        assert_eq!(loaded.mcp.port, 9999);
    }
    
    #[test]
    fn test_partial_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        
        // Only specify a few settings
        let toml_content = r#"
[indexing]
parallel_threads = 16

[languages.python]
enabled = true
"#;
        
        fs::write(&config_path, toml_content).unwrap();
        
        let settings = Settings::load_from(&config_path).unwrap();
        
        // Modified values
        assert_eq!(settings.indexing.parallel_threads, 16);
        assert!(settings.languages["python"].enabled);
        
        // Default values should still be present
        assert_eq!(settings.version, 1);
        assert_eq!(settings.mcp.port, 7777);
        // Default ignore patterns should be present
        assert!(!settings.indexing.ignore_patterns.is_empty());
    }
    
    #[test]
    fn test_layered_config() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();
        
        // Create config directory
        let config_dir = temp_dir.path().join(".codanna");
        fs::create_dir_all(&config_dir).unwrap();
        
        // Create a config file
        let toml_content = r#"
[indexing]
parallel_threads = 8
include_tests = true

[mcp]
port = 7777
"#;
        fs::write(config_dir.join("settings.toml"), toml_content).unwrap();
        
        // Set environment variables that should override config file
        unsafe {
            std::env::set_var("CI_INDEXING__PARALLEL_THREADS", "16");
            std::env::set_var("CI_MCP__DEBUG", "true");
        }
        
        let settings = Settings::load().unwrap();
        
        // Environment variable should override config file
        assert_eq!(settings.indexing.parallel_threads, 16);
        // Config file value should be used when no env var
        assert_eq!(settings.mcp.port, 7777);
        // Env var adds new value not in config
        assert!(settings.mcp.debug);
        // Config file value remains
        // Default ignore patterns should be present
        assert!(!settings.indexing.ignore_patterns.is_empty());
        
        // Clean up
        unsafe {
            std::env::remove_var("CI_INDEXING__PARALLEL_THREADS");
            std::env::remove_var("CI_MCP__DEBUG");
        }
        std::env::set_current_dir(original_dir).unwrap();
    }
}