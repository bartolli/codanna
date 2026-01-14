//! Unified JSON output envelope for all CLI commands.
//!
//! Schema version 1.0.0 - See `research/json-output/unified-schema-v1.json`
//!
//! This envelope provides consistent JSON output across all commands,
//! designed for Unix piping, AI integration, and future streaming.

use serde::{Deserialize, Serialize};

/// Schema version for this envelope format.
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Message type for stream discrimination.
///
/// Today: only `Result` and `Error` are used.
/// Future: `Begin`, `End`, `Summary` enable streaming without breaking changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Successful result (may contain data or be empty)
    Result,
    /// Error occurred
    Error,
    /// Stream start marker (reserved for future streaming)
    Begin,
    /// Stream end marker (reserved for future streaming)
    End,
    /// Summary after streaming (reserved for future streaming)
    Summary,
}

/// Operation outcome status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Operation succeeded with results
    Success,
    /// Operation succeeded but found nothing
    NotFound,
    /// Some operations succeeded, some failed
    PartialSuccess,
    /// Operation failed
    Error,
}

/// Machine-readable result codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultCode {
    Ok,
    NotFound,
    ParseError,
    IndexError,
    InvalidQuery,
    InternalError,
}

impl ResultCode {
    /// Convert to string for serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::NotFound => "NOT_FOUND",
            Self::ParseError => "PARSE_ERROR",
            Self::IndexError => "INDEX_ERROR",
            Self::InvalidQuery => "INVALID_QUERY",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

/// Entity type in the data payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Symbol,
    SearchResult,
    CallTree,
    ImpactGraph,
    Document,
    Callers,
    Calls,
}

/// Unified JSON output envelope.
///
/// All CLI commands output this structure when `--json` is used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T = serde_json::Value> {
    /// Message type for stream discrimination
    #[serde(rename = "type")]
    pub message_type: MessageType,

    /// Operation outcome
    pub status: Status,

    /// Machine-readable result code
    pub code: ResultCode,

    /// Unix exit code (0-255)
    pub exit_code: u8,

    /// Human-readable message
    pub message: String,

    /// AI assistant guidance (next steps)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,

    /// Result payload (null on error)
    pub data: Option<T>,

    /// Error details (null on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetails>,

    /// Response metadata
    pub meta: Meta,
}

/// Error details with suggestions and context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetails {
    /// Recovery suggestions
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,

    /// Additional error context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Response metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    /// Schema version (semver)
    pub schema_version: String,

    /// Entity type in data payload
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<EntityType>,

    /// Number of items in data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,

    /// Original query string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,

    /// Execution time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    /// True if results were truncated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,

    /// Traversal depth for tree/graph results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            entity_type: None,
            count: None,
            query: None,
            duration_ms: None,
            truncated: None,
            depth: None,
        }
    }
}

impl<T> Envelope<T> {
    /// Create a success envelope with data.
    pub fn success(data: T) -> Self {
        Self {
            message_type: MessageType::Result,
            status: Status::Success,
            code: ResultCode::Ok,
            exit_code: 0,
            message: "Operation completed successfully".to_string(),
            hint: None,
            data: Some(data),
            error: None,
            meta: Meta::default(),
        }
    }

    /// Create a not-found envelope.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message_type: MessageType::Result,
            status: Status::NotFound,
            code: ResultCode::NotFound,
            exit_code: 1,
            message: message.into(),
            hint: None,
            data: None,
            error: None,
            meta: Meta::default(),
        }
    }

    /// Create an error envelope.
    pub fn error(code: ResultCode, message: impl Into<String>) -> Self {
        Self {
            message_type: MessageType::Error,
            status: Status::Error,
            code,
            exit_code: 2,
            message: message.into(),
            hint: None,
            data: None,
            error: None,
            meta: Meta::default(),
        }
    }

    /// Add hint for AI assistants.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Add custom message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Set entity type in metadata.
    pub fn with_entity_type(mut self, entity_type: EntityType) -> Self {
        self.meta.entity_type = Some(entity_type);
        self
    }

    /// Set count in metadata.
    pub fn with_count(mut self, count: usize) -> Self {
        self.meta.count = Some(count);
        self
    }

    /// Set query in metadata.
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.meta.query = Some(query.into());
        self
    }

    /// Set duration in metadata.
    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.meta.duration_ms = Some(duration_ms);
        self
    }

    /// Set error details.
    pub fn with_error_details(mut self, details: ErrorDetails) -> Self {
        self.error = Some(details);
        self
    }

    /// Set truncated flag.
    pub fn with_truncated(mut self, truncated: bool) -> Self {
        self.meta.truncated = Some(truncated);
        self
    }

    /// Set traversal depth.
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.meta.depth = Some(depth);
        self
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error>
    where
        T: Serialize,
    {
        serde_json::to_string_pretty(self)
    }

    /// Serialize to compact JSON string (no whitespace).
    pub fn to_json_compact(&self) -> Result<String, serde_json::Error>
    where
        T: Serialize,
    {
        serde_json::to_string(self)
    }

    /// Serialize to JSON with field filtering on data items.
    ///
    /// Only includes specified fields in each data item.
    /// The envelope structure (type, status, code, etc.) is always included.
    /// Works with both array data (filters each item) and object data (filters the object).
    pub fn to_json_with_fields(&self, fields: &[String]) -> Result<String, serde_json::Error>
    where
        T: Serialize,
    {
        // Serialize to Value first
        let mut value = serde_json::to_value(self)?;

        // Helper to filter object fields
        fn filter_object(obj: &mut serde_json::Map<String, serde_json::Value>, fields: &[String]) {
            let keys_to_remove: Vec<String> = obj
                .keys()
                .filter(|k| !fields.contains(k))
                .cloned()
                .collect();
            for key in keys_to_remove {
                obj.remove(&key);
            }
        }

        // Filter fields in data (array or single object)
        if let Some(data) = value.get_mut("data") {
            if let Some(arr) = data.as_array_mut() {
                // Handle array: filter each item
                for item in arr.iter_mut() {
                    if let Some(obj) = item.as_object_mut() {
                        filter_object(obj, fields);
                    }
                }
            } else if let Some(obj) = data.as_object_mut() {
                // Handle single object
                filter_object(obj, fields);
            }
        }

        serde_json::to_string_pretty(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_envelope() {
        let data = vec!["item1", "item2"];
        let envelope = Envelope::success(data)
            .with_entity_type(EntityType::Symbol)
            .with_count(2)
            .with_message("Found 2 symbols");

        assert_eq!(envelope.message_type, MessageType::Result);
        assert_eq!(envelope.status, Status::Success);
        assert_eq!(envelope.code, ResultCode::Ok);
        assert_eq!(envelope.exit_code, 0);
        assert_eq!(envelope.meta.count, Some(2));
        assert!(envelope.data.is_some());
    }

    #[test]
    fn test_not_found_envelope() {
        let envelope: Envelope<()> = Envelope::not_found("Symbol 'foo' not found")
            .with_hint("Try semantic_search_with_context");

        assert_eq!(envelope.status, Status::NotFound);
        assert_eq!(envelope.code, ResultCode::NotFound);
        assert_eq!(envelope.exit_code, 1);
        assert!(envelope.data.is_none());
        assert!(envelope.hint.is_some());
    }

    #[test]
    fn test_error_envelope() {
        let envelope: Envelope<()> = Envelope::error(ResultCode::ParseError, "Invalid syntax")
            .with_error_details(ErrorDetails {
                suggestions: vec!["Check syntax".to_string()],
                context: None,
            });

        assert_eq!(envelope.message_type, MessageType::Error);
        assert_eq!(envelope.status, Status::Error);
        assert_eq!(envelope.code, ResultCode::ParseError);
        assert_eq!(envelope.exit_code, 2);
        assert!(envelope.error.is_some());
    }

    #[test]
    fn test_json_serialization() {
        let envelope = Envelope::success(vec!["a", "b"])
            .with_entity_type(EntityType::Symbol)
            .with_count(2);

        let json = envelope.to_json().unwrap();
        assert!(json.contains("\"type\": \"result\""));
        assert!(json.contains("\"status\": \"success\""));
        assert!(json.contains("\"schema_version\": \"1.0.0\""));
    }
}
