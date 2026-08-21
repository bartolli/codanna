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
    Relationship,
    Graph,
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

    /// Language filter applied (e.g., "rust", "python")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

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
            lang: None,
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

    /// Stream start marker: opens a JSON Lines stream of `result` envelopes.
    pub fn begin(message: impl Into<String>) -> Self {
        Self {
            message_type: MessageType::Begin,
            status: Status::Success,
            code: ResultCode::Ok,
            exit_code: 0,
            message: message.into(),
            hint: None,
            data: None,
            error: None,
            meta: Meta::default(),
        }
    }

    /// Stream end: the summary payload; its `exit_code` is the stream's outcome.
    pub fn summary(data: T) -> Self {
        Self {
            message_type: MessageType::Summary,
            status: Status::Success,
            code: ResultCode::Ok,
            exit_code: 0,
            message: "stream complete".to_string(),
            hint: None,
            data: Some(data),
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

    /// Set language filter in metadata.
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.meta.lang = Some(lang.into());
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

    /// Serialize to JSON with field projection on data items.
    ///
    /// `fields` entries are top-level keys or dotted paths (`symbol.name`).
    /// Output preserves nesting. A first path segment absent from the data
    /// item's top-level keys is rejected; deeper segments that miss on an
    /// individual item omit that leaf for that item only. Null data passes
    /// through unchanged. The envelope structure is always included.
    pub fn to_json_with_fields(&self, fields: &[String]) -> Result<String, FieldProjectionError>
    where
        T: Serialize,
    {
        use serde_json::{Map, Value};

        fn resolve<'v>(obj: &'v Map<String, Value>, path: &[&str]) -> Option<&'v Value> {
            let mut cur = obj.get(path[0])?;
            for seg in &path[1..] {
                cur = cur.as_object()?.get(*seg)?;
            }
            Some(cur)
        }

        fn nested_insert(out: &mut Map<String, Value>, path: &[&str], value: Value) {
            if path.len() == 1 {
                out.insert(path[0].to_string(), value);
                return;
            }
            let entry = out
                .entry(path[0].to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            nested_insert(
                entry.as_object_mut().expect("just ensured object"),
                &path[1..],
                value,
            );
        }

        fn project(obj: &Map<String, Value>, paths: &[Vec<&str>]) -> Map<String, Value> {
            let mut out = Map::new();
            for path in paths {
                if let Some(v) = resolve(obj, path) {
                    nested_insert(&mut out, path, v.clone());
                }
            }
            out
        }

        fn validate(
            obj: &Map<String, Value>,
            fields: &[String],
            paths: &[Vec<&str>],
        ) -> Result<(), FieldProjectionError> {
            for (field, path) in fields.iter().zip(paths) {
                if !obj.contains_key(path[0]) {
                    return Err(FieldProjectionError::UnknownField {
                        field: field.clone(),
                        available: obj.keys().cloned().collect(),
                    });
                }
            }
            Ok(())
        }

        let mut value = serde_json::to_value(self)?;
        let paths: Vec<Vec<&str>> = fields.iter().map(|f| f.split('.').collect()).collect();

        if let Some(data) = value.get_mut("data") {
            if let Some(arr) = data.as_array_mut() {
                if let Some(first) = arr.iter().find_map(|i| i.as_object()) {
                    validate(first, fields, &paths)?;
                }
                for item in arr.iter_mut() {
                    if let Some(obj) = item.as_object() {
                        *item = Value::Object(project(obj, &paths));
                    }
                }
            } else if let Some(obj) = data.as_object() {
                validate(obj, fields, &paths)?;
                *data = Value::Object(project(obj, &paths));
            }
        }

        Ok(serde_json::to_string_pretty(&value)?)
    }
}

/// Field projection failure for `to_json_with_fields`.
#[derive(Debug, thiserror::Error)]
pub enum FieldProjectionError {
    /// A requested path's first segment is absent from the data item's
    /// top-level keys.
    #[error("unknown field '{field}' in --fields; available top-level fields: {}", available.join(", "))]
    UnknownField {
        field: String,
        available: Vec<String>,
    },
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
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

    fn nested_item(with_doc: bool) -> serde_json::Value {
        let mut symbol = serde_json::json!({
            "name": "Envelope",
            "kind": "Struct"
        });
        if with_doc {
            symbol["doc_comment"] = serde_json::json!("Unified output envelope");
        }
        serde_json::json!({
            "symbol": symbol,
            "file_path": "src/io/envelope.rs",
            "relationships": null
        })
    }

    #[test]
    fn fields_projection_nested_path_preserves_nesting() {
        let envelope = Envelope::success(vec![nested_item(true)]);
        let json = envelope
            .to_json_with_fields(&["symbol.name".to_string(), "file_path".to_string()])
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["data"][0],
            serde_json::json!({
                "symbol": {"name": "Envelope"},
                "file_path": "src/io/envelope.rs"
            })
        );
    }

    #[test]
    fn fields_projection_unknown_first_segment_rejects() {
        let envelope = Envelope::success(vec![nested_item(true)]);
        let err = envelope
            .to_json_with_fields(&["name".to_string()])
            .unwrap_err();
        match err {
            FieldProjectionError::UnknownField { field, available } => {
                assert_eq!(field, "name");
                assert!(available.contains(&"symbol".to_string()));
                assert!(available.contains(&"file_path".to_string()));
            }
            other => panic!("expected UnknownField, got {other:?}"),
        }
    }

    #[test]
    fn fields_projection_deep_miss_omits_leaf_per_item() {
        let envelope = Envelope::success(vec![nested_item(true), nested_item(false)]);
        let json = envelope
            .to_json_with_fields(&["symbol.doc_comment".to_string()])
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["data"][0],
            serde_json::json!({"symbol": {"doc_comment": "Unified output envelope"}})
        );
        assert_eq!(value["data"][1], serde_json::json!({}));
    }

    #[test]
    fn fields_projection_null_data_noop() {
        let envelope: Envelope<serde_json::Value> = Envelope::not_found("Symbol 'x' not found");
        let json = envelope
            .to_json_with_fields(&["anything".to_string()])
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["data"].is_null());
        assert_eq!(value["exit_code"], 1);
    }

    #[test]
    fn fields_projection_single_object_data_validates() {
        let envelope = Envelope::success(nested_item(true));
        let err = envelope
            .to_json_with_fields(&["line".to_string()])
            .unwrap_err();
        assert!(matches!(err, FieldProjectionError::UnknownField { .. }));

        let json = envelope
            .to_json_with_fields(&["symbol.kind".to_string()])
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["data"],
            serde_json::json!({"symbol": {"kind": "Struct"}})
        );
    }
    #[test]
    fn begin_and_summary_are_stream_markers() {
        let begin: Envelope<()> = Envelope::begin("dump").with_entity_type(EntityType::Graph);
        let json = begin.to_json_compact().unwrap();
        assert!(json.starts_with("{\"type\":\"begin\""), "{json}");
        assert!(json.contains("\"data\":null"));
        assert!(json.contains("\"entity_type\":\"graph\""));

        let summary = Envelope::summary(serde_json::json!({"symbols": 2}))
            .with_entity_type(EntityType::Graph)
            .with_count(3);
        let json = summary.to_json_compact().unwrap();
        assert!(json.starts_with("{\"type\":\"summary\""), "{json}");
        assert!(json.contains("\"exit_code\":0"));
        assert!(json.contains("\"count\":3"));
        let back: Envelope<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message_type, MessageType::Summary);
        assert_eq!(back.meta.entity_type, Some(EntityType::Graph));
    }
}
