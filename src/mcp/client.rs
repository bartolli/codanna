//! MCP client for connecting to the code intelligence server

use anyhow::Result;
use std::path::PathBuf;

pub struct CodeIntelligenceClient;

impl CodeIntelligenceClient {
    /// Connect to MCP server and test it (thin client - no index loading)
    pub async fn test_server(server_binary: PathBuf) -> Result<()> {
        use rmcp::{
            model::CallToolRequestParam,
            service::ServiceExt,
            transport::{ConfigureCommandExt, TokioChildProcess},
        };
        use tokio::process::Command;
        
        println!("Starting MCP server process...");
        
        let client = ()
            .serve(
                TokioChildProcess::new(Command::new(&server_binary).configure(|cmd| {
                    cmd.arg("serve");
                }))?,
            )
            .await?;
        
        // Get server info
        let server_info = client.peer_info();
        println!("Connected to server: {:#?}", server_info);
        
        // List tools
        println!("\nListing available tools...");
        let tools = client.list_tools(Default::default()).await?;
        
        for tool in &tools.tools {
            println!("  - {}: {}", 
                tool.name, 
                tool.description.as_deref().unwrap_or("No description")
            );
        }
        
        // Try calling get_index_info tool
        println!("\nCalling get_index_info tool...");
        let result = client
            .call_tool(CallToolRequestParam {
                name: "get_index_info".into(),
                arguments: None,
            })
            .await?;
        
        println!("Result:");
        for content in &result.content {
            match &**content {
                rmcp::model::RawContent::Text(text) => {
                    println!("{}", text.text);
                }
                _ => println!("(Non-text content)"),
            }
        }
        
        // Shutdown
        println!("\nShutting down...");
        client.cancel().await?;
        
        Ok(())
    }
}