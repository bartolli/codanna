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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    /// Language-specific settings
    #[serde(default)]
    pub languages: HashMap<String, LanguageConfig>,

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

    /// Project root directory (defaults to workspace root)
    /// Used for gitignore resolution and module path calculation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,

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
    #[serde(default)]
    pub config_files: Vec<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpConfig {
    /// Maximum context size in bytes
    #[serde(default = "default_max_context_size")]
    pub max_context_size: usize,
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
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingConfig {
    /// Default log level for all modules
    /// Valid values: "error", "warn", "info", "debug", "trace"
    #[serde(default = "default_log_level")]
    pub default: String,

    /// Per-module log level overrides
    /// Example: { "tantivy" = "warn", "watcher" = "debug" }
    #[serde(default)]
    pub modules: HashMap<String, String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            default: default_log_level(),
            modules: default_logging_modules(),
        }
    }
}

fn default_log_level() -> String {
    "warn".to_string() // Quiet by default, use RUST_LOG=info for normal output
}

fn default_logging_modules() -> HashMap<String, String> {
    let mut modules = HashMap::new();
    // Suppress verbose Tantivy internal logs by default
    modules.insert("tantivy".to_string(), "warn".to_string());
    // Pipeline logs at warn by default (use "info" to see progress)
    modules.insert("pipeline".to_string(), "warn".to_string());
    modules
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuidanceConfig {
    /// Enable AI guidance system
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Templates for specific tools
    #[serde(default)]
    pub templates: HashMap<String, GuidanceTemplate>,

    /// Global template variables
    #[serde(default)]
    pub variables: HashMap<String, String>,
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

// Default value functions
fn default_version() -> u32 {
    1
}
fn default_index_path() -> PathBuf {
    // Use configurable directory name from init module
    let local_dir = crate::init::local_dir_name();
    PathBuf::from(local_dir).join("index")
}
fn default_parallelism() -> usize {
    num_cpus::get()
}
fn default_tantivy_heap_mb() -> usize {
    50 // Universal default that balances performance and permissions
}
fn default_max_retry_attempts() -> u32 {
    3 // Exponential backoff: 100ms, 200ms, 400ms
}
fn default_batch_size() -> usize {
    5000 // Symbols per batch before Tantivy flush
}
fn default_batches_per_commit() -> usize {
    10 // Commit every 10 batches (~50K symbols)
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_max_context_size() -> usize {
    100_000
}
fn default_embedding_model() -> String {
    "AllMiniLML6V2".to_string()
}
fn default_similarity_threshold() -> f32 {
    0.6
}
fn default_embedding_threads() -> usize {
    3
}
fn default_debounce_ms() -> u64 {
    500
}
fn default_server_mode() -> String {
    "stdio".to_string()
}
fn default_bind_address() -> String {
    "127.0.0.1:8080".to_string()
}
fn default_watch_interval() -> u64 {
    5
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
            project_root: None,
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

fn default_guidance_templates() -> HashMap<String, GuidanceTemplate> {
    let mut templates = HashMap::new();

    // Semantic search docs
    templates.insert("semantic_search_docs".to_string(), GuidanceTemplate {
        no_results: Some("No results found. Try broader search terms or check if the codebase is indexed.".to_string()),
        single_result: Some("Found one match. Consider using 'find_symbol' or 'get_calls' to explore this symbol's relationships.".to_string()),
        multiple_results: Some("Found {result_count} matches. Consider using 'find_symbol' on the most relevant result for detailed analysis, or refine your search query.".to_string()),
        custom: vec![
            GuidanceRange {
                min: 10,
                max: None,
                template: "Found {result_count} matches. Consider refining your search with more specific terms.".to_string(),
            }
        ],
    });

    // Find symbol
    templates.insert("find_symbol".to_string(), GuidanceTemplate {
        no_results: Some("Symbol not found. Use 'search_symbols' with fuzzy matching or 'semantic_search_docs' for broader search.".to_string()),
        single_result: Some("Symbol found with full context. Explore 'get_calls' to see what it calls, 'find_callers' to see usage, or 'analyze_impact' to understand change implications.".to_string()),
        multiple_results: Some("Found {result_count} symbols with that name. Review each to find the one you're looking for.".to_string()),
        custom: vec![],
    });

    // Get calls
    templates.insert("get_calls".to_string(), GuidanceTemplate {
        no_results: Some("No function calls found. This might be a leaf function or data structure.".to_string()),
        single_result: Some("Found 1 function call. Use 'find_symbol' to explore this dependency.".to_string()),
        multiple_results: Some("Found {result_count} function calls. Consider using 'find_symbol' on key dependencies or 'analyze_impact' to trace the call chain further.".to_string()),
        custom: vec![],
    });

    // Find callers
    templates.insert("find_callers".to_string(), GuidanceTemplate {
        no_results: Some("No callers found. This might be an entry point, unused code, or called dynamically.".to_string()),
        single_result: Some("Found 1 caller. Use 'find_symbol' to explore where this function is used.".to_string()),
        multiple_results: Some("Found {result_count} callers. Consider 'analyze_impact' for complete dependency graph or investigate specific callers with 'find_symbol'.".to_string()),
        custom: vec![],
    });

    // Analyze impact
    templates.insert("analyze_impact".to_string(), GuidanceTemplate {
        no_results: Some("No impact detected. This symbol appears isolated. Consider using the codanna-navigator agent for comprehensive multi-hop analysis of complex relationships.".to_string()),
        single_result: Some("Minimal impact radius. This symbol has limited dependencies.".to_string()),
        multiple_results: Some("Impact analysis shows {result_count} affected symbols. Focus on critical paths or use 'find_symbol' on key dependencies.".to_string()),
        custom: vec![
            GuidanceRange {
                min: 2,
                max: Some(5),
                template: "Limited impact radius with {result_count} affected symbols. This change is relatively contained.".to_string(),
            },
            GuidanceRange {
                min: 20,
                max: None,
                template: "Significant impact with {result_count} affected symbols. Consider breaking this change into smaller parts.".to_string(),
            }
        ],
    });

    // Search symbols
    templates.insert("search_symbols".to_string(), GuidanceTemplate {
        no_results: Some("No symbols match your query. Try 'semantic_search_docs' for natural language search or adjust your pattern.".to_string()),
        single_result: Some("Found exactly one match. Use 'find_symbol' to get full details about this symbol.".to_string()),
        multiple_results: Some("Found {result_count} matching symbols. Use 'find_symbol' on specific results for full context or narrow your search with 'kind' parameter.".to_string()),
        custom: vec![],
    });

    // Semantic search with context
    templates.insert("semantic_search_with_context".to_string(), GuidanceTemplate {
        no_results: Some("No semantic matches found. Try different phrasing or ensure documentation exists for the concepts you're searching.".to_string()),
        single_result: Some("Found one match with full context. Review the relationships to understand how this fits into the codebase.".to_string()),
        multiple_results: Some("Rich context provided for {result_count} matches. Investigate specific relationships using targeted tools like 'get_calls' or 'find_callers'.".to_string()),
        custom: vec![],
    });

    // Get index info
    templates.insert(
        "get_index_info".to_string(),
        GuidanceTemplate {
            no_results: None, // Not applicable
            single_result: Some(
                "Index statistics loaded. Use search tools to explore the codebase.".to_string(),
            ),
            multiple_results: None, // Not applicable
            custom: vec![],
        },
    );

    templates
}

fn default_guidance_variables() -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("project".to_string(), "codanna".to_string());
    vars
}

/// Generate language defaults from the registry
/// This queries the language registry to get all registered languages
/// and their default configurations
fn generate_language_defaults() -> HashMap<String, LanguageConfig> {
    // Try to get languages from the registry
    if let Ok(registry) = crate::parsing::get_registry().lock() {
        let mut configs = HashMap::new();

        // Iterate through all registered languages
        for def in registry.iter_all() {
            configs.insert(
                def.id().as_str().to_string(),
                LanguageConfig {
                    enabled: def.default_enabled(),
                    extensions: def.extensions().iter().map(|s| s.to_string()).collect(),
                    parser_options: HashMap::new(),
                    config_files: Vec::new(), // Empty by default - opt-in feature
                },
            );
        }

        // Return registry-generated configs if we got any
        if !configs.is_empty() {
            return configs;
        }
    }

    // Minimal fallback for catastrophic failure
    // Only include Rust as it's the most essential language
    fallback_minimal_languages()
}

/// Minimal fallback language configuration
/// Used only when registry is completely unavailable
fn fallback_minimal_languages() -> HashMap<String, LanguageConfig> {
    let mut langs = HashMap::new();

    // Include only Rust as the minimal working configuration
    langs.insert(
        "rust".to_string(),
        LanguageConfig {
            enabled: true,
            extensions: vec!["rs".to_string()],
            parser_options: HashMap::new(),
            config_files: Vec::new(),
        },
    );

    langs
}

impl Settings {
    fn sync_indexed_path_cache(&mut self) {
        self.indexed_paths_cache = self.indexing.indexed_paths.clone();
    }

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

    /// Create a default settings file with helpful comments
    pub fn init_config_file(force: bool) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Use configurable directory name from init module
        let local_dir = crate::init::local_dir_name();
        let config_path = PathBuf::from(local_dir).join("settings.toml");

        if !force && config_path.exists() {
            return Err("Configuration file already exists. Use --force to overwrite".into());
        }

        // Create parent directory if needed
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create settings with project-specific values
        let settings = Settings::for_init()?;

        // Convert to TOML
        let toml_string = toml::to_string_pretty(&settings)?;

        // Enhance with comments and documentation
        let final_toml = Self::add_config_comments(toml_string);

        std::fs::write(&config_path, final_toml)?;

        if force {
            println!("Overwrote configuration at: {}", config_path.display());
        } else {
            println!(
                "Created default configuration at: {}",
                config_path.display()
            );
        }

        // Create default .codannaignore file
        Self::create_default_ignore_file(force)?;

        // Initialize global directories and symlink
        crate::init::init_global_dirs()?;

        // Try to create symlink, but don't fail if it doesn't work (Windows privileges)
        // The symlink is optional since we use with_cache_dir() API in fastembed 5.0+
        if let Err(e) = crate::init::create_fastembed_symlink() {
            eprintln!("Note: Could not create model cache symlink: {e}");
            eprintln!("      This is normal on Windows without Developer Mode enabled.");
            eprintln!("      Models will be managed via cache directory API instead.");
        }

        // Create index directory structure (including tantivy subdirectory)
        let index_path = PathBuf::from(crate::init::local_dir_name()).join("index");
        std::fs::create_dir_all(&index_path)?;
        let tantivy_path = index_path.join("tantivy");
        std::fs::create_dir_all(&tantivy_path)?;

        // Check if project is already registered (by path in registry or by local file)
        let local_dir = crate::init::local_dir_name();
        let project_id_path = PathBuf::from(local_dir).join(".project-id");
        let project_path = std::env::current_dir()?;

        // Always use register_or_update which checks for existing projects by path
        let project_id = crate::init::ProjectRegistry::register_or_update_project(&project_path)?;

        // Check if we need to update the local .project-id file
        if project_id_path.exists() {
            let existing_id = std::fs::read_to_string(&project_id_path)?;
            if existing_id.trim() != project_id {
                // Update the file if the ID changed (shouldn't happen normally)
                std::fs::write(&project_id_path, &project_id)?;
                println!("Updated project ID: {project_id}");
            } else {
                println!("Project already registered with ID: {project_id}");
            }
        } else {
            // Create .project-id file for the first time
            std::fs::write(&project_id_path, &project_id)?;
            println!("Project registered with ID: {project_id}");
        }

        Ok(config_path)
    }

    /// Add helpful comments to the generated TOML configuration
    fn add_config_comments(toml: String) -> String {
        let mut result = String::from(
            "# Codanna Configuration File\n\
             # https://github.com/bartolli/codanna\n\n",
        );

        let mut in_languages_section = false;
        let mut prev_line_was_section = false;

        for line in toml.lines() {
            // Skip empty lines after section headers to avoid double spacing
            if line.is_empty() && prev_line_was_section {
                prev_line_was_section = false;
                continue;
            }
            prev_line_was_section = false;

            // Add section and field comments
            if line == "version = 1" {
                result.push_str("# Version of the configuration schema\n");
            } else if line.starts_with("index_path = ") {
                result.push_str("\n# Path to the index directory (relative to workspace root)\n");
            } else if line.starts_with("workspace_root = ") {
                result.push_str("\n# Workspace root directory (automatically detected)\n");
            } else if line == "[indexing]" {
                result.push_str("\n[indexing]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("parallelism = ") {
                result.push_str("# CPU cores to use for indexing (default: all cores)\n");
                result.push_str("# Thread counts for each stage are derived from this value\n");
            } else if line.starts_with("tantivy_heap_mb = ") {
                result.push_str("\n# Tantivy heap size in megabytes\n");
                result.push_str("# Reduce to 15-25MB if you have permission issues (antivirus, SELinux, containers)\n");
                result.push_str(
                    "# Increase to 100-200MB if you have plenty of RAM and no restrictions\n",
                );
            } else if line.starts_with("max_retry_attempts = ") {
                result.push_str("\n# Retry attempts for transient file system errors\n");
                result.push_str("# Exponential backoff: 100ms, 200ms, 400ms delays\n");
            } else if line.starts_with("ignore_patterns = ") {
                result.push_str("\n# Additional patterns to ignore during indexing\n");
            } else if line.starts_with("indexed_paths = ") {
                result.push_str("\n# List of directories to index\n");
                result.push_str("# Add folders using: codanna add-dir <path>\n");
                result.push_str("# Remove folders using: codanna remove-dir <path>\n");
                result.push_str("# List all folders using: codanna list-dirs\n");
            } else if line.starts_with("batch_size = ") {
                result.push_str("\n# Items per batch before flushing to index (default: 5000)\n");
            } else if line.starts_with("batches_per_commit = ") {
                result.push_str("\n# Number of batches before committing to disk (default: 10)\n");
            } else if line.starts_with("pipeline_tracing = ") {
                result.push_str("\n# Enable detailed pipeline stage tracing\n");
                result.push_str("# Shows timing, throughput, and memory for each stage\n");
                result.push_str("# Requires: logging.modules.pipeline = \"info\"\n");
            } else if line.starts_with("show_progress = ") {
                result.push_str("\n# Show progress bars during indexing (default: true)\n");
                result.push_str("# Use --no-progress CLI flag to override\n");
            } else if line == "[mcp]" {
                result.push_str("\n[mcp]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("max_context_size = ") {
                result.push_str("# Maximum context size in bytes for MCP server\n");
            } else if line == "[semantic_search]" {
                result.push_str("\n[semantic_search]\n");
                result.push_str("# Semantic search for natural language code queries\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("enabled = ") && !in_languages_section {
                // enabled field in semantic_search - comment already added above
            } else if line.starts_with("model = ") {
                result.push_str("\n# Model to use for embeddings\n");
                result.push_str(
                    "# Note: Changing models requires re-indexing (codanna index --force)\n",
                );
                result.push_str("# - AllMiniLML6V2: English-only, 384 dimensions (default)\n");
                result.push_str("# - MultilingualE5Small: 94 languages including, 384 dimensions (recommended for multilingual)\n");
                result.push_str(
                    "# - MultilingualE5Base: 94 languages, 768 dimensions (better quality)\n",
                );
                result.push_str(
                    "# - MultilingualE5Large: 94 languages, 1024 dimensions (best quality)\n",
                );
                result.push_str("# - BGESmallZHV15: Chinese-specialized, 512 dimensions\n");
                result.push_str("# - See documentation for full list of available models\n");
            } else if line.starts_with("threshold = ") {
                result.push_str("\n# Similarity threshold for search results (0.0 to 1.0)\n");
            } else if line.starts_with("embedding_threads = ") {
                result.push_str("\n# Number of parallel embedding model instances (default: 3)\n");
                result
                    .push_str("# Each instance uses ~86MB RAM. Higher values = faster indexing.\n");
                result.push_str("# Set to 1 for low-memory systems, 4-6 for high-end machines.\n");
            } else if line == "[file_watch]" {
                result.push_str("\n[file_watch]\n");
                result.push_str("# Enable automatic file watching for indexed files\n");
                result.push_str("# When enabled, the MCP server will automatically re-index files when they change\n");
                result.push_str("# Default: true (enabled for better user experience)\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("enabled = ") && in_languages_section {
                // Skip comment for language enabled field
            } else if line.starts_with("debounce_ms = ") {
                result.push_str("\n# Debounce interval in milliseconds\n");
                result.push_str("# How long to wait after a file change before re-indexing\n");
            } else if line == "[server]" {
                result.push_str("\n[server]\n");
                result.push_str("# Server mode: \"stdio\" (default) or \"http\"\n");
                result.push_str("# stdio: Lightweight, spawns per request (best for production)\n");
                result.push_str(
                    "# http: Persistent server, real-time file watching (best for development)\n",
                );
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("mode = ") {
                // mode field - comment already added above
            } else if line.starts_with("bind = ") {
                result.push_str("\n# HTTP server bind address (only used when mode = \"http\" or --http flag)\n");
            } else if line.starts_with("watch_interval = ") {
                result.push_str("\n# Watch interval for stdio mode in seconds (how often to check for file changes)\n");
            } else if line == "[logging]" {
                result.push_str("\n[logging]\n");
                result.push_str("# Logging configuration\n");
                result.push_str("# Levels: \"error\", \"warn\" (default/quiet), \"info\", \"debug\", \"trace\"\n");
                result.push_str("# Override with RUST_LOG env var: RUST_LOG=debug codanna index\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("default = ") && !in_languages_section {
                result.push_str("# Default log level (\"warn\" = quiet, \"info\" = normal, \"debug\" = verbose)\n");
            } else if line == "[logging.modules]" {
                result.push_str("\n[logging.modules]\n");
                result.push_str("# Per-module log level overrides\n");
                result.push_str(
                    "# Internal modules (auto-prefixed with codanna::): watcher, mcp, indexing, storage, semantic\n",
                );
                result.push_str("# External targets (used as-is): cli, tantivy, pipeline\n");
                result.push_str("# Example: cli = \"debug\"      # CLI startup logs\n");
                result.push_str("# Example: tantivy = \"error\"  # Suppress Tantivy logs\n");
                result.push_str("# Example: pipeline = \"info\"  # Pipeline progress logs\n");
                prev_line_was_section = true;
                continue;
            } else if line == "[documents]" {
                result.push_str("\n[documents]\n");
                result.push_str("# Document embedding for RAG (Retrieval-Augmented Generation)\n");
                result.push_str("# Index markdown and text files for semantic search\n");
                prev_line_was_section = true;
                continue;
            } else if line == "[documents.defaults]" {
                result.push_str("\n[documents.defaults]\n");
                result.push_str("# Default chunking settings for all collections\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("strategy = ") {
                result.push_str(
                    "# Chunking strategy: \"hybrid\" (paragraph-based with size constraints)\n",
                );
            } else if line.starts_with("min_chunk_chars = ") {
                result.push_str("\n# Minimum characters per chunk (small chunks merged)\n");
            } else if line.starts_with("max_chunk_chars = ") {
                result.push_str("\n# Maximum characters per chunk (large chunks split)\n");
            } else if line.starts_with("overlap_chars = ") {
                result.push_str("\n# Overlap between chunks when splitting\n");
            } else if line == "[documents.search]" {
                result.push_str("\n[documents.search]\n");
                result.push_str("# Search result display settings\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("preview_mode = ") {
                result.push_str("# Preview mode: \"kwic\" (Keyword In Context) or \"full\"\n");
                result.push_str("# kwic: Centers preview around keyword match (recommended)\n");
                result.push_str("# full: Shows entire chunk content\n");
            } else if line.starts_with("preview_chars = ") {
                result.push_str("\n# Number of characters to show in preview (for kwic mode)\n");
            } else if line.starts_with("highlight = ") {
                result.push_str("\n# Highlight matching keywords with **markers**\n");
            } else if line == "[documents.collections]" {
                result.push_str("\n[documents.collections]\n");
                result.push_str("# Add document collections to index. Example:\n");
                result.push_str("# [documents.collections.my-docs]\n");
                result.push_str("# paths = [\"docs/\"]\n");
                result.push_str("# patterns = [\"**/*.md\"]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("[documents.collections.") {
                result.push_str("\n# Collection configuration\n");
                result.push_str("# paths: directories or files to include\n");
                result.push_str("# patterns: glob patterns to match (default: [\"**/*.md\"])\n");
            } else if line.starts_with("[languages.") {
                if !in_languages_section {
                    result.push_str("\n# Language-specific settings\n");
                    result.push_str("# Currently supported: Rust, Python, PHP, TypeScript, Go, C, C++, CSharp, Gdscript\n");
                    in_languages_section = true;
                }
                result.push('\n');
            }

            result.push_str(line);
            result.push('\n');
        }

        result
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

    /// Add a folder to the list of indexed paths
    pub fn add_indexed_path(&mut self, path: PathBuf) -> Result<(), String> {
        // Canonicalize the path to avoid duplicates
        let canonical_path = path
            .canonicalize()
            .map_err(|e| format!("Invalid path: {e}"))?;

        // Track whether we should remove child paths that are covered by the new entry
        let mut has_descendants = false;

        // Check if path already exists or is covered by an existing parent
        for existing in &self.indexed_paths_cache {
            if *existing == canonical_path {
                return Err(format!("Path already indexed: {}", path.display()));
            }

            // If an existing entry is an ancestor of the new path, treat as already indexed
            if canonical_path.starts_with(existing) {
                return Err(format!(
                    "Path already indexed: {} (covered by {})",
                    path.display(),
                    existing.display()
                ));
            }

            // Record descendant paths so we can prune them before inserting the parent
            if existing.starts_with(&canonical_path) {
                has_descendants = true;
            }
        }

        if has_descendants {
            // Remove any paths that are descendants of the new canonical path
            self.indexing
                .indexed_paths
                .retain(|existing| !existing.starts_with(&canonical_path));
            self.indexed_paths_cache
                .retain(|existing| !existing.starts_with(&canonical_path));
        }

        // Add the path
        self.indexing.indexed_paths.push(canonical_path.clone());
        self.indexed_paths_cache.push(canonical_path);
        Ok(())
    }

    /// Remove a folder from the list of indexed paths
    pub fn remove_indexed_path(&mut self, path: &Path) -> Result<(), String> {
        let canonical_path = path
            .canonicalize()
            .map_err(|e| format!("Invalid path: {e}"))?;

        let original_len = self.indexing.indexed_paths.len();
        self.indexing.indexed_paths.retain(|p| p != &canonical_path);
        self.indexed_paths_cache.retain(|p| p != &canonical_path);

        if self.indexing.indexed_paths.len() == original_len {
            return Err(format!(
                "Path not found in indexed paths: {}",
                path.display()
            ));
        }

        Ok(())
    }

    /// Get all indexed paths
    /// Returns empty vector if none are configured (maintains backward compatibility)
    pub fn get_indexed_paths(&self) -> Vec<PathBuf> {
        self.indexing.indexed_paths.clone()
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
