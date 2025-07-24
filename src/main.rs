use clap::{Parser, Subcommand};
use codebase_intelligence::{SimpleIndexer, SymbolKind};
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
    /// Index a Rust source file
    Index {
        /// Path to the Rust file to index
        file: PathBuf,
    },
    
    /// Retrieve information from the index
    Retrieve {
        #[command(subcommand)]
        query: RetrieveQuery,
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
}

fn main() {
    let cli = Cli::parse();
    
    // Create a persistent indexer (in real app, we'd save/load this)
    let mut indexer = SimpleIndexer::new();
    
    // For retrieve commands, re-index the last indexed file
    if matches!(cli.command, Commands::Retrieve { .. }) {
        if let Ok(last_file) = fs::read_to_string(INDEX_STATE_FILE) {
            let last_file = last_file.trim();
            if !last_file.is_empty() {
                match indexer.index_file(last_file) {
                    Ok(_) => {
                        eprintln!("Re-indexed: {}", last_file);
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not re-index {}: {}", last_file, e);
                    }
                }
            }
        } else {
            eprintln!("No index found. Please run 'index' command first.");
            std::process::exit(1);
        }
    }
    
    match cli.command {
        Commands::Index { file } => {
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