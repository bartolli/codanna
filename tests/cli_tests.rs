// Gateway for CLI-related integration tests

#[path = "cli/support.rs"]
mod support;

#[path = "cli/test_plugin_commands.rs"]
mod test_plugin_commands;

#[path = "cli/test_mcp_index_info_remote_status.rs"]
mod test_mcp_index_info_remote_status;

#[path = "cli/test_serve_proxy_discovery.rs"]
mod test_serve_proxy_discovery;
