//! Test MCP semantic search tool persistence

use codanna::{SimpleIndexer, IndexPersistence, Settings};
use codanna::mcp::{CodeIntelligenceServer, SemanticSearchRequest};
use tempfile::TempDir;
use std::sync::Arc;
use rmcp::handler::server::tool::Parameters;

#[tokio::test]
async fn test_mcp_semantic_search_after_reload() {
    let temp_dir = TempDir::new().unwrap();
    let index_path = temp_dir.path().to_path_buf();
    
    // Phase 1: Create and save index with semantic search
    {
        let settings = Arc::new(Settings {
            index_path: index_path.clone(),
            ..Settings::default()
        });
        
        let mut indexer = SimpleIndexer::with_settings(settings);
        indexer.enable_semantic_search().unwrap();
        
        // Create test files
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        
        std::fs::write(src_dir.join("parser.rs"), r#"
/// Parse configuration from a TOML file
/// 
/// This function reads a TOML configuration file and parses it
/// into a strongly-typed Config structure.
pub fn parse_config(path: &Path) -> Result<Config, Error> {
    let content = std::fs::read_to_string(path)?;
    toml::from_str(&content)
}

/// Parse command line arguments
/// 
/// Uses clap to parse command line arguments into an Args struct
pub fn parse_args() -> Args {
    Args::parse()
}
"#).unwrap();

        std::fs::write(src_dir.join("network.rs"), r#"
/// Connect to a remote server
/// 
/// Establishes a TCP connection to the specified host and port
pub async fn connect_to_server(host: &str, port: u16) -> Result<Connection, NetworkError> {
    TcpStream::connect((host, port)).await
}
"#).unwrap();
        
        // Index files
        indexer.index_directory(&src_dir, false, false).unwrap();
        
        // Verify semantic search works before save
        let results = indexer.semantic_search_docs("parse configuration", 5).unwrap();
        assert!(!results.is_empty());
        
        // Save the index
        let persistence = IndexPersistence::new(index_path.clone());
        persistence.save(&indexer).unwrap();
    }
    
    // Phase 2: Load index and test MCP tool
    {
        let settings = Arc::new(Settings {
            index_path: index_path.clone(),
            ..Settings::default()
        });
        
        let persistence = IndexPersistence::new(index_path.clone());
        let loaded_indexer = persistence.load_with_settings(settings).unwrap();
        
        // Verify semantic search was loaded
        assert!(loaded_indexer.has_semantic_search(), "Semantic search should be loaded");
        
        // Create MCP server with loaded indexer
        let server = CodeIntelligenceServer::new(loaded_indexer);
        
        // Test semantic search through MCP tool
        let request = SemanticSearchRequest {
            query: "parse configuration from file".to_string(),
            limit: 5,
            threshold: None,
        };
        
        let result = server.semantic_search_docs(Parameters(request)).await.unwrap();
        
        // Verify we got results
        assert!(!result.content.is_empty(), "Should have search results");
        
        // Check the content is meaningful
        let text_content = result.content[0].as_text().unwrap();
        assert!(text_content.text.contains("parse_config"), "Should find the parse_config function");
        assert!(text_content.text.contains("TOML"), "Should include TOML in the documentation");
        
        // Test with threshold
        let request_with_threshold = SemanticSearchRequest {
            query: "TCP connection".to_string(),
            limit: 10,
            threshold: Some(0.6),
        };
        
        let result2 = server.semantic_search_docs(Parameters(request_with_threshold)).await.unwrap();
        let text_content2 = result2.content[0].as_text().unwrap();
        assert!(text_content2.text.contains("connect_to_server"), "Should find network function");
    }
}

#[tokio::test]
async fn test_mcp_tool_without_semantic_search() {
    let temp_dir = TempDir::new().unwrap();
    let index_path = temp_dir.path().to_path_buf();
    
    // Create index WITHOUT semantic search
    let settings = Arc::new(Settings {
        index_path: index_path.clone(),
        ..Settings::default()
    });
    
    let mut indexer = SimpleIndexer::with_settings(settings.clone());
    // Don't enable semantic search
    
    // Index a file
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("test.rs"), "pub fn test() {}").unwrap();
    indexer.index_directory(&src_dir, false, false).unwrap();
    
    // Save and reload
    let persistence = IndexPersistence::new(index_path.clone());
    persistence.save(&indexer).unwrap();
    let loaded_indexer = persistence.load_with_settings(settings).unwrap();
    
    // Create MCP server
    let server = CodeIntelligenceServer::new(loaded_indexer);
    
    // Try to use semantic search tool
    let request = SemanticSearchRequest {
        query: "test function".to_string(),
        limit: 5,
        threshold: None,
    };
    
    let result = server.semantic_search_docs(Parameters(request)).await.unwrap();
    
    // Should get an error message
    assert!(!result.content.is_empty());
    let error_text = result.content[0].as_text().unwrap();
    assert!(error_text.text.contains("Semantic search is not enabled"), 
            "Should indicate semantic search is not available");
}