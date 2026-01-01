//! Retrieve command - query symbol information from the index.

use crate::cli::RetrieveQuery;
use crate::indexing::facade::IndexFacade;
use crate::io::ExitCode;
use crate::io::OutputFormat;
use crate::retrieve;

/// Run the retrieve command.
pub fn run(query: RetrieveQuery, indexer: &IndexFacade) -> ExitCode {
    match query {
        RetrieveQuery::Symbol { args, json } => {
            use crate::io::args::parse_positional_args;

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
            retrieve::retrieve_symbol(indexer, &final_name, language, format)
        }
        RetrieveQuery::Callers { args, json } => {
            use crate::io::args::parse_positional_args;

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
            retrieve::retrieve_callers(indexer, &final_function, language, format)
        }
        RetrieveQuery::Calls { args, json } => {
            use crate::io::args::parse_positional_args;

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
            retrieve::retrieve_calls(indexer, &final_function, language, format)
        }
        RetrieveQuery::Implementations { args, json } => {
            use crate::io::args::parse_positional_args;

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
            retrieve::retrieve_implementations(indexer, &final_trait, language, format)
        }
        RetrieveQuery::Search {
            args,
            limit,
            json,
            kind,
            module,
        } => {
            use crate::io::args::parse_positional_args;

            // Parse positional arguments for query and key:value pairs
            let (positional_query, params) = parse_positional_args(&args);

            // Determine query source (priority: positional > key:value)
            let final_query = positional_query
                .or_else(|| params.get("query").cloned())
                .unwrap_or_else(|| {
                    eprintln!("Error: search requires a query");
                    eprintln!("Usage: codanna retrieve search \"query\" [options]");
                    eprintln!("   or: codanna retrieve search query:\"search text\" [options]");
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
                indexer,
                &final_query,
                final_limit,
                final_kind.as_deref(),
                final_module.as_deref(),
                language,
                format,
            )
        }
        RetrieveQuery::Describe { args, json } => {
            use crate::io::args::parse_positional_args;

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
            retrieve::retrieve_describe(indexer, &final_symbol, language, format)
        }
        RetrieveQuery::Uses { symbol } => {
            eprintln!("'retrieve uses' command not yet implemented for: {symbol}");
            ExitCode::GeneralError
        }
        RetrieveQuery::Defines { symbol } => {
            eprintln!("'retrieve defines' command not yet implemented for: {symbol}");
            ExitCode::GeneralError
        }
        RetrieveQuery::Dependencies { symbol } => {
            eprintln!("'retrieve dependencies' command not yet implemented for: {symbol}");
            ExitCode::GeneralError
        }
    }
}
