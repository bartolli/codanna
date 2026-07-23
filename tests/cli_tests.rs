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
