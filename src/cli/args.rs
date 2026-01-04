//! CLI argument parsing using clap.
//!
//! Contains the Cli struct, Commands enum, and all subcommand enums.

use clap::{
    Parser, Subcommand,
    builder::styling::{AnsiColor, Effects, Styles},
};
use std::path::PathBuf;

fn clap_cargo_style() -> Styles {
    Styles::styled()
        .header(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .usage(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

/// Create custom help text with consistent styling
fn create_custom_help() -> String {
    use crate::display::theme::Theme;
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
pub struct Cli {
    /// Path to custom settings.toml file
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Show detailed loading information
    #[arg(long, global = true)]
    pub info: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI commands
#[derive(Subcommand)]
pub enum Commands {
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

        /// Disable progress bars (overrides settings.toml show_progress)
        #[arg(long)]
        no_progress: bool,

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
        after_help = "Tools:\n  find_symbol       <name>              Exact name lookup\n  search_symbols    query:<text>        Fuzzy text search (kind:<type> limit:<n>)\n  get_calls         <name|symbol_id:N>  What this symbol calls\n  find_callers      <name|symbol_id:N>  What calls this symbol\n  analyze_impact    <name|symbol_id:N>  Full dependency graph\n  semantic_search_docs query:<text>     Code search by meaning\n  semantic_search_with_context query:<text>  Search with relationships\n  search_documents  query:<text>        Search markdown/text docs\n  get_index_info                        Index stats\n\nExamples:\n  codanna mcp find_symbol <name>\n  codanna mcp search_symbols query:<text> kind:function\n  codanna mcp get_calls <name>\n  codanna mcp get_calls symbol_id:<N>\n  codanna mcp semantic_search_docs query:\"<text>\" limit:5\n  codanna mcp search_symbols query:<text> --json | jq '.data[].symbol_id'"
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

        /// Check for file changes and reindex before running tool
        #[arg(long)]
        watch: bool,
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

    /// Index with parallel pipeline (experimental)
    #[command(
        name = "index-parallel",
        about = "Index using parallel pipeline with two-phase resolution",
        long_about = "Index source code using the parallel pipeline architecture.\n\nPhase 1: Parallel file discovery, reading, parsing, and indexing.\nPhase 2: Two-pass cross-file relationship resolution.",
        after_help = "Examples:\n  codanna index-parallel src\n  codanna index-parallel --no-progress\n  codanna index-parallel src lib --force"
    )]
    IndexParallel {
        /// Paths to directories to index (uses settings.toml indexed_paths if empty)
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// Force re-indexing (clears existing index)
        #[arg(short, long)]
        force: bool,

        /// Disable progress bars (overrides settings.toml show_progress)
        #[arg(long)]
        no_progress: bool,
    },

    /// Manage project profiles
    #[command(
        about = "Initialize and manage project profiles",
        long_about = "Manage project profiles for provider-specific initialization.\n\nProfiles set up project structure, configuration files, and provider integration.",
        after_help = "Examples:\n  codanna profile init claude\n  codanna profile install claude --source git@github.com:codanna/profiles.git\n  codanna profile list\n  codanna profile status"
    )]
    Profile {
        #[command(subcommand)]
        action: crate::profiles::commands::ProfileAction,
    },
}

/// Plugin management actions
#[derive(Subcommand)]
pub enum PluginAction {
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
pub enum DocumentAction {
    /// Index documents from a collection
    #[command(
        about = "Index documents from a configured collection",
        after_help = "Examples:\n  codanna documents index --collection docs\n  codanna documents index --all\n  codanna documents index --no-progress"
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

        /// Disable progress bars (overrides settings.toml show_progress)
        #[arg(long)]
        no_progress: bool,
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
pub enum RetrieveQuery {
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
