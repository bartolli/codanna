use clap::{Parser, Subcommand};
use codebase_intelligence::{SimpleIndexer, SymbolKind, RelationKind, Settings, IndexPersistence};
use std::path::PathBuf;
use std::fs;

const INDEX_STATE_FILE: &str = ".codebase-intelligence-index";

#[derive(Parser)]
#[command(name = "codebase-intelligence")]
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
    
    /// Index a Rust source file
    Index {
        /// Path to the Rust file to index
        file: PathBuf,
        
        /// Number of threads to use (overrides config)
        #[arg(short, long)]
        threads: Option<usize>,
        
        /// Force re-indexing even if index exists
        #[arg(short, long)]
        force: bool,
    },
    
    /// Retrieve information from the index
    Retrieve {
        #[command(subcommand)]
        query: RetrieveQuery,
    },
    
    /// Show current configuration
    Config,
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

fn main() {
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
            let config_path = PathBuf::from(".code-intelligence/settings.toml");
            
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
        
        _ => {}
    }
    
    // Set up persistence based on config
    let index_path = config.index_path.clone();
    let persistence = IndexPersistence::new(index_path);
    
    // Load existing index or create new one
    let force_reindex = matches!(cli.command, Commands::Index { force: true, .. });
    let mut indexer = if persistence.exists() && !force_reindex {
        eprintln!("DEBUG: Found existing index at {}", config.index_path.display());
        match persistence.load() {
            Ok(loaded) => {
                eprintln!("DEBUG: Successfully loaded index from disk");
                eprintln!("Loaded existing index ({} symbols)", loaded.symbol_count());
                loaded
            }
            Err(e) => {
                eprintln!("Warning: Could not load index: {}. Creating new index.", e);
                SimpleIndexer::new()
            }
        }
    } else {
        if force_reindex && persistence.exists() {
            eprintln!("Force re-indexing requested, creating new index");
        } else if !persistence.exists() {
            eprintln!("DEBUG: No existing index found at {}", config.index_path.display());
        }
        eprintln!("DEBUG: Creating new index");
        SimpleIndexer::new()
    };
    
    match cli.command {
        Commands::Init { .. } | Commands::Config => {
            // Already handled above
            unreachable!()
        }
        
        Commands::Index { file, force: _, .. } => {
            match indexer.index_file(&file) {
                Ok(file_id) => {
                    // Save the indexed file path for retrieve commands
                    if let Err(e) = fs::write(INDEX_STATE_FILE, file.to_string_lossy().as_ref()) {
                        eprintln!("Warning: Could not save index state: {}", e);
                    }
                    
                    println!("Successfully indexed: {}", file.display());
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
                            println!("  {:?} at line {}", 
                                symbol.kind, 
                                symbol.range.start_line + 1
                            );
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
                                        println!("    - {}", sym.name);
                                    }
                                }
                            }
                        }
                        None => {
                            println!("Symbol not found: {}", symbol);
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