//! Index command - index source code files and directories.

use std::path::PathBuf;

use crate::cli::commands::directories::{SkipReason, add_paths_to_settings};
use crate::config::Settings;
use crate::indexing::facade::IndexFacade;
use crate::storage::IndexPersistence;
use crate::types::SymbolKind;

/// Arguments for the index command.
pub struct IndexArgs {
    pub paths: Vec<PathBuf>,
    pub force: bool,
    pub progress: bool,
    pub dry_run: bool,
    pub max_files: Option<usize>,
    pub cli_config: Option<PathBuf>,
}

/// Run the index command.
///
/// This command handles both file and directory indexing with options for
/// force re-indexing, progress display, dry-run mode, and file limits.
pub fn run(
    args: IndexArgs,
    config: &mut Settings,
    indexer: &mut IndexFacade,
    persistence: &IndexPersistence,
    sync_made_changes: Option<bool>,
) {
    let IndexArgs {
        paths,
        force,
        progress,
        dry_run,
        max_files,
        cli_config,
    } = args;

    // Determine paths to index
    let paths_to_index = if !paths.is_empty() {
        // CLI paths provided - add them to settings.toml first
        let config_path = if let Some(custom_path) = cli_config {
            custom_path
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
                            "{}: Included in indexed directory {}",
                            skipped.path.display(),
                            parent.display()
                        ),
                        SkipReason::AlreadyPresent => {
                            println!("{}: Already indexed", skipped.path.display())
                        }
                        SkipReason::FileNotPersisted => println!(
                            "{}: Ad-hoc indexed (not in settings.toml)",
                            skipped.path.display()
                        ),
                    }
                }
                // Update config with the new settings
                *config = updated_settings;
                paths
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
                Some(true) => {
                    // Sync added new directories, already indexed - save and return
                    if let Err(e) = persistence.save_facade(indexer) {
                        eprintln!("Error saving index: {e}");
                        std::process::exit(1);
                    }
                    return;
                }
                Some(false) | None => {
                    // No directory changes - check file-level changes via incremental
                    tracing::debug!(target: "indexing", "checking {} paths for file-level changes", config_paths.len());
                }
            }
        }

        // Run incremental (force=false) or full reindex (force=true)
        config_paths
    };

    // Process each path, tracking total changes
    let mut total_indexed = 0usize;
    for path in &paths_to_index {
        if path.is_file() {
            if index_single_file(indexer, path, force) {
                total_indexed += 1;
            }
        } else if path.is_dir() {
            total_indexed += index_directory(indexer, path, progress, dry_run, force, max_files);
        } else {
            eprintln!("Error: Path does not exist: {}", path.display());
            std::process::exit(1);
        }
    }

    // Only save if changes were made and not in dry-run mode
    if !dry_run && total_indexed > 0 {
        save_index(indexer, persistence, config);
    } else if !dry_run && total_indexed == 0 {
        tracing::debug!(target: "indexing", "no changes detected, skipping save");
    }
}

/// Index a single file. Returns true if file was indexed (not cached).
fn index_single_file(indexer: &mut IndexFacade, path: &PathBuf, force: bool) -> bool {
    match indexer.index_file_with_force(path, force) {
        Ok(result) => {
            let language_name = path
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| {
                    let registry = crate::parsing::get_registry();
                    registry
                        .lock()
                        .ok()
                        .and_then(|r| r.get_by_extension(ext).map(|def| def.name().to_string()))
                })
                .unwrap_or_else(|| "unknown".to_string());

            let was_indexed = !result.is_cached();

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

            was_indexed
        }
        Err(e) => {
            eprintln!("Error indexing file {}: {e}", path.display());

            let suggestions = e.recovery_suggestions();
            if !suggestions.is_empty() {
                eprintln!("\nSuggestions:");
                for suggestion in suggestions {
                    eprintln!("  - {suggestion}");
                }
            }

            std::process::exit(1);
        }
    }
}

/// Index a directory. Returns the number of files indexed.
fn index_directory(
    indexer: &mut IndexFacade,
    path: &PathBuf,
    progress: bool,
    dry_run: bool,
    force: bool,
    max_files: Option<usize>,
) -> usize {
    // Visual separator between directory cycles (use stderr to sync with progress bars)
    eprintln!();

    // Show pre-indexing message only if we have a file limit (implies actual work)
    if let Some(max) = max_files {
        eprintln!(
            "Indexing directory: {} (limited to {} files)",
            path.display(),
            max
        );
    }

    // Track this directory as indexed
    indexer.add_indexed_path(path);

    match indexer.index_directory_with_options(path, progress, dry_run, force, max_files) {
        Ok(stats) => {
            // Print message only when no files need indexing (pipeline trace handles the rest)
            if stats.files_indexed == 0 {
                eprintln!("Index up to date: {}", path.display());
            }
            stats.files_indexed
        }
        Err(e) => {
            eprintln!("Error indexing directory {}: {e}", path.display());

            let suggestions = e.recovery_suggestions();
            if !suggestions.is_empty() {
                eprintln!("\nSuggestions:");
                for suggestion in suggestions {
                    eprintln!("  - {suggestion}");
                }
            }

            std::process::exit(1);
        }
    }
}

fn save_index(indexer: &mut IndexFacade, persistence: &IndexPersistence, config: &Settings) {
    // Save the index
    eprintln!(
        "\nSaving index with {} total symbols, {} total relationships...",
        indexer.symbol_count(),
        indexer.relationship_count()
    );
    match persistence.save_facade(indexer) {
        Ok(_) => {
            println!("Index saved to: {}", config.index_path.display());
        }
        Err(e) => {
            eprintln!("Error: Could not save index: {e}");
            std::process::exit(1);
        }
    }
}
