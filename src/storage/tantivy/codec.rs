use crate::relationship::RelationshipMetadata;
use crate::storage::{StorageError, StorageResult};
use crate::vector::{ClusterId, VectorId};
use crate::{FileId, SymbolId};
use serde::{Deserialize, Serialize};
use tantivy::{TantivyDocument as Document, schema::Value};

use super::DocumentIndex;

/// Metadata for tracking vector-related information per document
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMetadata {
    /// The vector ID associated with this document (maps to SymbolId)
    pub vector_id: Option<VectorId>,
    /// The cluster assignment for this vector
    pub cluster_id: Option<ClusterId>,
    /// Version of the embedding model used to generate this vector
    pub embedding_version: u32,
}

/// Internal representation for JSON serialization
#[derive(Serialize, Deserialize)]
struct VectorMetadataJson {
    vector_id: Option<u32>,
    cluster_id: Option<u32>,
    embedding_version: u32,
}

impl VectorMetadata {
    /// Creates a new VectorMetadata with no vector assignment
    pub fn new(embedding_version: u32) -> Self {
        Self {
            vector_id: None,
            cluster_id: None,
            embedding_version,
        }
    }

    /// Creates VectorMetadata with full vector information
    pub fn with_vector(vector_id: VectorId, cluster_id: ClusterId, embedding_version: u32) -> Self {
        Self {
            vector_id: Some(vector_id),
            cluster_id: Some(cluster_id),
            embedding_version,
        }
    }

    /// Checks if this document has an associated vector
    pub fn has_vector(&self) -> bool {
        self.vector_id.is_some()
    }

    /// Serializes the metadata to a JSON string for storage in Tantivy
    pub fn to_json(&self) -> StorageResult<String> {
        let json_repr = VectorMetadataJson {
            vector_id: self.vector_id.map(|id| id.get()),
            cluster_id: self.cluster_id.map(|id| id.get()),
            embedding_version: self.embedding_version,
        };
        serde_json::to_string(&json_repr).map_err(|e| {
            StorageError::Serialization(format!("Failed to serialize VectorMetadata: {e}"))
        })
    }

    /// Deserializes metadata from a JSON string
    pub fn from_json(json: &str) -> StorageResult<Self> {
        let json_repr: VectorMetadataJson = serde_json::from_str(json).map_err(|e| {
            StorageError::Serialization(format!("Failed to deserialize VectorMetadata: {e}"))
        })?;

        Ok(Self {
            vector_id: json_repr.vector_id.and_then(VectorId::new),
            cluster_id: json_repr.cluster_id.and_then(ClusterId::new),
            embedding_version: json_repr.embedding_version,
        })
    }
}

/// Encode the stored scope_context field. Explicit JSON, `null` when absent;
/// exhaustive over variants by serde derivation.
fn encode_scope_context(scope: Option<&crate::ScopeContext>) -> String {
    serde_json::to_string(&scope)
        .expect("ScopeContext is a closed enum of strings and bools; serialization is infallible")
}

/// Decode the stored scope_context field. Undecodable values (including
/// pre-JSON legacy strings) read as no-scope-info: the index is session-scoped
/// and rebuilt, never migrated.
fn decode_scope_context(s: &str) -> Option<crate::ScopeContext> {
    serde_json::from_str(s).ok().flatten()
}

impl DocumentIndex {
    /// Add a symbol document to the index (must call start_batch first)
    pub fn add_document(&self, symbol: &crate::Symbol, file_path: &str) -> StorageResult<()> {
        let writer_lock = self.writer.read().map_err(|_| StorageError::LockPoisoned)?;
        let writer = writer_lock.as_ref().ok_or(StorageError::NoActiveBatch)?;

        let mut doc = Document::new();
        doc.add_text(self.schema.doc_type, "symbol");
        doc.add_u64(self.schema.symbol_id, symbol.id.value() as u64);
        doc.add_u64(self.schema.file_id, symbol.file_id.value() as u64);
        doc.add_text(self.schema.name, &symbol.name);
        doc.add_text(self.schema.name_text, &symbol.name); // Also add to full-text searchable field
        doc.add_text(self.schema.file_path, file_path);
        doc.add_u64(self.schema.line_number, symbol.range.start_line as u64);
        doc.add_u64(self.schema.column, symbol.range.start_column as u64);
        doc.add_u64(self.schema.end_line, symbol.range.end_line as u64);
        doc.add_u64(self.schema.end_column, symbol.range.end_column as u64);

        if let Some(comment) = symbol.doc_comment.as_deref() {
            doc.add_text(self.schema.doc_comment, comment);
        }

        if let Some(sig) = symbol.signature.as_deref() {
            doc.add_text(self.schema.signature, sig);
        }

        // Add string fields for filtering
        doc.add_text(
            self.schema.module_path,
            symbol.module_path.as_deref().unwrap_or(""),
        );
        doc.add_text(self.schema.kind, format!("{:?}", symbol.kind));
        doc.add_u64(self.schema.visibility, symbol.visibility as u64);

        doc.add_text(
            self.schema.scope_context,
            encode_scope_context(symbol.scope_context.as_ref()),
        );

        doc.add_text(
            self.schema.language,
            symbol
                .language_id
                .as_ref()
                .map(|id| id.as_str())
                .unwrap_or(""),
        );

        writer.add_document(doc)?;

        Ok(())
    }

    /// Convert a Tantivy document to a Symbol
    pub(super) fn document_to_symbol(&self, doc: &Document) -> StorageResult<crate::Symbol> {
        use crate::{Range, Symbol, SymbolKind, Visibility};

        let symbol_id = doc
            .get_first(self.schema.symbol_id)
            .and_then(|v| v.as_u64())
            .ok_or(StorageError::InvalidFieldValue {
                field: "symbol_id".to_string(),
                reason: "missing from document".to_string(),
            })?;

        let name = doc
            .get_first(self.schema.name)
            .and_then(|v| v.as_str())
            .ok_or(StorageError::InvalidFieldValue {
                field: "name".to_string(),
                reason: "missing from document".to_string(),
            })?
            .to_string();

        let kind_str = doc
            .get_first(self.schema.kind)
            .and_then(|v| v.as_str())
            .ok_or(StorageError::InvalidFieldValue {
                field: "kind".to_string(),
                reason: "missing from document".to_string(),
            })?;
        let kind = SymbolKind::from_str_with_default(kind_str);

        let file_id = doc
            .get_first(self.schema.file_id)
            .and_then(|v| v.as_u64())
            .ok_or(StorageError::InvalidFieldValue {
                field: "file_id".to_string(),
                reason: "missing from document".to_string(),
            })?;

        let start_line = doc
            .get_first(self.schema.line_number)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let start_col = doc
            .get_first(self.schema.column)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;

        let end_line = doc
            .get_first(self.schema.end_line)
            .and_then(|v| v.as_u64())
            .unwrap_or(start_line as u64) as u32;

        let end_col = doc
            .get_first(self.schema.end_column)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;

        let signature = doc
            .get_first(self.schema.signature)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let doc_comment = doc
            .get_first(self.schema.doc_comment)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let module_path = doc
            .get_first(self.schema.module_path)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Get visibility from stored field
        let visibility = doc
            .get_first(self.schema.visibility)
            .and_then(|v| v.as_u64())
            .map(|v| match v {
                0 => Visibility::Public,
                1 => Visibility::Crate,
                2 => Visibility::Module,
                3 => Visibility::Private,
                _ => Visibility::Private,
            })
            .unwrap_or(Visibility::Private);

        let scope_context = doc
            .get_first(self.schema.scope_context)
            .and_then(|v| v.as_str())
            .and_then(decode_scope_context);

        Ok(Symbol {
            id: SymbolId(symbol_id as u32),
            name: name.into(),
            kind,
            file_id: FileId(file_id as u32),
            range: Range {
                start_line,
                start_column: start_col,
                end_line,
                end_column: end_col,
            },
            file_path: {
                let stored = doc
                    .get_first(self.schema.file_path)
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>");
                match self.to_portable_file_path(stored) {
                    Some(portable) => portable.into(),
                    None => stored.into(),
                }
            },
            signature: signature.map(|s| s.into()),
            doc_comment: doc_comment.map(|s| s.into()),
            module_path: module_path.map(|s| s.into()),
            visibility,
            scope_context,
            language_id: {
                // Read the language field from the document and convert to LanguageId
                // using the language registry (which maintains the static strings)
                doc.get_first(self.schema.language)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|lang_str| {
                        // Use the global registry to convert string to LanguageId
                        // This maintains language-agnostic storage while properly
                        // converting to the type-safe LanguageId at retrieval time
                        crate::parsing::get_registry()
                            .lock()
                            .ok()
                            .and_then(|registry| registry.find_language_id(lang_str))
                    })
            },
        })
    }

    /// Reconstruct `RelationshipMetadata` from a stored relationship document.
    /// Union-gate: returns `Some(metadata)` if ANY of `{line, column, context,
    /// receiver, static_call}` is present; each field independently `Option`-set.
    /// Symmetric with `store_relationship`'s per-field-conditional write.
    pub(super) fn metadata_for_relationship_doc(
        &self,
        doc: &Document,
    ) -> Option<RelationshipMetadata> {
        let line = doc
            .get_first(self.schema.relation_line)
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);
        let column = doc
            .get_first(self.schema.relation_column)
            .and_then(|v| v.as_u64())
            .map(|n| n as u16);
        let context = doc
            .get_first(self.schema.relation_context)
            .and_then(|v| v.as_str());
        let receiver = doc
            .get_first(self.schema.relation_receiver)
            .and_then(|v| v.as_str());
        let static_call = doc
            .get_first(self.schema.relation_static_call)
            .and_then(|v| v.as_u64())
            .is_some_and(|n| n != 0);

        if line.is_none()
            && column.is_none()
            && context.is_none()
            && receiver.is_none()
            && !static_call
        {
            return None;
        }

        let mut metadata = RelationshipMetadata::new();
        metadata.line = line;
        metadata.column = column;
        metadata.context = context.map(Into::into);
        metadata.receiver = receiver.map(Into::into);
        metadata.static_call = static_call;
        Some(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolKind;

    use tempfile::TempDir;

    #[test]
    fn test_three_reconstruction_paths_agree_on_context_only_metadata() {
        // Story 8 slice 4 (backlog B): all three reconstruction sites must
        // return the same metadata for the same on-disk doc. Context-only
        // metadata exercises the gating divergence — write side emits each
        // field independently, so read side must also reconstruct each field
        // independently. Pre-refactor: get_relationships_from / _to use
        // AND-gate (line+column required) → metadata=None. query_relationships
        // uses union-gate → metadata=Some({context}). Post-refactor: all three
        // converge on union-gate via metadata_for_relationship_doc().
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let meta = RelationshipMetadata::new().with_context("inside main function");
        let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);

        index.store_relationship(from_id, to_id, &rel).unwrap();
        index.commit_batch().unwrap();

        let from_rels = index
            .get_relationships_from(from_id, crate::RelationKind::Calls)
            .unwrap();
        let to_rels = index
            .get_relationships_to(to_id, crate::RelationKind::Calls)
            .unwrap();
        let all_rels = index.query_relationships().unwrap();

        let from_meta = from_rels[0].2.metadata.as_ref();
        let to_meta = to_rels[0].2.metadata.as_ref();
        let all_meta = all_rels[0].2.metadata.as_ref();

        assert!(
            from_meta.is_some(),
            "get_relationships_from must reconstruct context-only metadata"
        );
        assert!(
            to_meta.is_some(),
            "get_relationships_to must reconstruct context-only metadata"
        );
        assert!(
            all_meta.is_some(),
            "query_relationships must reconstruct context-only metadata"
        );

        let from_meta = from_meta.unwrap();
        assert_eq!(from_meta.context.as_deref(), Some("inside main function"));
        assert_eq!(from_meta.line, None);
        assert_eq!(from_meta.column, None);
        assert_eq!(from_meta.context, to_meta.unwrap().context);
        assert_eq!(from_meta.context, all_meta.unwrap().context);
    }

    #[test]
    fn test_relation_metadata_legacy_defaults_via_from() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        index.start_batch().unwrap();

        let from_id = SymbolId::new(1).unwrap();
        let to_id = SymbolId::new(2).unwrap();
        let meta = RelationshipMetadata::new().at_position(7, 2);
        let rel = crate::Relationship::new(crate::RelationKind::Calls).with_metadata(meta);

        index.store_relationship(from_id, to_id, &rel).unwrap();
        index.commit_batch().unwrap();

        let rels = index
            .get_relationships_from(from_id, crate::RelationKind::Calls)
            .unwrap();
        assert_eq!(rels.len(), 1);

        let (_, _, r) = &rels[0];
        let m = r.metadata.as_ref().expect("metadata should be present");
        assert_eq!(m.line, Some(7));
        assert_eq!(m.column, Some(2));
        assert_eq!(m.receiver, None);
        assert!(!m.static_call);
    }

    #[test]
    fn test_vector_metadata_creation() {
        // Test creating metadata without vector
        let metadata = VectorMetadata::new(1);
        assert_eq!(metadata.vector_id, None);
        assert_eq!(metadata.cluster_id, None);
        assert_eq!(metadata.embedding_version, 1);
        assert!(!metadata.has_vector());

        // Test creating metadata with vector
        let vector_id = VectorId::new(42).unwrap();
        let cluster_id = ClusterId::new(5).unwrap();
        let metadata_with_vector = VectorMetadata::with_vector(vector_id, cluster_id, 2);
        assert_eq!(metadata_with_vector.vector_id, Some(vector_id));
        assert_eq!(metadata_with_vector.cluster_id, Some(cluster_id));
        assert_eq!(metadata_with_vector.embedding_version, 2);
        assert!(metadata_with_vector.has_vector());
    }

    #[test]
    fn test_vector_metadata_serialization() {
        // Test serialization of metadata without vector
        let metadata = VectorMetadata::new(1);
        let json = metadata.to_json().unwrap();
        let deserialized = VectorMetadata::from_json(&json).unwrap();
        assert_eq!(metadata, deserialized);

        // Test serialization of metadata with vector
        let vector_id = VectorId::new(123).unwrap();
        let cluster_id = ClusterId::new(7).unwrap();
        let metadata_with_vector = VectorMetadata::with_vector(vector_id, cluster_id, 3);
        let json = metadata_with_vector.to_json().unwrap();
        let deserialized = VectorMetadata::from_json(&json).unwrap();
        assert_eq!(metadata_with_vector, deserialized);

        // Verify JSON structure
        assert!(json.contains("\"vector_id\""));
        assert!(json.contains("\"cluster_id\""));
        assert!(json.contains("\"embedding_version\":3"));
    }

    #[test]
    fn test_vector_metadata_deserialization_error() {
        // Test invalid JSON
        let invalid_json = "{ invalid json }";
        let result = VectorMetadata::from_json(invalid_json);
        assert!(result.is_err());
        match result {
            Err(StorageError::Serialization(msg)) => {
                assert!(msg.contains("Failed to deserialize VectorMetadata"));
            }
            _ => panic!("Expected Serialization error"),
        }
    }

    #[test]
    fn test_vector_metadata_tantivy_roundtrip() {
        use tantivy::schema::{STORED, SchemaBuilder, TEXT};
        use tantivy::{Index, TantivyDocument, doc};

        // Create a simple schema with a metadata field
        let mut schema_builder = SchemaBuilder::default();
        let metadata_field = schema_builder.add_text_field("vector_metadata", TEXT | STORED);
        let schema = schema_builder.build();

        // Create an in-memory index
        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer(50_000_000).unwrap();

        // Create test metadata
        let vector_id = VectorId::new(999).unwrap();
        let cluster_id = ClusterId::new(42).unwrap();
        let metadata = VectorMetadata::with_vector(vector_id, cluster_id, 5);

        // Serialize and store in document
        let json = metadata.to_json().unwrap();
        let doc = doc!(metadata_field => json.clone());
        index_writer.add_document(doc).unwrap();
        index_writer.commit().unwrap();

        // Read back from index
        let reader = index.reader().unwrap();
        let searcher = reader.searcher();
        let doc: TantivyDocument = searcher.doc(tantivy::DocAddress::new(0, 0)).unwrap();

        // Deserialize and verify
        let stored_json = doc
            .get_first(metadata_field)
            .and_then(|v| v.as_str())
            .unwrap();
        let retrieved_metadata = VectorMetadata::from_json(stored_json).unwrap();

        assert_eq!(metadata, retrieved_metadata);
        assert_eq!(json, stored_json);
    }

    #[test]
    fn test_scope_context_codec_round_trips_every_variant() {
        use crate::{ScopeContext, SymbolKind};

        const ALL_KINDS: [SymbolKind; 14] = [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Interface,
            SymbolKind::Class,
            SymbolKind::Module,
            SymbolKind::Variable,
            SymbolKind::Constant,
            SymbolKind::Field,
            SymbolKind::Parameter,
            SymbolKind::TypeAlias,
            SymbolKind::Macro,
        ];

        let mut cases: Vec<Option<ScopeContext>> = vec![
            None,
            Some(ScopeContext::Module),
            Some(ScopeContext::Global),
            Some(ScopeContext::Package),
            Some(ScopeContext::Parameter),
            Some(ScopeContext::ClassMember { class_name: None }),
            Some(ScopeContext::ClassMember {
                class_name: Some("com.example.MyClass".into()),
            }),
        ];
        for hoisted in [false, true] {
            for parent_name in [None, Some("outer_fn")] {
                cases.push(Some(ScopeContext::Local {
                    hoisted,
                    parent_name: parent_name.map(Into::into),
                    parent_kind: None,
                }));
                for kind in ALL_KINDS {
                    cases.push(Some(ScopeContext::Local {
                        hoisted,
                        parent_name: parent_name.map(Into::into),
                        parent_kind: Some(kind),
                    }));
                }
            }
        }

        for case in cases {
            let encoded = encode_scope_context(case.as_ref());
            let decoded = decode_scope_context(&encoded);
            assert_eq!(decoded, case, "round-trip failed via {encoded:?}");
        }
    }

    #[test]
    fn test_scope_context_survives_tantivy_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::config::Settings::default();
        let index = DocumentIndex::new(temp_dir.path(), &settings).unwrap();

        let scope = crate::ScopeContext::Local {
            hoisted: true,
            parent_name: Some("run_pipeline".into()),
            parent_kind: Some(SymbolKind::Struct),
        };
        let symbol = crate::Symbol::new(
            SymbolId::new(7).unwrap(),
            "worker",
            SymbolKind::Variable,
            FileId::new(1).unwrap(),
            crate::Range::new(4, 8, 4, 14),
        )
        .with_scope(scope.clone());

        index.start_batch().unwrap();
        index.index_symbol(&symbol, "src/lib.rs").unwrap();
        index.commit_batch().unwrap();

        let retrieved = index
            .find_symbol_by_id(SymbolId::new(7).unwrap())
            .unwrap()
            .expect("symbol stored by this test");
        assert_eq!(retrieved.scope_context, Some(scope));
    }

    fn portable_test_symbol(id: u32) -> crate::Symbol {
        crate::Symbol::new(
            SymbolId::new(id).unwrap(),
            "portable_probe",
            SymbolKind::Function,
            FileId::new(1).unwrap(),
            crate::Range::new(0, 0, 2, 1),
        )
    }

    fn roundtrip_file_path(settings: &crate::config::Settings, stored: &str) -> String {
        let index_dir = TempDir::new().unwrap();
        let index = DocumentIndex::new(index_dir.path(), settings).unwrap();
        index.start_batch().unwrap();
        index
            .index_symbol(&portable_test_symbol(11), stored)
            .unwrap();
        index.commit_batch().unwrap();
        index
            .find_symbol_by_id(SymbolId::new(11).unwrap())
            .unwrap()
            .expect("symbol stored by this test")
            .file_path
            .to_string()
    }

    #[test]
    fn absolute_stored_path_under_indexed_root_emits_relative() {
        let root = TempDir::new().unwrap();
        let base = root.path().canonicalize().unwrap();
        let mut settings = crate::config::Settings::default();
        settings.indexing.indexed_paths = vec![base.clone()];
        let stored = base.join("src/lib.rs");
        assert_eq!(
            roundtrip_file_path(&settings, &stored.to_string_lossy()),
            "src/lib.rs"
        );
    }

    #[test]
    fn absolute_stored_path_under_workspace_root_emits_relative() {
        let root = TempDir::new().unwrap();
        let base = root.path().canonicalize().unwrap();
        let settings = crate::config::Settings {
            workspace_root: Some(base.clone()),
            ..Default::default()
        };
        let stored = base.join("src/lib.rs");
        assert_eq!(
            roundtrip_file_path(&settings, &stored.to_string_lossy()),
            "src/lib.rs"
        );
    }

    #[test]
    fn relative_stored_path_passes_through() {
        let settings = crate::config::Settings::default();
        assert_eq!(roundtrip_file_path(&settings, "src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn absolute_stored_path_outside_bases_passes_through() {
        let root = TempDir::new().unwrap();
        let base = root.path().canonicalize().unwrap();
        let mut settings = crate::config::Settings::default();
        settings.indexing.indexed_paths = vec![base];
        let stored = "/nonexistent-root/other/src/lib.rs";
        assert_eq!(roundtrip_file_path(&settings, stored), stored);
    }
}
