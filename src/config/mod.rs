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
//! - `CI_INDEXING__PARALLELISM=8` sets `indexing.parallelism`
//! - `CI_LOGGING__DEFAULT=debug` sets `logging.default`
//! - `CI_INDEXING__INCLUDE_TESTS=false` sets `indexing.include_tests`
//!
//! For logging, use `RUST_LOG` environment variable directly (standard Rust pattern).

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

mod defaults;
mod init;
mod paths;

use defaults::*;

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

    /// Indexing configuration
    #[serde(default)]
    pub indexing: IndexingConfig,

    /// Cached canonicalized paths for fast lookups (not serialized)
    #[serde(skip)]
    pub indexed_paths_cache: Vec<PathBuf>,

    /// Language-specific settings (IndexMap preserves insertion order)
    #[serde(default)]
    pub languages: IndexMap<String, LanguageConfig>,

    /// MCP server settings
    #[serde(default)]
    pub mcp: McpConfig,

    /// Semantic search settings
    #[serde(default)]
    pub semantic_search: SemanticSearchConfig,

    /// File watching settings
    #[serde(default)]
    pub file_watch: FileWatchConfig,

    /// Server settings (stdio/http mode)
    #[serde(default)]
    pub server: ServerConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// AI guidance settings for multi-hop queries
    #[serde(default)]
    pub guidance: GuidanceConfig,

    /// Document embedding settings for RAG
    #[serde(default)]
    pub documents: crate::documents::DocumentsConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IndexingConfig {
    /// CPU cores to use for indexing (0 = auto-detect all cores)
    /// Thread counts for each stage are derived from this value
    #[serde(default = "default_parallelism")]
    pub parallelism: usize,

    /// Tantivy heap size in megabytes
    /// Controls memory usage before flushing to disk
    #[serde(default = "default_tantivy_heap_mb")]
    pub tantivy_heap_mb: usize,

    /// Maximum retry attempts for transient file system errors
    /// Handles permission delays from antivirus, SELinux, etc.
    #[serde(default = "default_max_retry_attempts")]
    pub max_retry_attempts: u32,

    /// Patterns to ignore during indexing
    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    /// List of directories to index
    /// This list is managed by the add-dir and remove-dir commands
    #[serde(default)]
    pub indexed_paths: Vec<PathBuf>,

    // Pipeline settings (parallel indexer)
    /// Symbols per batch before flushing to Tantivy
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Batches to accumulate before Tantivy commit
    #[serde(default = "default_batches_per_commit")]
    pub batches_per_commit: usize,

    /// Enable detailed pipeline stage tracing (timing, memory, throughput)
    /// Set logging.modules.pipeline = "info" to see output
    #[serde(default)]
    pub pipeline_tracing: bool,

    /// Show progress bars during indexing (default: true)
    #[serde(default = "default_true")]
    pub show_progress: bool,
}

/// Source layout for project resolution
/// Determines how source roots are discovered from build configuration files
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SourceLayout {
    /// Standard JVM layout: src/main/{lang}, src/test/{lang}
    #[default]
    Jvm,
    /// Standard Kotlin Multiplatform: src/commonMain/kotlin, src/jvmMain/kotlin, etc.
    StandardKmp,
    /// Flat KMP layout (ktor-style): common/src/, jvm/src/, posix/src/
    FlatKmp,
}

/// Per-project configuration with explicit source layout
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProjectConfig {
    /// Path to the project configuration file (e.g., build.gradle.kts)
    pub config_file: PathBuf,

    /// Source layout for this project
    #[serde(default)]
    pub source_layout: SourceLayout,
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
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parser_options: HashMap<String, serde_json::Value>,

    /// Project configuration files to monitor (e.g., tsconfig.json, pyproject.toml)
    /// Empty by default - project resolution is opt-in
    /// For simple cases where auto-detection works
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_files: Vec<PathBuf>,

    /// Per-project configuration with explicit source layout
    /// Use when auto-detection fails (e.g., custom build plugins)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpConfig {
    /// Maximum context size in bytes
    #[serde(default = "default_max_context_size")]
    pub max_context_size: usize,

    /// `Host` allowlist for Streamable HTTP inbound. None ⇒ loopback-only default.
    #[serde(default)]
    pub allowed_hosts: Option<Vec<String>>,

    /// `Origin` allowlist for Streamable HTTP inbound. None ⇒ no Origin check.
    #[serde(default)]
    pub allowed_origins: Option<Vec<String>>,
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

    /// Number of parallel embedding model instances
    #[serde(default = "default_embedding_threads")]
    pub embedding_threads: usize,

    /// Remote embedding server URL (OpenAI-compatible, e.g. http://host:8100).
    /// When set, local fastembed is bypassed and this endpoint is used instead.
    /// Overrideable via CODANNA_EMBED_URL env var.
    #[serde(default)]
    pub remote_url: Option<String>,

    /// Model name to send to the remote embedding server.
    /// Overrideable via CODANNA_EMBED_MODEL env var.
    #[serde(default)]
    pub remote_model: Option<String>,

    /// Output dimension of the remote embedding model.
    /// Required when remote_url is set. Overrideable via CODANNA_EMBED_DIM env var.
    #[serde(default)]
    pub remote_dim: Option<usize>,
    // API key: set CODANNA_EMBED_API_KEY environment variable.
    // Intentionally not a config field -- secrets must not live in shared config files.
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FileWatchConfig {
    /// Enable automatic file watching for indexed files
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Debounce interval in milliseconds (default: 500ms)
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    /// Default server mode: "stdio" or "http"
    #[serde(default = "default_server_mode")]
    pub mode: String,

    /// HTTP server bind address
    #[serde(default = "default_bind_address")]
    pub bind: String,

    /// Watch interval for stdio mode (seconds)
    #[serde(default = "default_watch_interval")]
    pub watch_interval: u64,

    /// Whether the proxy mode auto-spawns a backing server process
    #[serde(default = "default_auto_spawn")]
    pub auto_spawn: bool,

    /// Idle timeout for the auto-spawned server, in seconds (0 = never)
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,

    /// Timeout for spawning the backing server, in milliseconds
    #[serde(default = "default_spawn_timeout_ms")]
    pub spawn_timeout_ms: u64,

    /// Poll interval while waiting for the backing server to become healthy, in milliseconds
    #[serde(default = "default_health_poll_ms")]
    pub health_poll_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingConfig {
    /// Default log level for all modules
    /// Valid values: "error", "warn", "info", "debug", "trace"
    #[serde(default = "default_log_level")]
    pub default: String,

    /// Per-module log level overrides (IndexMap preserves insertion order)
    /// Example: { "tantivy" = "warn", "watcher" = "debug" }
    #[serde(default)]
    pub modules: IndexMap<String, String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            default: default_log_level(),
            modules: default_logging_modules(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuidanceConfig {
    /// Enable AI guidance system
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Templates for specific tools
    #[serde(default)]
    pub templates: IndexMap<String, GuidanceTemplate>,

    /// Global template variables
    #[serde(default)]
    pub variables: IndexMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuidanceTemplate {
    /// Template for no results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_results: Option<String>,

    /// Template for single result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub single_result: Option<String>,

    /// Template for multiple results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple_results: Option<String>,

    /// Custom templates for specific count ranges
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<GuidanceRange>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuidanceRange {
    /// Minimum count (inclusive)
    pub min: usize,
    /// Maximum count (inclusive, None = unbounded)
    pub max: Option<usize>,
    /// Template to use
    pub template: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: default_version(),
            index_path: default_index_path(),
            workspace_root: None,
            indexing: IndexingConfig::default(),
            indexed_paths_cache: Vec::new(),
            languages: generate_language_defaults(), // Now uses registry
            mcp: McpConfig::default(),
            semantic_search: SemanticSearchConfig::default(),
            file_watch: FileWatchConfig::default(),
            server: ServerConfig::default(),
            logging: LoggingConfig::default(),
            guidance: GuidanceConfig::default(),
            documents: crate::documents::DocumentsConfig::default(),
        }
    }
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            parallelism: default_parallelism(),
            tantivy_heap_mb: default_tantivy_heap_mb(),
            max_retry_attempts: default_max_retry_attempts(),
            ignore_patterns: vec![
                "target/**".to_string(),
                "node_modules/**".to_string(),
                ".git/**".to_string(),
                "*.generated.*".to_string(),
            ],
            indexed_paths: Vec::new(),
            batch_size: default_batch_size(),
            batches_per_commit: default_batches_per_commit(),
            pipeline_tracing: false,
            show_progress: true,
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            max_context_size: default_max_context_size(),
            allowed_hosts: None,
            allowed_origins: None,
        }
    }
}

impl Default for SemanticSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Enabled by default for better code intelligence
            model: default_embedding_model(),
            threshold: default_similarity_threshold(),
            embedding_threads: default_embedding_threads(),
            remote_url: None,
            remote_model: None,
            remote_dim: None,
        }
    }
}

impl Default for FileWatchConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Default to enabled for better user experience
            debounce_ms: default_debounce_ms(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            mode: default_server_mode(),
            bind: default_bind_address(),
            watch_interval: default_watch_interval(),
            auto_spawn: default_auto_spawn(),
            idle_timeout_secs: default_idle_timeout_secs(),
            spawn_timeout_ms: default_spawn_timeout_ms(),
            health_poll_ms: default_health_poll_ms(),
        }
    }
}

impl Default for GuidanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            templates: default_guidance_templates(),
            variables: default_guidance_variables(),
        }
    }
}

impl Settings {
    /// Create settings specifically for init_config_file
    /// This populates all dynamic fields based on the current environment
    pub fn for_init() -> Result<Self, Box<dyn std::error::Error>> {
        // Create settings with project-specific values in one initialization
        let settings = Self {
            workspace_root: Some(std::env::current_dir()?),
            // All other fields use defaults (including registry languages)
            ..Self::default()
        };

        Ok(settings)
    }

    /// Load configuration from all sources
    pub fn load() -> Result<Self, Box<figment::Error>> {
        // Try to find the workspace root by looking for config directory
        let local_dir = crate::init::local_dir_name();
        let config_path = Self::find_workspace_config()
            .unwrap_or_else(|| PathBuf::from(local_dir).join("settings.toml"));

        Figment::new()
            // Start with defaults
            .merge(Serialized::defaults(Settings::default()))
            // Layer in config file if it exists
            .merge(Toml::file(config_path))
            // Layer in environment variables with CI_ prefix
            // Use double underscore (__) to separate nested levels
            // Single underscore (_) remains as is within field names
            .merge(Env::prefixed("CI_").map(|key| {
                key.as_str()
                    .to_lowercase()
                    .replace("__", ".") // Double underscore becomes dot
                    .into()
            }))
            // Extract into Settings struct
            .extract()
            .map_err(Box::new)
            .map(|mut settings: Settings| {
                // If workspace_root is not set in config, detect it
                if settings.workspace_root.is_none() {
                    settings.workspace_root = Self::workspace_root();
                }
                settings.sync_indexed_path_cache();
                settings
            })
    }

    /// Find the workspace root by looking for .codanna directory
    /// Searches from current directory up to root
    pub fn find_workspace_config() -> Option<PathBuf> {
        let current = std::env::current_dir().ok()?;
        let local_dir = crate::init::local_dir_name();

        for ancestor in current.ancestors() {
            let config_dir = ancestor.join(local_dir);
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
                    return Err(format!(
                        "Configuration file is corrupted: {e}\nRun 'codanna init --force' to regenerate."
                    ));
                }
            }
            Err(e) => {
                return Err(format!("Cannot read configuration file: {e}"));
            }
        }

        Ok(())
    }

    /// Get the workspace root directory (where config directory is located)
    pub fn workspace_root() -> Option<PathBuf> {
        let current = std::env::current_dir().ok()?;
        let local_dir = crate::init::local_dir_name();

        for ancestor in current.ancestors() {
            let config_dir = ancestor.join(local_dir);
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
            .map(|mut settings: Settings| {
                settings.sync_indexed_path_cache();
                settings
            })
            .map_err(Box::new)
    }

    /// Save current configuration to file
    pub fn save(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.as_ref().parent().ok_or("Invalid path")?;
        std::fs::create_dir_all(parent)?;

        let toml_string = toml::to_string_pretty(self)?;
        let toml_with_comments = Self::add_config_comments(toml_string);
        std::fs::write(path, toml_with_comments)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_guidance_templates_serialize_in_insertion_order() {
        let settings = Settings::default();
        let toml_string = toml::to_string_pretty(&settings).unwrap();

        let expected_order = [
            "semantic_search_docs",
            "find_symbol",
            "get_calls",
            "find_callers",
            "analyze_impact",
            "search_symbols",
            "semantic_search_with_context",
            "get_index_info",
        ];

        let positions: Vec<usize> = expected_order
            .iter()
            .map(|name| {
                let header = format!("[guidance.templates.{name}]");
                toml_string
                    .find(&header)
                    .unwrap_or_else(|| panic!("missing table header {header}"))
            })
            .collect();

        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "guidance templates serialized out of insertion order: {positions:?}"
        );
    }

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.version, 1);
        // Use the correct local dir name for test mode
        let expected_index_path = PathBuf::from(format!("{}/index", crate::init::local_dir_name()));
        assert_eq!(settings.index_path, expected_index_path);
        assert!(settings.indexing.parallelism > 0);
        assert!(settings.languages.contains_key("rust"));
    }

    #[test]
    fn test_load_from_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        let toml_content = r#"
version = 2

[indexing]
parallelism = 4
ignore_patterns = ["custom/**"]
include_tests = false

[mcp]
max_context_size = 200000

[languages.rust]
enabled = false
"#;

        fs::write(&config_path, toml_content).unwrap();

        let settings = Settings::load_from(&config_path).unwrap();
        assert_eq!(settings.version, 2);
        assert_eq!(settings.indexing.parallelism, 4);
        assert_eq!(settings.indexing.ignore_patterns, vec!["custom/**"]);
        // Default ignore patterns should be replaced by custom ones
        assert_eq!(settings.indexing.ignore_patterns.len(), 1);
        assert_eq!(settings.mcp.max_context_size, 200000);
        assert!(!settings.languages["rust"].enabled);
    }

    #[test]
    fn test_server_config_minimal_toml_uses_proxy_defaults() {
        let toml_content = r#"
mode = "stdio"
"#;
        let server: ServerConfig = toml::from_str(toml_content).unwrap();
        assert_eq!(server.mode, "stdio");
        assert_eq!(server.bind, default_bind_address());
        assert_eq!(server.watch_interval, default_watch_interval());
        assert!(server.auto_spawn);
        assert_eq!(server.idle_timeout_secs, 0);
        assert_eq!(server.spawn_timeout_ms, 8000);
        assert_eq!(server.health_poll_ms, 100);
    }

    #[test]
    fn test_server_config_proxy_mode_round_trips() {
        let toml_content = r#"
mode = "proxy"
"#;
        let server: ServerConfig = toml::from_str(toml_content).unwrap();
        assert_eq!(server.mode, "proxy");

        let serialized = toml::to_string(&server).unwrap();
        let round_tripped: ServerConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(round_tripped.mode, "proxy");
        assert_eq!(round_tripped.auto_spawn, server.auto_spawn);
        assert_eq!(round_tripped.idle_timeout_secs, server.idle_timeout_secs);
        assert_eq!(round_tripped.spawn_timeout_ms, server.spawn_timeout_ms);
        assert_eq!(round_tripped.health_poll_ms, server.health_poll_ms);
    }

    #[test]
    fn test_save_settings() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        let mut settings = Settings::default();
        settings.indexing.parallelism = 2;
        settings.mcp.max_context_size = 50000;

        settings.save(&config_path).unwrap();

        let loaded = Settings::load_from(&config_path).unwrap();
        assert_eq!(loaded.indexing.parallelism, 2);
        assert_eq!(loaded.mcp.max_context_size, 50000);
    }

    #[test]
    fn test_partial_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        // Only specify a few settings
        let toml_content = r#"
[indexing]
parallelism = 16

[languages.python]
enabled = true
"#;

        fs::write(&config_path, toml_content).unwrap();

        let settings = Settings::load_from(&config_path).unwrap();

        // Modified values
        assert_eq!(settings.indexing.parallelism, 16);
        assert!(settings.languages["python"].enabled);

        // Default values should still be present
        assert_eq!(settings.version, 1);
        assert_eq!(settings.mcp.max_context_size, 100_000);
        // Default ignore patterns should be present
        assert!(!settings.indexing.ignore_patterns.is_empty());
    }

    #[test]
    #[ignore = "mutates process CWD via set_current_dir; races with subprocess tests under parallel execution; run via cargo test -- --ignored --test-threads=1"]
    fn test_layered_config() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        // Create config directory using the correct test directory name
        let config_dir = temp_dir.path().join(crate::init::local_dir_name());
        fs::create_dir_all(&config_dir).unwrap();

        // Create a config file
        let toml_content = r#"
[indexing]
parallelism = 8
include_tests = true

[mcp]
max_context_size = 50000

[logging]
default = "info"
"#;
        fs::write(config_dir.join("settings.toml"), toml_content).unwrap();

        // Set environment variables that should override config file
        unsafe {
            std::env::set_var("CI_INDEXING__PARALLELISM", "16");
            std::env::set_var("CI_LOGGING__DEFAULT", "debug");
        }

        let settings = Settings::load().unwrap();

        // Environment variable should override config file
        assert_eq!(settings.indexing.parallelism, 16);
        // Config file value should be used when no env var
        assert_eq!(settings.mcp.max_context_size, 50000);
        // Env var overrides logging default
        assert_eq!(settings.logging.default, "debug");
        // Default ignore patterns should be present
        assert!(!settings.indexing.ignore_patterns.is_empty());

        // Clean up
        unsafe {
            std::env::remove_var("CI_INDEXING__PARALLELISM");
            std::env::remove_var("CI_LOGGING__DEFAULT");
        }
        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_file_watch_config_defaults() {
        println!("\n=== TEST: FileWatchConfig Defaults ===");

        let config = FileWatchConfig::default();
        assert!(config.enabled); // Now defaults to true
        assert_eq!(config.debounce_ms, 500);

        println!(
            "  ✓ Default config: enabled={}, debounce_ms={}",
            config.enabled, config.debounce_ms
        );
        println!("=== TEST PASSED ===");
    }

    #[test]
    fn test_file_watch_config_from_toml() {
        println!("\n=== TEST: FileWatchConfig from TOML ===");

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        // Write test config
        let config_content = r#"
[file_watch]
enabled = true
debounce_ms = 1000
"#;
        fs::write(&config_path, config_content).unwrap();
        println!("  Created test config: {}", config_path.display());

        // Load config using Figment directly
        let settings: Settings = Figment::new()
            .merge(Serialized::defaults(Settings::default()))
            .merge(Toml::file(config_path))
            .extract()
            .unwrap();

        assert!(settings.file_watch.enabled);
        assert_eq!(settings.file_watch.debounce_ms, 1000);

        println!(
            "  ✓ Loaded config: enabled={}, debounce_ms={}",
            settings.file_watch.enabled, settings.file_watch.debounce_ms
        );
        println!("=== TEST PASSED ===");
    }

    #[test]
    fn test_file_watch_partial_config() {
        println!("\n=== TEST: FileWatchConfig Partial Configuration ===");

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        // Only specify enabled, debounce_ms should use default
        let config_content = r#"
[file_watch]
enabled = true
"#;
        fs::write(&config_path, config_content).unwrap();

        // Load config using Figment directly
        let settings: Settings = Figment::new()
            .merge(Serialized::defaults(Settings::default()))
            .merge(Toml::file(config_path))
            .extract()
            .unwrap();

        assert!(settings.file_watch.enabled);
        assert_eq!(settings.file_watch.debounce_ms, 500); // default value

        println!(
            "  ✓ Partial config works: enabled={}, debounce_ms={} (default)",
            settings.file_watch.enabled, settings.file_watch.debounce_ms
        );
        println!("=== TEST PASSED ===");
    }

    #[test]
    fn test_add_indexed_path() {
        let temp_dir = TempDir::new().unwrap();
        let test_folder = temp_dir.path().join("test_folder");
        fs::create_dir(&test_folder).unwrap();

        let mut settings = Settings::default();

        // Add a path
        assert!(settings.add_indexed_path(test_folder.clone()).is_ok());
        assert_eq!(settings.indexing.indexed_paths.len(), 1);

        // Try to add the same path again - should fail
        let result = settings.add_indexed_path(test_folder.clone());
        assert!(result.is_err());
        assert_eq!(settings.indexing.indexed_paths.len(), 1);
    }

    #[test]
    fn test_remove_indexed_path() {
        let temp_dir = TempDir::new().unwrap();
        let test_folder = temp_dir.path().join("test_folder");
        fs::create_dir(&test_folder).unwrap();

        let mut settings = Settings::default();

        // Add a path
        settings.add_indexed_path(test_folder.clone()).unwrap();
        assert_eq!(settings.indexing.indexed_paths.len(), 1);

        // Remove the path
        assert!(settings.remove_indexed_path(&test_folder).is_ok());
        assert_eq!(settings.indexing.indexed_paths.len(), 0);

        // Try to remove it again - should fail
        let result = settings.remove_indexed_path(&test_folder);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_indexed_paths() {
        let temp_dir = TempDir::new().unwrap();
        let folder1 = temp_dir.path().join("folder1");
        let folder2 = temp_dir.path().join("folder2");
        let folder3 = temp_dir.path().join("folder3");

        fs::create_dir(&folder1).unwrap();
        fs::create_dir(&folder2).unwrap();
        fs::create_dir(&folder3).unwrap();

        let mut settings = Settings::default();

        // Add multiple paths
        settings.add_indexed_path(folder1.clone()).unwrap();
        settings.add_indexed_path(folder2.clone()).unwrap();
        settings.add_indexed_path(folder3.clone()).unwrap();

        assert_eq!(settings.indexing.indexed_paths.len(), 3);

        // Remove one path
        settings.remove_indexed_path(&folder2).unwrap();
        assert_eq!(settings.indexing.indexed_paths.len(), 2);

        // Verify the right paths remain
        let canonical_folder1 = folder1.canonicalize().unwrap();
        let canonical_folder3 = folder3.canonicalize().unwrap();

        let remaining_paths: Vec<_> = settings
            .indexing
            .indexed_paths
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();

        assert!(remaining_paths.contains(&canonical_folder1));
        assert!(remaining_paths.contains(&canonical_folder3));
    }

    #[test]
    fn test_add_indexed_path_skips_child_when_parent_exists() {
        let temp_dir = TempDir::new().unwrap();
        let parent = temp_dir.path().join("parent");
        let child = parent.join("child");

        fs::create_dir_all(&child).unwrap();

        let mut settings = Settings::default();
        settings.add_indexed_path(parent.clone()).unwrap();
        assert_eq!(settings.indexing.indexed_paths.len(), 1);

        let result = settings.add_indexed_path(child.clone());
        assert!(result.is_err());
        assert_eq!(settings.indexing.indexed_paths.len(), 1);

        let error_message = result.unwrap_err();
        assert!(
            error_message.contains("already indexed"),
            "expected duplicate error, got: {error_message}"
        );
    }

    #[test]
    fn test_add_indexed_path_replaces_children_when_adding_parent() {
        let temp_dir = TempDir::new().unwrap();
        let parent = temp_dir.path().join("parent");
        let child = parent.join("child");

        fs::create_dir_all(&child).unwrap();

        let mut settings = Settings::default();
        settings.add_indexed_path(child.clone()).unwrap();
        assert_eq!(settings.indexing.indexed_paths.len(), 1);

        settings.add_indexed_path(parent.clone()).unwrap();
        assert_eq!(settings.indexing.indexed_paths.len(), 1);

        let stored = settings
            .indexing
            .indexed_paths
            .first()
            .expect("expected parent path");
        assert_eq!(stored, &parent.canonicalize().unwrap());
    }

    #[test]
    fn test_get_indexed_paths_with_default() {
        let settings = Settings::default();

        // Should return empty vector when no paths configured (backward compatible)
        let paths = settings.get_indexed_paths();
        assert_eq!(paths.len(), 0);
    }

    #[test]
    fn test_get_indexed_paths_with_configured_paths() {
        let temp_dir = TempDir::new().unwrap();
        let test_folder = temp_dir.path().join("test_folder");
        fs::create_dir(&test_folder).unwrap();

        let mut settings = Settings::default();
        settings.add_indexed_path(test_folder.clone()).unwrap();

        // Should return the configured paths
        let paths = settings.get_indexed_paths();
        assert_eq!(paths.len(), 1);

        let canonical_test = test_folder.canonicalize().unwrap();
        let canonical_returned = paths[0].canonicalize().unwrap();
        assert_eq!(canonical_returned, canonical_test);
    }

    #[test]
    fn test_indexed_paths_from_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        let test_folder1 = temp_dir.path().join("src");
        let test_folder2 = temp_dir.path().join("lib");

        fs::create_dir(&test_folder1).unwrap();
        fs::create_dir(&test_folder2).unwrap();

        // Convert paths to strings with forward slashes for TOML compatibility
        let path1_str = test_folder1.display().to_string().replace('\\', "/");
        let path2_str = test_folder2.display().to_string().replace('\\', "/");

        let toml_content = format!(
            r#"
version = 1

[indexing]
indexed_paths = ["{path1_str}", "{path2_str}"]
"#
        );

        fs::write(&config_path, toml_content).unwrap();

        let settings = Settings::load_from(&config_path).unwrap();
        assert_eq!(settings.indexing.indexed_paths.len(), 2);
        assert_eq!(settings.indexing.indexed_paths[0], test_folder1);
        assert_eq!(settings.indexing.indexed_paths[1], test_folder2);
    }

    #[test]
    fn test_save_indexed_paths_to_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        let test_folder = temp_dir.path().join("test_folder");

        fs::create_dir(&test_folder).unwrap();

        let mut settings = Settings::default();
        settings.add_indexed_path(test_folder.clone()).unwrap();

        // Save to file
        settings.save(&config_path).unwrap();

        // Load from file and verify
        let loaded_settings = Settings::load_from(&config_path).unwrap();
        assert_eq!(loaded_settings.indexing.indexed_paths.len(), 1);

        let canonical_test = test_folder.canonicalize().unwrap();
        let canonical_loaded = loaded_settings.indexing.indexed_paths[0]
            .canonicalize()
            .unwrap();
        assert_eq!(canonical_loaded, canonical_test);
    }

    #[test]
    fn test_documents_config_loading() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        let toml_content = r#"
[documents]
enabled = true

[documents.defaults]
min_chunk_chars = 300
max_chunk_chars = 2000
overlap_chars = 150

[documents.collections.project-docs]
paths = ["docs/", "README.md"]
patterns = ["**/*.md"]

[documents.collections.external-books]
paths = ["/path/to/books"]
max_chunk_chars = 2500
"#;

        fs::write(&config_path, toml_content).unwrap();

        let settings = Settings::load_from(&config_path).unwrap();

        // Check enabled
        assert!(settings.documents.enabled);

        // Check defaults
        assert_eq!(settings.documents.defaults.min_chunk_chars, 300);
        assert_eq!(settings.documents.defaults.max_chunk_chars, 2000);
        assert_eq!(settings.documents.defaults.overlap_chars, 150);

        // Check collections
        assert_eq!(settings.documents.collections.len(), 2);

        let project_docs = settings.documents.collections.get("project-docs").unwrap();
        assert_eq!(project_docs.paths.len(), 2);
        assert_eq!(project_docs.patterns, vec!["**/*.md"]);

        let external = settings
            .documents
            .collections
            .get("external-books")
            .unwrap();
        assert_eq!(external.max_chunk_chars, Some(2500));
    }

    #[test]
    fn test_documents_config_defaults() {
        // When no [documents] section exists, defaults should apply
        let settings = Settings::default();

        assert!(!settings.documents.enabled);
        assert_eq!(settings.documents.defaults.min_chunk_chars, 200);
        assert_eq!(settings.documents.defaults.max_chunk_chars, 1500);
        assert_eq!(settings.documents.defaults.overlap_chars, 100);
        assert!(settings.documents.collections.is_empty());
    }
}
