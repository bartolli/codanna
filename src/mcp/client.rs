//! MCP client for connecting to the code intelligence server

use anyhow::{Result, anyhow};
use serde_json::Value;
use std::path::PathBuf;

/// Spec MUST (2026-07-28 back-compat): a result whose `resultType` is
/// absent on the wire is treated as `"complete"`.
pub fn effective_result_type(result: &rmcp::model::CallToolResult) -> rmcp::model::ResultType {
    result
        .result_type
        .clone()
        .unwrap_or(rmcp::model::ResultType::COMPLETE)
}

pub struct CodeIntelligenceClient;

impl CodeIntelligenceClient {
    /// Connect to MCP server and test it (thin client - no index loading)
    pub async fn test_server(
        server_binary: PathBuf,
        config_path: Option<PathBuf>,
        tool: Option<String>,
        args: Option<String>,
        delay_before_tool_secs: Option<u64>,
    ) -> Result<()> {
        use rmcp::{
            ClientLifecycleMode, ClientServiceExt,
            model::{
                CallToolRequestParams, ClientRequest, CustomRequest, JsonObject, ProtocolVersion,
            },
            transport::{ConfigureCommandExt, TokioChildProcess},
        };
        use tokio::process::Command;
        use tokio::time::{Duration, sleep};

        println!("Starting MCP server process...");

        // Discover-only: the legacy fallback fires on METHOD_NOT_FOUND,
        // but every shipped codanna server that would need it exits on
        // the pre-handshake probe instead of answering — the fallback's
        // population is empty, so carrying it would claim compatibility
        // that does not exist.
        let client = ()
            .serve_with_lifecycle(
                TokioChildProcess::new(Command::new(&server_binary).configure(|cmd| {
                    if let Some(cfg) = &config_path {
                        cmd.arg("--config");
                        cmd.arg(cfg);
                    }

                    cmd.arg("serve");
                }))?,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .map_err(|e| {
                // Probe death has two witnessed shapes: send-side
                // ("Broken pipe ... when send discover request") and
                // response-side ("connection closed: discover
                // response"); both name the discover step.
                if e.to_string().contains("discover") {
                    anyhow!(
                        "{e}\nnote: codanna servers without 2026-07-28 support (shipped \
                         releases up to 0.12.0) exit on the pre-handshake server/discover \
                         probe; run that binary's own mcp-test instead"
                    )
                } else {
                    anyhow!(e)
                }
            })?;

        // Get server info
        let server_info = client.peer_info();
        println!("Connected to server: {server_info:#?}");

        // List tools
        println!("\nListing available tools...");
        let tools = client.list_tools(Default::default()).await?;

        for tool in &tools.tools {
            println!(
                "  - {}: {}",
                tool.name,
                tool.description.as_deref().unwrap_or("No description")
            );
        }

        // Always call get_index_info first to verify semantic availability
        println!("\nCalling get_index_info tool...");
        let get_info_result = client
            .call_tool(CallToolRequestParams::new("get_index_info"))
            .await?;
        Self::print_tool_output(&get_info_result)?;

        // Optionally call a specific tool supplied by the user
        if let Some(tool_name) = tool {
            if let Some(delay) = delay_before_tool_secs {
                if delay > 0 {
                    println!("\nWaiting {delay} seconds before calling '{tool_name}'...");
                    sleep(Duration::from_secs(delay)).await;
                }
            }

            println!("\nCalling tool '{tool_name}'...");

            let parsed_args: Option<JsonObject> = if let Some(raw) = args.as_ref() {
                let value: Value = serde_json::from_str(raw)
                    .map_err(|e| anyhow!("Failed to parse --args as JSON object: {e}"))?;

                match value {
                    Value::Object(map) => Some(map),
                    _ => {
                        return Err(anyhow!(
                            "Tool arguments must be a JSON object (e.g. {{\"query\":\"test\"}})"
                        ));
                    }
                }
            } else {
                None
            };

            let mut call_params = CallToolRequestParams::new(tool_name);
            if let Some(args) = parsed_args {
                call_params = call_params.with_arguments(args);
            }
            let tool_result = client.call_tool(call_params).await?;
            Self::print_tool_output(&tool_result)?;
        }

        // Test custom requests
        println!("\n--- Testing Custom Requests ---");

        // Test index-stats custom request
        println!("\nSending custom request: requests/codanna/index-stats");
        let stats_request =
            ClientRequest::CustomRequest(CustomRequest::new("requests/codanna/index-stats", None));
        match client.peer().send_request(stats_request).await {
            Ok(rmcp::model::ServerResult::CustomResult(custom)) => {
                println!("Response: {}", serde_json::to_string_pretty(&custom.0)?);
            }
            Ok(other) => println!("Unexpected response type: {other:?}"),
            Err(e) => println!("Request failed: {e}"),
        }

        // Test force-reindex custom request (with a small path)
        println!("\nSending custom request: requests/codanna/force-reindex");
        let reindex_request = ClientRequest::CustomRequest(CustomRequest::new(
            "requests/codanna/force-reindex",
            Some(serde_json::json!({"paths": ["src/mcp/client.rs"]})),
        ));
        match client.peer().send_request(reindex_request).await {
            Ok(rmcp::model::ServerResult::CustomResult(custom)) => {
                println!("Response: {}", serde_json::to_string_pretty(&custom.0)?);
            }
            Ok(other) => println!("Unexpected response type: {other:?}"),
            Err(e) => println!("Request failed: {e}"),
        }

        println!("\n--- Custom Request Tests Complete ---");

        // Shutdown
        println!("\nShutting down...");
        client.cancel().await?;

        Ok(())
    }

    fn print_tool_output(result: &rmcp::model::CallToolResult) -> Result<()> {
        let result_type = effective_result_type(result);
        if !result_type.is_complete() {
            return Err(anyhow!(
                "unsupported tool resultType \"{}\": this client renders \"complete\" results only",
                result_type.as_str()
            ));
        }

        println!("Result:");
        for content in &result.content {
            match content {
                rmcp::model::ContentBlock::Text(text) => println!("{}", text.text),
                _ => println!("(Non-text content)"),
            }
        }

        if result.is_error.unwrap_or(false) {
            println!("Tool returned an error status");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolResult, ResultType};

    #[test]
    fn absent_result_type_is_complete() {
        let legacy: CallToolResult =
            serde_json::from_str(r#"{"content":[{"type":"text","text":"hi"}]}"#)
                .expect("legacy wire shape deserializes");
        assert!(legacy.result_type.is_none(), "fixture omits resultType");
        assert!(effective_result_type(&legacy).is_complete());
    }

    #[test]
    fn explicit_complete_result_renders() {
        let complete: CallToolResult = serde_json::from_str(
            r#"{"resultType":"complete","content":[{"type":"text","text":"hi"}]}"#,
        )
        .expect("stateless wire shape deserializes");
        assert!(
            CodeIntelligenceClient::print_tool_output(&complete).is_ok(),
            "an explicit complete result renders"
        );
    }

    #[test]
    fn noncomplete_result_type_is_refused_at_parse() {
        let err = serde_json::from_str::<CallToolResult>(r#"{"resultType":"task","content":[]}"#)
            .expect_err("a non-complete wire result never becomes renderable output");
        assert!(
            err.to_string().contains("resultType"),
            "parse refusal names the contract field: {err}"
        );
    }

    #[test]
    fn noncomplete_result_type_is_refused_by_name() {
        let mut task = CallToolResult::success(vec![]);
        task.result_type = Some(ResultType::TASK);
        let err = CodeIntelligenceClient::print_tool_output(&task)
            .expect_err("a task result is not renderable tool output");
        assert!(
            err.to_string().contains("task"),
            "error names the resultType: {err}"
        );
    }
}
