//! Degraded MCP server for stale-index refusal.

use rmcp::ServerHandler;
use rmcp::model::*;

/// Served when the emission-semantics gate refuses the index in stdio
/// serve mode. Client-spawned servers lose stderr, so a pre-handshake
/// exit surfaces as an opaque connection failure; completing the
/// handshake with zero tools keeps the refusal fail-closed while the
/// `instructions` field carries the heal command to the client.
#[derive(Clone)]
pub struct StaleIndexServer {
    instructions: String,
}

impl StaleIndexServer {
    pub fn new(stored: Option<u32>, current: u32) -> Self {
        let stored_txt = stored.map_or_else(|| "none".to_string(), |v| format!("v{v}"));
        Self {
            instructions: format!(
                "INDEX STALE - ALL TOOLS DISABLED. Index emission semantics changed \
                 (index: {stored_txt}, binary: v{current}); reading it would mix stale \
                 and current rows. Tell the user to run 'codanna index' in this \
                 workspace to rebuild, then restart this MCP server. Workspaces with \
                 semantic search enabled re-embed during the rebuild; large \
                 repositories take minutes."
            ),
        }
    }

    pub fn instructions(&self) -> &str {
        &self.instructions
    }
}

impl ServerHandler for StaleIndexServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("codanna", env!("CARGO_PKG_VERSION"))
                    .with_title("Codanna Code Intelligence (stale index)")
                    .with_website_url("https://github.com/bartolli/codanna"),
            )
            .with_instructions(self.instructions.clone())
    }
}
