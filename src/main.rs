//! CLI entry point for the codebase intelligence system.
//!
//! Provides commands for indexing, querying, and serving code intelligence data.
//! Main components: Cli parser, Commands enum, and async runtime with MCP server support.

use clap::{
    Parser, Subcommand,
    builder::styling::{AnsiColor, Effects, Styles},
};
use codanna::FileId;
use codanna::parsing::{
    CSharpParser, GoParser, LanguageParser, PhpParser, PythonParser, RustParser, TypeScriptParser,
};
use codanna::project_resolver::{
    providers::{
        java::JavaProvider, javascript::JavaScriptProvider, swift::SwiftProvider,
        typescript::TypeScriptProvider,
    },
    registry::SimpleProviderRegistry,
};
use codanna::storage::IndexMetadata;
use codanna::types::SymbolCounter;
use codanna::{IndexPersistence, Settings, SimpleIndexer, Symbol, SymbolKind};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

// MCP tool JSON output structures
#[derive(Debug, Serialize)]
struct IndexInfo {
    symbol_count: usize,
    file_count: usize,
    relationship_count: usize,
    symbol_kinds: SymbolKindBreakdown,
    semantic_search: SemanticSearchInfo,
}

#[derive(Debug, Serialize)]
struct SymbolKindBreakdown {
    functions: usize,
    methods: usize,
    structs: usize,
    traits: usize,
}

#[derive(Debug, Serialize)]
struct SemanticSearchInfo {
    enabled: bool,
    model_name: Option<String>,
    embeddings: Option<usize>,
    dimensions: Option<usize>,
    created: Option<String>,
    updated: Option<String>,
}

fn clap_cargo_style() -> Styles {
    Styles::styled()
        .header(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .usage(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

/// Create custom help text with consistent styling
fn create_custom_help() -> String {
    use codanna::display::theme::Theme;
    use console::style;

    let mut help = String::new();

    // Quick Start section
    if Theme::should_disable_colors() {
        help.push_str("Quick Start:\n");
    } else {
        help.push_str(&format!("{}\n", style("Quick Start:").cyan().bold()));
    }
    help.push_str("  $ codanna init                      # Initialize in current directory\n");
    help.push_str("  $ codanna index src lib            # Index multiple directories\n");
    help.push_str("  $ codanna add-dir tests            # Add tests directory to indexed paths\n");
    help.push_str("  $ codanna list-dirs                # List all indexed directories\n");
    help.push_str("  $ codanna serve --http --watch     # HTTP server with OAuth\n");
    help.push_str("  $ codanna serve --https --watch    # HTTPS server with TLS\n");
    help.push_str("  $ codanna documents add-collection docs ./docs  # Add doc collection\n");
    help.push_str("  $ codanna documents index          # Index all document collections\n\n");

    // About section
    help.push_str("Index code and query relationships, symbols, and dependencies.\n\n");

    // Usage
    if Theme::should_disable_colors() {
        help.push_str("Usage:");
    } else {
        help.push_str(&format!("{}", style("Usage:").cyan().bold()));
    }
    help.push_str(" codanna [OPTIONS] <COMMAND>\n\n");

    // Commands
    if Theme::should_disable_colors() {
        help.push_str("Commands:\n");
    } else {
        help.push_str(&format!("{}\n", style("Commands:").cyan().bold()));
    }
    help.push_str("  init          Set up .codanna directory\n");
    help.push_str("  index         Build searchable index from codebase\n");
    help.push_str("  add-dir       Add a directory to be indexed\n");
    help.push_str("  remove-dir    Remove a directory from indexed paths\n");
    help.push_str("  list-dirs     List all directories that are being indexed\n");
    help.push_str("  retrieve      Query symbols, relationships, and dependencies\n");
    help.push_str("  serve         Start MCP server\n");
    help.push_str("  config        Display active settings\n");
    help.push_str("  mcp-test      Test MCP connection\n");
    help.push_str("  mcp           Execute MCP tools directly\n");
    help.push_str("  benchmark     Benchmark parser performance\n");
    help.push_str("  parse         Output AST nodes in JSONL format\n");
    help.push_str("  plugin        Manage Claude Code plugins\n");
    help.push_str("  documents     Index and search document collections\n");
    help.push_str("  help          Print this message or the help of the given subcommand(s)\n\n");

    help.push_str("See 'codanna help <command>' for more information on a specific command.\n\n");

    // Options
    if Theme::should_disable_colors() {
        help.push_str("Options:\n");
    } else {
        help.push_str(&format!("{}\n", style("Options:").cyan().bold()));
    }
    help.push_str("  -c, --config <CONFIG>  Path to custom settings.toml file\n");
    help.push_str("      --info             Show detailed loading information\n");
    help.push_str("  -h, --help             Print help\n");
    help.push_str("  -V, --version          Print version\n\n");

    // Learn More
    if Theme::should_disable_colors() {
        help.push_str("Learn More:\n");
    } else {
        help.push_str(&format!("{}\n", style("Learn More:").cyan().bold()));
    }
    help.push_str("  GitHub: https://github.com/bartolli/codanna");

    help
}

/// Code intelligence system
#[derive(Parser)]
#[command(
    name = "codanna",
    version = env!("CARGO_PKG_VERSION"),
    about = "Code intelligence system",
    long_about = "Index code and query relationships, symbols, and dependencies.",
    next_line_help = true,
    styles = clap_cargo_style(),
    override_help = create_custom_help()
)]
struct Cli {
    /// Path to custom settings.toml file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Show detailed loading information
    #[arg(long, global = true)]
    info: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Available CLI commands
#[derive(Subcommand)]
enum Commands {
    /// Initialize project
    #[command(about = "Set up .codanna directory with default configuration")]
    Init {
        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },

    /// Index source files or directories
    #[command(about = "Build searchable index from codebase")]
    Index {
        /// Paths to files or directories to index (multiple paths allowed)
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Number of threads to use (overrides config)
        #[arg(short, long)]
        threads: Option<usize>,

        /// Force re-indexing even if index exists
        #[arg(short, long)]
        force: bool,

        /// Show progress during indexing
        #[arg(short, long)]
        progress: bool,

        /// Dry run - show what would be indexed without indexing
        #[arg(long)]
        dry_run: bool,

        /// Maximum number of files to index
        #[arg(long)]
        max_files: Option<usize>,
    },

    /// Add a directory to the indexed paths list
    #[command(about = "Add a directory to be indexed")]
    AddDir {
        /// Path to directory to add
        path: PathBuf,
    },

    /// Remove a directory from the indexed paths list
    #[command(about = "Remove a directory from indexed paths")]
    RemoveDir {
        /// Path to directory to remove
        path: PathBuf,
    },

    /// List all indexed directories
    #[command(about = "List all directories that are being indexed")]
    ListDirs,

    /// Query code relationships and dependencies
    #[command(
        about = "Search symbols, find callers/callees, analyze impact",
        long_about = "Query indexed symbols, relationships, and dependencies.",
        after_help = "Examples:\n  codanna retrieve symbol main\n  codanna retrieve callers process_file\n  codanna retrieve callers symbol_id:1771\n  codanna retrieve calls init\n  codanna retrieve calls symbol_id:1771\n  codanna retrieve implementations Parser\n  codanna retrieve describe OutputManager\n  codanna retrieve search \"parse\" --limit 10\n\nJSON paths:\n  retrieve symbol     .data.items[0].symbol.name\n  retrieve search     .data.items[].symbol.name\n  retrieve callers    .data.items[].symbol.name\n  retrieve describe   .data.items[0].symbol.name"
    )]
    Retrieve {
        #[command(subcommand)]
        query: RetrieveQuery,
    },

    /// Show current configuration settings
    #[command(about = "Display active settings from .codanna/settings.toml")]
    Config,

    /// Start MCP server
    #[command(
        about = "Start MCP server",
        long_about = "Start MCP server with optional HTTP/HTTPS modes.",
        after_help = "Examples:\n  codanna serve\n  codanna serve --http --watch\n  codanna serve --https --watch\n  codanna serve --http --bind 0.0.0.0:3000\n\nModes:\n  Default: stdio\n  --http: HTTP with OAuth\n  --https: HTTPS with TLS"
    )]
    Serve {
        /// Watch index file for changes and auto-reload
        #[arg(long, help = "Enable hot-reload when index changes")]
        watch: bool,

        /// Check interval in seconds (default: 5)
        #[arg(
            long,
            default_value = "5",
            help = "How often to check for index changes"
        )]
        watch_interval: u64,

        /// Enable HTTP server mode instead of stdio
        #[arg(long, help = "Run as HTTP server instead of stdio transport")]
        http: bool,

        /// Enable HTTPS server mode with TLS
        #[arg(
            long,
            conflicts_with = "http",
            help = "Run as HTTPS server with TLS support"
        )]
        https: bool,

        /// Bind address for HTTP/HTTPS server
        #[arg(
            long,
            default_value = "127.0.0.1:8080",
            help = "Address to bind HTTP/HTTPS server to"
        )]
        bind: String,
    },

    /// Test MCP connection
    #[command(name = "mcp-test", about = "Test MCP connection and list tools")]
    McpTest {
        /// Path to server binary (defaults to current binary)
        #[arg(long)]
        server_binary: Option<PathBuf>,

        /// Tool to call (if not specified, just lists tools)
        #[arg(long)]
        tool: Option<String>,

        /// Tool arguments as JSON
        #[arg(long)]
        args: Option<String>,

        /// Delay (seconds) before calling the tool, to exercise watcher reloads
        #[arg(
            long,
            help = "Wait N seconds before calling the tool",
            value_name = "SECONDS"
        )]
        delay: Option<u64>,
    },

    /// Call MCP tools directly (advanced)
    #[command(
        about = "Execute MCP tools directly",
        long_about = "Execute MCP tools directly without spawning a server.\n\nSupports positional arguments, key=value pairs, and JSON arguments.",
        after_help = "Examples:\n  codanna mcp find_symbol main\n  codanna mcp get_calls process_file\n  codanna mcp semantic_search_docs query:\"error handling\" limit:5\n  codanna mcp search_symbols query:parse kind:function\n  codanna mcp find_symbol Parser --json | jq '.data[].symbol.name'\n  codanna mcp search_symbols query:Parser --json | jq '.data[].name'\n\nTools:\n  find_symbol                  Find symbol by exact name\n  search_symbols               Full-text search with fuzzy matching\n  semantic_search_docs         Natural language search\n  semantic_search_with_context Natural language search with relationships\n  get_calls                    Functions called by a function\n  find_callers                 Functions that call a function\n  analyze_impact               Impact radius of symbol changes\n  get_index_info               Index statistics"
    )]
    Mcp {
        /// Tool to call
        tool: String,

        /// Positional arguments (can be simple values or key:value pairs)
        #[arg(num_args = 0..)]
        positional: Vec<String>,

        /// Tool arguments as JSON (for backward compatibility and complex cases)
        #[arg(long)]
        args: Option<String>,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Benchmark parser performance
    #[command(about = "Benchmark parser performance")]
    Benchmark {
        /// Language to benchmark (rust, python, php, typescript, go, csharp, all)
        #[arg(default_value = "all")]
        language: String,

        /// Custom file to benchmark
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// Parse a file and output AST nodes in JSONL format
    #[command(about = "Parse file and output AST as JSON Lines")]
    Parse {
        /// File to parse
        file: PathBuf,

        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Maximum depth to traverse
        #[arg(short = 'd', long)]
        max_depth: Option<usize>,

        /// Include all nodes (by default only named nodes are shown, like tree-sitter)
        #[arg(short = 'a', long)]
        all_nodes: bool,
    },

    /// Manage Claude Code plugins
    #[command(
        about = "Install, update, and manage Claude Code plugins from marketplaces",
        long_about = "Manage Claude Code plugins by installing from Git-based marketplaces.\n\nPlugins extend Claude Code with custom commands, agents, hooks, and MCP servers.\n\nNote: Plugins are installed and managed by codanna per-project in .claude/plugins, not managed by claude code CLI directly.",
        after_help = "Examples:\n  codanna plugin add https://github.com/user/marketplace plugin-name\n  codanna plugin remove plugin-name\n  codanna plugin update plugin-name --ref v2.0\n  codanna plugin list\n  codanna plugin verify plugin-name"
    )]
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },

    /// Index and search document collections for RAG
    #[command(
        about = "Index and search document collections",
        long_about = "Index markdown and text documents for semantic search.\n\nDocuments are chunked, embedded, and stored separately from code symbols.",
        after_help = "Examples:\n  codanna documents index --collection docs\n  codanna documents search \"error handling\" --collection docs\n  codanna documents list\n  codanna documents stats docs"
    )]
    Documents {
        #[command(subcommand)]
        action: DocumentAction,
    },

    /// Manage project profiles
    #[command(
        about = "Initialize and manage project profiles",
        long_about = "Manage project profiles for provider-specific initialization.\n\nProfiles set up project structure, configuration files, and provider integration.",
        after_help = "Examples:\n  codanna profile init claude\n  codanna profile install claude --source git@github.com:codanna/profiles.git\n  codanna profile list\n  codanna profile status"
    )]
    Profile {
        #[command(subcommand)]
        action: codanna::profiles::commands::ProfileAction,
    },
}

/// Plugin management actions
#[derive(Subcommand)]
enum PluginAction {
    /// Install a plugin from a marketplace
    #[command(
        about = "Install a plugin from a marketplace repository",
        after_help = "Examples:\n  codanna plugin add https://github.com/user/marketplace plugin-name\n  codanna plugin add ./local-marketplace my-plugin --ref v1.0"
    )]
    Add {
        /// Marketplace repository URL or local path
        marketplace: String,

        /// Plugin name to install
        plugin_name: String,

        /// Git reference (branch, tag, or commit SHA)
        #[arg(long)]
        r#ref: Option<String>,

        /// Force installation even if conflicts exist
        #[arg(short, long)]
        force: bool,

        /// Perform a dry run without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove an installed plugin
    #[command(
        about = "Remove an installed plugin and clean up its files",
        after_help = "Example:\n  codanna plugin remove plugin-name"
    )]
    Remove {
        /// Plugin name to remove
        plugin_name: String,

        /// Force removal even if other plugins depend on it
        #[arg(short, long)]
        force: bool,

        /// Perform a dry run without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Update an installed plugin
    #[command(
        about = "Update a plugin to a newer version",
        after_help = "Examples:\n  codanna plugin update plugin-name\n  codanna plugin update plugin-name --ref v2.0"
    )]
    Update {
        /// Plugin name to update
        plugin_name: String,

        /// Git reference to update to (branch, tag, or commit SHA)
        #[arg(long)]
        r#ref: Option<String>,

        /// Force update even if local modifications exist
        #[arg(short, long)]
        force: bool,

        /// Perform a dry run without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// List installed plugins
    #[command(
        about = "List all installed plugins with their versions",
        after_help = "Example:\n  codanna plugin list"
    )]
    List {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Verify plugin integrity
    #[command(
        about = "Verify that a plugin's files match their expected checksums",
        after_help = "Examples:\n  codanna plugin verify plugin-name\n  codanna plugin verify --all"
    )]
    Verify {
        /// Plugin name to verify (omit to verify all)
        plugin_name: Option<String>,

        /// Verify all installed plugins
        #[arg(long)]
        all: bool,

        /// Show detailed verification results
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Document collection management actions
#[derive(Subcommand)]
enum DocumentAction {
    /// Index documents from a collection
    #[command(
        about = "Index documents from a configured collection",
        after_help = "Examples:\n  codanna documents index --collection docs\n  codanna documents index --all\n  codanna documents index --progress"
    )]
    Index {
        /// Collection name to index (from settings.toml)
        #[arg(long)]
        collection: Option<String>,

        /// Index all configured collections
        #[arg(long)]
        all: bool,

        /// Force re-indexing of all files
        #[arg(short, long)]
        force: bool,

        /// Show progress bar during indexing
        #[arg(short, long)]
        progress: bool,
    },

    /// Search documents
    #[command(
        about = "Search indexed documents using natural language",
        after_help = "Examples:\n  codanna documents search \"error handling\"\n  codanna documents search \"authentication\" --collection docs --limit 5"
    )]
    Search {
        /// Search query text
        query: String,

        /// Filter by collection name
        #[arg(long)]
        collection: Option<String>,

        /// Maximum results to return
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// List collections
    #[command(
        about = "List all document collections",
        after_help = "Example:\n  codanna documents list"
    )]
    List {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Show collection statistics
    #[command(
        about = "Show statistics for a collection",
        after_help = "Example:\n  codanna documents stats docs"
    )]
    Stats {
        /// Collection name
        collection: String,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Add a collection to settings.toml
    #[command(
        about = "Add a document collection to settings.toml",
        after_help = "Examples:\n  codanna documents add-collection docs ./docs\n  codanna documents add-collection api-docs ./api --pattern \"**/*.md\""
    )]
    AddCollection {
        /// Collection name
        name: String,

        /// Path to include in the collection
        path: PathBuf,

        /// Glob pattern for file matching (default: **/*.md)
        #[arg(short, long)]
        pattern: Option<String>,
    },

    /// Remove a collection from settings.toml
    #[command(
        about = "Remove a document collection from settings.toml",
        after_help = "Examples:\n  codanna documents remove-collection docs\n\nNote: Run 'codanna documents index' after to clean the index."
    )]
    RemoveCollection {
        /// Collection name to remove
        name: String,
    },
}

/// Query types for retrieving indexed information.
///
/// Supports symbol lookups, relationship queries, impact analysis, and full-text search.
#[derive(Subcommand)]
enum RetrieveQuery {
    /// Find a symbol by name
    #[command(
        after_help = "Examples:\n  codanna retrieve symbol main\n  codanna retrieve symbol symbol_id:1771\n  codanna retrieve symbol name:main --json\n  codanna retrieve symbol MyStruct --json | jq '.file'"
    )]
    Symbol {
        /// Positional arguments (symbol name and/or key:value pairs)
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Show what functions a given function calls
    #[command(
        after_help = "Examples:\n  codanna retrieve calls process_file\n  codanna retrieve calls symbol_id:1771\n  codanna retrieve calls function:process_file --json"
    )]
    Calls {
        /// Positional arguments (function name and/or key:value pairs)
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Show what functions call a given function
    #[command(
        after_help = "Examples:\n  codanna retrieve callers main\n  codanna retrieve callers symbol_id:1771\n  codanna retrieve callers function:main --json"
    )]
    Callers {
        /// Positional arguments (function name and/or key:value pairs)
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Show what types implement a given trait
    #[command(
        after_help = "Examples:\n  codanna retrieve implementations Parser\n  codanna retrieve implementations trait:Parser --json"
    )]
    Implementations {
        /// Positional arguments (trait name and/or key:value pairs)
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Show what types a given symbol uses
    Uses {
        /// Name of the symbol
        symbol: String,
    },

    // DISABLED: Impact command has been deprecated in favor of:
    // 1. MCP semantic_search_with_context for comprehensive analysis
    // 2. Slash commands that compose simple retrieve commands
    // 3. Enhanced describe command (future)
    //
    // The impact command had fundamental issues:
    // - Didn't work for non-function symbols (structs, traits, enums)
    // - Returned empty results for valid symbols
    // - Conceptually flawed (not all symbols have "impact" in the same way)
    //
    // /// Show the impact radius of changing a symbol
    // #[command(
    //     after_help = "Examples:\n  codanna retrieve impact MyStruct\n  codanna retrieve impact symbol:MyStruct depth:3\n  codanna retrieve impact main --depth 2 --json"
    // )]
    // Impact {
    //     /// Positional arguments (symbol name and/or key:value pairs)
    //     #[arg(num_args = 0..)]
    //     args: Vec<String>,
    //     /// Maximum depth to search (flag format)
    //     #[arg(short, long)]
    //     depth: Option<usize>,
    //     /// Output in JSON format
    //     #[arg(long)]
    //     json: bool,
    // },
    /// Search for symbols using full-text search
    #[command(
        after_help = "Examples:\n  # Traditional flag format\n  codanna retrieve search \"parse\" --limit 5 --kind function\n  \n  # Key:value format (Unix-style)\n  codanna retrieve search query:parse limit:5 kind:function\n  \n  # Mixed format\n  codanna retrieve search \"parse\" limit:5 --json"
    )]
    Search {
        /// Positional arguments (query and/or key:value pairs)
        #[arg(num_args = 0..)]
        args: Vec<String>,

        /// Maximum number of results (flag format)
        #[arg(short, long)]
        limit: Option<usize>,

        /// Filter by symbol kind (flag format)
        #[arg(short, long)]
        kind: Option<String>,

        /// Filter by module path (flag format)
        #[arg(short, long)]
        module: Option<String>,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Show what methods a type or trait defines
    Defines {
        /// Name of the type or trait
        symbol: String,
    },

    /// Show dependency analysis for a symbol
    Dependencies {
        /// Name of the symbol
        symbol: String,
    },

    /// Show information about a symbol
    #[command(
        after_help = "Examples:\n  codanna retrieve describe SimpleIndexer\n  codanna retrieve describe symbol:SimpleIndexer --json"
    )]
    Describe {
        /// Positional arguments (symbol name and/or key:value pairs)
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Create and populate the provider registry with all language providers.
///
/// This registry manages project-specific resolution providers that handle
/// configuration files (like tsconfig.json) for enhanced import resolution.
fn create_provider_registry() -> SimpleProviderRegistry {
    let mut registry = SimpleProviderRegistry::new();

    // Add TypeScript provider for tsconfig.json resolution
    registry.add(Arc::new(TypeScriptProvider::new()));

    // Add JavaScript provider for jsconfig.json resolution
    registry.add(Arc::new(JavaScriptProvider::new()));

    // Add Java provider for pom.xml/build.gradle resolution
    registry.add(Arc::new(JavaProvider::new()));

    // Add Swift provider for Package.swift resolution
    registry.add(Arc::new(SwiftProvider::new()));

    registry
}

/// Initialize project resolution providers before indexing.
///
/// This validates configuration files and builds resolution caches for
/// languages that have config_files specified in settings.toml.
fn initialize_providers(
    registry: &SimpleProviderRegistry,
    settings: &Settings,
) -> Result<(), codanna::IndexError> {
    use codanna::IndexError;

    let mut validation_errors = Vec::new();

    for provider in registry.active_providers(settings) {
        let lang_id = provider.language_id();
        let config_paths = provider.config_paths(settings);

        if config_paths.is_empty() {
            continue; // Skip if no config files specified
        }

        tracing::debug!(target: "cli", "initializing {lang_id} project resolver...");

        // Validate config paths
        let mut invalid_paths = Vec::new();
        for path in &config_paths {
            if !path.exists() {
                invalid_paths.push(path.clone());
            }
        }

        if !invalid_paths.is_empty() {
            // Collect all invalid paths for error reporting
            for path in &invalid_paths {
                eprintln!("  ✗ {} config file not found: {}", lang_id, path.display());
            }
            validation_errors.push((lang_id.to_string(), invalid_paths));
            continue;
        }

        // Build cache
        tracing::debug!(
            target: "cli",
            "building resolution cache from {} config file(s)...",
            config_paths.len()
        );
        if let Err(e) = provider.rebuild_cache(settings) {
            // Warning only - continue without failing
            tracing::warn!(target: "cli", "failed to build {lang_id} resolution cache: {e}");
            tracing::warn!(target: "cli", "continuing without alias resolution for {lang_id}");
        } else {
            tracing::debug!(target: "cli", "{lang_id} resolution cache built successfully");
        }
    }

    if !validation_errors.is_empty() {
        // Build detailed error message
        let mut error_details = String::from("Invalid project configuration files:\n");
        for (lang, paths) in &validation_errors {
            error_details.push_str(&format!("\n{lang} configuration:\n"));
            for path in paths {
                error_details.push_str(&format!("  • {} not found\n", path.display()));
            }
        }
        error_details.push_str("\nSuggestion: Check paths in .codanna/settings.toml");
        error_details.push_str("\nExample for TypeScript:\n");
        error_details.push_str("  [languages.typescript]\n");
        error_details
            .push_str("  config_files = [\"tsconfig.json\", \"packages/web/tsconfig.json\"]");

        Err(IndexError::ConfigError {
            reason: error_details,
        })
    } else {
        Ok(())
    }
}

/// Add paths to settings.toml indexed_paths (idempotent)
///
/// Reusable helper for both `add-dir` and `index` commands.
/// Loads settings, adds paths (with deduplication), saves back to file.
///
/// For `add-dir`: Returns error if path already exists (strict)
/// For `index`: Silently skips existing paths (idempotent)
#[derive(Debug)]
enum SkipReason {
    CoveredBy(PathBuf),
    AlreadyPresent,
    FileNotPersisted,
}

#[derive(Debug)]
struct SkippedPath {
    path: PathBuf,
    reason: SkipReason,
}

#[derive(Default)]
struct SeedReport {
    newly_seeded: Vec<PathBuf>,
    missing_paths: Vec<PathBuf>,
}

fn seed_indexer_with_config_paths(
    indexer: &mut SimpleIndexer,
    config_paths: &[PathBuf],
) -> SeedReport {
    let mut report = SeedReport::default();

    if config_paths.is_empty() {
        return report;
    }

    // Collect existing tracked paths once to avoid repeated borrow issues
    let mut existing: std::collections::HashSet<PathBuf> =
        indexer.get_indexed_paths().iter().cloned().collect();

    for path in config_paths {
        if !path.exists() {
            report.missing_paths.push(path.clone());
            continue;
        }

        if !path.is_dir() {
            tracing::debug!(
                target: "cli",
                "skipping configured path (not a directory): {}",
                path.display()
            );
            continue;
        }

        if existing.contains(path) {
            continue;
        }

        let len_before = existing.len();
        match indexer.add_indexed_path(path) {
            Ok(_) => {
                // Refresh our view of tracked paths to honor internal dedup logic
                existing = indexer.get_indexed_paths().iter().cloned().collect();
                if existing.len() > len_before {
                    report.newly_seeded.push(path.clone());
                }
                tracing::debug!(
                    target: "cli",
                    "seeded configured directory into tracked paths: {}",
                    path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to track configured directory '{}': {e}",
                    path.display()
                );
            }
        }
    }

    report
}

fn add_paths_to_settings(
    paths: &[PathBuf],
    config_path: &Path,
    strict: bool,
) -> Result<(Settings, Vec<PathBuf>, Vec<SkippedPath>), String> {
    // Load settings from file
    let mut settings = Settings::load_from(config_path)
        .map_err(|e| format!("Error loading configuration: {e}"))?;

    let mut added_paths = Vec::new();
    let mut skipped_paths = Vec::new();

    // Add each path (Settings::add_indexed_path handles deduplication)
    for path in paths {
        if path.is_file() {
            if strict {
                return Err(format!(
                    "Path must be a directory (got file): {}",
                    path.display()
                ));
            }
            skipped_paths.push(SkippedPath {
                path: path.clone(),
                reason: SkipReason::FileNotPersisted,
            });
            continue;
        }

        match settings.add_indexed_path(path.clone()) {
            Ok(_) => {
                added_paths.push(path.clone());
            }
            Err(e) if e.contains("already indexed") => {
                // Path already exists
                if strict {
                    // add-dir is strict - return error
                    return Err(e);
                }
                // index is idempotent - report and skip
                let reason = path.canonicalize().ok().and_then(|canonical| {
                    settings
                        .indexing
                        .indexed_paths
                        .iter()
                        .find(|existing| canonical.starts_with(existing.as_path()))
                        .map(|existing| SkipReason::CoveredBy(existing.clone()))
                });
                skipped_paths.push(SkippedPath {
                    path: path.clone(),
                    reason: reason.unwrap_or(SkipReason::AlreadyPresent),
                });
            }
            Err(e) => {
                // Other errors (invalid path, etc.) always propagate
                return Err(format!("Error adding path {}: {e}", path.display()));
            }
        }
    }

    // Save only if we added new paths
    if !added_paths.is_empty() {
        settings
            .save(config_path)
            .map_err(|e| format!("Error saving configuration: {e}"))?;
    }

    Ok((settings, added_paths, skipped_paths))
}

/// Entry point with tokio async runtime.
///
/// Handles config initialization, index loading/creation, and command dispatch.
/// Auto-initializes config for index command. Persists index after modifications.
#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // For index command, auto-initialize if needed (but not when using --config)
    if matches!(cli.command, Commands::Index { .. }) && cli.config.is_none() {
        if Settings::check_init().is_err() {
            // Auto-initialize for index command
            eprintln!("Initializing project configuration...");
            match Settings::init_config_file(false) {
                Ok(path) => {
                    eprintln!("Created configuration file at: {}", path.display());
                }
                Err(e) => {
                    eprintln!("Warning: Could not create config file: {e}");
                    eprintln!("Using default configuration.");
                }
            }
        }
    } else if !matches!(cli.command, Commands::Init { .. }) && cli.config.is_none() {
        // For other commands without --config flag, just warn
        if let Err(warning) = Settings::check_init() {
            eprintln!("Warning: {warning}");
            eprintln!("Using default configuration for now.");
        }
    }

    // Load configuration
    let mut config = if let Some(config_path) = &cli.config {
        Settings::load_from(config_path).unwrap_or_else(|e| {
            eprintln!(
                "Configuration error loading from {}: {}",
                config_path.display(),
                e
            );
            std::process::exit(1);
        })
    } else {
        Settings::load().unwrap_or_else(|e| {
            eprintln!("Configuration error: {e}");
            Settings::default()
        })
    };

    // Initialize logging with config (supports RUST_LOG env var override)
    codanna::logging::init_with_config(&config.logging);

    // Handle thin client commands early (before index loading)
    // These commands don't need the index and should not conflict with server processes
    if let Commands::McpTest {
        server_binary,
        tool,
        args,
        delay,
    } = &cli.command
    {
        use codanna::mcp::client::CodeIntelligenceClient;

        let server_path = server_binary.clone().unwrap_or_else(|| {
            std::env::current_exe().expect("Failed to get current executable path")
        });

        if let Err(e) = CodeIntelligenceClient::test_server(
            server_path,
            cli.config.clone(),
            tool.clone(),
            args.clone(),
            *delay,
        )
        .await
        {
            eprintln!("MCP test failed: {e}");
            std::process::exit(1);
        }
        return; // Exit early - don't load index
    }

    // Initialize project resolution providers BEFORE any command
    // This ensures caches are built before indexing starts
    let provider_registry = create_provider_registry();
    if let Err(e) = initialize_providers(&provider_registry, &config) {
        // Only fatal for commands that need providers (like index)
        if matches!(cli.command, Commands::Index { .. }) {
            eprintln!("\n{e}");
            let suggestions = e.recovery_suggestions();
            if !suggestions.is_empty() {
                eprintln!("\nSuggestions:");
                for suggestion in suggestions {
                    eprintln!("  • {suggestion}");
                }
            }
            std::process::exit(1);
        } else {
            // For other commands, just warn
            eprintln!("Warning: Provider initialization failed: {e}");
        }
    }

    match &cli.command {
        Commands::Init { force } => {
            let config_path = PathBuf::from(".codanna/settings.toml");

            if config_path.exists() && !force {
                eprintln!(
                    "Configuration file already exists at: {}",
                    config_path.display()
                );
                eprintln!("Use --force to overwrite");
                std::process::exit(1);
            }

            match Settings::init_config_file(*force) {
                Ok(path) => {
                    println!("Created configuration file at: {}", path.display());
                    println!("Edit this file to customize your settings.");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }

        Commands::Config => {
            println!("Current Configuration:");
            println!("{}", "=".repeat(50));
            match toml::to_string_pretty(&config) {
                Ok(toml_str) => println!("{toml_str}"),
                Err(e) => eprintln!("Error displaying config: {e}"),
            }
            return;
        }

        Commands::Index {
            threads: Some(t),
            force: _,
            ..
        } => {
            // Override config with CLI args
            config.indexing.parallel_threads = *t;
        }
        Commands::Index {
            threads: None,
            force: _,
            ..
        } => {
            // Use default from config
        }

        Commands::Serve { .. } => {
            // No configuration overrides for serve
        }

        _ => {}
    }

    // Early return for parse command - it needs no indexing infrastructure
    if let Commands::Parse {
        ref file,
        ref output,
        max_depth,
        all_nodes,
    } = cli.command
    {
        run_parse_command(file, output.clone(), max_depth, all_nodes);
        // run_parse_command already calls std::process::exit
    }

    // Set up persistence based on config
    // Use global path resolution that handles --config properly
    let index_path = codanna::init::resolve_index_path(&config, cli.config.as_deref());

    // Update the config with the resolved index_path so SimpleIndexer uses the correct path
    config.index_path = index_path.clone();

    let persistence = IndexPersistence::new(index_path.clone());

    // Skip loading index for commands that don't need it
    let skip_index_load = matches!(
        cli.command,
        Commands::McpTest { .. }
            | Commands::Parse { .. }
            | Commands::Init { .. }
            | Commands::Config
            | Commands::Benchmark { .. }
            | Commands::Plugin { .. }
            | Commands::Documents { .. }
            | Commands::AddDir { .. }
            | Commands::RemoveDir { .. }
            | Commands::ListDirs
    );

    // Determine if we need full trait resolver initialization
    // Only needed for trait-related commands: implementations, trait analysis, etc.
    let needs_trait_resolver = matches!(
        cli.command,
        Commands::Retrieve {
            query: RetrieveQuery::Implementations { .. },
            ..
        } | Commands::Index { .. }
            | Commands::Serve { .. }
    );

    // Load existing index or create new one (unless we're in thin client mode)
    let settings = Arc::new(config.clone());
    let mut indexer = if skip_index_load {
        SimpleIndexer::with_settings(settings.clone()) // Empty indexer, won't be used
    } else {
        // Force flag always means fresh index, regardless of path source (CLI or settings.toml)
        let force_recreate_index = matches!(cli.command, Commands::Index { force: true, .. });
        if persistence.exists() && !force_recreate_index {
            tracing::debug!(target: "cli", "found existing index at {}", config.index_path.display());
            // Use lazy loading for simple commands to improve startup time
            let skip_trait_resolver = !needs_trait_resolver;
            if skip_trait_resolver {
                tracing::debug!(target: "cli", "using lazy initialization (skipping trait resolver)");
            }

            match persistence.load_with_settings_lazy(settings.clone(), skip_trait_resolver) {
                Ok(loaded) => {
                    tracing::debug!(target: "cli", "successfully loaded index from disk");
                    if cli.info {
                        eprintln!(
                            "Loaded existing index (total: {} symbols)",
                            loaded.symbol_count()
                        );
                    }
                    loaded
                }
                Err(e) => {
                    eprintln!("Warning: Could not load index: {e}. Creating new index.");
                    SimpleIndexer::with_settings(settings.clone())
                }
            }
        } else {
            if force_recreate_index && persistence.exists() {
                eprintln!("Force re-indexing requested, creating new index");
            } else if !persistence.exists() {
                tracing::debug!(
                    target: "cli",
                    "no existing index found at {}",
                    config.index_path.display()
                );
            }
            tracing::debug!(target: "cli", "creating new index");
            // Clear Tantivy index if force re-indexing directory
            if force_recreate_index {
                // Clear the persisted Tantivy files on disk BEFORE creating indexer
                if let Err(e) = persistence.clear() {
                    eprintln!("Warning: Failed to clear persisted Tantivy index: {e}");
                }
            }

            // Create a new indexer with the given settings (after clearing)
            let mut new_indexer = SimpleIndexer::with_settings_lazy(settings.clone());

            if force_recreate_index {
                // Clear symbol cache to fix Windows file locking issues
                if let Err(e) = new_indexer.clear_symbol_cache(true) {
                    eprintln!("Warning: Failed to clear symbol cache: {e}");
                }
            }
            new_indexer
        }
    };

    // Enable semantic search if configured
    let seed_report = if !skip_index_load {
        Some(seed_indexer_with_config_paths(
            &mut indexer,
            &config.indexing.indexed_paths,
        ))
    } else {
        None
    };

    if !skip_index_load && config.semantic_search.enabled && !indexer.has_semantic_search() {
        if let Err(e) = indexer.enable_semantic_search() {
            eprintln!("Warning: Failed to enable semantic search: {e}");
        } else {
            eprintln!(
                "Semantic search enabled (model: {}, threshold: {})",
                config.semantic_search.model, config.semantic_search.threshold
            );
        }
    }

    // Sync indexed paths with config - auto-index new directories
    // This handles changes made while the index was not in use (e.g., add-dir command)
    // Skip sync if force flag is present (force means fresh start, not incremental)
    let is_force_index = matches!(cli.command, Commands::Index { force: true, .. });

    // Extract progress flag from Index command for sync operations
    let show_progress = matches!(cli.command, Commands::Index { progress: true, .. });
    if let Some(report) = &seed_report {
        if is_force_index {
            if !report.newly_seeded.is_empty() {
                let roots: Vec<String> = report
                    .newly_seeded
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                println!(
                    "Rebuilding index for configured roots: {}",
                    roots.join(", ")
                );
            } else if !config.indexing.indexed_paths.is_empty() {
                let roots: Vec<String> = config
                    .indexing
                    .indexed_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                println!(
                    "Rebuilding index for configured roots: {}",
                    roots.join(", ")
                );
            } else {
                println!("Rebuilding index with provided paths only (no configured roots).");
            }
        }

        if !report.missing_paths.is_empty() {
            if report.missing_paths.len() == 1 {
                eprintln!(
                    "Warning: Skipping configured path (not found): {}",
                    report.missing_paths[0].display()
                );
            } else {
                let listed: Vec<String> = report
                    .missing_paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                eprintln!(
                    "Warning: Skipping {} configured paths (not found): {}",
                    report.missing_paths.len(),
                    listed.join(", ")
                );
            }
        }
    }
    // Track whether sync made changes (for later check); None means sync did not run
    let mut sync_made_changes: Option<bool> = None;

    if !skip_index_load && persistence.exists() && !is_force_index {
        // Load stored indexed_paths from metadata
        match IndexMetadata::load(&config.index_path) {
            Ok(metadata) => {
                let stored_paths = metadata.indexed_paths.clone();

                // Sync with current config (settings.toml is source of truth)
                match indexer.sync_with_config(
                    stored_paths,
                    &config.indexing.indexed_paths,
                    show_progress,
                ) {
                    Ok((added, removed, files, symbols)) => {
                        if added > 0 || removed > 0 {
                            sync_made_changes = Some(true);
                            if added > 0 {
                                eprintln!(
                                    "  ✓ Added {added} new directories ({files} files, {symbols} symbols)"
                                );
                            }
                            if removed > 0 {
                                eprintln!("  ✓ Removed {removed} directories from index");
                            }

                            // Save updated index
                            if let Err(e) = persistence.save(&indexer) {
                                eprintln!("Warning: Failed to save updated index: {e}");
                            }
                        } else {
                            sync_made_changes = Some(false);
                        }
                    }
                    Err(e) => {
                        eprintln!("\nFailed to sync indexed paths: {e}");
                        let suggestions = e.recovery_suggestions();
                        if !suggestions.is_empty() {
                            eprintln!("\nRecovery steps:");
                            for suggestion in suggestions {
                                eprintln!("  • {suggestion}");
                            }
                        }
                        use codanna::io::ExitCode;
                        let exit_code = ExitCode::from_error(&e);
                        std::process::exit(exit_code as i32);
                    }
                }
            }
            Err(e) => {
                eprintln!("\nWarning: Could not load index metadata; skipping sync: {e}");
                tracing::debug!(
                    target: "cli",
                    "expected path: {}",
                    config.index_path.join("metadata.json").display()
                );

                eprintln!("\nRecovery steps:");
                let suggestions = e.recovery_suggestions();
                if suggestions.is_empty() {
                    eprintln!("  • Run 'codanna index' to rebuild metadata");
                } else {
                    for suggestion in suggestions {
                        eprintln!("  • {suggestion}");
                    }
                }
                eprintln!("  • Or use 'codanna index --force' for a full rebuild");

                sync_made_changes = None;
            }
        }
    }

    match cli.command {
        Commands::Init { .. } | Commands::Config => {
            // Already handled above
            unreachable!()
        }

        Commands::Serve {
            watch,
            watch_interval,
            http,
            https,
            bind,
        } => {
            // Determine server mode:
            // 1. CLI --https flag takes highest precedence
            // 2. CLI --http flag takes second precedence
            // 3. Otherwise, check config.server.mode
            let server_mode = if https {
                "https"
            } else if http || config.server.mode == "http" {
                "http"
            } else {
                "stdio"
            };

            // Use bind address from CLI if provided, otherwise from config
            // For HTTPS, default to port 8443 if using default bind
            let bind_address = if bind != "127.0.0.1:8080" {
                // CLI flag was explicitly set (not default)
                bind
            } else if https {
                // For HTTPS, use port 8443 by default
                "127.0.0.1:8443".to_string()
            } else {
                // Use config value
                config.server.bind.clone()
            };

            // Use watch interval from CLI if provided, otherwise from config
            let actual_watch_interval = if watch_interval != 5 {
                // CLI flag was explicitly set (not default)
                watch_interval
            } else {
                config.server.watch_interval
            };

            match server_mode {
                "https" => {
                    // HTTPS mode - secure server with TLS
                    tracing::info!(target: "mcp", "starting HTTPS server on {bind_address}");
                    if watch || config.file_watch.enabled {
                        tracing::debug!(
                            target: "mcp",
                            "file watching enabled with {}ms debounce",
                            config.file_watch.debounce_ms
                        );
                    }

                    // Use the HTTPS server implementation
                    #[cfg(feature = "https-server")]
                    {
                        use codanna::mcp::https_server::serve_https;
                        if let Err(e) = serve_https(config, watch, bind_address).await {
                            eprintln!("HTTPS server error: {e}");
                            std::process::exit(1);
                        }
                    }

                    #[cfg(not(feature = "https-server"))]
                    {
                        eprintln!("HTTPS server support is not compiled in.");
                        eprintln!("Please rebuild with: cargo build --features https-server");
                        std::process::exit(1);
                    }
                }
                "http" => {
                    // HTTP mode - persistent server with event-driven file watching
                    eprintln!("Starting MCP server in HTTP mode");
                    eprintln!("Bind address: {bind_address}");
                    if watch || config.file_watch.enabled {
                        eprintln!(
                            "File watching: ENABLED (event-driven with {}ms debounce)",
                            config.file_watch.debounce_ms
                        );
                    }

                    // Use the HTTP server implementation
                    use codanna::mcp::http_server::serve_http;
                    if let Err(e) = serve_http(config, watch, bind_address).await {
                        eprintln!("HTTP server error: {e}");
                        std::process::exit(1);
                    }
                }
                _ => {
                    // stdio mode - current implementation
                    eprintln!("Starting MCP server on stdio transport");
                    if watch {
                        eprintln!("Index watching enabled (interval: {actual_watch_interval}s)");
                    }
                    eprintln!("To test: npx @modelcontextprotocol/inspector cargo run -- serve");

                    // Create MCP server using the already-loaded indexer
                    tracing::debug!(
                        target: "mcp",
                        "creating server with indexer - symbols: {}, semantic: {}",
                        indexer.symbol_count(),
                        indexer.has_semantic_search()
                    );
                    let server = codanna::mcp::CodeIntelligenceServer::new(indexer);

                    // If watch mode is enabled, start the index watcher
                    if watch {
                        use codanna::mcp::watcher::IndexWatcher;
                        use std::time::Duration;

                        let indexer_arc = server.get_indexer_arc();
                        let server_arc = Arc::new(server.clone());
                        let watcher = IndexWatcher::new(
                            indexer_arc,
                            settings.clone(),
                            Duration::from_secs(actual_watch_interval),
                        )
                        .with_mcp_server(server_arc);

                        // Spawn watcher in background
                        tokio::spawn(async move {
                            watcher.watch().await;
                        });

                        eprintln!("Index watcher started with notification support");
                    }

                    // Start unified file watcher if enabled
                    if watch || config.file_watch.enabled {
                        use codanna::documents::DocumentStore;
                        use codanna::mcp::notifications::NotificationBroadcaster;
                        use codanna::vector::{EmbeddingGenerator, FastEmbedGenerator};
                        use codanna::watcher::UnifiedWatcher;
                        use codanna::watcher::handlers::{
                            CodeFileHandler, ConfigFileHandler, DocumentFileHandler,
                        };
                        use std::path::PathBuf;
                        use tokio::sync::RwLock;

                        let broadcaster = Arc::new(NotificationBroadcaster::new(100));

                        let workspace_root = config.workspace_root.clone().unwrap_or_else(|| {
                            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                        });

                        let settings_path = workspace_root.join(".codanna/settings.toml");
                        let debounce_ms = config.file_watch.debounce_ms;
                        let indexer_arc = server.get_indexer_arc();

                        // Build unified watcher with handlers
                        let mut builder = UnifiedWatcher::builder()
                            .broadcaster(broadcaster.clone())
                            .indexer(indexer_arc.clone())
                            .index_path(index_path.clone())
                            .workspace_root(workspace_root.clone())
                            .debounce_ms(debounce_ms);

                        // Add code file handler
                        builder = builder.handler(CodeFileHandler::new(
                            indexer_arc.clone(),
                            workspace_root.clone(),
                        ));

                        // Add config file handler
                        match ConfigFileHandler::new(settings_path.clone()) {
                            Ok(config_handler) => {
                                builder = builder.handler(config_handler);
                            }
                            Err(e) => {
                                eprintln!("Failed to create config handler: {e}");
                            }
                        }

                        // Add document handler if documents are enabled
                        if config.documents.enabled {
                            let doc_path = config.index_path.join("documents");
                            if doc_path.exists() {
                                if let Ok(generator) = FastEmbedGenerator::from_settings(
                                    &config.semantic_search.model,
                                    false,
                                ) {
                                    let dimension = generator.dimension();
                                    if let Ok(store) = DocumentStore::new(&doc_path, dimension) {
                                        if let Ok(store_with_emb) =
                                            store.with_embeddings(Box::new(generator))
                                        {
                                            let store_arc = Arc::new(RwLock::new(store_with_emb));
                                            builder = builder
                                                .document_store(store_arc.clone())
                                                .chunking_config(config.documents.defaults.clone())
                                                .handler(DocumentFileHandler::new(
                                                    store_arc,
                                                    workspace_root.clone(),
                                                ));
                                        }
                                    }
                                }
                            }
                        }

                        // Build and start the unified watcher
                        match builder.build() {
                            Ok(unified_watcher) => {
                                tokio::spawn(async move {
                                    if let Err(e) = unified_watcher.watch().await {
                                        eprintln!("Unified watcher error: {e}");
                                    }
                                });
                                eprintln!(
                                    "Unified watcher started (debounce: {debounce_ms}ms, config: {})",
                                    settings_path.display()
                                );
                            }
                            Err(e) => {
                                eprintln!("Failed to start unified watcher: {e}");
                            }
                        }
                    }

                    // Start server with stdio transport
                    use rmcp::{ServiceExt, transport::stdio};
                    let service = server
                        .serve(stdio())
                        .await
                        .map_err(|e| {
                            eprintln!("Failed to start MCP server: {e}");
                            std::process::exit(1);
                        })
                        .unwrap();

                    // Wait for server to complete
                    service
                        .waiting()
                        .await
                        .map_err(|e| {
                            eprintln!("MCP server error: {e}");
                            std::process::exit(1);
                        })
                        .unwrap();
                } // End of else block for stdio mode
            } // End of match
        }

        Commands::Index {
            paths,
            force,
            progress,
            dry_run,
            max_files,
            ..
        } => {
            // Determine paths to index
            let paths_to_index = if !paths.is_empty() {
                // CLI paths provided - add them to settings.toml first
                let config_path = if let Some(custom_path) = &cli.config {
                    custom_path.clone()
                } else {
                    Settings::find_workspace_config().unwrap_or_else(|| {
                        eprintln!("Error: No configuration file found. Run 'codanna init' first.");
                        std::process::exit(1);
                    })
                };

                match add_paths_to_settings(&paths, &config_path, false) {
                    Ok((updated_settings, added_paths, skipped_paths)) => {
                        if !added_paths.is_empty() {
                            eprintln!("Added {} path(s) to settings.toml", added_paths.len());
                        }
                        for skipped in &skipped_paths {
                            match &skipped.reason {
                                SkipReason::CoveredBy(parent) => println!(
                                    "Skipping {} (already covered by {})",
                                    skipped.path.display(),
                                    parent.display()
                                ),
                                SkipReason::AlreadyPresent => println!(
                                    "Skipping {} (already present in indexed paths)",
                                    skipped.path.display()
                                ),
                                SkipReason::FileNotPersisted => println!(
                                    "Skipping {} (indexed file is tracked ad-hoc and not stored in settings)",
                                    skipped.path.display()
                                ),
                            }
                        }
                        // Reload config to get updated indexed_paths
                        config = updated_settings;
                        paths.clone()
                    }
                    Err(e) => {
                        eprintln!("Error updating settings: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                // No CLI paths - use settings.toml indexed_paths
                let config_paths = config.get_indexed_paths();

                if config_paths.is_empty() {
                    eprintln!("Error: No paths to index");
                    eprintln!();
                    eprintln!("Options:");
                    eprintln!("  1. Provide paths: codanna index <path> [<path>...]");
                    eprintln!("  2. Configure paths: codanna add-dir <path>");
                    std::process::exit(1);
                }

                if !force {
                    match sync_made_changes {
                        Some(false) => {
                            println!("Index already up to date (no changes detected).");
                            if let Err(e) = persistence.save(&indexer) {
                                eprintln!("Error saving index: {e}");
                                std::process::exit(1);
                            }
                        }
                        Some(true) => {
                            // Sync already performed work and saved the index above.
                        }
                        None => {
                            println!(
                                "Skipping incremental update (metadata unavailable); index already up to date."
                            );
                        }
                    }
                    return;
                }

                // Force with config paths - will clear and re-index below
                config_paths
            };

            // Providers already initialized at startup (see line 924)
            // Cache is ready before indexing starts

            // Process each path
            for path in &paths_to_index {
                if path.is_file() {
                    // Single file indexing
                    match indexer.index_file_with_force(path, force) {
                        Ok(result) => {
                            let language_name = path
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .and_then(|ext| {
                                    let registry = codanna::parsing::get_registry();
                                    registry.lock().ok().and_then(|r| {
                                        r.get_by_extension(ext).map(|def| def.name().to_string())
                                    })
                                })
                                .unwrap_or_else(|| "unknown".to_string());

                            if result.is_cached() {
                                println!(
                                    "Successfully loaded from cache: {} [{}]",
                                    path.display(),
                                    language_name
                                );
                            } else {
                                println!(
                                    "Successfully indexed: {} [{}]",
                                    path.display(),
                                    language_name
                                );
                            }
                            println!("File ID: {}", result.file_id().value());

                            // Get symbols for just this file
                            let file_symbols = indexer.get_symbols_by_file(result.file_id());
                            println!("Found {} symbols in this file", file_symbols.len());
                            println!("Total symbols in index: {}", indexer.symbol_count());

                            // Show summary of what was found in this file
                            let functions = file_symbols
                                .iter()
                                .filter(|s| s.kind == SymbolKind::Function)
                                .count();
                            let methods = file_symbols
                                .iter()
                                .filter(|s| s.kind == SymbolKind::Method)
                                .count();
                            let structs = file_symbols
                                .iter()
                                .filter(|s| s.kind == SymbolKind::Struct)
                                .count();
                            let traits = file_symbols
                                .iter()
                                .filter(|s| s.kind == SymbolKind::Trait)
                                .count();

                            println!("  Functions: {functions}");
                            println!("  Methods: {methods}");
                            println!("  Structs: {structs}");
                            println!("  Traits: {traits}");
                        }
                        Err(e) => {
                            eprintln!("Error indexing file {}: {e}", path.display());

                            // Display recovery suggestions
                            let suggestions = e.recovery_suggestions();
                            if !suggestions.is_empty() {
                                eprintln!("\nSuggestions:");
                                for suggestion in suggestions {
                                    eprintln!("  • {suggestion}");
                                }
                            }

                            std::process::exit(1);
                        }
                    }
                } else if path.is_dir() {
                    // Directory indexing
                    if let Some(max) = max_files {
                        println!(
                            "Indexing directory: {} (limited to {} files)",
                            path.display(),
                            max
                        );
                    } else {
                        println!("Indexing directory: {}", path.display());
                    }

                    // Track this directory as indexed
                    if let Err(e) = indexer.add_indexed_path(path) {
                        eprintln!("Warning: Failed to track indexed directory: {e}");
                    }

                    match indexer
                        .index_directory_with_options(path, progress, dry_run, force, max_files)
                    {
                        Ok(stats) => {
                            stats.display();
                        }
                        Err(e) => {
                            eprintln!("Error indexing directory {}: {e}", path.display());

                            // Display recovery suggestions
                            let suggestions = e.recovery_suggestions();
                            if !suggestions.is_empty() {
                                eprintln!("\nSuggestions:");
                                for suggestion in suggestions {
                                    eprintln!("  • {suggestion}");
                                }
                            }

                            std::process::exit(1);
                        }
                    }
                } else {
                    eprintln!("Error: Path does not exist: {}", path.display());
                    std::process::exit(1);
                }
            }

            // After processing all paths, save the index if not in dry-run mode
            if !dry_run {
                // Build symbol cache before saving
                if let Err(e) = indexer.build_symbol_cache() {
                    eprintln!("Warning: Failed to build symbol cache: {e}");
                }

                // Save the index
                eprintln!(
                    "\nSaving index with {} total symbols, {} total relationships...",
                    indexer.symbol_count(),
                    indexer.relationship_count()
                );
                match persistence.save(&indexer) {
                    Ok(_) => {
                        println!("Index saved to: {}", config.index_path.display());
                    }
                    Err(e) => {
                        eprintln!("Error: Could not save index: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }

        Commands::AddDir { path } => {
            // Find config file
            let config_path = if let Some(custom_path) = &cli.config {
                custom_path.clone()
            } else {
                Settings::find_workspace_config().unwrap_or_else(|| {
                    eprintln!("Error: No configuration file found. Run 'codanna init' first.");
                    std::process::exit(1);
                })
            };

            // Use helper to add path and save (non-strict to handle already-indexed gracefully)
            match add_paths_to_settings(std::slice::from_ref(&path), &config_path, false) {
                Ok((settings, added_paths, skipped_paths)) => {
                    if !added_paths.is_empty() {
                        println!("Added directory to indexed paths: {}", path.display());
                        println!("Configuration saved to: {}", config_path.display());
                    } else if !skipped_paths.is_empty() {
                        // Path was skipped (already in config)
                        println!("Directory already in indexed paths: {}", path.display());
                    }
                    println!("\nCurrent indexed paths:");
                    for indexed_path in &settings.indexing.indexed_paths {
                        println!("  - {}", indexed_path.display());
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::RemoveDir { path } => {
            // Load the config file
            let config_path = if let Some(custom_path) = &cli.config {
                custom_path.clone()
            } else {
                Settings::find_workspace_config().unwrap_or_else(|| {
                    eprintln!("Error: No configuration file found. Run 'codanna init' first.");
                    std::process::exit(1);
                })
            };

            // Load settings from file
            let mut settings = Settings::load_from(&config_path).unwrap_or_else(|e| {
                eprintln!("Error loading configuration: {e}");
                std::process::exit(1);
            });

            // Remove the directory
            match settings.remove_indexed_path(&path) {
                Ok(_) => {
                    println!("Removed directory from indexed paths: {}", path.display());

                    // Save the updated configuration
                    if let Err(e) = settings.save(&config_path) {
                        eprintln!("Error saving configuration: {e}");
                        std::process::exit(1);
                    }

                    println!("Configuration saved to: {}", config_path.display());

                    if settings.indexing.indexed_paths.is_empty() {
                        println!("\nNo indexed paths configured.");
                    } else {
                        println!("\nRemaining indexed paths:");
                        for indexed_path in &settings.indexing.indexed_paths {
                            println!("  - {}", indexed_path.display());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::ListDirs => {
            println!("Indexed directories:");
            if config.indexing.indexed_paths.is_empty() {
                println!("  (none configured)");
                println!("\nTo add directories: codanna add-dir <path>");
            } else {
                for path in &config.indexing.indexed_paths {
                    println!("  - {}", path.display());
                }
            }
        }

        Commands::Retrieve { query } => {
            use codanna::io::OutputFormat;
            use codanna::retrieve;

            let exit_code = match query {
                RetrieveQuery::Symbol { args, json } => {
                    use codanna::io::args::parse_positional_args;

                    // Parse positional arguments for symbol name and key:value pairs
                    let (positional_name, params) = parse_positional_args(&args);

                    // Determine symbol name or symbol_id (priority: positional > key:value)
                    let final_name = positional_name
                        .or_else(|| params.get("name").cloned())
                        .or_else(|| params.get("symbol_id").map(|id| format!("symbol_id:{id}")))
                        .unwrap_or_else(|| {
                            eprintln!("Error: symbol requires a name or symbol_id");
                            eprintln!("Usage: codanna retrieve symbol main");
                            eprintln!("   or: codanna retrieve symbol name:main");
                            eprintln!("   or: codanna retrieve symbol symbol_id:1771");
                            std::process::exit(1);
                        });

                    // Extract language filter
                    let language = params.get("lang").map(|s| s.as_str());

                    let format = OutputFormat::from_json_flag(json);
                    retrieve::retrieve_symbol(&indexer, &final_name, language, format)
                }
                RetrieveQuery::Callers { args, json } => {
                    use codanna::io::args::parse_positional_args;

                    // Parse positional arguments for function name and key:value pairs
                    let (positional_function, params) = parse_positional_args(&args);

                    // Determine function name or symbol_id (priority: positional > key:value)
                    let final_function = positional_function
                        .or_else(|| params.get("function").cloned())
                        .or_else(|| params.get("symbol_id").map(|id| format!("symbol_id:{id}")))
                        .unwrap_or_else(|| {
                            eprintln!("Error: callers requires a function name or symbol_id");
                            eprintln!("Usage: codanna retrieve callers main");
                            eprintln!("   or: codanna retrieve callers function:main");
                            eprintln!("   or: codanna retrieve callers symbol_id:1771");
                            std::process::exit(1);
                        });

                    // Extract language filter
                    let language = params.get("lang").map(|s| s.as_str());

                    let format = OutputFormat::from_json_flag(json);
                    retrieve::retrieve_callers(&indexer, &final_function, language, format)
                }
                RetrieveQuery::Calls { args, json } => {
                    use codanna::io::args::parse_positional_args;

                    // Parse positional arguments for function name and key:value pairs
                    let (positional_function, params) = parse_positional_args(&args);

                    // Determine function name or symbol_id (priority: positional > key:value)
                    let final_function = positional_function
                        .or_else(|| params.get("function").cloned())
                        .or_else(|| params.get("symbol_id").map(|id| format!("symbol_id:{id}")))
                        .unwrap_or_else(|| {
                            eprintln!("Error: calls requires a function name or symbol_id");
                            eprintln!("Usage: codanna retrieve calls process_file");
                            eprintln!("   or: codanna retrieve calls function:process_file");
                            eprintln!("   or: codanna retrieve calls symbol_id:1771");
                            std::process::exit(1);
                        });

                    // Extract language filter
                    let language = params.get("lang").map(|s| s.as_str());

                    let format = OutputFormat::from_json_flag(json);
                    retrieve::retrieve_calls(&indexer, &final_function, language, format)
                }
                RetrieveQuery::Implementations { args, json } => {
                    use codanna::io::args::parse_positional_args;

                    // Parse positional arguments for trait name and key:value pairs
                    let (positional_trait, params) = parse_positional_args(&args);

                    // Determine trait name (priority: positional > key:value)
                    let final_trait = positional_trait
                        .or_else(|| params.get("trait").cloned())
                        .unwrap_or_else(|| {
                            eprintln!("Error: implementations requires a trait name");
                            eprintln!("Usage: codanna retrieve implementations Parser");
                            eprintln!("   or: codanna retrieve implementations trait:Parser");
                            std::process::exit(1);
                        });

                    // Extract language filter
                    let language = params.get("lang").map(|s| s.as_str());

                    let format = OutputFormat::from_json_flag(json);
                    retrieve::retrieve_implementations(&indexer, &final_trait, language, format)
                }
                RetrieveQuery::Search {
                    args,
                    limit,
                    json,
                    kind,
                    module,
                } => {
                    use codanna::io::args::parse_positional_args;

                    // Parse positional arguments for query and key:value pairs
                    let (positional_query, params) = parse_positional_args(&args);

                    // Determine query source (priority: positional > key:value)
                    let final_query = positional_query
                        .or_else(|| params.get("query").cloned())
                        .unwrap_or_else(|| {
                            eprintln!("Error: search requires a query");
                            eprintln!("Usage: codanna retrieve search \"query\" [options]");
                            eprintln!(
                                "   or: codanna retrieve search query:\"search text\" [options]"
                            );
                            std::process::exit(1);
                        });

                    // Merge parameters (flags take precedence over key:value)
                    let final_limit = limit.unwrap_or_else(|| {
                        params
                            .get("limit")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(10)
                    });

                    let final_kind = kind.or_else(|| params.get("kind").cloned());
                    let final_module = module.or_else(|| params.get("module").cloned());

                    // Extract language filter
                    let language = params.get("lang").map(|s| s.as_str());

                    // Call retrieve function with merged parameters
                    let format = OutputFormat::from_json_flag(json);
                    retrieve::retrieve_search(
                        &indexer,
                        &final_query,
                        final_limit,
                        final_kind.as_deref(),
                        final_module.as_deref(),
                        language,
                        format,
                    )
                }
                // DISABLED: Impact command handler commented out
                // See the RetrieveQuery enum for deprecation details
                // RetrieveQuery::Impact { args, depth, json } => {
                //     use codanna::io::args::parse_positional_args;
                //
                //     // Parse positional arguments for symbol name and key:value pairs
                //     let (positional_symbol, params) = parse_positional_args(&args);
                //
                //     // Determine symbol name (priority: positional > key:value)
                //     let final_symbol = positional_symbol
                //         .or_else(|| params.get("symbol").cloned())
                //         .unwrap_or_else(|| {
                //             eprintln!("Error: impact requires a symbol name");
                //             eprintln!("Usage: codanna retrieve impact MyStruct");
                //             eprintln!("   or: codanna retrieve impact symbol:MyStruct depth:3");
                //             std::process::exit(1);
                //         });
                //
                //     // Merge depth parameter (flags take precedence over key:value)
                //     let final_depth = depth.unwrap_or_else(|| {
                //         params
                //             .get("depth")
                //             .and_then(|s| s.parse::<usize>().ok())
                //             .unwrap_or(5)
                //     });
                //
                //     let format = OutputFormat::from_json_flag(json);
                //     retrieve::retrieve_impact(&indexer, &final_symbol, final_depth, format)
                // }
                RetrieveQuery::Describe { args, json } => {
                    use codanna::io::args::parse_positional_args;

                    // Parse positional arguments for symbol name and key:value pairs
                    let (positional_symbol, params) = parse_positional_args(&args);

                    // Determine symbol name or symbol_id (priority: positional > key:value)
                    let final_symbol = positional_symbol
                        .or_else(|| params.get("symbol").cloned())
                        .or_else(|| params.get("symbol_id").map(|id| format!("symbol_id:{id}")))
                        .unwrap_or_else(|| {
                            eprintln!("Error: describe requires a symbol name or symbol_id");
                            eprintln!("Usage: codanna retrieve describe SimpleIndexer");
                            eprintln!("   or: codanna retrieve describe symbol:SimpleIndexer");
                            eprintln!("   or: codanna retrieve describe symbol_id:1771");
                            std::process::exit(1);
                        });

                    // Extract language filter
                    let language = params.get("lang").map(|s| s.as_str());

                    let format = OutputFormat::from_json_flag(json);
                    retrieve::retrieve_describe(&indexer, &final_symbol, language, format)
                }
                RetrieveQuery::Uses { symbol } => {
                    eprintln!("'retrieve uses' command not yet implemented for: {symbol}");
                    codanna::io::ExitCode::GeneralError
                }
                RetrieveQuery::Defines { symbol } => {
                    eprintln!("'retrieve defines' command not yet implemented for: {symbol}");
                    codanna::io::ExitCode::GeneralError
                }
                RetrieveQuery::Dependencies { symbol } => {
                    eprintln!("'retrieve dependencies' command not yet implemented for: {symbol}");
                    codanna::io::ExitCode::GeneralError
                }
            };

            std::process::exit(exit_code as i32);
        }

        Commands::McpTest {
            server_binary,
            tool,
            args,
            delay,
        } => {
            use codanna::mcp::client::CodeIntelligenceClient;

            // Get server binary path (default to current executable)
            let server_path = server_binary.clone().unwrap_or_else(|| {
                std::env::current_exe().expect("Failed to get current executable path")
            });

            // Run the test
            if let Err(e) = CodeIntelligenceClient::test_server(
                server_path,
                cli.config.clone(),
                tool.clone(),
                args.clone(),
                delay,
            )
            .await
            {
                eprintln!("MCP test failed: {e}");
                std::process::exit(1);
            }
        }

        Commands::Mcp {
            tool,
            positional,
            args,
            json,
        } => {
            use codanna::io::args::parse_positional_args;

            // Build arguments from both positional and --args
            let mut arguments = if let Some(args_str) = &args {
                // Parse JSON arguments if provided (backward compatibility)
                match serde_json::from_str::<serde_json::Value>(args_str) {
                    Ok(serde_json::Value::Object(map)) => Some(map),
                    Ok(_) => {
                        eprintln!("Error: Arguments must be a JSON object");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("Error parsing arguments: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                // Start with empty map if no --args
                Some(serde_json::Map::new())
            };

            // Process positional arguments using unified parser
            if !positional.is_empty() {
                if let Some(ref mut args_map) = arguments {
                    // Use the unified parser from args.rs
                    let (first_positional, params) = parse_positional_args(&positional);

                    // Handle the first positional argument based on tool type
                    if let Some(pos_arg) = first_positional {
                        match tool.as_str() {
                            "find_symbol" => {
                                args_map.insert(
                                    "name".to_string(),
                                    serde_json::Value::String(pos_arg.clone()),
                                );
                            }
                            "get_calls" | "find_callers" => {
                                args_map.insert(
                                    "function_name".to_string(),
                                    serde_json::Value::String(pos_arg.clone()),
                                );
                            }
                            "analyze_impact" => {
                                args_map.insert(
                                    "symbol_name".to_string(),
                                    serde_json::Value::String(pos_arg.clone()),
                                );
                            }
                            "semantic_search_docs"
                            | "semantic_search_with_context"
                            | "search_documents" => {
                                args_map.insert(
                                    "query".to_string(),
                                    serde_json::Value::String(pos_arg.clone()),
                                );
                            }
                            "search_symbols" => {
                                args_map.insert(
                                    "query".to_string(),
                                    serde_json::Value::String(pos_arg.clone()),
                                );
                            }
                            _ => {
                                eprintln!(
                                    "Warning: Unknown tool '{tool}', ignoring positional argument"
                                );
                            }
                        }
                    }

                    // Add all key:value pairs from params
                    for (key, value) in params {
                        // Try to parse as number first, then boolean, fallback to string
                        let json_value = if let Ok(n) = value.parse::<i64>() {
                            serde_json::Value::Number(n.into())
                        } else if let Ok(f) = value.parse::<f64>() {
                            serde_json::Value::Number(
                                serde_json::Number::from_f64(f)
                                    .unwrap_or_else(|| serde_json::Number::from(0)),
                            )
                        } else if let Ok(b) = value.parse::<bool>() {
                            serde_json::Value::Bool(b)
                        } else {
                            serde_json::Value::String(value)
                        };
                        args_map.insert(key, json_value);
                    }
                }
            }

            // Convert to Option<Map> only if we have arguments
            let arguments = arguments.filter(|map| !map.is_empty());

            // Collect data for find_symbol if JSON output is requested
            let find_symbol_data = if json && tool == "find_symbol" {
                let name = arguments
                    .as_ref()
                    .and_then(|m| m.get("name"))
                    .and_then(|v| v.as_str());
                let language = arguments
                    .as_ref()
                    .and_then(|m| m.get("lang"))
                    .and_then(|v| v.as_str());

                if let Some(symbol_name) = name {
                    let symbols = indexer.find_symbols_by_name(symbol_name, language);
                    if !symbols.is_empty() {
                        use codanna::symbol::context::ContextIncludes;
                        let mut results = Vec::new();

                        for symbol in symbols {
                            // Get full context with callers using the same approach as MCP
                            let context = indexer.get_symbol_context(
                                symbol.id,
                                ContextIncludes::CALLERS
                                    | ContextIncludes::IMPLEMENTATIONS
                                    | ContextIncludes::DEFINITIONS,
                            );

                            // Build result with context if available
                            if let Some(ctx) = context {
                                results.push(ctx);
                            } else {
                                // Fallback: create minimal context
                                let file_path = indexer
                                    .get_file_path(symbol.file_id)
                                    .unwrap_or_else(|| "unknown".to_string());

                                results.push(codanna::symbol::context::SymbolContext {
                                    symbol,
                                    file_path,
                                    relationships: Default::default(),
                                });
                            }
                        }
                        Some(results)
                    } else {
                        Some(Vec::new())
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Collect data for get_calls if JSON output is requested
            let get_calls_data = if json && tool == "get_calls" {
                let symbol_id = arguments
                    .as_ref()
                    .and_then(|m| m.get("symbol_id"))
                    .and_then(|v| v.as_u64())
                    .map(|id| id as u32);
                let function_name = arguments
                    .as_ref()
                    .and_then(|m| m.get("function_name"))
                    .and_then(|v| v.as_str());
                let language = arguments
                    .as_ref()
                    .and_then(|m| m.get("lang"))
                    .and_then(|v| v.as_str());

                if let Some(id) = symbol_id {
                    use codanna::symbol::context::ContextIncludes;

                    // Direct lookup by symbol ID
                    if let Some(symbol) = indexer.get_symbol(codanna::SymbolId(id)) {
                        let mut all_calls = Vec::new();

                        let context = indexer.get_symbol_context(symbol.id, ContextIncludes::CALLS);
                        if let Some(ctx) = context {
                            if let Some(calls) = ctx.relationships.calls {
                                for (called, metadata) in calls {
                                    all_calls.push((called, metadata));
                                }
                            }
                        }

                        Some(all_calls)
                    } else {
                        None // Symbol not found
                    }
                } else if let Some(func_name) = function_name {
                    use codanna::symbol::context::ContextIncludes;
                    use std::collections::HashSet;

                    // Find ALL symbols with this name
                    let symbols = indexer.find_symbols_by_name(func_name, language);
                    let function_symbols: Vec<_> = symbols
                        .into_iter()
                        .filter(|s| {
                            matches!(
                                s.kind,
                                crate::SymbolKind::Function | crate::SymbolKind::Method
                            )
                        })
                        .collect();

                    if function_symbols.is_empty() {
                        None // Function not found
                    } else {
                        // Aggregate calls from ALL symbols with this name (same as MCP handler)
                        let mut all_calls = Vec::new();
                        let mut seen_ids = HashSet::new();

                        for symbol in function_symbols {
                            let context =
                                indexer.get_symbol_context(symbol.id, ContextIncludes::CALLS);
                            if let Some(ctx) = context {
                                if let Some(calls) = ctx.relationships.calls {
                                    for (called, metadata) in calls {
                                        // Deduplicate by symbol ID
                                        if seen_ids.insert(called.id) {
                                            all_calls.push((called, metadata));
                                        }
                                    }
                                }
                            }
                        }

                        Some(all_calls)
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Collect data for find_callers if JSON output is requested
            let find_callers_data = if json && tool == "find_callers" {
                let symbol_id = arguments
                    .as_ref()
                    .and_then(|m| m.get("symbol_id"))
                    .and_then(|v| v.as_u64())
                    .map(|id| id as u32);
                let function_name = arguments
                    .as_ref()
                    .and_then(|m| m.get("function_name"))
                    .and_then(|v| v.as_str());
                let language = arguments
                    .as_ref()
                    .and_then(|m| m.get("lang"))
                    .and_then(|v| v.as_str());

                if let Some(id) = symbol_id {
                    // Direct lookup by symbol ID
                    if let Some(symbol) = indexer.get_symbol(codanna::SymbolId(id)) {
                        let callers = indexer.get_calling_functions_with_metadata(symbol.id);
                        let all_callers: Vec<_> = callers.into_iter().collect();
                        Some(all_callers)
                    } else {
                        None // Symbol not found
                    }
                } else if let Some(func_name) = function_name {
                    use std::collections::HashSet;

                    // Find all functions with this name
                    let symbols = indexer.find_symbols_by_name(func_name, language);
                    if !symbols.is_empty() {
                        let mut all_callers = Vec::new();
                        let mut seen_ids = HashSet::new();

                        // Check all symbols with this name and deduplicate (same as MCP handler)
                        for symbol in &symbols {
                            let callers = indexer.get_calling_functions_with_metadata(symbol.id);
                            for (caller, metadata) in callers {
                                // Deduplicate by symbol ID
                                if seen_ids.insert(caller.id) {
                                    all_callers.push((caller, metadata));
                                }
                            }
                        }

                        Some(all_callers)
                    } else {
                        None // Function not found
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Collect data for analyze_impact if JSON output is requested
            let analyze_impact_data = if json && tool == "analyze_impact" {
                let symbol_id = arguments
                    .as_ref()
                    .and_then(|m| m.get("symbol_id"))
                    .and_then(|v| v.as_u64())
                    .map(|id| id as u32);
                let symbol_name = arguments
                    .as_ref()
                    .and_then(|m| m.get("symbol_name"))
                    .and_then(|v| v.as_str());
                let language = arguments
                    .as_ref()
                    .and_then(|m| m.get("lang"))
                    .and_then(|v| v.as_str());

                if let Some(id) = symbol_id {
                    // Direct lookup by symbol ID
                    if let Some(symbol) = indexer.get_symbol(codanna::SymbolId(id)) {
                        let max_depth = arguments
                            .as_ref()
                            .and_then(|m| m.get("max_depth"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(3) as usize;

                        let impacted_ids = indexer.get_impact_radius(symbol.id, Some(max_depth));

                        // Convert SymbolIds to full Symbols
                        let mut impacted_symbols = Vec::new();
                        for impact_id in impacted_ids {
                            if let Some(sym) = indexer.get_symbol(impact_id) {
                                impacted_symbols.push(sym);
                            }
                        }

                        Some(impacted_symbols)
                    } else {
                        None // Symbol not found
                    }
                } else if let Some(sym_name) = symbol_name {
                    use std::collections::HashSet;

                    // Find ALL symbols with this name (same as MCP handler)
                    let symbols = indexer.find_symbols_by_name(sym_name, language);

                    if symbols.is_empty() {
                        None // Symbol not found
                    } else {
                        let max_depth = arguments
                            .as_ref()
                            .and_then(|m| m.get("max_depth"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(3) as usize;

                        // Aggregate impact from ALL symbols with this name (same as MCP handler)
                        let mut all_impacted_ids = HashSet::new();
                        for symbol in &symbols {
                            let impacted_ids =
                                indexer.get_impact_radius(symbol.id, Some(max_depth));
                            all_impacted_ids.extend(impacted_ids);
                        }

                        // Convert SymbolIds to full Symbols
                        let mut impacted_symbols = Vec::new();
                        for id in all_impacted_ids {
                            if let Some(sym) = indexer.get_symbol(id) {
                                impacted_symbols.push(sym);
                            }
                        }

                        Some(impacted_symbols)
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Collect data for search_symbols if JSON output is requested
            let search_symbols_data = if json && tool == "search_symbols" {
                let query = arguments
                    .as_ref()
                    .and_then(|m| m.get("query"))
                    .and_then(|v| v.as_str());

                if let Some(q) = query {
                    let limit = arguments
                        .as_ref()
                        .and_then(|m| m.get("limit"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10) as u32;
                    let kind = arguments
                        .as_ref()
                        .and_then(|m| m.get("kind"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let module = arguments
                        .as_ref()
                        .and_then(|m| m.get("module"))
                        .and_then(|v| v.as_str());
                    let language = arguments
                        .as_ref()
                        .and_then(|m| m.get("lang"))
                        .and_then(|v| v.as_str());

                    // Parse the kind filter if provided
                    let kind_filter = kind.as_ref().and_then(|k| match k.to_lowercase().as_str() {
                        "function" => Some(crate::SymbolKind::Function),
                        "struct" => Some(crate::SymbolKind::Struct),
                        "trait" => Some(crate::SymbolKind::Trait),
                        "method" => Some(crate::SymbolKind::Method),
                        "field" => Some(crate::SymbolKind::Field),
                        "module" => Some(crate::SymbolKind::Module),
                        "constant" => Some(crate::SymbolKind::Constant),
                        _ => None,
                    });

                    match indexer.search(q, limit as usize, kind_filter, module, language) {
                        Ok(results) => Some(results),
                        Err(_) => Some(Vec::new()),
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Collect data for semantic_search_docs if JSON output is requested
            #[derive(serde::Serialize)]
            struct SemanticSearchResult {
                symbol: Symbol,
                score: f32,
            }

            #[derive(serde::Serialize)]
            struct SemanticSearchWithContextResult {
                symbol: Symbol,
                score: f32,
                context: codanna::symbol::context::SymbolContext,
            }

            // Get guidance config before moving indexer
            let guidance_config = indexer.settings().guidance.clone();

            let semantic_search_docs_data = if json && tool == "semantic_search_docs" {
                if !indexer.has_semantic_search() {
                    None // Semantic search not enabled
                } else {
                    let query = arguments
                        .as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str());

                    if let Some(q) = query {
                        let limit = arguments
                            .as_ref()
                            .and_then(|m| m.get("limit"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(10) as usize;
                        let threshold = arguments
                            .as_ref()
                            .and_then(|m| m.get("threshold"))
                            .and_then(|v| v.as_f64())
                            .map(|t| t as f32);
                        let language = arguments
                            .as_ref()
                            .and_then(|m| m.get("lang"))
                            .and_then(|v| v.as_str());

                        let results = match threshold {
                            Some(t) => indexer.semantic_search_docs_with_threshold_and_language(
                                q, limit, t, language,
                            ),
                            None => indexer.semantic_search_docs_with_language(q, limit, language),
                        };

                        match results {
                            Ok(results) => {
                                let semantic_results: Vec<SemanticSearchResult> = results
                                    .into_iter()
                                    .map(|(symbol, score)| SemanticSearchResult { symbol, score })
                                    .collect();
                                Some(semantic_results)
                            }
                            Err(_) => Some(Vec::new()),
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            // Collect data for semantic_search_with_context if JSON output is requested
            let semantic_search_with_context_data = if json
                && tool == "semantic_search_with_context"
            {
                if !indexer.has_semantic_search() {
                    None // Semantic search not enabled
                } else {
                    let query = arguments
                        .as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str());

                    if let Some(q) = query {
                        let limit = arguments
                            .as_ref()
                            .and_then(|m| m.get("limit"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(5) as u32; // Default 5 for context version
                        let threshold = arguments
                            .as_ref()
                            .and_then(|m| m.get("threshold"))
                            .and_then(|v| v.as_f64())
                            .map(|t| t as f32);
                        let language = arguments
                            .as_ref()
                            .and_then(|m| m.get("lang"))
                            .and_then(|v| v.as_str());

                        let search_results = match threshold {
                            Some(t) => indexer.semantic_search_docs_with_threshold_and_language(
                                q,
                                limit as usize,
                                t,
                                language,
                            ),
                            None => indexer.semantic_search_docs_with_language(
                                q,
                                limit as usize,
                                language,
                            ),
                        };

                        match search_results {
                            Ok(results) => {
                                use codanna::symbol::context::ContextIncludes;
                                let context_results: Vec<SemanticSearchWithContextResult> = results
                                    .into_iter()
                                    .filter_map(|(symbol, score)| {
                                        // Get full context for each symbol
                                        let context = indexer.get_symbol_context(
                                            symbol.id,
                                            ContextIncludes::CALLERS
                                                | ContextIncludes::CALLS
                                                | ContextIncludes::IMPLEMENTATIONS
                                                | ContextIncludes::DEFINITIONS,
                                        );

                                        context.map(|ctx| SemanticSearchWithContextResult {
                                            symbol,
                                            score,
                                            context: ctx,
                                        })
                                    })
                                    .collect();
                                Some(context_results)
                            }
                            Err(_) => Some(Vec::new()),
                        }
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            // Check semantic search status before moving indexer
            let has_semantic_search = indexer.has_semantic_search();

            // If we need JSON output for get_index_info, collect data before moving indexer
            let index_info_data = if json && tool == "get_index_info" {
                let symbol_count = indexer.symbol_count();
                let file_count = indexer.file_count();
                let relationship_count = indexer.relationship_count();

                // Count symbols by kind
                let mut kind_counts = std::collections::HashMap::new();
                for symbol in indexer.get_all_symbols() {
                    *kind_counts.entry(symbol.kind).or_insert(0) += 1;
                }

                let functions = *kind_counts.get(&crate::SymbolKind::Function).unwrap_or(&0);
                let methods = *kind_counts.get(&crate::SymbolKind::Method).unwrap_or(&0);
                let structs = *kind_counts.get(&crate::SymbolKind::Struct).unwrap_or(&0);
                let traits = *kind_counts.get(&crate::SymbolKind::Trait).unwrap_or(&0);

                // Get semantic search info
                let semantic_search = if let Some(metadata) = indexer.get_semantic_metadata() {
                    SemanticSearchInfo {
                        enabled: true,
                        model_name: Some(metadata.model_name),
                        embeddings: Some(metadata.embedding_count),
                        dimensions: Some(metadata.dimension),
                        created: Some(codanna::mcp::format_relative_time(metadata.created_at)),
                        updated: Some(codanna::mcp::format_relative_time(metadata.updated_at)),
                    }
                } else {
                    SemanticSearchInfo {
                        enabled: false,
                        model_name: None,
                        embeddings: None,
                        dimensions: None,
                        created: None,
                        updated: None,
                    }
                };

                Some(IndexInfo {
                    symbol_count,
                    file_count: file_count as usize,
                    relationship_count,
                    symbol_kinds: SymbolKindBreakdown {
                        functions,
                        methods,
                        structs,
                        traits,
                    },
                    semantic_search,
                })
            } else {
                None
            };

            // Embedded mode - use already loaded indexer directly
            // Try to load DocumentStore for search_documents tool
            let server = {
                let mut server = codanna::mcp::CodeIntelligenceServer::new(indexer);

                // Add DocumentStore if documents are enabled and indexed
                if config.documents.enabled && config.semantic_search.enabled {
                    let doc_path = config.index_path.join("documents");
                    if doc_path.exists() {
                        use codanna::documents::DocumentStore;
                        use codanna::vector::{EmbeddingGenerator, FastEmbedGenerator};

                        // Create generator first to get dimension from model
                        if let Ok(generator) =
                            FastEmbedGenerator::from_settings(&config.semantic_search.model, false)
                        {
                            let dimension = generator.dimension();
                            if let Ok(store) = DocumentStore::new(&doc_path, dimension) {
                                if let Ok(store_with_emb) =
                                    store.with_embeddings(Box::new(generator))
                                {
                                    server = server.with_document_store(store_with_emb);
                                }
                            }
                        }
                    }
                }
                server
            };

            // Call the tool directly
            use codanna::mcp::*;
            use rmcp::handler::server::wrapper::Parameters;

            let result = match tool.as_str() {
                "find_symbol" => {
                    let name = arguments
                        .as_ref()
                        .and_then(|m| m.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: find_symbol requires 'name' parameter");
                            std::process::exit(1);
                        });
                    let lang = arguments
                        .as_ref()
                        .and_then(|m| m.get("lang"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    server
                        .find_symbol(Parameters(FindSymbolRequest {
                            name: name.to_string(),
                            lang,
                        }))
                        .await
                }
                "get_calls" => {
                    let function_name = arguments
                        .as_ref()
                        .and_then(|m| m.get("function_name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let symbol_id = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_id"))
                        .and_then(|v| v.as_u64())
                        .map(|id| id as u32);

                    // Require either function_name or symbol_id
                    if function_name.is_none() && symbol_id.is_none() {
                        eprintln!(
                            "Error: get_calls requires either 'function_name' or 'symbol_id' parameter"
                        );
                        std::process::exit(1);
                    }

                    server
                        .get_calls(Parameters(GetCallsRequest {
                            function_name,
                            symbol_id,
                        }))
                        .await
                }
                "find_callers" => {
                    let function_name = arguments
                        .as_ref()
                        .and_then(|m| m.get("function_name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let symbol_id = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_id"))
                        .and_then(|v| v.as_u64())
                        .map(|id| id as u32);

                    // Require either function_name or symbol_id
                    if function_name.is_none() && symbol_id.is_none() {
                        eprintln!(
                            "Error: find_callers requires either 'function_name' or 'symbol_id' parameter"
                        );
                        std::process::exit(1);
                    }

                    server
                        .find_callers(Parameters(FindCallersRequest {
                            function_name,
                            symbol_id,
                        }))
                        .await
                }
                "analyze_impact" => {
                    let symbol_name = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let symbol_id = arguments
                        .as_ref()
                        .and_then(|m| m.get("symbol_id"))
                        .and_then(|v| v.as_u64())
                        .map(|id| id as u32);

                    // Require either symbol_name or symbol_id
                    if symbol_name.is_none() && symbol_id.is_none() {
                        eprintln!(
                            "Error: analyze_impact requires either 'symbol_name' or 'symbol_id' parameter"
                        );
                        std::process::exit(1);
                    }

                    let max_depth = arguments
                        .as_ref()
                        .and_then(|m| m.get("max_depth"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(3) as u32;
                    server
                        .analyze_impact(Parameters(AnalyzeImpactRequest {
                            symbol_name,
                            symbol_id,
                            max_depth,
                        }))
                        .await
                }
                "get_index_info" => {
                    use codanna::mcp::GetIndexInfoRequest;
                    use rmcp::handler::server::wrapper::Parameters;
                    server
                        .get_index_info(Parameters(GetIndexInfoRequest {}))
                        .await
                }
                "search_symbols" => {
                    let query = arguments
                        .as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: search_symbols requires 'query' parameter");
                            std::process::exit(1);
                        });
                    let limit = arguments
                        .as_ref()
                        .and_then(|m| m.get("limit"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10) as u32;
                    let kind = arguments
                        .as_ref()
                        .and_then(|m| m.get("kind"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let module = arguments
                        .as_ref()
                        .and_then(|m| m.get("module"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let lang = arguments
                        .as_ref()
                        .and_then(|m| m.get("lang"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    server
                        .search_symbols(Parameters(SearchSymbolsRequest {
                            query: query.to_string(),
                            limit,
                            kind,
                            module,
                            lang,
                        }))
                        .await
                }
                "semantic_search_docs" => {
                    let query = arguments
                        .as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: semantic_search_docs requires 'query' parameter");
                            std::process::exit(1);
                        });
                    let limit = arguments
                        .as_ref()
                        .and_then(|m| m.get("limit"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10) as u32;
                    let threshold = arguments
                        .as_ref()
                        .and_then(|m| m.get("threshold"))
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);
                    let lang = arguments
                        .as_ref()
                        .and_then(|m| m.get("lang"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    server
                        .semantic_search_docs(Parameters(SemanticSearchRequest {
                            query: query.to_string(),
                            limit,
                            threshold,
                            lang,
                        }))
                        .await
                }
                "semantic_search_with_context" => {
                    let query = arguments
                        .as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!(
                                "Error: semantic_search_with_context requires 'query' parameter"
                            );
                            std::process::exit(1);
                        });
                    let limit = arguments
                        .as_ref()
                        .and_then(|m| m.get("limit"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(5) as u32;
                    let threshold = arguments
                        .as_ref()
                        .and_then(|m| m.get("threshold"))
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32);
                    let lang = arguments
                        .as_ref()
                        .and_then(|m| m.get("lang"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    server
                        .semantic_search_with_context(Parameters(
                            SemanticSearchWithContextRequest {
                                query: query.to_string(),
                                limit,
                                threshold,
                                lang,
                            },
                        ))
                        .await
                }
                "search_documents" => {
                    use codanna::mcp::SearchDocumentsRequest;
                    let query = arguments
                        .as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: search_documents requires 'query' parameter");
                            std::process::exit(1);
                        })
                        .to_string();
                    let collection = arguments
                        .as_ref()
                        .and_then(|m| m.get("collection"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let limit = arguments
                        .as_ref()
                        .and_then(|m| m.get("limit"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(5) as u32;
                    server
                        .search_documents(Parameters(SearchDocumentsRequest {
                            query,
                            collection,
                            limit,
                        }))
                        .await
                }
                _ => {
                    if json {
                        use codanna::io::exit_code::ExitCode;
                        use codanna::io::format::JsonResponse;
                        let response = JsonResponse::error(
                            ExitCode::GeneralError,
                            &format!("Unknown tool: {tool}"),
                            vec![
                                "Available tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info, search_symbols, semantic_search_docs, semantic_search_with_context, search_documents",
                            ],
                        );
                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                    } else {
                        eprintln!("Unknown tool: {tool}");
                        eprintln!(
                            "Available tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info, search_symbols, semantic_search_docs, semantic_search_with_context, search_documents"
                        );
                    }
                    std::process::exit(1);
                }
            };

            // Print result
            match result {
                Ok(call_result) => {
                    if json && tool == "get_index_info" {
                        // Use pre-collected data for JSON output
                        if let Some(index_info) = index_info_data {
                            use codanna::io::format::JsonResponse;
                            use codanna::io::guidance_engine::generate_guidance_from_config;
                            let mut response = JsonResponse::success(index_info);

                            // Add system guidance (using single_result template since this returns stats)
                            if let Some(guidance) = generate_guidance_from_config(
                                &guidance_config,
                                "get_index_info",
                                None,
                                1,
                            ) {
                                // Use 1 to trigger single_result template
                                response = response.with_system_message(&guidance);
                            }

                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                        }
                    } else if json && tool == "find_symbol" {
                        // Use pre-collected data for JSON output
                        if let Some(symbol_contexts) = find_symbol_data {
                            use codanna::io::format::JsonResponse;
                            if symbol_contexts.is_empty() {
                                let name = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let response = JsonResponse::not_found("Symbol", name);
                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                                std::process::exit(3);
                            } else {
                                use codanna::io::guidance_engine::generate_guidance_from_config;
                                let mut response = JsonResponse::success(symbol_contexts);

                                // Add system guidance
                                let result_count =
                                    response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "find_symbol",
                                    arguments
                                        .as_ref()
                                        .and_then(|m| m.get("name"))
                                        .and_then(|v| v.as_str()),
                                    result_count,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            }
                        }
                    } else if json && tool == "get_calls" {
                        // Use pre-collected data for JSON output
                        if let Some(calls) = get_calls_data {
                            use codanna::io::format::JsonResponse;
                            use codanna::io::guidance_engine::generate_guidance_from_config;
                            let mut response = JsonResponse::success(calls);

                            // Add system guidance
                            let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                            if let Some(guidance) = generate_guidance_from_config(
                                &guidance_config,
                                "get_calls",
                                arguments
                                    .as_ref()
                                    .and_then(|m| m.get("function_name"))
                                    .and_then(|v| v.as_str()),
                                result_count,
                            ) {
                                response = response.with_system_message(&guidance);
                            }

                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                        } else {
                            // Function not found
                            use codanna::io::format::JsonResponse;
                            let response = if let Some(id) = arguments
                                .as_ref()
                                .and_then(|m| m.get("symbol_id"))
                                .and_then(|v| v.as_u64())
                            {
                                JsonResponse::not_found("Symbol", &format!("symbol_id:{id}"))
                            } else {
                                let name = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("function_name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                JsonResponse::not_found("Function", name)
                            };
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(3);
                        }
                    } else if json && tool == "find_callers" {
                        // Use pre-collected data for JSON output
                        if let Some(callers) = find_callers_data {
                            use codanna::io::format::JsonResponse;
                            use codanna::io::guidance_engine::generate_guidance_from_config;
                            let mut response = JsonResponse::success(callers);

                            // Add system guidance
                            let result_count = response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                            if let Some(guidance) = generate_guidance_from_config(
                                &guidance_config,
                                "find_callers",
                                arguments
                                    .as_ref()
                                    .and_then(|m| m.get("function_name"))
                                    .and_then(|v| v.as_str()),
                                result_count,
                            ) {
                                response = response.with_system_message(&guidance);
                            }

                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                        } else {
                            // Function not found
                            use codanna::io::format::JsonResponse;
                            let response = if let Some(id) = arguments
                                .as_ref()
                                .and_then(|m| m.get("symbol_id"))
                                .and_then(|v| v.as_u64())
                            {
                                JsonResponse::not_found("Symbol", &format!("symbol_id:{id}"))
                            } else {
                                let name = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("function_name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                JsonResponse::not_found("Function", name)
                            };
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(3);
                        }
                    } else if json && tool == "analyze_impact" {
                        // Use pre-collected data for JSON output
                        if let Some(impacted) = analyze_impact_data {
                            use codanna::io::format::JsonResponse;
                            if impacted.is_empty() {
                                // No symbols would be impacted
                                let identifier = if let Some(id) = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("symbol_id"))
                                    .and_then(|v| v.as_u64())
                                {
                                    format!("symbol_id:{id}")
                                } else {
                                    arguments
                                        .as_ref()
                                        .and_then(|m| m.get("symbol_name"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string()
                                };
                                use codanna::io::guidance_engine::generate_guidance_from_config;

                                // Create a proper struct for the empty case
                                #[derive(serde::Serialize)]
                                struct EmptyImpactResult {
                                    symbol: String,
                                    impacted_count: usize,
                                    impacted_symbols: Vec<String>,
                                    message: String,
                                }

                                let impact_result = EmptyImpactResult {
                                    symbol: identifier.clone(),
                                    impacted_count: 0,
                                    impacted_symbols: vec![],
                                    message:
                                        "No symbols would be impacted by changes to this symbol"
                                            .to_string(),
                                };

                                let mut response = JsonResponse::success(impact_result);

                                // Add guidance for no results case
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "analyze_impact",
                                    Some(&identifier),
                                    0,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            } else {
                                use codanna::io::guidance_engine::generate_guidance_from_config;
                                let mut response = JsonResponse::success(impacted);

                                // Add system guidance
                                let result_count =
                                    response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "analyze_impact",
                                    arguments
                                        .as_ref()
                                        .and_then(|m| m.get("symbol_name"))
                                        .and_then(|v| v.as_str()),
                                    result_count,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            }
                        } else {
                            // Symbol not found
                            use codanna::io::format::JsonResponse;
                            let response = if let Some(id) = arguments
                                .as_ref()
                                .and_then(|m| m.get("symbol_id"))
                                .and_then(|v| v.as_u64())
                            {
                                JsonResponse::not_found("Symbol", &format!("symbol_id:{id}"))
                            } else {
                                let name = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("symbol_name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                JsonResponse::not_found("Symbol", name)
                            };
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(3);
                        }
                    } else if json && tool == "search_symbols" {
                        // Use pre-collected data for JSON output
                        if let Some(results) = search_symbols_data {
                            use codanna::io::format::JsonResponse;
                            use codanna::io::guidance_engine::generate_guidance_from_config;
                            if results.is_empty() {
                                // Create proper struct for empty search results
                                #[derive(serde::Serialize)]
                                struct EmptySearchResult {
                                    query: String,
                                    result_count: usize,
                                    results: Vec<String>,
                                    message: String,
                                }

                                let query = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("query"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");

                                let search_result = EmptySearchResult {
                                    query: query.to_string(),
                                    result_count: 0,
                                    results: vec![],
                                    message: "No results found for query".to_string(),
                                };

                                let mut response = JsonResponse::success(search_result);

                                // Add guidance for no results
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "search_symbols",
                                    Some(query),
                                    0,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            } else {
                                use codanna::io::guidance_engine::generate_guidance_from_config;
                                let mut response = JsonResponse::success(results);

                                // Add system guidance
                                let result_count =
                                    response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "search_symbols",
                                    arguments
                                        .as_ref()
                                        .and_then(|m| m.get("query"))
                                        .and_then(|v| v.as_str()),
                                    result_count,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            }
                        } else {
                            use codanna::io::exit_code::ExitCode;
                            use codanna::io::format::JsonResponse;
                            let response = JsonResponse::error(
                                ExitCode::GeneralError,
                                "Failed to execute search",
                                vec!["Check query syntax"],
                            );
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(1);
                        }
                    } else if json && tool == "semantic_search_docs" {
                        // Use pre-collected data for JSON output
                        if let Some(results) = semantic_search_docs_data {
                            use codanna::io::format::JsonResponse;
                            use codanna::io::guidance_engine::generate_guidance_from_config;
                            if results.is_empty() {
                                // Create proper struct for empty semantic search
                                #[derive(serde::Serialize)]
                                struct EmptySemanticResult {
                                    query: String,
                                    result_count: usize,
                                    results: Vec<String>,
                                    message: String,
                                }

                                let query = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("query"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");

                                let semantic_result = EmptySemanticResult {
                                    query: query.to_string(),
                                    result_count: 0,
                                    results: vec![],
                                    message: "No semantically similar documentation found"
                                        .to_string(),
                                };

                                let mut response = JsonResponse::success(semantic_result);

                                // Add guidance for no results
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "semantic_search_docs",
                                    Some(query),
                                    0,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            } else {
                                let mut response = JsonResponse::success(results);

                                // Add system guidance for AI assistants
                                let result_count =
                                    response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "semantic_search_docs",
                                    arguments
                                        .as_ref()
                                        .and_then(|m| m.get("query"))
                                        .and_then(|v| v.as_str()),
                                    result_count,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            }
                        } else if !has_semantic_search {
                            use codanna::io::exit_code::ExitCode;
                            use codanna::io::format::JsonResponse;
                            let response = JsonResponse::error(
                                ExitCode::GeneralError,
                                "Semantic search is not enabled",
                                vec![
                                    "Enable semantic search in settings.toml and rebuild the index",
                                ],
                            );
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(1);
                        } else {
                            use codanna::io::exit_code::ExitCode;
                            use codanna::io::format::JsonResponse;
                            let response = JsonResponse::error(
                                ExitCode::GeneralError,
                                "Failed to execute semantic search",
                                vec!["Check query syntax"],
                            );
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(1);
                        }
                    } else if json && tool == "semantic_search_with_context" {
                        // Use pre-collected data for JSON output
                        if let Some(results) = semantic_search_with_context_data {
                            use codanna::io::format::JsonResponse;
                            use codanna::io::guidance_engine::generate_guidance_from_config;
                            if results.is_empty() {
                                // Create proper struct for empty semantic search with context
                                #[derive(serde::Serialize)]
                                struct EmptyContextResult {
                                    query: String,
                                    result_count: usize,
                                    results: Vec<String>,
                                    message: String,
                                }

                                let query = arguments
                                    .as_ref()
                                    .and_then(|m| m.get("query"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");

                                let context_result = EmptyContextResult {
                                    query: query.to_string(),
                                    result_count: 0,
                                    results: vec![],
                                    message: "No semantically similar documentation found"
                                        .to_string(),
                                };

                                let mut response = JsonResponse::success(context_result);

                                // Add guidance for no results
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "semantic_search_with_context",
                                    Some(query),
                                    0,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            } else {
                                use codanna::io::guidance_engine::generate_guidance_from_config;
                                let mut response = JsonResponse::success(results);

                                // Add system guidance
                                let result_count =
                                    response.data.as_ref().map(|d| d.len()).unwrap_or(0);
                                if let Some(guidance) = generate_guidance_from_config(
                                    &guidance_config,
                                    "semantic_search_with_context",
                                    arguments
                                        .as_ref()
                                        .and_then(|m| m.get("query"))
                                        .and_then(|v| v.as_str()),
                                    result_count,
                                ) {
                                    response = response.with_system_message(&guidance);
                                }

                                println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            }
                        } else if !has_semantic_search {
                            use codanna::io::exit_code::ExitCode;
                            use codanna::io::format::JsonResponse;
                            let response = JsonResponse::error(
                                ExitCode::GeneralError,
                                "Semantic search is not enabled",
                                vec![
                                    "Enable semantic search in settings.toml and rebuild the index",
                                ],
                            );
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(1);
                        } else {
                            use codanna::io::exit_code::ExitCode;
                            use codanna::io::format::JsonResponse;
                            let response = JsonResponse::error(
                                ExitCode::GeneralError,
                                "Failed to execute semantic search with context",
                                vec!["Check query syntax"],
                            );
                            println!("{}", serde_json::to_string_pretty(&response).unwrap());
                            std::process::exit(1);
                        }
                    } else {
                        // Default text output
                        for content in &call_result.content {
                            match &**content {
                                rmcp::model::RawContent::Text(text_content) => {
                                    println!("{}", text_content.text);
                                }
                                _ => {
                                    eprintln!("Warning: Non-text content returned");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    if json {
                        use codanna::io::exit_code::ExitCode;
                        use codanna::io::format::JsonResponse;
                        let response = JsonResponse::error(
                            ExitCode::GeneralError,
                            &e.message,
                            vec!["Check the tool name and arguments"],
                        );
                        println!("{}", serde_json::to_string_pretty(&response).unwrap());
                        std::process::exit(1);
                    } else {
                        eprintln!("Error calling tool: {}", e.message);
                        std::process::exit(1);
                    }
                }
            }
        }

        Commands::Benchmark { language, file } => {
            run_benchmark_command(&language, file);
        }

        Commands::Parse { .. } => {
            // Already handled with early return above
            unreachable!("Parse command should have been handled earlier");
        }

        Commands::Plugin { action } => {
            // Execute plugin management command
            use codanna::plugins;
            let result = match action {
                PluginAction::Add {
                    marketplace,
                    plugin_name,
                    r#ref,
                    force,
                    dry_run,
                } => plugins::add_plugin(
                    &config,
                    &marketplace,
                    &plugin_name,
                    r#ref.as_deref(),
                    force,
                    dry_run,
                ),
                PluginAction::Remove {
                    plugin_name,
                    force,
                    dry_run,
                } => plugins::remove_plugin(&config, &plugin_name, force, dry_run),
                PluginAction::Update {
                    plugin_name,
                    r#ref,
                    force,
                    dry_run,
                } => {
                    plugins::update_plugin(&config, &plugin_name, r#ref.as_deref(), force, dry_run)
                }
                PluginAction::List { verbose, json } => {
                    plugins::list_plugins(&config, verbose, json)
                }
                PluginAction::Verify {
                    plugin_name,
                    all,
                    verbose,
                } => {
                    if all {
                        plugins::verify_all_plugins(&config, verbose)
                    } else {
                        match plugin_name {
                            Some(name) => plugins::verify_plugin(&config, &name, verbose),
                            None => {
                                eprintln!(
                                    "Error: plugin_name is required when --all is not specified"
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                }
            };

            if let Err(e) = result {
                let code: codanna::io::exit_code::ExitCode = e.exit_code();
                eprintln!("Plugin operation failed: {e}");
                std::process::exit(i32::from(code));
            }
        }

        Commands::Documents { action } => {
            // Execute document management command
            use codanna::documents::{DocumentStore, SearchQuery};
            use codanna::vector::{FastEmbedGenerator, VectorDimension};

            let doc_path = config.index_path.join("documents");
            let dimension = VectorDimension::dimension_384();

            // Helper to create store with optional embeddings
            let create_store_with_embeddings = || -> Result<DocumentStore, String> {
                let store = DocumentStore::new(&doc_path, dimension)
                    .map_err(|e| format!("Failed to open document store: {e}"))?;

                if config.semantic_search.enabled {
                    let generator = FastEmbedGenerator::new()
                        .map_err(|e| format!("Failed to create embedding generator: {e}"))?;
                    store
                        .with_embeddings(Box::new(generator))
                        .map_err(|e| format!("Failed to enable embeddings: {e}"))
                } else {
                    Ok(store)
                }
            };

            match action {
                DocumentAction::Index {
                    collection,
                    all,
                    force,
                    progress,
                } => {
                    use codanna::io::status_line::StatusLine;
                    use codanna::io::{ProgressBar, ProgressBarOptions, ProgressBarStyle};
                    use std::sync::Arc;

                    // Create or open document store with embeddings
                    let mut store = match create_store_with_embeddings() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("{e}");
                            std::process::exit(1);
                        }
                    };

                    // Force flag: clear file states to treat all files as new
                    if force {
                        eprintln!("Force re-indexing: clearing file state cache");
                        store.clear_file_states();
                    }

                    // Determine which collections to index:
                    // - --collection <name>: specific collection
                    // - --all: all collections
                    // - (no args): all collections if enabled=true
                    let collections_to_index: Vec<(String, codanna::documents::CollectionConfig)> =
                        if let Some(name) = collection {
                            // Specific collection requested
                            match config.documents.collections.get(&name) {
                                Some(col_config) => vec![(name, col_config.clone())],
                                None => {
                                    eprintln!("Collection '{name}' not found in settings.toml");
                                    eprintln!("\nConfigured collections:");
                                    for name in config.documents.collections.keys() {
                                        eprintln!("  - {name}");
                                    }
                                    std::process::exit(1);
                                }
                            }
                        } else if all || config.documents.enabled {
                            // Index all collections (--all flag OR enabled=true in config)
                            // Note: empty check happens after sync below
                            config
                                .documents
                                .collections
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        } else {
                            eprintln!("Document indexing is disabled. Enable with:");
                            eprintln!("  [documents]");
                            eprintln!("  enabled = true");
                            eprintln!("\nOr specify a collection: --collection <name>");
                            std::process::exit(1);
                        };

                    // Sync: remove collections that are indexed but not in config
                    // (mirrors sync_with_config pattern from code indexing)
                    // This runs BEFORE checking if collections_to_index is empty,
                    // so stale collections get cleaned even when all collections are removed.
                    let indexed_collections: std::collections::HashSet<String> =
                        store.list_collections().into_iter().collect();
                    let configured_collections: std::collections::HashSet<String> =
                        config.documents.collections.keys().cloned().collect();

                    let stale_collections: Vec<String> = indexed_collections
                        .difference(&configured_collections)
                        .cloned()
                        .collect();

                    if !stale_collections.is_empty() {
                        eprintln!(
                            "Sync: removing {} stale collections",
                            stale_collections.len()
                        );
                        for name in &stale_collections {
                            eprintln!("  - {name}");
                            if let Err(e) = store.delete_collection(name) {
                                eprintln!("    Failed to remove: {e}");
                            }
                        }
                    }

                    // Now check if there's anything to index
                    if collections_to_index.is_empty() {
                        if stale_collections.is_empty() {
                            eprintln!("No collections configured in settings.toml");
                            eprintln!("\nTo add a collection:");
                            eprintln!("  codanna documents add-collection <name> <path>");
                        } else {
                            eprintln!("Cleaned stale collections. No collections to index.");
                        }
                        return;
                    }

                    use codanna::documents::IndexProgress;
                    use std::cell::RefCell;

                    let mut total_files = 0usize;
                    let mut total_chunks = 0usize;

                    // Progress state for two-phase display
                    type ProgressState = Option<(Arc<ProgressBar>, StatusLine<Arc<ProgressBar>>)>;
                    let phase1_bar: RefCell<ProgressState> = RefCell::new(None);
                    let phase2_bar: RefCell<ProgressState> = RefCell::new(None);

                    for (name, col_config) in collections_to_index {
                        let chunking = col_config.effective_chunking(&config.documents.defaults);

                        if !progress {
                            eprintln!("Indexing collection: {name}");
                        }

                        // Index with two-phase progress callback
                        let result = store.index_collection_with_progress(
                            &name,
                            &col_config,
                            &chunking,
                            |prog| {
                                if !progress {
                                    return;
                                }
                                match prog {
                                    IndexProgress::ProcessingFile { current, total, .. } => {
                                        let mut p1 = phase1_bar.borrow_mut();
                                        if p1.is_none() && total > 0 {
                                            // Create Phase 1 progress bar
                                            let options = ProgressBarOptions::default()
                                                .with_style(ProgressBarStyle::VerticalSolid)
                                                .with_width(28);
                                            let bar = Arc::new(ProgressBar::with_options(
                                                total as u64,
                                                "files",
                                                "chunked",
                                                "failed",
                                                options,
                                            ));
                                            let status = StatusLine::new(Arc::clone(&bar));
                                            *p1 = Some((bar, status));
                                        }
                                        if let Some((bar, _)) = p1.as_ref() {
                                            bar.set_progress(current as u64);
                                        }
                                    }
                                    IndexProgress::GeneratingEmbeddings { current, total } => {
                                        // Finish Phase 1 if active
                                        let mut p1 = phase1_bar.borrow_mut();
                                        if let Some((bar, status)) = p1.take() {
                                            drop(status);
                                            eprintln!("{bar}");
                                        }

                                        let mut p2 = phase2_bar.borrow_mut();
                                        if p2.is_none() && total > 0 {
                                            // Create Phase 2 progress bar
                                            let options = ProgressBarOptions::default()
                                                .with_style(ProgressBarStyle::VerticalSolid)
                                                .with_width(28);
                                            let bar = Arc::new(ProgressBar::with_options(
                                                total as u64,
                                                "chunks",
                                                "embedded",
                                                "failed",
                                                options,
                                            ));
                                            let status = StatusLine::new(Arc::clone(&bar));
                                            *p2 = Some((bar, status));
                                        }
                                        if let Some((bar, _)) = p2.as_ref() {
                                            bar.set_progress(current as u64);
                                        }
                                    }
                                }
                            },
                        );

                        match result {
                            Ok(stats) => {
                                total_files += stats.files_processed;
                                total_chunks += stats.chunks_created;
                                if !progress {
                                    eprintln!("  Files processed: {}", stats.files_processed);
                                    eprintln!("  Files skipped: {}", stats.files_skipped);
                                    eprintln!("  Chunks created: {}", stats.chunks_created);
                                    eprintln!("  Chunks removed: {}", stats.chunks_removed);
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to index collection '{name}': {e}");
                            }
                        };
                    }

                    // Print final progress bars and summary
                    if let Some((bar, status)) = phase1_bar.borrow_mut().take() {
                        drop(status);
                        eprintln!("{bar}");
                    }
                    if let Some((bar, status)) = phase2_bar.borrow_mut().take() {
                        drop(status);
                        eprintln!("{bar}");
                    }
                    if progress {
                        eprintln!("Total: {total_files} files, {total_chunks} chunks");
                    }
                }

                DocumentAction::Search {
                    query,
                    collection,
                    limit,
                    json,
                } => {
                    let mut store = match create_store_with_embeddings() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("{e}");
                            std::process::exit(1);
                        }
                    };

                    let search_query = SearchQuery {
                        text: query,
                        collection,
                        document: None,
                        limit,
                        preview_config: Some(config.documents.search.clone()),
                    };

                    match store.search(search_query) {
                        Ok(results) => {
                            if json {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&results).unwrap_or_default()
                                );
                            } else if results.is_empty() {
                                eprintln!("No results found.");
                            } else {
                                for (i, result) in results.iter().enumerate() {
                                    println!(
                                        "\n{}. {} (score: {:.3})",
                                        i + 1,
                                        result.source_path.display(),
                                        result.similarity
                                    );
                                    if !result.heading_context.is_empty() {
                                        println!(
                                            "   Context: {}",
                                            result.heading_context.join(" > ")
                                        );
                                    }
                                    println!("   Preview: {}", result.content_preview);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Search failed: {e}");
                            std::process::exit(1);
                        }
                    }
                }

                DocumentAction::List { json } => {
                    let store = match create_store_with_embeddings() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("{e}");
                            std::process::exit(1);
                        }
                    };

                    let collections = store.list_collections();
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&collections).unwrap_or_default()
                        );
                    } else if collections.is_empty() {
                        eprintln!("No collections indexed.");
                        eprintln!("\nConfigured collections in settings.toml:");
                        for name in config.documents.collections.keys() {
                            eprintln!("  - {name}");
                        }
                    } else {
                        println!("Indexed collections:");
                        for name in collections {
                            println!("  - {name}");
                        }
                    }
                }

                DocumentAction::Stats { collection, json } => {
                    let store = match create_store_with_embeddings() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("{e}");
                            std::process::exit(1);
                        }
                    };

                    match store.collection_stats(&collection) {
                        Ok(stats) => {
                            if json {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&stats).unwrap_or_default()
                                );
                            } else {
                                println!("Collection: {}", stats.name);
                                println!("  Chunks: {}", stats.chunk_count);
                                println!("  Files: {}", stats.file_count);
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to get stats for '{collection}': {e}");
                            std::process::exit(1);
                        }
                    }
                }

                DocumentAction::AddCollection {
                    name,
                    path,
                    pattern,
                } => {
                    // Find config file (same logic as add-dir)
                    let config_path = if let Some(custom_path) = &cli.config {
                        custom_path.clone()
                    } else {
                        Settings::find_workspace_config().unwrap_or_else(|| {
                            eprintln!(
                                "Error: No configuration file found. Run 'codanna init' first."
                            );
                            std::process::exit(1);
                        })
                    };

                    // Canonicalize the path
                    let canonical_path = match path.canonicalize() {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error: Invalid path '{}': {e}", path.display());
                            std::process::exit(1);
                        }
                    };

                    // Check if path exists and is a directory
                    if !canonical_path.exists() {
                        eprintln!("Error: Path does not exist: {}", canonical_path.display());
                        std::process::exit(1);
                    }

                    // Load current settings
                    let mut settings = match Settings::load() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Error loading settings: {e}");
                            std::process::exit(1);
                        }
                    };

                    // Check if collection already exists
                    if settings.documents.collections.contains_key(&name) {
                        // Add path to existing collection
                        let collection = settings.documents.collections.get_mut(&name).unwrap();
                        if collection.paths.contains(&canonical_path) {
                            eprintln!(
                                "Path already in collection '{name}': {}",
                                canonical_path.display()
                            );
                            std::process::exit(1);
                        }
                        collection.paths.push(canonical_path.clone());
                        if let Some(pat) = pattern {
                            if !collection.patterns.contains(&pat) {
                                collection.patterns.push(pat);
                            }
                        }
                        println!(
                            "Added path to existing collection '{name}': {}",
                            canonical_path.display()
                        );
                    } else {
                        // Create new collection
                        let patterns = pattern.map(|p| vec![p]).unwrap_or_default();
                        let collection_config = codanna::documents::CollectionConfig {
                            paths: vec![canonical_path.clone()],
                            patterns,
                            strategy: None,
                            min_chunk_chars: None,
                            max_chunk_chars: None,
                            overlap_chars: None,
                        };
                        settings
                            .documents
                            .collections
                            .insert(name.clone(), collection_config);
                        println!(
                            "Created collection '{name}' with path: {}",
                            canonical_path.display()
                        );
                    }

                    // Enable documents if not already
                    if !settings.documents.enabled {
                        settings.documents.enabled = true;
                        println!("Enabled document indexing (documents.enabled = true)");
                    }

                    // Save settings
                    if let Err(e) = settings.save(&config_path) {
                        eprintln!("Error saving settings: {e}");
                        std::process::exit(1);
                    }

                    println!("Configuration saved to: {}", config_path.display());
                    println!("\nTo index this collection:");
                    println!("  codanna documents index --collection {name}");
                    println!("  codanna documents index  # indexes all collections");
                }

                DocumentAction::RemoveCollection { name } => {
                    // Find config file (same logic as remove-dir)
                    let config_path = if let Some(custom_path) = &cli.config {
                        custom_path.clone()
                    } else {
                        Settings::find_workspace_config().unwrap_or_else(|| {
                            eprintln!(
                                "Error: No configuration file found. Run 'codanna init' first."
                            );
                            std::process::exit(1);
                        })
                    };

                    // Load current settings
                    let mut settings = match Settings::load() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Error loading settings: {e}");
                            std::process::exit(1);
                        }
                    };

                    // Check if collection exists
                    if !settings.documents.collections.contains_key(&name) {
                        eprintln!("Collection '{name}' not found in settings.toml");
                        if settings.documents.collections.is_empty() {
                            eprintln!("\nNo collections configured.");
                        } else {
                            eprintln!("\nConfigured collections:");
                            for coll_name in settings.documents.collections.keys() {
                                eprintln!("  - {coll_name}");
                            }
                        }
                        std::process::exit(1);
                    }

                    // Remove collection
                    settings.documents.collections.remove(&name);
                    println!("Removed collection '{name}' from settings.toml");

                    // Save settings
                    if let Err(e) = settings.save(&config_path) {
                        eprintln!("Error saving settings: {e}");
                        std::process::exit(1);
                    }

                    println!("Configuration saved to: {}", config_path.display());

                    if settings.documents.collections.is_empty() {
                        println!("\nNo collections remaining.");
                    } else {
                        println!("\nRemaining collections:");
                        for coll_name in settings.documents.collections.keys() {
                            println!("  - {coll_name}");
                        }
                    }

                    println!("\nTo clean the index, run:");
                    println!("  codanna documents index");
                }
            }
        }

        Commands::Profile { action } => {
            // Execute profile management command
            use codanna::profiles;
            use codanna::profiles::commands::{ProfileAction, ProviderAction};

            let result = match action {
                ProfileAction::Init {
                    profile_name,
                    source,
                    force,
                } => profiles::init_profile(&profile_name, source.as_deref(), force),
                ProfileAction::Install {
                    profile_name,
                    source,
                    r#ref,
                    force,
                } => {
                    // Check if --source or --ref flags are provided
                    if source.is_some() || r#ref.is_some() {
                        // Legacy direct installation from git source (not yet implemented)
                        eprintln!("Direct git source installation not yet implemented");
                        eprintln!("Use provider-based installation instead:");
                        eprintln!("  1. codanna profile provider add <source>");
                        eprintln!("  2. codanna profile install {profile_name}");
                        Err(codanna::profiles::error::ProfileError::InvalidManifest {
                            reason: "Git source installation not yet implemented. Use provider registry.".to_string(),
                        })
                    } else {
                        // Use registry-based installation (supports profile@provider syntax)
                        codanna::profiles::install_profile_from_registry(&profile_name, force)
                    }
                }
                ProfileAction::List { verbose, json } => profiles::list_profiles(verbose, json),
                ProfileAction::Status { verbose } => profiles::show_status(verbose),
                ProfileAction::Sync { force } => profiles::sync_team_config(force),
                ProfileAction::Update {
                    profile_name,
                    force,
                } => profiles::update_profile(&profile_name, force),
                ProfileAction::Provider { action } => match action {
                    ProviderAction::Add { source, id } => {
                        profiles::add_provider(&source, id.as_deref())
                    }
                    ProviderAction::Remove { provider_id } => {
                        profiles::remove_provider(&provider_id)
                    }
                    ProviderAction::List { verbose } => profiles::list_providers(verbose),
                },
                ProfileAction::Remove {
                    profile_name,
                    verbose,
                } => profiles::remove_profile(&profile_name, verbose),
                ProfileAction::Verify {
                    profile_name,
                    all,
                    verbose,
                } => {
                    if all {
                        profiles::verify_all_profiles(verbose)
                    } else if let Some(name) = profile_name {
                        profiles::verify_profile(&name, verbose)
                    } else {
                        eprintln!("Error: Must provide profile name or use --all");
                        Err(profiles::error::ProfileError::InvalidManifest {
                            reason: "Must provide profile name or use --all".to_string(),
                        })
                    }
                }
            };

            if let Err(e) = result {
                let code: codanna::io::exit_code::ExitCode = e.exit_code();
                eprintln!("Profile operation failed: {e}");
                std::process::exit(i32::from(code));
            }
        }
    }
}

/// Run parse command to output AST as JSONL
fn run_parse_command(
    file_path: &Path,
    output: Option<PathBuf>,
    max_depth: Option<usize>,
    all_nodes: bool,
) {
    use codanna::io::parse::execute_parse;

    match execute_parse(file_path, output, max_depth, all_nodes) {
        Ok(()) => {
            // Success - exit with code 0
            std::process::exit(0);
        }
        Err(e) => {
            // Print error to stderr
            eprintln!("Error: {e}");

            // Exit with appropriate error code
            let exit_code = e.exit_code();
            std::process::exit(exit_code as i32);
        }
    }
}

#[cfg(test)]
mod add_paths_tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn test_add_paths_to_settings_records_skipped_paths() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        let parent = temp_dir.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();

        let settings = Settings::default();
        settings
            .save(&config_path)
            .expect("failed to write initial config");

        // Add parent to config
        let (settings, added, skipped) =
            add_paths_to_settings(std::slice::from_ref(&parent), &config_path, false)
                .expect("parent addition should succeed");
        assert_eq!(added.len(), 1);
        assert!(skipped.is_empty());
        settings
            .save(&config_path)
            .expect("failed to persist updated config");

        // Attempt to add child - should be skipped and report parent coverage
        let (_, added_again, skipped_paths) =
            add_paths_to_settings(std::slice::from_ref(&child), &config_path, false)
                .expect("child addition should be skipped gracefully");
        assert!(added_again.is_empty(), "child path should not be added");
        assert_eq!(skipped_paths.len(), 1);
        let skipped = &skipped_paths[0];
        assert_eq!(skipped.path, child);
        let parent_canonical = parent.canonicalize().unwrap();
        match &skipped.reason {
            SkipReason::CoveredBy(parent) => assert_eq!(
                parent, &parent_canonical,
                "Expected skipped path to report coverage by parent"
            ),
            other => panic!("Unexpected skip reason: {other:?}"),
        }
    }

    #[test]
    fn test_seed_indexer_with_config_paths_tracks_configured_roots() {
        let temp_dir = TempDir::new().unwrap();
        let parent = temp_dir.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();

        let settings = Settings {
            index_path: temp_dir.path().join("index"),
            ..Settings::default()
        };
        let mut indexer = SimpleIndexer::with_settings(Arc::new(settings));
        assert!(indexer.get_indexed_paths().is_empty());

        let canonical_parent = parent.canonicalize().unwrap();
        let report =
            seed_indexer_with_config_paths(&mut indexer, std::slice::from_ref(&canonical_parent));
        assert_eq!(report.newly_seeded.len(), 1);
        assert_eq!(report.newly_seeded[0], canonical_parent);
        assert!(report.missing_paths.is_empty());

        let tracked: Vec<_> = indexer.get_indexed_paths().iter().cloned().collect();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0], canonical_parent);

        // Adding a child after the parent should be a no-op
        let canonical_child = child.canonicalize().unwrap();
        let child_report =
            seed_indexer_with_config_paths(&mut indexer, std::slice::from_ref(&canonical_child));
        assert!(
            child_report.newly_seeded.is_empty(),
            "child seeding should not add new directories"
        );
        let tracked_after_child: Vec<_> = indexer.get_indexed_paths().iter().cloned().collect();
        assert_eq!(tracked_after_child.len(), 1, "child should not be tracked");
        assert_eq!(tracked_after_child[0], canonical_parent);
    }

    #[test]
    fn test_seed_indexer_with_config_paths_reports_missing() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("missing_dir");

        let settings = Arc::new(Settings {
            index_path: temp_dir.path().join("index"),
            ..Settings::default()
        });
        let mut indexer = SimpleIndexer::with_settings(settings);

        let report = seed_indexer_with_config_paths(&mut indexer, std::slice::from_ref(&missing));
        assert!(
            report.newly_seeded.is_empty(),
            "missing directory should not be seeded"
        );
        assert_eq!(report.missing_paths.len(), 1);
        assert_eq!(report.missing_paths[0], missing);
    }

    #[test]
    fn test_add_paths_to_settings_skips_files_without_persisting() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("settings.toml");
        let file_path = temp_dir.path().join("single.rs");
        fs::write(&file_path, "fn main() {}\n").unwrap();

        Settings::default()
            .save(&config_path)
            .expect("failed to write initial config");

        let (settings, added, skipped) =
            add_paths_to_settings(std::slice::from_ref(&file_path), &config_path, false)
                .expect("file addition should succeed");
        assert!(added.is_empty(), "file should not be persisted in config");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].path, file_path);
        assert!(matches!(skipped[0].reason, SkipReason::FileNotPersisted));

        let reloaded = Settings::load_from(&config_path).expect("config reload failed");
        assert!(
            reloaded.indexing.indexed_paths.is_empty(),
            "file path should not be stored in indexed_paths"
        );
        drop(settings); // ensure no unused warnings
    }
}
/// Run parser performance benchmarks
fn run_benchmark_command(language: &str, custom_file: Option<PathBuf>) {
    use codanna::display::theme::Theme;
    use console::style;

    // Print styled header
    if Theme::should_disable_colors() {
        println!("\n=== Codanna Parser Benchmarks ===\n");
    } else {
        println!(
            "\n{}\n",
            style("=== Codanna Parser Benchmarks ===").cyan().bold()
        );
    }

    match language.to_lowercase().as_str() {
        "rust" => benchmark_rust_parser(custom_file),
        "python" => benchmark_python_parser(custom_file),
        "php" => benchmark_php_parser(custom_file),
        "typescript" | "ts" => benchmark_typescript_parser(custom_file),
        "go" => benchmark_go_parser(custom_file),
        "csharp" | "c#" | "cs" => benchmark_csharp_parser(custom_file),
        "all" => {
            benchmark_rust_parser(None);
            println!();
            benchmark_python_parser(None);
            println!();
            benchmark_php_parser(None);
            println!();
            benchmark_typescript_parser(None);
            println!();
            benchmark_go_parser(None);
            println!();
            benchmark_csharp_parser(None);
        }
        _ => {
            eprintln!("Unknown language: {language}");
            eprintln!("Available languages: rust, python, php, typescript, go, csharp, all");
            std::process::exit(1);
        }
    }

    // Print target info with styling
    if Theme::should_disable_colors() {
        println!("\nTarget: >10,000 symbols/second ✅");
    } else {
        println!(
            "\n{}: {} ✅",
            style("Target").dim(),
            style(">10,000 symbols/second").dim()
        );
    }
}

fn benchmark_rust_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        // Generate benchmark code
        (generate_rust_benchmark_code(), None)
    };

    let mut parser = RustParser::new().expect("Failed to create Rust parser");
    benchmark_parser("Rust", &mut parser, &code, file_path);
}

fn benchmark_python_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else if std::path::Path::new("tests/python_comprehensive_features.py").exists() {
        match std::fs::read_to_string("tests/python_comprehensive_features.py") {
            Ok(content) => (content, None),
            Err(e) => {
                eprintln!("Warning: Failed to read test file: {e}");
                eprintln!("Generating benchmark code instead...");
                (generate_python_benchmark_code(), None)
            }
        }
    } else {
        // Generate benchmark code
        (generate_python_benchmark_code(), None)
    };

    let mut parser = PythonParser::new().expect("Failed to create Python parser");
    benchmark_parser("Python", &mut parser, &code, file_path);
}

fn benchmark_php_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        // Generate benchmark code
        (generate_php_benchmark_code(), None)
    };

    let mut parser = PhpParser::new().expect("Failed to create PHP parser");
    benchmark_parser("PHP", &mut parser, &code, file_path);
}

fn benchmark_typescript_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        // Generate benchmark code
        (generate_typescript_benchmark_code(), None)
    };

    let mut parser = TypeScriptParser::new().expect("Failed to create TypeScript parser");
    benchmark_parser("TypeScript", &mut parser, &code, file_path);
}

fn benchmark_go_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        // Generate benchmark code
        (generate_go_benchmark_code(), None)
    };

    let mut parser = GoParser::new().expect("Failed to create Go parser");
    benchmark_parser("Go", &mut parser, &code, file_path);
}

fn benchmark_csharp_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        // Generate benchmark code
        (generate_csharp_benchmark_code(), None)
    };

    let mut parser = CSharpParser::new().expect("Failed to create C# parser");
    benchmark_parser("C#", &mut parser, &code, file_path);
}

fn benchmark_parser(
    language: &str,
    parser: &mut dyn LanguageParser,
    code: &str,
    file_path: Option<PathBuf>,
) {
    let file_id = FileId::new(1).expect("Failed to create file ID");
    let mut counter = SymbolCounter::new();

    // Warm up
    let _ = parser.parse(code, file_id, &mut counter);

    // Measure parsing performance (average of 3 runs)
    let mut total_duration = std::time::Duration::ZERO;
    let mut symbols_count = 0;

    for _ in 0..3 {
        counter = SymbolCounter::new();
        let start = Instant::now();
        let symbols = parser.parse(code, file_id, &mut counter);
        total_duration += start.elapsed();
        symbols_count = symbols.len();
    }

    let avg_duration = total_duration / 3;
    let rate = symbols_count as f64 / avg_duration.as_secs_f64();

    // Display results using rich table
    use codanna::display::tables::create_benchmark_table;

    let table = create_benchmark_table(
        language,
        file_path
            .as_ref()
            .map(|p| p.to_str().unwrap_or("<invalid path>")),
        symbols_count,
        avg_duration,
        rate,
    );

    println!("\n{table}");

    // Verify zero-cost abstractions (silently)
    let calls = parser.find_calls(code);
    if !calls.is_empty() {
        let (caller, _callee, _) = &calls[0];
        let caller_ptr = caller.as_ptr();
        let code_ptr = code.as_ptr();
        let within_bounds =
            caller_ptr >= code_ptr && caller_ptr < unsafe { code_ptr.add(code.len()) };

        if !within_bounds {
            println!("\n⚠️  Warning: String allocation detected!");
        }
    }
}

fn generate_rust_benchmark_code() -> String {
    let mut code = String::from("//! Rust benchmark file\n\n");

    // Generate 500 functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"/// Function {i} documentation
fn function_{i}(param1: i32, param2: &str) -> bool {{
    let result = param1 * 2;
    result > 0
}}

"#
        ));
    }

    // Generate 50 structs with methods
    for i in 0..50 {
        code.push_str(&format!(
            r#"/// Struct {i} documentation
struct Struct{i} {{
    value: i32,
}}

impl Struct{i} {{
    fn new(value: i32) -> Self {{
        Self {{ value }}
    }}

    fn method_a(&self) -> i32 {{
        self.value * 2
    }}
}}

"#
        ));
    }

    code
}

fn generate_python_benchmark_code() -> String {
    let mut code = String::from("\"\"\"Python benchmark file\"\"\"\n\n");

    // Generate 500 functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"def function_{i}(param1: int, param2: str = 'default') -> bool:
    """Function {i} documentation."""
    result = param1 * 2
    return result > 0

"#
        ));
    }

    // Generate 50 classes
    for i in 0..50 {
        code.push_str(&format!(
            r#"class Class_{i}:
    """Class {i} documentation."""

    def __init__(self, value: int):
        self.value = value

    def method_a(self) -> int:
        return self.value * 2

"#
        ));
    }

    code
}

fn generate_php_benchmark_code() -> String {
    let mut code = String::from("<?php\n/**\n * PHP benchmark file\n */\n\n");

    // Generate 500 functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"/**
 * Function {i} documentation
 */
function function_{i}(int $param1, string $param2 = 'default'): bool {{
    $result = $param1 * 2;
    return $result > 0;
}}

"#
        ));
    }

    // Generate 50 classes with methods
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Class {i} documentation
 */
class Class_{i} {{
    private int $value;

    public function __construct(int $value) {{
        $this->value = $value;
    }}

    public function methodA(): int {{
        return $this->value * 2;
    }}

    public function methodB(string $param): string {{
        return strtoupper($param);
    }}
}}

"#
        ));
    }

    // Generate 25 interfaces
    for i in 0..25 {
        code.push_str(&format!(
            r#"interface Interface_{i} {{
    public function method_{i}(): void;
}}

"#
        ));
    }

    // Generate 25 traits
    for i in 0..25 {
        code.push_str(&format!(
            r#"trait Trait_{i} {{
    public function traitMethod_{i}(): string {{
        return 'trait_{i}';
    }}
}}

"#
        ));
    }

    code.push_str("?>");
    code
}

fn generate_typescript_benchmark_code() -> String {
    let mut code = String::from("// TypeScript benchmark file\n\n");

    // Generate 500 functions with various TypeScript features
    for i in 0..500 {
        code.push_str(&format!(
            r#"/**
 * Function {i} documentation
 * @param param1 The first parameter
 * @param param2 The second parameter
 * @returns A boolean result
 */
export function function_{i}(param1: number, param2: string = 'default'): boolean {{
    const result = param1 > 0 && param2.length > 0;
    return result;
}}

"#
        ));
    }

    // Generate 50 interfaces
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Interface {i} for data structure
 */
export interface Interface_{i} {{
    id: number;
    name: string;
    optional?: boolean;
    readonly immutable: string;
    method(param: string): void;
}}

"#
        ));
    }

    // Generate 50 classes with methods
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Class {i} implementation
 */
export class Class_{i} implements Interface_{i} {{
    public id: number;
    public name: string;
    public optional?: boolean;
    public readonly immutable: string;
    private _internal: number;
    protected _protected: string;

    constructor(id: number, name: string) {{
        this.id = id;
        this.name = name;
        this.immutable = 'fixed';
        this._internal = 0;
        this._protected = 'protected';
    }}

    public method(param: string): void {{
        console.log(param);
    }}

    private privateMethod(): number {{
        return this._internal;
    }}

    protected protectedMethod(): string {{
        return this._protected;
    }}

    static staticMethod(): void {{
        console.log('static');
    }}
}}

"#
        ));
    }

    // Generate 50 type aliases
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Type alias {i}
 */
export type TypeAlias_{i} = string | number | boolean;

type ComplexType_{i} = {{
    field1: TypeAlias_{i};
    field2: Interface_{i};
    field3: (param: string) => void;
}};

"#
        ));
    }

    // Generate 50 enums
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Enum {i} definition
 */
export enum Enum_{i} {{
    First = 0,
    Second = 1,
    Third = 'three',
    Fourth = 'four'
}}

"#
        ));
    }

    // Add some arrow functions and const declarations
    for i in 0..50 {
        code.push_str(&format!(
            r#"export const arrowFunction_{i} = (x: number, y: number): number => x + y;

export const constant_{i}: string = 'constant value';

let variable_{i}: number = {i};

"#
        ));
    }

    code
}

fn generate_go_benchmark_code() -> String {
    let mut code =
        String::from("// Go benchmark file\n\npackage bench\n\nimport (\n\t\"fmt\"\n)\n\n");

    // Generate 500 free functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"// Function {i} documentation
func Function_{i}(param1 int, param2 string) bool {{
    result := param1 * 2
    return result > 0 && len(param2) > 0
}}

"#
        ));
    }

    // Generate 50 structs with methods and interface satisfaction
    for i in 0..50 {
        code.push_str(&format!(
            r#"// Struct {i} documentation
type Struct{i} struct {{
    Value int
}}

func NewStruct{i}(v int) *Struct{i} {{
    return &Struct{i}{{Value: v}}
}}

func (s *Struct{i}) MethodA() int {{
    return s.Value * 2
}}

func (s *Struct{i}) Do(param string) int {{
    fmt.Println(param)
    return len(param) + s.Value
}}

"#
        ));
    }

    // Generate 25 interfaces
    for i in 0..25 {
        code.push_str(&format!(
            r#"// Interface {i} documentation
type Interface_{i} interface {{
    Do(param string) int
}}

"#
        ));
    }

    // A small main-like entry to keep parser busy with calls/selectors
    code.push_str(
        r#"// Entry point (not used, just for call patterns)
func main() {
    s := NewStruct0(42)
    _ = s.MethodA()
    _ = s.Do("hello")
    ok := Function_0(1, "x")
    if ok {
        fmt.Println("ok")
    }
}
"#,
    );

    code
}

fn generate_csharp_benchmark_code() -> String {
    let mut code = String::from(
        "// C# benchmark file\n\nusing System;\nusing System.Collections.Generic;\nusing System.Linq;\n\nnamespace BenchmarkNamespace\n{\n",
    );

    // Generate 500 static classes with methods
    for i in 0..500 {
        code.push_str(&format!(
            r#"    /// <summary>
    /// Static class {i} documentation
    /// </summary>
    public static class StaticClass{i}
    {{
        public static int Method{i}(int param)
        {{
            return param * 2;
        }}

        public static string Process{i}(string input)
        {{
            return input.ToUpper();
        }}
    }}

"#
        ));
    }

    // Generate 50 classes with properties/fields/constructors
    for i in 0..50 {
        code.push_str(&format!(
            r#"    /// <summary>
    /// Class {i} documentation
    /// </summary>
    public class Class{i}
    {{
        private int _value;

        public int Value {{ get; set; }}
        public string Name {{ get; set; }}

        public Class{i}(int value, string name)
        {{
            _value = value;
            Value = value;
            Name = name;
        }}

        public int Calculate()
        {{
            return _value * 2;
        }}

        public void Process(string input)
        {{
            Console.WriteLine(input);
        }}
    }}

"#
        ));
    }

    // Generate 25 interfaces
    for i in 0..25 {
        code.push_str(&format!(
            r#"    /// <summary>
    /// Interface {i} documentation
    /// </summary>
    public interface IInterface{i}
    {{
        void Process(string input);
        int Calculate();
    }}

"#
        ));
    }

    // Program class with Main method
    code.push_str(
        r#"    /// <summary>
    /// Entry point class
    /// </summary>
    class Program
    {
        static void Main(string[] args)
        {
            var obj = new Class0(42, "test");
            var result = obj.Calculate();
            obj.Process("hello");

            var staticResult = StaticClass0.Method0(10);
            var processed = StaticClass0.Process0("world");

            Console.WriteLine($"Result: {result}, Static: {staticResult}, Processed: {processed}");
        }
    }
}
"#,
    );

    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Verifies CLI structure is valid at compile time.
    ///
    /// Uses clap's debug_assert to catch configuration errors.
    #[test]
    fn verify_cli() {
        // This test ensures the CLI structure is valid
        Cli::command().debug_assert();
    }
}
