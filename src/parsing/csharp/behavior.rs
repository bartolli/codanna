//! C# language behavior implementation
//!
//! This module defines how C# code is processed during indexing, including:
//! - Module path calculation (namespace handling)
//! - Import resolution (using directives)
//! - Relationship mapping (method calls, implementations)
//! - Symbol visibility rules
//! - Caller name normalization
//!
//! The behavior system provides language-specific logic that complements
//! the generic parser, allowing for proper C# semantics.

use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::resolution::ResolutionScope;
use crate::storage::DocumentIndex;
use crate::symbol::ScopeContext;
use crate::types::FileId;
use crate::{Symbol, SymbolId, Visibility};
use std::path::{Path, PathBuf};
use tree_sitter::Language;

use super::resolution::CSharpResolutionContext;

/// C# language behavior implementation
///
/// Provides C#-specific logic for code analysis including namespace resolution,
/// using directive handling, and symbol visibility rules.
///
/// # Architecture
///
/// The behavior maintains state about processed files, imports, and module paths
/// to enable proper cross-file resolution of C# symbols.
#[derive(Clone)]
pub struct CSharpBehavior {
    state: BehaviorState,
}

impl CSharpBehavior {
    /// Create a new C# behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }

    /// Get the fully qualified class name containing this symbol (C# implementation)
    pub fn get_containing_class(&self, symbol: &Symbol) -> Option<String> {
        if let Some(ScopeContext::ClassMember {
            class_name: Some(class),
        }) = &symbol.scope_context
        {
            if let Some(pkg) = &symbol.module_path {
                if pkg.is_empty() {
                    return Some(class.to_string());
                }
                return Some(format!("{pkg}.{class}"));
            }
            return Some(class.to_string());
        }
        None
    }

    /// Check if symbol is visible from another file (C# visibility rules)
    ///
    /// C# visibility: private, protected, internal, protected internal, private protected, public
    /// For now, simplified implementation (similar to backward-compatible mode)
    pub fn is_symbol_visible_from_file(&self, symbol: &Symbol, from_file: FileId) -> bool {
        // Same file: always visible
        if symbol.file_id == from_file {
            return true;
        }

        // C# visibility check - simplified for now
        // TODO: Implement full C# visibility rules (internal, protected, etc.)
        match symbol.visibility {
            Visibility::Private => false,
            Visibility::Public => true,
            _ => true, // Be permissive for other levels
        }
    }
}

impl Default for CSharpBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for CSharpBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for CSharpBehavior {
    fn language_id(&self) -> crate::parsing::registry::LanguageId {
        crate::parsing::registry::LanguageId::new("csharp")
    }

    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Set namespace as module path for C# symbols
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // C# uses namespaces as module paths, not including the symbol name
        // All symbols in the same namespace share the same module path
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn module_separator(&self) -> &'static str {
        "." // C# uses dots for namespace separation
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Convert file path to namespace path relative to project root
        // e.g., src/Services/UserService.cs -> MyApp.Services.UserService

        // Get relative path from project root
        let relative_path = file_path
            .strip_prefix(project_root)
            .ok()
            .or_else(|| file_path.strip_prefix("./").ok())
            .unwrap_or(file_path);

        let path = relative_path.to_str()?;

        // Remove common directory prefixes and file extensions
        let module_path = path
            .trim_start_matches("./")
            .trim_start_matches("src/")
            .trim_start_matches("lib/")
            .trim_end_matches(".cs")
            .trim_end_matches(".csx");

        // Replace path separators with namespace separators
        // Remove the filename part to get the directory-based namespace
        let namespace_path = module_path.replace(['/', '\\'], ".");

        // For C#, we typically want the directory structure as namespace
        // but we might need to extract from actual namespace declarations in the file
        Some(namespace_path)
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // C# visibility modifiers in order of precedence
        if signature.contains("public ") {
            Visibility::Public
        } else if signature.contains("private ") {
            Visibility::Private
        } else if signature.contains("protected ") {
            // Map protected to Module visibility as closest approximation
            Visibility::Module
        } else if signature.contains("internal ") {
            // Internal is assembly-level visibility, map to Module
            Visibility::Module
        } else {
            // Default C# visibility depends on context:
            // - Top-level types: internal
            // - Class members: private
            // We'll default to private as most conservative
            Visibility::Private
        }
    }

    fn supports_traits(&self) -> bool {
        true // C# has interfaces
    }

    fn supports_inherent_methods(&self) -> bool {
        true // C# has class and struct methods
    }

    // C#-specific resolution
    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(CSharpResolutionContext::new(file_id))
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // C# uses both ":" for inheritance and explicit implements
        "inherits"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;

        match language_specific {
            "inherits" | "extends" => RelationKind::Extends,
            "implements" => RelationKind::Implements,
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
            _ => RelationKind::References,
        }
    }

    // Use state-based import tracking
    fn register_file(&self, path: PathBuf, file_id: FileId, module_path: String) {
        self.register_file_with_state(path, file_id, module_path);
    }

    fn add_import(&self, import: crate::parsing::Import) {
        self.add_import_with_state(import);
    }

    fn get_imports_for_file(&self, file_id: FileId) -> Vec<crate::parsing::Import> {
        self.get_imports_from_state(file_id)
    }

    fn resolve_external_call_target(
        &self,
        to_name: &str,
        from_file: FileId,
    ) -> Option<(String, String)> {
        // C# external call resolution
        // Cases:
        // - Qualified call: System.Console.WriteLine -> System.Console
        // - Using alias: using Alias = System.Collections.Generic.List; -> Alias.Add
        // - Using directive: using System; -> Console.WriteLine

        let imports = self.get_imports_for_file(from_file);
        if imports.is_empty() {
            return None;
        }

        // Check for qualified names (Namespace.Type.Member)
        if let Some((qualifier, member)) = to_name.rsplit_once('.') {
            for import in &imports {
                // Check if this is a using alias that matches the qualifier
                if let Some(alias) = &import.alias {
                    if alias == qualifier {
                        // Map alias to its full namespace
                        return Some((import.path.clone(), member.to_string()));
                    }
                }

                // Check if the qualifier matches an imported namespace
                if import.path == qualifier || import.path.ends_with(&format!(".{qualifier}")) {
                    return Some((import.path.clone(), member.to_string()));
                }
            }
        } else {
            // Unqualified name - check using directives
            for import in &imports {
                if !import.is_glob && import.alias.is_none() {
                    // This is a "using Namespace;" directive
                    // The symbol could be Namespace.SymbolName
                    return Some((import.path.clone(), to_name.to_string()));
                }
            }
        }

        None
    }

    fn create_external_symbol(
        &self,
        document_index: &mut DocumentIndex,
        module_path: &str,
        symbol_name: &str,
        language_id: crate::parsing::LanguageId,
    ) -> crate::IndexResult<SymbolId> {
        use crate::storage::MetadataKey;
        use crate::{IndexError, Symbol, SymbolId, SymbolKind, Visibility};

        // Check if symbol already exists
        if let Ok(cands) = document_index.find_symbols_by_name(symbol_name, None) {
            for s in cands {
                if let Some(mp) = &s.module_path {
                    if mp.as_ref() == module_path {
                        return Ok(s.id);
                    }
                }
            }
        }

        // Create virtual file path for external C# symbol
        let mut path_buf = String::from(".codanna/external/");
        path_buf.push_str(&module_path.replace('.', "/"));
        path_buf.push_str(".cs");
        let path_str = path_buf;

        // Ensure file_info exists
        let file_id = if let Ok(Some((fid, _))) = document_index.get_file_info(&path_str) {
            fid
        } else {
            let next_file_id =
                document_index
                    .get_next_file_id()
                    .map_err(|e| IndexError::TantivyError {
                        operation: "get_next_file_id".to_string(),
                        cause: e.to_string(),
                    })?;
            let file_id = crate::FileId::new(next_file_id).ok_or(IndexError::FileIdExhausted)?;
            let hash = format!("external:{module_path}");
            let ts = crate::indexing::get_utc_timestamp();
            document_index
                .store_file_info(file_id, &path_str, &hash, ts)
                .map_err(|e| IndexError::TantivyError {
                    operation: "store_file_info".to_string(),
                    cause: e.to_string(),
                })?;
            file_id
        };

        // Allocate symbol ID
        let next_id =
            document_index
                .get_next_symbol_id()
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_next_symbol_id".to_string(),
                    cause: e.to_string(),
                })?;
        let symbol_id = SymbolId::new(next_id).ok_or(IndexError::SymbolIdExhausted)?;

        // Create external symbol
        let mut symbol = Symbol::new(
            symbol_id,
            symbol_name.to_string(),
            SymbolKind::Class, // Default to class for C# external symbols
            file_id,
            crate::Range::new(0, 0, 0, 0),
        )
        .with_visibility(Visibility::Public);
        symbol.module_path = Some(module_path.to_string().into());
        symbol.scope_context = Some(crate::symbol::ScopeContext::Global);
        symbol.language_id = Some(language_id);

        document_index
            .index_symbol(&symbol, &path_str)
            .map_err(|e| IndexError::TantivyError {
                operation: "index_symbol".to_string(),
                cause: e.to_string(),
            })?;

        // Update symbol counter
        document_index
            .store_metadata(MetadataKey::SymbolCounter, symbol_id.value() as u64)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_metadata(SymbolCounter)".to_string(),
                cause: e.to_string(),
            })?;

        Ok(symbol_id)
    }

    fn build_resolution_context(
        &self,
        file_id: FileId,
        document_index: &DocumentIndex,
    ) -> crate::error::IndexResult<Box<dyn ResolutionScope>> {
        use crate::error::IndexError;

        let mut context = CSharpResolutionContext::new(file_id);

        // Add imported symbols from using directives
        let imports = self.get_imports_for_file(file_id);
        for import in imports {
            if let Some(symbol_id) = self.resolve_import(&import, document_index) {
                let name = if let Some(alias) = &import.alias {
                    alias.clone()
                } else {
                    import
                        .path
                        .split('.')
                        .next_back()
                        .unwrap_or(&import.path)
                        .to_string()
                };
                context.add_import_symbol(name, symbol_id, false); // C# doesn't have type-only imports
            }
        }

        // Add file's own symbols
        let file_symbols =
            document_index
                .find_symbols_by_file(file_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in file_symbols {
            if self.is_resolvable_symbol(&symbol) {
                context.add_symbol_with_context(
                    symbol.name.to_string(),
                    symbol.id,
                    symbol.scope_context.as_ref(),
                );

                // Add by module path for cross-module resolution
                if let Some(ref module_path) = symbol.module_path {
                    context.add_symbol(
                        module_path.to_string(),
                        symbol.id,
                        crate::parsing::ScopeLevel::Global,
                    );
                }
            }
        }

        // Add visible symbols from other files
        let all_symbols =
            document_index
                .get_all_symbols(10000)
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_all_symbols".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in all_symbols {
            if symbol.file_id != file_id && self.is_symbol_visible_from_file(&symbol, file_id) {
                let scope_level = match symbol.visibility {
                    Visibility::Public => crate::parsing::ScopeLevel::Global,
                    _ => crate::parsing::ScopeLevel::Module,
                };

                context.add_symbol(symbol.name.to_string(), symbol.id, scope_level);

                if let Some(ref module_path) = symbol.module_path {
                    context.add_symbol(
                        module_path.to_string(),
                        symbol.id,
                        crate::parsing::ScopeLevel::Global,
                    );
                }
            }
        }

        Ok(Box::new(context))
    }

    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // C# symbols that are always resolvable
        let always_resolvable = matches!(
            symbol.kind,
            SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Method
                | SymbolKind::Field
        );

        if always_resolvable {
            return true;
        }

        // Check scope context
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember { .. } => {
                    matches!(symbol.visibility, Visibility::Public | Visibility::Module)
                }
            }
        } else {
            // Fallback for symbols without scope context
            matches!(
                symbol.kind,
                SymbolKind::TypeAlias | SymbolKind::Constant | SymbolKind::Variable
            )
        }
    }

    fn resolve_import(
        &self,
        import: &crate::parsing::Import,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // C# import resolution for using directives
        // using System.Collections.Generic; -> look for symbols in that namespace
        // using Alias = System.Collections.Generic.List; -> look for List in that namespace

        if let Some(alias) = &import.alias {
            // Using alias - look for the specific symbol
            if let Ok(cands) = document_index.find_symbols_by_name(alias, None) {
                for s in cands {
                    if let Some(module_path) = &s.module_path {
                        if module_path.as_ref() == import.path {
                            return Some(s.id);
                        }
                    }
                }
            }
        } else {
            // Regular using directive - look for any public symbol in that namespace
            if let Ok(symbols) = document_index.get_all_symbols(10000) {
                for symbol in symbols {
                    if let Some(module_path) = &symbol.module_path {
                        if module_path.as_ref().starts_with(&import.path)
                            && matches!(symbol.visibility, Visibility::Public)
                        {
                            return Some(symbol.id);
                        }
                    }
                }
            }
        }

        None
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        _importing_module: Option<&str>,
    ) -> bool {
        // C# namespace matching
        // Exact match or symbol is in a sub-namespace of the import
        import_path == symbol_module_path
            || symbol_module_path.starts_with(&format!("{import_path}."))
    }
}
