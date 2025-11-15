//! 設定管理モジュール
//!
//! このモジュールは、コードベースインテリジェンスシステムのための
//! レイヤード設定システムを提供します。以下の優先順位で設定を読み込みます：
//! - デフォルト値
//! - TOML設定ファイル
//! - 環境変数によるオーバーライド
//! - CLIコマンドライン引数によるオーバーライド
//!
//! # 環境変数
//!
//! 環境変数は `CI_` プレフィックスを必要とし、ネストされたレベルを
//! 区切るためにダブルアンダースコアを使用します：
//! - `CI_INDEXING__PARALLEL_THREADS=8` は `indexing.parallel_threads` を設定
//! - `CI_MCP__DEBUG=true` は `mcp.debug` を設定
//! - `CI_INDEXING__INCLUDE_TESTS=false` は `indexing.include_tests` を設定
//!
//! # 使用例
//!
//! ```no_run
//! use codanna::config::Settings;
//!
//! // デフォルト設定を読み込む
//! let settings = Settings::default();
//!
//! // 特定のパスから設定を読み込む
//! let settings = Settings::from_workspace_root("/path/to/workspace")
//!     .expect("設定の読み込みに失敗しました");
//! ```

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// メイン設定構造体
///
/// Codannaシステム全体の設定を保持します。TOML形式の設定ファイル、
/// 環境変数、CLIオプションから読み込まれます。
///
/// # フィールド
///
/// * `version` - 設定スキーマのバージョン
/// * `index_path` - インデックスディレクトリへのパス
/// * `workspace_root` - ワークスペースルートディレクトリ (.codanna の場所)
/// * `debug` - グローバルデバッグモード
/// * `indexing` - インデックス作成設定
/// * `languages` - 言語固有の設定
/// * `mcp` - MCPサーバー設定
/// * `semantic_search` - セマンティック検索設定
/// * `file_watch` - ファイル監視設定
/// * `server` - サーバー設定
/// * `guidance` - AIガイダンス設定
///
/// # 使用例
///
/// ```no_run
/// use codanna::config::Settings;
///
/// let settings = Settings::default();
/// println!("インデックスパス: {:?}", settings.index_path);
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Settings {
    /// 設定スキーマのバージョン
    #[serde(default = "default_version")]
    pub version: u32,

    /// インデックスディレクトリへのパス
    #[serde(default = "default_index_path")]
    pub index_path: PathBuf,

    /// ワークスペースルートディレクトリ (.codanna の場所)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,

    /// グローバルデバッグモード
    #[serde(default = "default_false")]
    pub debug: bool,

    /// インデックス作成設定
    #[serde(default)]
    pub indexing: IndexingConfig,

    /// 高速検索のためのキャッシュされた正規化パス (シリアライズ対象外)
    #[serde(skip)]
    pub indexed_paths_cache: Vec<PathBuf>,

    /// 言語固有の設定
    #[serde(default)]
    pub languages: HashMap<String, LanguageConfig>,

    /// MCPサーバー設定
    #[serde(default)]
    pub mcp: McpConfig,

    /// セマンティック検索設定
    #[serde(default)]
    pub semantic_search: SemanticSearchConfig,

    /// ファイル監視設定
    #[serde(default)]
    pub file_watch: FileWatchConfig,

    /// サーバー設定 (stdio/httpモード)
    #[serde(default)]
    pub server: ServerConfig,

    /// マルチホップクエリ用のAIガイダンス設定
    #[serde(default)]
    pub guidance: GuidanceConfig,
}

/// インデックス作成設定
///
/// コードベースのインデックス作成に関する設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::IndexingConfig;
///
/// let config = IndexingConfig::default();
/// assert!(config.parallel_threads > 0);
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IndexingConfig {
    /// インデックス作成の並列スレッド数
    #[serde(default = "default_parallel_threads")]
    pub parallel_threads: usize,

    /// Tantivyヒープサイズ（メガバイト単位）
    /// ディスクにフラッシュする前のメモリ使用量を制御
    #[serde(default = "default_tantivy_heap_mb")]
    pub tantivy_heap_mb: usize,

    /// 一時的なファイルシステムエラーの最大再試行回数
    /// アンチウイルス、SELinuxなどによる権限遅延に対処
    #[serde(default = "default_max_retry_attempts")]
    pub max_retry_attempts: u32,

    /// プロジェクトルートディレクトリ（デフォルトはワークスペースルート）
    /// gitignore解決とモジュールパス計算に使用
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,

    /// インデックス作成時に無視するパターン
    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    /// インデックス対象ディレクトリのリスト
    /// このリストは add-dir および remove-dir コマンドで管理される
    #[serde(default)]
    pub indexed_paths: Vec<PathBuf>,
}

/// 言語固有の設定
///
/// 各プログラミング言語のパーサーと動作設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::LanguageConfig;
///
/// let mut config = LanguageConfig::default();
/// config.enabled = true;
/// config.extensions = vec!["rs".to_string(), "rust".to_string()];
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LanguageConfig {
    /// この言語が有効かどうか
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// この言語のファイル拡張子
    #[serde(default)]
    pub extensions: Vec<String>,

    /// 追加のパーサーオプション
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parser_options: HashMap<String, serde_json::Value>,

    /// 監視するプロジェクト設定ファイル (例: tsconfig.json, pyproject.toml)
    /// デフォルトは空 - プロジェクト解決はオプトイン
    #[serde(default)]
    pub config_files: Vec<PathBuf>,
}

/// MCP (Model Context Protocol) サーバー設定
///
/// MCPサーバーの動作設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::McpConfig;
///
/// let config = McpConfig::default();
/// assert!(config.max_context_size > 0);
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct McpConfig {
    /// 最大コンテキストサイズ（バイト単位）
    #[serde(default = "default_max_context_size")]
    pub max_context_size: usize,

    /// デバッグログを有効化
    #[serde(default = "default_false")]
    pub debug: bool,
}

/// セマンティック検索設定
///
/// コードの意味的な検索機能の設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::SemanticSearchConfig;
///
/// let mut config = SemanticSearchConfig::default();
/// config.enabled = true;
/// config.threshold = 0.7;
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SemanticSearchConfig {
    /// セマンティック検索を有効化
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// 埋め込みに使用するモデル
    #[serde(default = "default_embedding_model")]
    pub model: String,

    /// 検索結果の類似度閾値
    #[serde(default = "default_similarity_threshold")]
    pub threshold: f32,
}

/// ファイル監視設定
///
/// インデックス対象ファイルの自動監視設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::FileWatchConfig;
///
/// let config = FileWatchConfig::default();
/// assert!(config.enabled);
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FileWatchConfig {
    /// インデックス対象ファイルの自動監視を有効化
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// デバウンス間隔（ミリ秒単位、デフォルト: 500ms）
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
}

/// サーバー設定
///
/// MCPサーバーの起動モードと接続設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::ServerConfig;
///
/// let config = ServerConfig::default();
/// assert_eq!(config.mode, "stdio");
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    /// デフォルトのサーバーモード: "stdio" または "http"
    #[serde(default = "default_server_mode")]
    pub mode: String,

    /// HTTPサーバーのバインドアドレス
    #[serde(default = "default_bind_address")]
    pub bind: String,

    /// stdioモードの監視間隔（秒単位）
    #[serde(default = "default_watch_interval")]
    pub watch_interval: u64,
}

/// AIガイダンス設定
///
/// マルチホップクエリのためのAIガイダンスシステムの設定を保持します。
///
/// # 使用例
///
/// ```
/// use codanna::config::GuidanceConfig;
/// use std::collections::HashMap;
///
/// let mut config = GuidanceConfig::default();
/// config.enabled = true;
/// config.variables.insert("key".to_string(), "value".to_string());
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuidanceConfig {
    /// AIガイダンスシステムを有効化
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// 特定のツール用のテンプレート
    #[serde(default)]
    pub templates: HashMap<String, GuidanceTemplate>,

    /// グローバルテンプレート変数
    #[serde(default)]
    pub variables: HashMap<String, String>,
}

/// ガイダンステンプレート
///
/// 特定の状況におけるガイダンスメッセージのテンプレートを保持します。
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuidanceTemplate {
    /// 結果がない場合のテンプレート
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
fn default_parallel_threads() -> usize {
    num_cpus::get()
}
fn default_tantivy_heap_mb() -> usize {
    50 // Universal default that balances performance and permissions
}
fn default_max_retry_attempts() -> u32 {
    3 // Exponential backoff: 100ms, 200ms, 400ms
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
            debug: false,
            indexing: IndexingConfig::default(),
            indexed_paths_cache: Vec::new(),
            languages: generate_language_defaults(), // Now uses registry
            mcp: McpConfig::default(),
            semantic_search: SemanticSearchConfig::default(),
            file_watch: FileWatchConfig::default(),
            server: ServerConfig::default(),
            guidance: GuidanceConfig::default(),
        }
    }
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            parallel_threads: default_parallel_threads(),
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
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            max_context_size: default_max_context_size(),
            debug: false,
        }
    }
}

impl Default for SemanticSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Enabled by default for better code intelligence
            model: default_embedding_model(),
            threshold: default_similarity_threshold(),
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
        std::fs::write(path, toml_string)?;

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
            } else if line.starts_with("debug = ") && !in_languages_section {
                result.push_str("\n# Global debug mode\n");
            } else if line == "[indexing]" {
                result.push_str("\n[indexing]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("parallel_threads = ") {
                result.push_str(
                    "# Number of parallel threads for indexing (defaults to CPU count)\n",
                );
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
            } else if line == "[mcp]" {
                result.push_str("\n[mcp]\n");
                prev_line_was_section = true;
                continue;
            } else if line.starts_with("max_context_size = ") {
                result.push_str("# Maximum context size in bytes for MCP server\n");
            } else if line.starts_with("debug = ")
                && !line.contains("false")
                && in_languages_section
            {
                // Skip MCP debug comment if in languages section
            } else if line.starts_with("debug = ") && line.contains("false") {
                result.push_str("\n# Enable debug logging for MCP server\n");
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

/// Global check for whether debug logging is enabled.
/// Uses settings from .codanna/settings.toml and caches the result.
pub fn is_global_debug_enabled() -> bool {
    static DEBUG_FLAG: OnceLock<bool> = OnceLock::new();
    *DEBUG_FLAG.get_or_init(|| Settings::load().map(|s| s.debug).unwrap_or(false))
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
        assert!(settings.mcp.debug);
        assert!(!settings.languages["rust"].enabled);
    }

    #[test]
    fn test_save_settings() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");

        let mut settings = Settings::default();
        settings.indexing.parallel_threads = 2;
        settings.mcp.debug = true;

        settings.save(&config_path).unwrap();

        let loaded = Settings::load_from(&config_path).unwrap();
        assert_eq!(loaded.indexing.parallel_threads, 2);
        assert!(loaded.mcp.debug);
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
parallel_threads = 8
include_tests = true

[mcp]
max_context_size = 50000
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
        assert_eq!(settings.mcp.max_context_size, 50000);
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
}
