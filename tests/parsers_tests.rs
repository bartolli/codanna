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

#[path = "parsers/typescript/test_jsx_uses.rs"]
mod test_typescript_jsx_uses;

#[path = "parsers/javascript/test_nested_functions.rs"]
mod test_javascript_nested_functions;

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

#[path = "parsers/gdscript/test_behavior_api.rs"]
mod test_gdscript_behavior_api;

#[path = "parsers/gdscript/test_import_extraction.rs"]
mod test_gdscript_import_extraction;

#[path = "parsers/gdscript/test_relationships.rs"]
mod test_gdscript_relationships;

#[path = "parsers/kotlin/test_type_usage.rs"]
mod test_kotlin_type_usage;

#[path = "parsers/kotlin/test_method_definitions.rs"]
mod test_kotlin_method_definitions;

#[path = "parsers/kotlin/test_integration.rs"]
mod test_kotlin_integration;

#[path = "parsers/kotlin/test_interfaces_and_enums.rs"]
mod test_kotlin_interfaces_and_enums;

#[path = "parsers/kotlin/test_nested_scopes.rs"]
mod test_kotlin_nested_scopes;

#[path = "parsers/kotlin/test_extension_calls.rs"]
mod test_kotlin_extension_calls;

#[path = "parsers/kotlin/test_extension_resolution.rs"]
mod test_kotlin_extension_resolution;

#[path = "parsers/kotlin/test_generic_flow.rs"]
mod test_kotlin_generic_flow;

#[path = "parsers/kotlin/test_reddit_challenge.rs"]
mod test_kotlin_reddit_challenge;

#[path = "parsers/kotlin/test_visibility.rs"]
mod test_kotlin_visibility;

#[path = "parsers/swift/test_relationships.rs"]
mod test_swift_relationships;

#[path = "parsers/swift/debug_relationships.rs"]
mod debug_swift_relationships;

#[path = "parsers/swift/test_visibility.rs"]
mod test_swift_visibility;

#[path = "parsers/swift/test_error_recovery.rs"]
mod test_swift_error_recovery;

#[path = "parsers/typescript/test_error_recovery.rs"]
mod test_typescript_error_recovery;

#[path = "parsers/kotlin/test_value_class.rs"]
mod test_kotlin_value_class;

#[path = "parsers/php/test_readonly_class.rs"]
mod test_php_readonly_class;

#[path = "parsers/kotlin/test_context_receiver.rs"]
mod test_kotlin_context_receiver;

#[path = "parsers/swift/test_nested_types.rs"]
mod test_swift_nested_types;
