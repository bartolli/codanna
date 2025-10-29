// Gateway file to expose parser tests from the parsers/ subdirectory
// This file allows Rust's test runner to discover tests in subdirectories

// Re-export the parser test modules
// Each test file in parsers/ needs to be included here

#[path = "parsers/typescript/test_resolution_pipeline.rs"]
mod test_typescript_resolution_pipeline;

#[path = "parsers/typescript/test_call_tracking.rs"]
mod test_typescript_call_tracking;

#[path = "parsers/typescript/test_nested_functions.rs"]
mod test_typescript_nested_functions;

#[path = "parsers/typescript/test_alias_resolution.rs"]
mod test_typescript_alias_resolution;

#[path = "parsers/c/test_resolution.rs"]
mod test_c_resolution;

#[path = "parsers/cpp/test_resolution.rs"]
mod test_cpp_resolution;

#[path = "parsers/python/test_module_level_calls.rs"]
mod test_python_module_level_calls;

#[path = "parsers/csharp/test_parser.rs"]
mod test_csharp_parser;

#[path = "parsers/gdscript/test_parser.rs"]
mod test_gdscript_parser;

#[path = "parsers/gdscript/test_resolution.rs"]
mod test_gdscript_resolution;
