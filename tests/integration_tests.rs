// Gateway file to expose integration tests from the integration/ subdirectory
// This file allows Rust's test runner to discover tests in subdirectories

// Re-export the integration test modules
// Each test file in integration/ needs to be included here

#[path = "integration/test_mcp_schema.rs"]
mod test_mcp_schema;

#[path = "integration/embedding_model_comparison.rs"]
mod embedding_model_comparison;

#[path = "integration/test_resolution_persistence.rs"]
mod test_resolution_persistence;

#[path = "integration/test_init_module.rs"]
mod test_init_module;

#[path = "integration/test_parse_command.rs"]
mod test_parse_command;

#[path = "integration/test_settings_init_integration.rs"]
mod test_settings_init_integration;

#[path = "integration/test_project_registry.rs"]
mod test_project_registry;

#[path = "integration/test_config_path_resolution.rs"]
mod test_config_path_resolution;

#[path = "integration/test_cross_module_resolution.rs"]
mod test_cross_module_resolution;

#[path = "integration/test_python_cross_module_resolution.rs"]
mod test_python_cross_module_resolution;

#[path = "integration/test_provider_initialization.rs"]
mod test_provider_initialization;

#[path = "integration/test_typescript_alias_relationships.rs"]
mod test_typescript_alias_relationships;

#[path = "integration/test_typescript_object_property_call.rs"]
mod test_typescript_object_property_call;

#[path = "integration/test_external_import_resolution.rs"]
mod test_external_import_resolution;

#[path = "integration/test_gdscript_mcp.rs"]
mod test_gdscript_mcp;
