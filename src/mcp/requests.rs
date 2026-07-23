//! MCP tool request types.

use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindSymbolRequest {
    /// Name of the symbol to find
    pub name: String,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetCallsRequest {
    /// Name of the function to analyze (use symbol_id for unambiguous lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    /// Symbol ID for direct lookup (recommended to avoid ambiguity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindCallersRequest {
    /// Name of the function to find callers for (use symbol_id for unambiguous lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    /// Symbol ID for direct lookup (recommended to avoid ambiguity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeImpactRequest {
    /// Name of the symbol to analyze impact for (use symbol_id for unambiguous lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_name: Option<String>,
    /// Symbol ID for direct lookup (recommended to avoid ambiguity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<u32>,
    /// Maximum depth to search (default: 3)
    #[serde(default = "default_depth", alias = "depth")]
    pub max_depth: u32,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchSymbolsRequest {
    /// Search query (supports fuzzy matching)
    pub query: String,
    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by symbol kind (e.g., "Function", "Struct", "Trait")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Filter by module path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticSearchRequest {
    /// Natural language search query
    pub query: String,
    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Minimum similarity score (0-1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticSearchWithContextRequest {
    /// Natural language search query
    pub query: String,
    /// Maximum number of results (default: 5, as each includes full context)
    #[serde(default = "default_context_limit")]
    pub limit: u32,
    /// Minimum similarity score (0-1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    /// Filter by programming language (e.g., "rust", "python", "typescript", "php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GetIndexInfoRequest {}

impl schemars::JsonSchema for GetIndexInfoRequest {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("GetIndexInfoRequest")
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(concat!(module_path!(), "::GetIndexInfoRequest"))
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // MCP spec recommends `{"type":"object","additionalProperties":false}` for
        // no-parameter tools. We also include an empty `properties` map because
        // OpenAI's strict function-calling validation rejects object schemas that
        // lack `properties` entirely.
        schemars::Schema::from(
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
            .as_object()
            .unwrap()
            .clone(),
        )
    }
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchDocumentsRequest {
    /// Natural language search query
    pub query: String,
    /// Filter by collection name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    /// Maximum number of results (default: 5)
    #[serde(default = "default_context_limit")]
    pub limit: u32,
}

fn default_depth() -> u32 {
    3
}

fn default_limit() -> u32 {
    10
}

fn default_context_limit() -> u32 {
    5
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_keys_reject_across_request_structs() {
        assert!(
            serde_json::from_value::<FindSymbolRequest>(json!({"name": "x", "bogus": 1})).is_err()
        );
        assert!(serde_json::from_value::<GetCallsRequest>(json!({"bogus": 1})).is_err());
        assert!(serde_json::from_value::<FindCallersRequest>(json!({"langg": "rust"})).is_err());
        assert!(
            serde_json::from_value::<AnalyzeImpactRequest>(
                json!({"symbol_name": "x", "depths": 2})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<SearchSymbolsRequest>(
                json!({"query": "q", "kindd": "function"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<SemanticSearchRequest>(json!({"query": "q", "treshold": 0.5}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<SemanticSearchWithContextRequest>(
                json!({"query": "q", "x": 1})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<GetIndexInfoRequest>(json!({"bogus": 1})).is_err());
        assert!(
            serde_json::from_value::<SearchDocumentsRequest>(
                json!({"query": "q", "collections": "a"})
            )
            .is_err()
        );
    }

    #[test]
    fn depth_aliases_max_depth() {
        let req: AnalyzeImpactRequest =
            serde_json::from_value(json!({"symbol_name": "x", "depth": 2})).expect("alias applies");
        assert_eq!(req.max_depth, 2);
        let req: AnalyzeImpactRequest =
            serde_json::from_value(json!({"symbol_name": "x"})).expect("default applies");
        assert_eq!(req.max_depth, 3);
    }

    #[test]
    fn rejection_names_the_field_and_accepted_keys() {
        let err = serde_json::from_value::<GetCallsRequest>(json!({"bogus": 1})).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bogus") && msg.contains("function_name"),
            "rejection must name the offending field and the accepted set: {msg}"
        );
    }
}
