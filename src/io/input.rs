//! Input parsing for tool integration.
//!
//! Future support for JSON-RPC 2.0 to enable LSP-like integration
//! with IDEs and other tools.

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 Request for future tool integration.
///
/// Follows the JSON-RPC 2.0 specification for request messages.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version (must be "2.0")
    pub jsonrpc: String,
    /// Method name to invoke
    pub method: String,
    /// Method parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    /// Request ID for matching responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Response.
///
/// Follows the JSON-RPC 2.0 specification for response messages.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Protocol version (must be "2.0")
    pub jsonrpc: String,
    /// Successful result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request ID this response corresponds to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Error object.
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// Future implementation for parsing JSON-RPC requests from stdin
// This would enable codanna to be used as a backend for IDEs
//
// Example future usage:
// ```
// let request = JsonRpcRequest::from_stdin()?;
// let response = match request.method.as_str() {
//     "textDocument/symbols" => handle_symbols(request.params),
//     "textDocument/references" => handle_references(request.params),
//     _ => JsonRpcResponse::method_not_found(request.id),
// };
// response.write_to_stdout()?;
// ```
