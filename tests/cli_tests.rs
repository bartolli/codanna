// Gateway for CLI-related integration tests

#[path = "cli/test_plugin_commands.rs"]
mod test_plugin_commands;

#[path = "cli/test_mcp_index_info_remote_status.rs"]
mod test_mcp_index_info_remote_status;

#[path = "cli/test_mcp_exit_code_matrix.rs"]
mod test_mcp_exit_code_matrix;

#[path = "cli/test_mcp_line_convention.rs"]
mod test_mcp_line_convention;

#[path = "cli/test_mcp_call_metadata_matrix.rs"]
mod test_mcp_call_metadata_matrix;

#[path = "cli/test_emission_version_gate.rs"]
mod test_emission_version_gate;

#[path = "cli/test_file_path_portable.rs"]
mod test_file_path_portable;

#[path = "cli/test_index_seed_ownership.rs"]
mod test_index_seed_ownership;

#[path = "cli/test_serve_stdio_dual_generation.rs"]
mod test_serve_stdio_dual_generation;

#[path = "cli/test_serve_http_sessionless.rs"]
mod test_serve_http_sessionless;

#[path = "cli/test_mcp_test_client_generation.rs"]
mod test_mcp_test_client_generation;
