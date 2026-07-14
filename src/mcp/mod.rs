//! MCP (Model Context Protocol) server implementation for code intelligence
//!
//! This module provides MCP tools that allow AI assistants to query
//! the code intelligence index.
//!
//! ## Architecture
//!
//! The MCP server can run in two modes:
//!
//! 1. **Standalone Server Mode**: Run with `cargo run -- serve`
//!    - Loads index once into memory
//!    - Listens for client connections via stdio
//!    - Efficient for production use with AI assistants
//!
//! 2. **Embedded Mode**: Used by the CLI directly
//!    - No separate process needed
//!    - Direct access to already-loaded index
//!    - Most memory efficient for CLI operations

pub mod client;
pub mod http_server;
pub mod https_server;
pub mod notifications;
pub mod proxy;
pub mod requests;
pub mod server;
pub mod service;
pub mod tools;

pub use proxy::{ProxyError, ProxyResult, serve_proxy};
pub use requests::*;
pub use server::{CodeIntelligenceServer, format_relative_time};

/// Bearer token accepted by the dev-mode auth middleware. Bare token — rmcp
/// `auth_header` adds the "Bearer " prefix itself.
pub(crate) const DUMMY_BEARER_TOKEN: &str = "mcp-access-token-dummy";
