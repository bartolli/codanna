//! Documents management command.

use std::path::PathBuf;

use crate::cli::DocumentAction;
use crate::config::Settings;
use crate::documents::{CollectionConfig, DocumentStore, IndexProgress, SearchQuery};
use crate::io::status_line::StatusLine;
use crate::io::{ProgressBar, ProgressBarOptions, ProgressBarStyle};
use crate::vector::{FastEmbedGenerator, VectorDimension};

/// Run documents management command.
pub fn run(action: DocumentAction, config: &Settings, cli_config: Option<&PathBuf>) {
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
            no_progress,
        } => {
            // Progress enabled by default from settings, --no-progress overrides
            let show_progress = config.indexing.show_progress && !no_progress;
            run_index(
                config,
                collection,
                all,
                force,
                show_progress,
                create_store_with_embeddings,
            );
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
                                println!("   Context: {}", result.heading_context.join(" > "));
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
            run_add_collection(config, cli_config, name, path, pattern);
        }

        DocumentAction::RemoveCollection { name } => {
            run_remove_collection(config, cli_config, name);
        }
    }
}

fn run_index<F>(
    config: &Settings,
    collection: Option<String>,
    all: bool,
    force: bool,
    progress: bool,
    create_store_with_embeddings: F,
) where
    F: Fn() -> Result<DocumentStore, String>,
{
    use std::cell::RefCell;
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

    // Determine which collections to index
    let collections_to_index: Vec<(String, CollectionConfig)> = if let Some(name) = collection {
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

    // Progress state for two-phase display
    type ProgressState = Option<(Arc<ProgressBar>, StatusLine<Arc<ProgressBar>>)>;
    let phase1_bar: RefCell<ProgressState> = RefCell::new(None);
    let phase2_bar: RefCell<ProgressState> = RefCell::new(None);

    let mut total_files = 0usize;
    let mut total_chunks = 0usize;

    for (name, col_config) in collections_to_index {
        let chunking = col_config.effective_chunking(&config.documents.defaults);

        if !progress {
            eprintln!("Indexing collection: {name}");
        }

        // Index with two-phase progress callback
        let result = store.index_collection_with_progress(&name, &col_config, &chunking, |prog| {
            if !progress {
                return;
            }
            match prog {
                IndexProgress::ProcessingFile { current, total, .. } => {
                    let mut p1 = phase1_bar.borrow_mut();
                    if p1.is_none() && total > 0 {
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
        });

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

fn run_add_collection(
    _config: &Settings,
    cli_config: Option<&PathBuf>,
    name: String,
    path: PathBuf,
    pattern: Option<String>,
) {
    // Find config file
    let config_path = if let Some(custom_path) = cli_config {
        custom_path.clone()
    } else {
        Settings::find_workspace_config().unwrap_or_else(|| {
            eprintln!("Error: No configuration file found. Run 'codanna init' first.");
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

    // Check if path exists
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
        let collection_config = CollectionConfig {
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

fn run_remove_collection(_config: &Settings, cli_config: Option<&PathBuf>, name: String) {
    // Find config file
    let config_path = if let Some(custom_path) = cli_config {
        custom_path.clone()
    } else {
        Settings::find_workspace_config().unwrap_or_else(|| {
            eprintln!("Error: No configuration file found. Run 'codanna init' first.");
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
