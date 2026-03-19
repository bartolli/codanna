//! Test to verify MCP schema generation for usize fields

use codanna::mcp::{
    AnalyzeImpactRequest, GetIndexInfoRequest, SearchSymbolsRequest, SemanticSearchRequest,
};

#[test]
fn test_mcp_schema_uint_format() {
    println!("\n=== Testing MCP Schema Generation for 'uint' Format Issue ===\n");

    // Test SearchSymbolsRequest schema
    let search_schema = rmcp::schemars::schema_for!(SearchSymbolsRequest);
    let search_json = serde_json::to_string_pretty(&search_schema).unwrap();

    println!("SearchSymbolsRequest schema:");
    println!("{search_json}");

    if search_json.contains(r#""format":"uint"#) {
        println!("\n❌ WARNING: SearchSymbolsRequest contains 'uint' format!");
        println!("   This may cause issues with MCP clients like Gemini.");
    }

    println!("\n{}", "=".repeat(50));

    // Test SemanticSearchRequest schema
    let semantic_schema = rmcp::schemars::schema_for!(SemanticSearchRequest);
    let semantic_json = serde_json::to_string_pretty(&semantic_schema).unwrap();

    println!("\nSemanticSearchRequest schema:");
    println!("{semantic_json}");

    if semantic_json.contains(r#""format":"uint"#) {
        println!("\n❌ WARNING: SemanticSearchRequest contains 'uint' format!");
    }

    println!("\n{}", "=".repeat(50));

    // Test AnalyzeImpactRequest schema
    let impact_schema = rmcp::schemars::schema_for!(AnalyzeImpactRequest);
    let impact_json = serde_json::to_string_pretty(&impact_schema).unwrap();

    println!("\nAnalyzeImpactRequest schema:");
    println!("{impact_json}");

    if impact_json.contains(r#""format":"uint"#) {
        println!("\n❌ WARNING: AnalyzeImpactRequest contains 'uint' format!");
    }

    // Summary
    println!("\n{}", "=".repeat(50));
    println!("SUMMARY:");

    let has_uint = search_json.contains(r#""format":"uint"#)
        || semantic_json.contains(r#""format":"uint"#)
        || impact_json.contains(r#""format":"uint"#);

    if has_uint {
        println!("❌ Schema contains 'uint' format which is not standard JSON Schema.");
        println!("   This causes compatibility issues with MCP clients.");
        println!("   Fix: Change usize fields to u32 or u64 in MCP request structs.");
    } else {
        println!("✅ No 'uint' format found in schemas.");
    }
}

/// Regression test: `get_index_info` is a no-parameter tool whose inputSchema must satisfy
/// both MCP spec (recommends `additionalProperties: false`) and OpenAI's strict
/// function-calling validation (requires `properties` field).
#[test]
fn test_get_index_info_schema_has_properties() {
    let schema = rmcp::schemars::schema_for!(GetIndexInfoRequest);
    let json = serde_json::to_string_pretty(&schema).unwrap();
    println!("GetIndexInfoRequest schema:\n{json}");

    let root: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(
        root.get("type").and_then(|v| v.as_str()),
        Some("object"),
        "schema must have type=object\nGot:\n{json}"
    );
    assert!(
        root.get("properties").is_some(),
        "schema must contain 'properties' for OpenAI compatibility\nGot:\n{json}"
    );
    assert_eq!(
        root.get("additionalProperties").and_then(|v| v.as_bool()),
        Some(false),
        "schema should set additionalProperties=false per MCP spec\nGot:\n{json}"
    );
    println!("✅ GetIndexInfoRequest schema is MCP-spec compliant and OpenAI-compatible.");
}
