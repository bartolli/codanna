use clap::{Parser, Subcommand};
use codanna::{SimpleIndexer, SymbolKind, RelationKind, Settings, IndexPersistence};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "codanna")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A code intelligence system for understanding codebases")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize configuration file
    Init {
        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },
    
    /// Index source files or directories
    Index {
        /// Path to file or directory to index
        path: PathBuf,
        
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
    
    /// Retrieve information from the index
    Retrieve {
        #[command(subcommand)]
        query: RetrieveQuery,
    },
    
    /// Show current configuration
    Config,
    
    /// Start MCP server for AI assistants
    Serve {
        /// Port to listen on (overrides config)
        #[arg(short, long)]
        port: Option<u16>,
    },
    
    /// Test MCP client functionality
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
    },
    
    /// Call MCP tools directly without spawning a server (embedded mode)
    Mcp {
        /// Tool to call
        tool: String,
        
        /// Tool arguments as JSON
        #[arg(long)]
        args: Option<String>,
    },
}

#[derive(Subcommand)]
enum RetrieveQuery {
    /// Find a symbol by name
    Symbol {
        /// Name of the symbol to find
        name: String,
    },
    
    /// Show what functions a given function calls
    Calls {
        /// Name of the function
        function: String,
    },
    
    /// Show what functions call a given function
    Callers {
        /// Name of the function
        function: String,
    },
    
    /// Show what types implement a given trait
    Implementations {
        /// Name of the trait
        trait_name: String,
    },
    
    /// Show what types a given symbol uses
    Uses {
        /// Name of the symbol
        symbol: String,
    },
    
    /// Show the impact radius of changing a symbol
    Impact {
        /// Name of the symbol
        symbol: String,
        /// Maximum depth to search (default: 5)
        #[arg(short, long)]
        depth: Option<usize>,
    },
    
    /// Search for symbols using full-text search
    Search {
        /// Search query
        query: String,
        
        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,
        
        /// Filter by symbol kind (e.g., Function, Struct, Trait)
        #[arg(short, long)]
        kind: Option<String>,
        
        /// Filter by module path
        #[arg(short, long)]
        module: Option<String>,
    },
    
    /// Show what methods a type or trait defines
    Defines {
        /// Name of the type or trait
        symbol: String,
    },
    
    /// Show comprehensive dependency analysis for a symbol
    Dependencies {
        /// Name of the symbol
        symbol: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    
    // For non-init commands, check if project is initialized
    if !matches!(cli.command, Commands::Init { .. }) {
        if let Err(warning) = Settings::check_init() {
            eprintln!("Warning: {}", warning);
            eprintln!("Using default configuration for now.");
        }
    }
    
    // Load configuration
    let mut config = Settings::load().unwrap_or_else(|e| {
        eprintln!("Configuration error: {}", e);
        Settings::default()
    });
    
    match &cli.command {
        Commands::Init { force } => {
            let config_path = PathBuf::from(".codanna/settings.toml");
            
            if config_path.exists() && !force {
                eprintln!("Configuration file already exists at: {}", config_path.display());
                eprintln!("Use --force to overwrite");
                std::process::exit(1);
            }
            
            match Settings::init_config_file(*force) {
                Ok(path) => {
                    println!("Created configuration file at: {}", path.display());
                    println!("Edit this file to customize your settings.");
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            return;
        }
        
        Commands::Config => {
            println!("Current Configuration:");
            println!("{}", "=".repeat(50));
            match toml::to_string_pretty(&config) {
                Ok(toml_str) => println!("{}", toml_str),
                Err(e) => eprintln!("Error displaying config: {}", e),
            }
            return;
        }
        
        Commands::Index { threads, force: _, .. } => {
            // Override config with CLI args
            if let Some(t) = threads {
                config.indexing.parallel_threads = *t;
            }
        }
        
        Commands::Serve { port } => {
            // Port override is handled in the serve command itself
            let _ = port; // Suppress unused warning
        }
        
        _ => {}
    }
    
    // Set up persistence based on config
    let index_path = config.index_path.clone();
    let persistence = IndexPersistence::new(index_path);
    
    // Skip loading index for mcp-test (thin client mode)
    let skip_index_load = matches!(cli.command, Commands::McpTest { .. });
    
    // Load existing index or create new one (unless we're in thin client mode)
    let settings = Arc::new(config.clone());
    let mut indexer = if skip_index_load {
        SimpleIndexer::with_settings(settings.clone()) // Empty indexer, won't be used
    } else {
        let force_reindex = matches!(cli.command, Commands::Index { force: true, .. });
        if persistence.exists() && !force_reindex {
            eprintln!("DEBUG: Found existing index at {}", config.index_path.display());
            match persistence.load_with_settings(settings.clone()) {
                Ok(loaded) => {
                    eprintln!("DEBUG: Successfully loaded index from disk");
                    eprintln!("Loaded existing index ({} symbols)", loaded.symbol_count());
                    loaded
                }
                Err(e) => {
                    eprintln!("Warning: Could not load index: {}. Creating new index.", e);
                    SimpleIndexer::with_settings(settings.clone())
                }
            }
        } else {
            if force_reindex && persistence.exists() {
                eprintln!("Force re-indexing requested, creating new index");
            } else if !persistence.exists() {
                eprintln!("DEBUG: No existing index found at {}", config.index_path.display());
            }
            eprintln!("DEBUG: Creating new index");
            let new_indexer = SimpleIndexer::with_settings(settings.clone());
            // Clear Tantivy index if force re-indexing
            if force_reindex {
                if let Err(e) = new_indexer.clear_tantivy_index() {
                    eprintln!("Warning: Failed to clear Tantivy index: {}", e);
                }
            }
            new_indexer
        }
    };
    
    match cli.command {
        Commands::Init { .. } | Commands::Config => {
            // Already handled above
            unreachable!()
        }
        
        Commands::Serve { port } => {
            // Override port from config if provided
            let server_port = port.unwrap_or(config.mcp.port);
            
            eprintln!("Starting MCP server on stdio transport");
            eprintln!("Port configuration: {} (not used for stdio)", server_port);
            eprintln!("To test: npx @modelcontextprotocol/inspector cargo run -- serve");
            
            // Create MCP server from existing index
            let server = codanna::mcp::CodeIntelligenceServer::from_persistence(&config)
                .await
                .map_err(|e| {
                    eprintln!("Failed to create MCP server: {}", e);
                    std::process::exit(1);
                }).unwrap();
            
            // Start server with stdio transport
            use rmcp::{ServiceExt, transport::stdio};
            let service = server.serve(stdio()).await.map_err(|e| {
                eprintln!("Failed to start MCP server: {}", e);
                std::process::exit(1);
            }).unwrap();
            
            // Wait for server to complete
            service.waiting().await.map_err(|e| {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }).unwrap();
        }
        
        Commands::Index { path, force: _, progress, dry_run, max_files, .. } => {
            // Determine if path is a file or directory
            if path.is_file() {
                // Single file indexing
                match indexer.index_file(&path) {
                    Ok(file_id) => {
                        
                        let language_name = codanna::parsing::Language::from_path(&path)
                            .map(|l| l.to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        println!("Successfully indexed: {} [{}]", path.display(), language_name);
                        println!("File ID: {}", file_id.value());
                        
                        let symbol_count = indexer.symbol_count();
                        println!("Found {} symbols", symbol_count);
                        
                        // Show summary of what was found
                        let symbols = indexer.get_all_symbols();
                        let functions = symbols.iter()
                            .filter(|s| s.kind == SymbolKind::Function)
                            .count();
                        let methods = symbols.iter()
                            .filter(|s| s.kind == SymbolKind::Method)
                            .count();
                        let structs = symbols.iter()
                            .filter(|s| s.kind == SymbolKind::Struct)
                            .count();
                        let traits = symbols.iter()
                            .filter(|s| s.kind == SymbolKind::Trait)
                            .count();
                        
                        println!("  Functions: {}", functions);
                        println!("  Methods: {}", methods);
                        println!("  Structs: {}", structs);
                        println!("  Traits: {}", traits);
                        
                        // Save the index
                        eprintln!("DEBUG: Saving index with {} symbols", indexer.symbol_count());
                        match persistence.save(&indexer) {
                            Ok(_) => {
                                println!("\nIndex saved to: {}", config.index_path.display());
                                eprintln!("DEBUG: Index saved successfully");
                            }
                            Err(e) => eprintln!("\nWarning: Could not save index: {}", e),
                        }
                    }
                    Err(e) => {
                        eprintln!("Error indexing file: {}", e);
                        std::process::exit(1);
                    }
                }
            } else if path.is_dir() {
                // Directory indexing
                println!("Indexing directory: {}", path.display());
                
                match indexer.index_directory(&path, progress, dry_run, max_files.clone()) {
                    Ok(stats) => {
                        stats.display();
                        
                        if !dry_run && stats.files_indexed > 0 {
                            // Save the index
                            eprintln!("\nSaving index with {} symbols (data: {})...", indexer.symbol_count(), indexer.data_symbol_count());
                            match persistence.save(&indexer) {
                                Ok(_) => {
                                    println!("Index saved to: {}", config.index_path.display());
                                }
                                Err(e) => {
                                    eprintln!("Error: Could not save index: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error indexing directory: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Error: Path does not exist: {}", path.display());
                std::process::exit(1);
            }
        }
        
        Commands::Retrieve { query } => {
            match query {
                RetrieveQuery::Symbol { name } => {
                    let symbols = indexer.find_symbols_by_name(&name);
                    
                    if symbols.is_empty() {
                        println!("No symbols found with name: {}", name);
                    } else {
                        println!("Found {} symbol(s) named '{}':", symbols.len(), name);
                        for symbol in symbols {
                            let file_path = indexer.get_file_path(symbol.file_id)
                                .unwrap_or("<unknown>");
                            println!("  {:?} at {}:{}", 
                                symbol.kind,
                                file_path,
                                symbol.range.start_line + 1
                            );
                            
                            // Show documentation if available
                            if let Some(ref doc) = symbol.doc_comment {
                                // Show first 3 lines or less
                                let lines: Vec<&str> = doc.lines().take(3).collect();
                                let preview = if doc.lines().count() > 3 {
                                    format!("{}...", lines.join(" "))
                                } else {
                                    lines.join(" ")
                                };
                                println!("    Documentation: {}", preview);
                            }
                            
                            // Show signature if available
                            if let Some(ref sig) = symbol.signature {
                                println!("    Signature: {}", sig);
                            }
                        }
                    }
                }
                
                RetrieveQuery::Calls { function } => {
                    match indexer.find_symbol(&function) {
                        Some(symbol_id) => {
                            let called = indexer.get_called_functions(symbol_id);
                            
                            if called.is_empty() {
                                println!("{} doesn't call any functions", function);
                            } else {
                                println!("{} calls {} function(s):", function, called.len());
                                for callee in called {
                                    println!("  -> {}", callee.name);
                                }
                            }
                        }
                        None => {
                            println!("Function not found: {}", function);
                        }
                    }
                }
                
                RetrieveQuery::Callers { function } => {
                    match indexer.find_symbol(&function) {
                        Some(symbol_id) => {
                            let callers = indexer.get_calling_functions(symbol_id);
                            
                            if callers.is_empty() {
                                println!("No functions call {}", function);
                            } else {
                                println!("{} function(s) call {}:", callers.len(), function);
                                for caller in callers {
                                    println!("  <- {}", caller.name);
                                }
                            }
                        }
                        None => {
                            println!("Function not found: {}", function);
                        }
                    }
                }
                
                RetrieveQuery::Implementations { trait_name } => {
                    match indexer.find_symbol(&trait_name) {
                        Some(trait_id) => {
                            let symbol = indexer.get_symbol(trait_id).unwrap();
                            if symbol.kind != SymbolKind::Trait {
                                println!("{} is not a trait", trait_name);
                                return;
                            }
                            
                            let implementations = indexer.get_implementations(trait_id);
                            
                            if implementations.is_empty() {
                                println!("No types implement {}", trait_name);
                            } else {
                                println!("{} type(s) implement {}:", implementations.len(), trait_name);
                                for impl_type in implementations {
                                    println!("  - {}", impl_type.name);
                                }
                            }
                        }
                        None => {
                            println!("Trait not found: {}", trait_name);
                        }
                    }
                }
                
                RetrieveQuery::Uses { symbol } => {
                    match indexer.find_symbol(&symbol) {
                        Some(symbol_id) => {
                            let used_types = indexer.graph.get_relationships(symbol_id, RelationKind::Uses)
                                .into_iter()
                                .filter_map(|id| indexer.get_symbol(id))
                                .collect::<Vec<_>>();
                            
                            if used_types.is_empty() {
                                println!("{} doesn't use any types", symbol);
                            } else {
                                println!("{} uses {} type(s):", symbol, used_types.len());
                                for used in used_types {
                                    println!("  - {}", used.name);
                                }
                            }
                        }
                        None => {
                            println!("Symbol not found: {}", symbol);
                        }
                    }
                }
                
                RetrieveQuery::Impact { symbol, depth } => {
                    match indexer.find_symbol(&symbol) {
                        Some(symbol_id) => {
                            let impacted = indexer.graph.get_impact_radius(symbol_id, depth);
                            
                            if impacted.is_empty() {
                                println!("No symbols would be impacted by changing {}", symbol);
                            } else {
                                println!("Changing {} would impact {} symbol(s):", symbol, impacted.len());
                                
                                // Group by symbol kind for better readability
                                let mut by_kind: std::collections::HashMap<SymbolKind, Vec<_>> = std::collections::HashMap::new();
                                for id in impacted {
                                    if let Some(sym) = indexer.get_symbol(id) {
                                        by_kind.entry(sym.kind).or_default().push(sym);
                                    }
                                }
                                
                                // Display grouped by kind
                                for (kind, symbols) in by_kind {
                                    println!("\n  {}s:", format!("{:?}", kind).to_lowercase());
                                    for sym in symbols {
                                        let file_path = indexer.get_file_path(sym.file_id)
                                            .unwrap_or("<unknown>");
                                        println!("    - {} ({}:{})", sym.name, file_path, sym.range.start_line + 1);
                                    }
                                }
                            }
                        }
                        None => {
                            println!("Symbol not found: {}", symbol);
                        }
                    }
                }
                
                RetrieveQuery::Search { query, limit, kind, module } => {
                    // Parse the kind filter if provided
                    let kind_filter = kind.as_ref().and_then(|k| {
                        match k.to_lowercase().as_str() {
                            "function" => Some(SymbolKind::Function),
                            "struct" => Some(SymbolKind::Struct),
                            "trait" => Some(SymbolKind::Trait),
                            "method" => Some(SymbolKind::Method),
                            "field" => Some(SymbolKind::Field),
                            "module" => Some(SymbolKind::Module),
                            "constant" => Some(SymbolKind::Constant),
                            _ => {
                                eprintln!("Warning: Unknown symbol kind '{}', ignoring filter", k);
                                None
                            }
                        }
                    });
                    
                    match indexer.search(&query, limit, kind_filter, module.as_deref()) {
                        Ok(results) => {
                            if results.is_empty() {
                                println!("No results found for query: {}", query);
                            } else {
                                println!("Found {} result(s) for query '{}':\n", results.len(), query);
                                
                                for (i, result) in results.iter().enumerate() {
                                    println!("{}. {} ({})", i + 1, result.name, format!("{:?}", result.kind));
                                    println!("   File: {}:{}", result.file_path, result.line);
                                    if !result.module_path.is_empty() {
                                        println!("   Module: {}", result.module_path);
                                    }
                                    if let Some(ref doc) = result.doc_comment {
                                        println!("   Doc: {}", doc.lines().next().unwrap_or(""));
                                    }
                                    println!("   Score: {:.2}", result.score);
                                    println!();
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Search failed: {}", e);
                        }
                    }
                }
                
                RetrieveQuery::Defines { symbol } => {
                    match indexer.find_symbol(&symbol) {
                        Some(symbol_id) => {
                            let defined = indexer.graph.get_relationships(symbol_id, RelationKind::Defines)
                                .into_iter()
                                .filter_map(|id| indexer.get_symbol(id))
                                .collect::<Vec<_>>();
                            
                            if defined.is_empty() {
                                println!("{} doesn't define any methods", symbol);
                            } else {
                                println!("{} defines {} method(s):", symbol, defined.len());
                                for def in defined {
                                    println!("  - {}", def.name);
                                }
                            }
                        }
                        None => {
                            println!("Symbol not found: {}", symbol);
                        }
                    }
                }
                
                RetrieveQuery::Dependencies { symbol } => {
                    match indexer.find_symbol(&symbol) {
                        Some(symbol_id) => {
                            let sym = indexer.get_symbol(symbol_id).unwrap();
                            println!("Dependency Analysis for {} ({:?}):", symbol, sym.kind);
                            println!("{}", "=".repeat(50));
                            
                            // Outgoing dependencies (what this symbol depends on)
                            let dependencies = indexer.graph.get_dependencies(symbol_id);
                            if dependencies.is_empty() {
                                println!("\nNo outgoing dependencies");
                            } else {
                                println!("\nOutgoing Dependencies (what {} depends on):", symbol);
                                for (kind, ids) in dependencies {
                                    let symbols: Vec<_> = ids.into_iter()
                                        .filter_map(|id| indexer.get_symbol(id))
                                        .collect();
                                    if !symbols.is_empty() {
                                        println!("\n  {:?}:", kind);
                                        for sym in symbols {
                                            println!("    → {} ({:?})", sym.name, sym.kind);
                                        }
                                    }
                                }
                            }
                            
                            // Incoming dependencies (what depends on this symbol)
                            let dependents = indexer.graph.get_dependents(symbol_id);
                            if dependents.is_empty() {
                                println!("\nNo incoming dependencies");
                            } else {
                                println!("\nIncoming Dependencies (what depends on {}):", symbol);
                                for (kind, ids) in dependents {
                                    let symbols: Vec<_> = ids.into_iter()
                                        .filter_map(|id| indexer.get_symbol(id))
                                        .collect();
                                    if !symbols.is_empty() {
                                        println!("\n  {:?} by:", kind);
                                        for sym in symbols {
                                            println!("    ← {} ({:?})", sym.name, sym.kind);
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            println!("Symbol not found: {}", symbol);
                        }
                    }
                }
            }
        }
        
        Commands::McpTest { server_binary, tool: _, args: _ } => {
            use codanna::mcp::client::CodeIntelligenceClient;
            
            // Get server binary path (default to current executable)
            let server_path = server_binary.unwrap_or_else(|| {
                std::env::current_exe()
                    .expect("Failed to get current executable path")
            });
            
            // Run the test
            if let Err(e) = CodeIntelligenceClient::test_server(server_path).await {
                eprintln!("MCP test failed: {}", e);
                std::process::exit(1);
            }
        }
        
        Commands::Mcp { tool, args } => {
            // Embedded mode - use already loaded indexer directly
            let server = codanna::mcp::CodeIntelligenceServer::new(indexer);
            
            // Parse arguments if provided
            let arguments = if let Some(args_str) = args {
                match serde_json::from_str::<serde_json::Value>(&args_str) {
                    Ok(serde_json::Value::Object(map)) => Some(map),
                    Ok(_) => {
                        eprintln!("Error: Arguments must be a JSON object");
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("Error parsing arguments: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                None
            };
            
            // Call the tool directly
            use codanna::mcp::*;
            use rmcp::handler::server::tool::Parameters;
            
            let result = match tool.as_str() {
                "find_symbol" => {
                    let name = arguments.as_ref()
                        .and_then(|m| m.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: find_symbol requires 'name' parameter");
                            std::process::exit(1);
                        });
                    server.find_symbol(Parameters(FindSymbolRequest {
                        name: name.to_string()
                    })).await
                }
                "get_calls" => {
                    let function_name = arguments.as_ref()
                        .and_then(|m| m.get("function_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: get_calls requires 'function_name' parameter");
                            std::process::exit(1);
                        });
                    server.get_calls(Parameters(GetCallsRequest {
                        function_name: function_name.to_string()
                    })).await
                }
                "find_callers" => {
                    let function_name = arguments.as_ref()
                        .and_then(|m| m.get("function_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: find_callers requires 'function_name' parameter");
                            std::process::exit(1);
                        });
                    server.find_callers(Parameters(FindCallersRequest {
                        function_name: function_name.to_string()
                    })).await
                }
                "analyze_impact" => {
                    let symbol_name = arguments.as_ref()
                        .and_then(|m| m.get("symbol_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: analyze_impact requires 'symbol_name' parameter");
                            std::process::exit(1);
                        });
                    let max_depth = arguments.as_ref()
                        .and_then(|m| m.get("max_depth"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(3) as usize;
                    server.analyze_impact(Parameters(AnalyzeImpactRequest {
                        symbol_name: symbol_name.to_string(),
                        max_depth,
                    })).await
                }
                "get_index_info" => {
                    server.get_index_info().await
                }
                "search_symbols" => {
                    let query = arguments.as_ref()
                        .and_then(|m| m.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            eprintln!("Error: search_symbols requires 'query' parameter");
                            std::process::exit(1);
                        });
                    let limit = arguments.as_ref()
                        .and_then(|m| m.get("limit"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10) as usize;
                    let kind = arguments.as_ref()
                        .and_then(|m| m.get("kind"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let module = arguments.as_ref()
                        .and_then(|m| m.get("module"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    server.search_symbols(Parameters(SearchSymbolsRequest {
                        query: query.to_string(),
                        limit,
                        kind,
                        module,
                    })).await
                }
                _ => {
                    eprintln!("Unknown tool: {}", tool);
                    eprintln!("Available tools: find_symbol, get_calls, find_callers, analyze_impact, get_index_info, search_symbols");
                    std::process::exit(1);
                }
            };
            
            // Print result
            match result {
                Ok(call_result) => {
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
                Err(e) => {
                    eprintln!("Error calling tool: {}", e.message);
                    std::process::exit(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    
    #[test]
    fn verify_cli() {
        // This test ensures the CLI structure is valid
        Cli::command().debug_assert();
    }
}