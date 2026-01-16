//! CLI entry point for the codebase intelligence system.
//!
//! Provides commands for indexing, querying, and serving code intelligence data.
//! Uses the cli module for argument parsing and command definitions.

use clap::Parser;
use codanna::cli::{Cli, Commands, RetrieveQuery};
use codanna::indexing::facade::IndexFacade;
use codanna::project_resolver::{
    providers::{
        go::GoProvider, java::JavaProvider, javascript::JavaScriptProvider, python::PythonProvider,
        swift::SwiftProvider, typescript::TypeScriptProvider,
    },
    registry::SimpleProviderRegistry,
};
use codanna::storage::IndexMetadata;
use codanna::{IndexPersistence, Settings};
use std::path::PathBuf;
use std::sync::Arc;

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

    // Add Go provider for go.mod resolution
    registry.add(Arc::new(GoProvider::new()));

    // Add Python provider for pyproject.toml resolution
    registry.add(Arc::new(PythonProvider::new()));

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

#[derive(Default)]
struct SeedReport {
    newly_seeded: Vec<PathBuf>,
    missing_paths: Vec<PathBuf>,
}

fn seed_indexer_with_config_paths(
    indexer: &mut IndexFacade,
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
        indexer.add_indexed_path(path);
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

    report
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
    // All logging goes to stderr to avoid polluting stdout (JSON output, piping)
    codanna::logging::init_with_config(&config.logging);

    // Determine resource requirements based on command type
    // Commands are categorized by what infrastructure they need:
    // - Thin: No index, no providers (Parse, McpTest, Benchmark)
    // - Config-only: Settings but no index (Init, Config, AddDir, RemoveDir, ListDirs, Plugin, Profile, Documents)
    // - Full: Index + providers (Retrieve, Mcp, Serve, Index)
    let needs_providers = !matches!(
        &cli.command,
        Commands::Parse { .. } | Commands::McpTest { .. } | Commands::Benchmark { .. }
    );

    let needs_indexer = !matches!(
        &cli.command,
        Commands::Init { .. }
            | Commands::Config
            | Commands::Parse { .. }
            | Commands::McpTest { .. }
            | Commands::Benchmark { .. }
            | Commands::AddDir { .. }
            | Commands::RemoveDir { .. }
            | Commands::ListDirs
            | Commands::Plugin { .. }
            | Commands::Documents { .. }
            | Commands::Profile { .. }
            | Commands::IndexParallel { .. }
    );

    // Initialize project resolution providers (only if needed)
    // This ensures caches are built before indexing starts
    if needs_providers {
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
    }

    // Apply config overrides from CLI args
    if let Commands::Index {
        threads: Some(t), ..
    } = &cli.command
    {
        config.indexing.parallelism = *t;
    }

    // Set up persistence based on config
    // Use global path resolution that handles --config properly
    let index_path = codanna::init::resolve_index_path(&config, cli.config.as_deref());

    // Update the config with the resolved index_path so SimpleIndexer uses the correct path
    config.index_path = index_path.clone();

    let persistence = IndexPersistence::new(index_path.clone());

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

    // Determine if we need semantic search (ML model loading)
    // Retrieve commands use Tantivy text search only - no ML model needed
    let needs_semantic_search = match &cli.command {
        Commands::Mcp { tool, .. } => {
            // Only these MCP tools need semantic search
            ["semantic_search_docs", "semantic_search_with_context"].contains(&tool.as_str())
        }
        Commands::Index { .. } | Commands::Serve { .. } => true,
        _ => false,
    };

    // Load existing index or create new one (only if command needs it)
    let settings = Arc::new(config.clone());
    let mut indexer: Option<IndexFacade> = if !needs_indexer {
        None
    } else {
        Some({
            // Force flag always means fresh index, regardless of path source (CLI or settings.toml)
            let force_recreate_index = matches!(cli.command, Commands::Index { force: true, .. });
            if persistence.exists() && !force_recreate_index {
                tracing::debug!(target: "cli", "found existing index at {}", config.index_path.display());
                // Use lazy loading for simple commands to improve startup time
                let skip_trait_resolver = !needs_trait_resolver;
                if skip_trait_resolver {
                    tracing::debug!(target: "cli", "using lazy initialization (skipping trait resolver)");
                }

                // Use lite loading for commands that don't need semantic search
                let load_result = if needs_semantic_search {
                    persistence.load_facade(settings.clone())
                } else {
                    tracing::debug!(target: "cli", "using lite loading (skipping semantic search)");
                    persistence.load_facade_lite(settings.clone())
                };

                match load_result {
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
                        IndexFacade::new(settings.clone()).expect("Failed to create IndexFacade")
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
                IndexFacade::new(settings.clone()).expect("Failed to create IndexFacade")
            }
        })
    };

    // Enable semantic search if configured
    let seed_report = if let Some(ref mut idx) = indexer {
        Some(seed_indexer_with_config_paths(
            idx,
            &config.indexing.indexed_paths,
        ))
    } else {
        None
    };

    if let Some(ref mut idx) = indexer {
        // Only enable semantic search for commands that need it
        if needs_semantic_search && config.semantic_search.enabled && !idx.has_semantic_search() {
            if let Err(e) = idx.enable_semantic_search() {
                eprintln!("Warning: Failed to enable semantic search: {e}");
            } else {
                eprintln!(
                    "Semantic search enabled (model: {}, threshold: {})",
                    config.semantic_search.model, config.semantic_search.threshold
                );
            }
        }
    }

    // Sync indexed paths with config - auto-index new directories
    // This handles changes made while the index was not in use (e.g., add-dir command)
    // Skip sync if force flag is present (force means fresh start, not incremental)
    let is_force_index = matches!(cli.command, Commands::Index { force: true, .. });

    // Progress is enabled by default from settings, can be disabled with --no-progress
    let no_progress_flag = matches!(
        cli.command,
        Commands::Index {
            no_progress: true,
            ..
        }
    );
    let show_progress = config.indexing.show_progress && !no_progress_flag;
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

    if let Some(ref mut idx) = indexer {
        if persistence.exists() && !is_force_index {
            // Load stored indexed_paths from metadata
            match IndexMetadata::load(&config.index_path) {
                Ok(metadata) => {
                    let stored_paths = metadata.indexed_paths.clone();

                    // Sync with current config (settings.toml is source of truth)
                    match idx.sync_with_config(
                        stored_paths,
                        &config.indexing.indexed_paths,
                        show_progress,
                    ) {
                        Ok(stats) => {
                            if stats.has_changes() {
                                sync_made_changes = Some(true);
                                if stats.added_dirs > 0 {
                                    tracing::info!(
                                        target: "sync",
                                        "indexed {} directories ({} files, {} symbols)",
                                        stats.added_dirs, stats.files_indexed, stats.symbols_found
                                    );
                                }
                                if stats.removed_dirs > 0 {
                                    tracing::info!(
                                        target: "sync",
                                        "removed {} directories from index",
                                        stats.removed_dirs
                                    );
                                }
                                if stats.files_modified > 0 || stats.files_added > 0 {
                                    tracing::info!(
                                        target: "sync",
                                        "synced {} modified, {} new files",
                                        stats.files_modified, stats.files_added
                                    );
                                }

                                // Save updated index
                                if let Err(e) = persistence.save_facade(idx) {
                                    tracing::warn!(target: "sync", "failed to save updated index: {e}");
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
    }

    match cli.command {
        Commands::Init { force } => {
            codanna::cli::commands::init::run_init(force);
        }

        Commands::Config => {
            codanna::cli::commands::init::run_config(&config);
        }

        Commands::Parse {
            file,
            output,
            max_depth,
            all_nodes,
        } => {
            codanna::cli::commands::parse::run(&file, output, max_depth, all_nodes);
        }

        Commands::McpTest {
            server_binary,
            tool,
            args,
            delay,
        } => {
            use codanna::mcp::client::CodeIntelligenceClient;

            let server_path = server_binary.unwrap_or_else(|| {
                std::env::current_exe().expect("Failed to get current executable path")
            });

            if let Err(e) = CodeIntelligenceClient::test_server(
                server_path,
                cli.config.clone(),
                tool,
                args,
                delay,
            )
            .await
            {
                eprintln!("MCP test failed: {e}");
                std::process::exit(1);
            }
        }

        Commands::Serve {
            watch,
            watch_interval,
            http,
            https,
            bind,
        } => {
            use codanna::cli::commands::serve::{ServeArgs, run as run_serve};
            run_serve(
                ServeArgs {
                    watch,
                    watch_interval,
                    http,
                    https,
                    bind,
                },
                config,
                settings,
                indexer.expect("serve requires indexer"),
                index_path,
            )
            .await;
        }

        Commands::Index {
            paths,
            force,
            no_progress,
            dry_run,
            max_files,
            ..
        } => {
            use codanna::cli::commands::index::{IndexArgs, run as run_index};
            // Progress enabled by default from settings, --no-progress overrides
            let progress = config.indexing.show_progress && !no_progress;
            run_index(
                IndexArgs {
                    paths,
                    force,
                    progress,
                    dry_run,
                    max_files,
                    cli_config: cli.config.clone(),
                },
                &mut config,
                indexer.as_mut().expect("index requires indexer"),
                &persistence,
                sync_made_changes,
            );
        }

        Commands::AddDir { path } => {
            codanna::cli::commands::directories::run_add_dir(path, cli.config.as_deref());
        }

        Commands::RemoveDir { path } => {
            codanna::cli::commands::directories::run_remove_dir(path, cli.config.as_deref());
        }

        Commands::ListDirs => {
            codanna::cli::commands::directories::run_list_dirs(&config);
        }

        Commands::Retrieve { query } => {
            let exit_code = codanna::cli::commands::retrieve::run(
                query,
                indexer.as_ref().expect("retrieve requires indexer"),
            );
            std::process::exit(exit_code as i32);
        }

        Commands::Mcp {
            tool,
            positional,
            args,
            json,
            fields,
            watch,
        } => {
            let mut indexer = indexer.expect("mcp requires indexer");

            // If --watch is enabled, check for file changes and reindex
            if watch {
                let paths = config.get_indexed_paths();
                if !paths.is_empty() {
                    let mut total_indexed = 0usize;
                    for path in &paths {
                        if path.is_dir() {
                            // Run incremental indexing (force=false)
                            match indexer.index_directory_with_options(
                                path, false, // no progress bars for watch mode
                                false, // not dry run
                                false, // not force (incremental)
                                None,  // no max_files limit
                            ) {
                                Ok(stats) => total_indexed += stats.files_indexed,
                                Err(e) => {
                                    tracing::warn!(target: "mcp", "watch reindex failed for {}: {e}", path.display());
                                }
                            }
                        }
                    }
                    // Only save if changes were made
                    if total_indexed > 0 {
                        if let Err(e) = persistence.save_facade(&indexer) {
                            tracing::warn!(target: "mcp", "failed to save index after watch reindex: {e}");
                        }
                    }
                }
            }

            codanna::cli::commands::mcp::run(
                tool, positional, args, json, fields, indexer, &config,
            )
            .await;
        }

        Commands::Benchmark { language, file } => {
            codanna::cli::commands::benchmark::run(&language, file);
        }

        Commands::Plugin { action } => {
            codanna::cli::commands::plugin::run(action, &config);
        }

        Commands::Documents { action } => {
            codanna::cli::commands::documents::run(action, &config, cli.config.as_ref());
        }

        Commands::Profile { action } => {
            codanna::cli::commands::profile::run(action);
        }

        Commands::IndexParallel {
            paths,
            force,
            no_progress,
        } => {
            use codanna::cli::commands::index_parallel::{
                IndexParallelArgs, run as run_index_parallel,
            };
            // Progress enabled by default from settings, --no-progress overrides
            let progress = config.indexing.show_progress && !no_progress;
            run_index_parallel(
                IndexParallelArgs {
                    paths,
                    force,
                    progress,
                },
                &config,
            );
        }
    }
}

#[cfg(test)]
mod seed_indexer_tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

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
        let mut indexer =
            IndexFacade::new(Arc::new(settings)).expect("Failed to create IndexFacade");
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
        let mut indexer = IndexFacade::new(settings).expect("Failed to create IndexFacade");

        let report = seed_indexer_with_config_paths(&mut indexer, std::slice::from_ref(&missing));
        assert!(
            report.newly_seeded.is_empty(),
            "missing directory should not be seeded"
        );
        assert_eq!(report.missing_paths.len(), 1);
        assert_eq!(report.missing_paths[0], missing);
    }
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
