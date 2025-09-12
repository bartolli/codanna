//! TypeScript-specific language behavior implementation

use crate::debug_print;
use crate::parsing::LanguageBehavior;
use crate::parsing::behavior_state::{BehaviorState, StatefulBehavior};
use crate::parsing::resolution::{InheritanceResolver, ResolutionScope};
use crate::project_resolver::persist::{ResolutionPersistence, ResolutionRules};
use crate::storage::DocumentIndex;
use crate::types::FileId;
use crate::{SymbolId, Visibility};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tree_sitter::Language;

use super::resolution::{TypeScriptInheritanceResolver, TypeScriptResolutionContext};

/// TypeScript language behavior implementation
#[derive(Clone)]
pub struct TypeScriptBehavior {
    state: BehaviorState,
}

impl TypeScriptBehavior {
    /// Create a new TypeScript behavior instance
    pub fn new() -> Self {
        Self {
            state: BehaviorState::new(),
        }
    }

    /// Load project resolution rules for a file from the persisted index
    ///
    /// Uses a thread-local cache to avoid repeated disk reads.
    /// Cache is invalidated after 1 second to pick up changes.
    fn load_project_rules_for_file(&self, file_id: FileId) -> Option<ResolutionRules> {
        thread_local! {
            static RULES_CACHE: RefCell<Option<(Instant, crate::project_resolver::persist::ResolutionIndex)>> = const { RefCell::new(None) };
        }

        RULES_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();

            // Check if cache is fresh (< 1 second old)
            let needs_reload = if let Some((timestamp, _)) = *cache {
                timestamp.elapsed() >= Duration::from_secs(1)
            } else {
                true
            };

            // Load fresh from disk if needed
            if needs_reload {
                let persistence = ResolutionPersistence::new(Path::new(".codanna"));
                if let Ok(index) = persistence.load("typescript") {
                    *cache = Some((Instant::now(), index));
                } else {
                    // No index file exists yet - that's OK
                    return None;
                }
            }

            // Get rules for the file
            if let Some((_, ref index)) = *cache {
                // Get the file path for this FileId from our behavior state
                if let Some(file_path) = self.state.get_file_path(file_id) {
                    // Find the config that applies to this file
                    if let Some(config_path) = index.get_config_for_file(&file_path) {
                        return index.rules.get(config_path).cloned();
                    }
                }

                // Fallback: return any rules we have (for tests without proper file registration)
                index.rules.values().next().cloned()
            } else {
                None
            }
        })
    }
}

impl Default for TypeScriptBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulBehavior for TypeScriptBehavior {
    fn state(&self) -> &BehaviorState {
        &self.state
    }
}

impl LanguageBehavior for TypeScriptBehavior {
    fn configure_symbol(&self, symbol: &mut crate::Symbol, module_path: Option<&str>) {
        // Preserve parser-derived visibility (export detection), only set module path.
        if let Some(path) = module_path {
            let full_path = self.format_module_path(path, &symbol.name);
            symbol.module_path = Some(full_path.into());
        }
    }

    fn format_module_path(&self, base_path: &str, _symbol_name: &str) -> String {
        // TypeScript uses file paths as module paths, not including the symbol name
        // All symbols in the same file share the same module path for visibility
        base_path.to_string()
    }

    fn get_language(&self) -> Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }
    fn module_separator(&self) -> &'static str {
        "."
    }

    fn module_path_from_file(&self, file_path: &Path, project_root: &Path) -> Option<String> {
        // Convert file path to module path relative to project root
        // e.g., src/utils/helpers.ts -> src.utils.helpers

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
            .trim_end_matches(".ts")
            .trim_end_matches(".tsx")
            .trim_end_matches(".mts")
            .trim_end_matches(".cts")
            .trim_end_matches(".d.ts")
            .trim_end_matches("/index");

        // Replace path separators with module separators
        Some(module_path.replace('/', "."))
    }

    fn parse_visibility(&self, signature: &str) -> Visibility {
        // TypeScript visibility modifiers
        if signature.contains("export ") || signature.contains("export default") {
            Visibility::Public
        } else if signature.contains("private ") || signature.contains("#") {
            Visibility::Private
        } else if signature.contains("protected ") {
            // TypeScript has protected but Rust's Visibility enum doesn't
            // Map protected to Module visibility as a reasonable approximation
            Visibility::Module
        } else {
            // Default visibility for TypeScript symbols
            // Module-level symbols are private by default unless exported
            Visibility::Private
        }
    }

    fn supports_traits(&self) -> bool {
        true // TypeScript has interfaces
    }

    fn supports_inherent_methods(&self) -> bool {
        true // TypeScript has class methods
    }

    // TypeScript-specific resolution overrides

    fn create_resolution_context(&self, file_id: FileId) -> Box<dyn ResolutionScope> {
        Box::new(TypeScriptResolutionContext::new(file_id))
    }

    fn create_inheritance_resolver(&self) -> Box<dyn InheritanceResolver> {
        Box::new(TypeScriptInheritanceResolver::new())
    }

    fn inheritance_relation_name(&self) -> &'static str {
        // TypeScript uses both "extends" and "implements"
        // Default to "extends" as it's more general
        "extends"
    }

    fn map_relationship(&self, language_specific: &str) -> crate::relationship::RelationKind {
        use crate::relationship::RelationKind;

        match language_specific {
            "extends" => RelationKind::Extends,
            "implements" => RelationKind::Implements,
            "uses" => RelationKind::Uses,
            "calls" => RelationKind::Calls,
            "defines" => RelationKind::Defines,
            _ => RelationKind::References,
        }
    }

    // Override import tracking methods to use state

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
        // Use tracked imports and module path to map unresolved calls to externals.
        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "DEBUG[TS]: resolve_external_call_target to='{to_name}' file_id={from_file:?}"
            );
        }
        // Cases:
        // - Namespace import: `import * as React from 'react'` -> React.useState
        // - Default import:  `import React from 'react'` -> React.useState
        // - Named import:    `import { useState } from 'react'` -> useState

        let imports = self.get_imports_for_file(from_file);
        if imports.is_empty() {
            if crate::config::is_global_debug_enabled() {
                eprintln!("DEBUG[TS]: no imports tracked for file {from_file:?}");
            }
            return None;
        }

        // Helper: normalize TS import path relative to importing module
        fn parent_module(m: &str) -> String {
            let mut parts: Vec<&str> = if m.is_empty() {
                Vec::new()
            } else {
                m.split('.').collect()
            };
            if !parts.is_empty() {
                parts.pop();
            }
            parts.join(".")
        }
        fn normalize_ts_import(import_path: &str, importing_mod: &str) -> String {
            if import_path.starts_with("./") {
                let base_owned = parent_module(importing_mod);
                let base = base_owned.as_str();
                let rel = import_path.trim_start_matches("./").replace('/', ".");
                if base.is_empty() {
                    rel
                } else {
                    format!("{base}.{rel}")
                }
            } else if import_path.starts_with("../") {
                let base_owned = parent_module(importing_mod);
                let mut parts: Vec<&str> = base_owned.split('.').collect();
                let mut rest = import_path;
                while rest.starts_with("../") {
                    if !parts.is_empty() {
                        parts.pop();
                    }
                    rest = &rest[3..];
                }
                let rest = rest.trim_start_matches("./").replace('/', ".");
                let mut combined = parts.join(".");
                if !rest.is_empty() {
                    combined = if combined.is_empty() {
                        rest
                    } else {
                        format!("{combined}.{rest}")
                    };
                }
                combined
            } else {
                import_path.replace('/', ".")
            }
        }

        let importing_module = self.get_module_path_for_file(from_file).unwrap_or_default();
        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "DEBUG[TS]: importing_module='{}', imports={}",
                importing_module,
                imports.len()
            );
            for imp in &imports {
                eprintln!(
                    "  import path='{}' alias={:?} glob={} type_only={}",
                    imp.path, imp.alias, imp.is_glob, imp.is_type_only
                );
            }
        }

        // Namespace form only: Alias.member from `import * as Alias from 'module'`
        if let Some((alias, member)) = to_name.split_once('.') {
            for import in &imports {
                // Guard: only namespace imports (is_glob == true)
                if import.is_glob {
                    if let Some(a) = &import.alias {
                        if a == alias {
                            let module_path = normalize_ts_import(&import.path, &importing_module);
                            if crate::config::is_global_debug_enabled() {
                                eprintln!(
                                    "DEBUG[TS]: mapped namespace alias.member: {alias}.{member} -> module '{module_path}'"
                                );
                            }
                            return Some((module_path, member.to_string()));
                        }
                    }
                }
            }
        } else {
            // Named import form only (is_glob == false): e.g., import { useState } from 'react'
            for import in &imports {
                if !import.is_glob {
                    if let Some(a) = &import.alias {
                        if a == to_name {
                            let module_path = normalize_ts_import(&import.path, &importing_module);
                            if crate::config::is_global_debug_enabled() {
                                eprintln!(
                                    "DEBUG[TS]: mapped named import: {to_name} -> module '{module_path}'"
                                );
                            }
                            return Some((module_path, to_name.to_string()));
                        }
                    }
                }
            }
        }

        None
    }

    fn create_external_symbol(
        &self,
        document_index: &mut crate::storage::DocumentIndex,
        module_path: &str,
        symbol_name: &str,
        language_id: crate::parsing::LanguageId,
    ) -> crate::IndexResult<crate::SymbolId> {
        use crate::storage::MetadataKey;
        use crate::{IndexError, Symbol, SymbolId, SymbolKind, Visibility};

        // If symbol already exists with same name and module_path, reuse it
        if let Ok(cands) = document_index.find_symbols_by_name(symbol_name, None) {
            debug_print!(
                self,
                "Found {} existing symbols with name '{}'",
                cands.len(),
                symbol_name
            );
            for s in cands {
                if let Some(mp) = &s.module_path {
                    debug_print!(
                        self,
                        "Checking symbol '{}' module '{}' vs '{}' (ID: {:?})",
                        s.name,
                        mp.as_ref(),
                        module_path,
                        s.id
                    );
                    if mp.as_ref() == module_path {
                        debug_print!(
                            self,
                            "Reusing existing external symbol '{}' in module '{}' with ID {:?}",
                            symbol_name,
                            module_path,
                            s.id
                        );
                        return Ok(s.id);
                    }
                }
            }
        }

        // Compute virtual file path
        let mut path_buf = String::from(".codanna/external/");
        path_buf.push_str(&module_path.replace('.', "/"));
        path_buf.push_str(".d.ts");
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

        // Allocate a new symbol id
        let next_id =
            document_index
                .get_next_symbol_id()
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_next_symbol_id".to_string(),
                    cause: e.to_string(),
                })?;
        let symbol_id = SymbolId::new(next_id).ok_or(IndexError::SymbolIdExhausted)?;

        // Build and index the stub symbol
        let mut symbol = Symbol::new(
            symbol_id,
            symbol_name.to_string(),
            SymbolKind::Function,
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

        // Update the symbol counter metadata
        document_index
            .store_metadata(MetadataKey::SymbolCounter, symbol_id.value() as u64)
            .map_err(|e| IndexError::TantivyError {
                operation: "store_metadata(SymbolCounter)".to_string(),
                cause: e.to_string(),
            })?;

        debug_print!(
            self,
            "Created new external symbol '{}' in module '{}' with ID {:?}",
            symbol_name,
            module_path,
            symbol_id
        );

        Ok(symbol_id)
    }

    fn build_resolution_context(
        &self,
        file_id: FileId,
        document_index: &DocumentIndex,
    ) -> crate::error::IndexResult<Box<dyn ResolutionScope>> {
        use crate::error::IndexError;

        // Create TypeScript-specific resolution context
        let mut context = TypeScriptResolutionContext::new(file_id);

        // 1. Add imported symbols (using behavior's tracked imports)
        let imports = self.get_imports_for_file(file_id);
        // Collect namespace imports for qualified-name precomputation
        let mut namespace_imports: Vec<(String, String)> = Vec::new(); // (alias, normalized_module)

        for import in imports {
            if let Some(symbol_id) = self.resolve_import(&import, document_index) {
                // Use alias if provided, otherwise use the last segment of the path
                let name = if let Some(alias) = &import.alias {
                    alias.clone()
                } else {
                    import
                        .path
                        .split(self.module_separator())
                        .last()
                        .unwrap_or(&import.path)
                        .to_string()
                };

                // Use the is_type_only field to determine where to place the import
                context.add_import_symbol(name, symbol_id, import.is_type_only);
            } else if import.is_glob {
                // Namespace import that didn't resolve to a concrete symbol set.
                // Record alias -> target module mapping for qualified-name resolution.
                if let Some(alias) = &import.alias {
                    // Normalize target module relative to current file's module path
                    let importing_module =
                        self.get_module_path_for_file(file_id).unwrap_or_default();
                    let normalized = {
                        // Reuse normalize helper from resolve_import
                        fn parent_module(m: &str) -> String {
                            let mut parts: Vec<&str> = if m.is_empty() {
                                Vec::new()
                            } else {
                                m.split('.').collect()
                            };
                            if !parts.is_empty() {
                                parts.pop();
                            }
                            parts.join(".")
                        }
                        let p = &import.path;
                        if p.starts_with("./") {
                            let base = parent_module(&importing_module);
                            let rel = p.trim_start_matches("./").replace('/', ".");
                            if base.is_empty() {
                                rel
                            } else {
                                format!("{base}.{rel}")
                            }
                        } else if p.starts_with("../") {
                            let base_owned = parent_module(&importing_module);
                            let mut parts: Vec<&str> = base_owned.split('.').collect();
                            let mut rest = p.as_str();
                            while rest.starts_with("../") {
                                if !parts.is_empty() {
                                    parts.pop();
                                }
                                rest = &rest[3..];
                            }
                            let rest = rest.trim_start_matches("./").replace('/', ".");
                            let mut combined = parts.join(".");
                            if !rest.is_empty() {
                                combined = if combined.is_empty() {
                                    rest
                                } else {
                                    format!("{combined}.{rest}")
                                };
                            }
                            combined
                        } else {
                            p.replace('/', ".")
                        }
                    };
                    namespace_imports.push((alias.clone(), normalized));
                }
            }
        }

        // 2. Add file's module-level symbols with proper scope context
        let file_symbols =
            document_index
                .find_symbols_by_file(file_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file".to_string(),
                    cause: e.to_string(),
                })?;

        for symbol in file_symbols {
            if self.is_resolvable_symbol(&symbol) {
                // Use the new method that respects scope_context for hoisting
                context.add_symbol_with_context(
                    symbol.name.to_string(),
                    symbol.id,
                    symbol.scope_context.as_ref(),
                );
            }
        }

        // 3. Add visible symbols from other files (public/exported symbols)
        // Note: This is expensive, so we limit to a reasonable number
        let all_symbols =
            document_index
                .get_all_symbols(10000)
                .map_err(|e| IndexError::TantivyError {
                    operation: "get_all_symbols".to_string(),
                    cause: e.to_string(),
                })?;

        let mut public_symbols: Vec<crate::Symbol> = Vec::new();
        for symbol in all_symbols {
            // Only add if visible from this file
            if symbol.file_id != file_id && self.is_symbol_visible_from_file(&symbol, file_id) {
                // Global symbols go to global scope, others to module scope
                let scope_level = match symbol.visibility {
                    Visibility::Public => crate::parsing::ScopeLevel::Global,
                    _ => crate::parsing::ScopeLevel::Module,
                };

                context.add_symbol(symbol.name.to_string(), symbol.id, scope_level);
                public_symbols.push(symbol);
            }
        }

        // 3.1 Precompute qualified names for namespace imports against visible symbols
        if !namespace_imports.is_empty() {
            // Downcast to access TypeScript-specific API
            if let Some(ts_ctx) = context
                .as_any_mut()
                .downcast_mut::<TypeScriptResolutionContext>()
            {
                for (alias, target_module) in namespace_imports {
                    ts_ctx.add_namespace_alias(alias.clone(), target_module.clone());
                    for sym in &public_symbols {
                        if let Some(mod_path) = &sym.module_path {
                            if mod_path.as_ref() == target_module {
                                ts_ctx.add_qualified_name(format!("{alias}.{}", sym.name), sym.id);
                            }
                        }
                    }
                }
            }
        }

        Ok(Box::new(context))
    }

    /// Build resolution context using symbol cache (fast path) with TypeScript semantics
    fn build_resolution_context_with_cache(
        &self,
        file_id: FileId,
        cache: &crate::storage::symbol_cache::ConcurrentSymbolCache,
        document_index: &DocumentIndex,
    ) -> crate::error::IndexResult<Box<dyn ResolutionScope>> {
        use crate::error::IndexError;
        // Create TypeScript-specific resolution context
        let mut context = TypeScriptResolutionContext::new(file_id);

        // Helper: normalize TS import to module path relative to importing module
        fn normalize_ts_import(import_path: &str, importing_mod: &str) -> String {
            // Helper: parent module (drop the last segment of the importing module)
            fn parent_module(m: &str) -> String {
                let mut parts: Vec<&str> = if m.is_empty() {
                    Vec::new()
                } else {
                    m.split('.').collect()
                };
                if !parts.is_empty() {
                    parts.pop();
                }
                parts.join(".")
            }

            if import_path.starts_with("./") {
                // Same directory as the file: use parent of importing module
                let base_owned = parent_module(importing_mod);
                let base = base_owned.as_str();
                let rel = import_path.trim_start_matches("./").replace('/', ".");
                if base.is_empty() {
                    rel
                } else {
                    format!("{base}.{rel}")
                }
            } else if import_path.starts_with("../") {
                // Walk up segments from the importing module's parent
                let base_owned = parent_module(importing_mod);
                let mut parts: Vec<&str> = base_owned.split('.').collect();
                let mut rest = import_path;
                while rest.starts_with("../") {
                    if !parts.is_empty() {
                        parts.pop();
                    }
                    rest = &rest[3..];
                }
                let rest = rest.trim_start_matches("./").replace('/', ".");
                let mut combined = parts.join(".");
                if !rest.is_empty() {
                    combined = if combined.is_empty() {
                        rest
                    } else {
                        format!("{combined}.{rest}")
                    };
                }
                combined
            } else {
                // Bare module or path alias; leave as-is (converted separators)
                import_path.replace('/', ".")
            }
        }

        // 1) Imports: prefer cache for imported names; skip side-effect and unnamed named imports
        let imports = self.get_imports_for_file(file_id);
        if crate::config::is_global_debug_enabled() {
            eprintln!("DEBUG: TS building context: {} imports", imports.len());
        }
        let importing_module = self.get_module_path_for_file(file_id).unwrap_or_default();
        for import in imports {
            // Determine the local name to bind in this file (alias or named import).
            let Some(local_name) = import.alias.clone() else {
                // Named imports without explicit alias are not captured individually yet.
                // Fall back to database resolution which may still assist via module-level usage.
                if crate::config::is_global_debug_enabled() {
                    eprintln!("DEBUG: TS import without alias skipped: '{}'", import.path);
                }
                continue;
            };

            let target_module = normalize_ts_import(&import.path, &importing_module);
            if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "DEBUG: TS import lookup: local='{local_name}', target_module='{target_module}'"
                );
            }

            // Try cache candidates by local name
            let mut matched: Option<SymbolId> = None;
            let candidates = cache.lookup_candidates(&local_name, 16);
            if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "DEBUG: TS cache candidates for '{}': {}",
                    local_name,
                    candidates.len()
                );
            }
            for id in candidates {
                if let Ok(Some(symbol)) = document_index.find_symbol_by_id(id) {
                    if let Some(module_path) = &symbol.module_path {
                        if module_path.as_ref() == target_module {
                            if crate::config::is_global_debug_enabled() {
                                eprintln!(
                                    "DEBUG: TS cache hit verified for '{local_name}': {id:?}"
                                );
                            }
                            matched = Some(id);
                            break;
                        }
                        if crate::config::is_global_debug_enabled() {
                            eprintln!(
                                "DEBUG: TS candidate mismatch: symbol_module='{module_path}' vs target='{target_module}'"
                            );
                        }
                    }
                }
            }

            // Fallback to DB by name if cache path match not found
            if matched.is_none() {
                if crate::config::is_global_debug_enabled() {
                    eprintln!("DEBUG: TS cache miss for '{local_name}', falling back to DB");
                }
                if let Ok(cands) = document_index.find_symbols_by_name(&local_name, None) {
                    for s in cands {
                        if let Some(module_path) = &s.module_path {
                            if module_path.as_ref() == target_module {
                                if crate::config::is_global_debug_enabled() {
                                    eprintln!(
                                        "DEBUG: TS DB match for '{}': {:?}",
                                        local_name, s.id
                                    );
                                }
                                matched = Some(s.id);
                                break;
                            }
                        }
                    }
                }
            }

            if let Some(symbol_id) = matched {
                // Respect type-only imports for proper space placement
                context.add_import_symbol(local_name, symbol_id, import.is_type_only);
            } else if crate::config::is_global_debug_enabled() {
                eprintln!(
                    "DEBUG: TS import unresolved: local='{local_name}', module='{target_module}'"
                );
            }
        }

        // 2) File's own symbols (module-level, with scope context)
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
            }
        }

        // 3) Avoid global get_all_symbols; we rely on imported files where possible
        // Collect imported files via cache to add visible symbols sparingly
        let mut imported_files = std::collections::HashSet::new();
        for import in self.get_imports_for_file(file_id) {
            if let Some(alias) = &import.alias {
                for id in cache.lookup_candidates(alias, 4) {
                    if let Ok(Some(sym)) = document_index.find_symbol_by_id(id) {
                        imported_files.insert(sym.file_id);
                    }
                }
            }
        }
        if crate::config::is_global_debug_enabled() {
            eprintln!(
                "DEBUG: TS imported files discovered via cache: {}",
                imported_files.len()
            );
        }

        for imported_file_id in imported_files {
            if imported_file_id == file_id {
                continue;
            }
            let imported_syms = document_index
                .find_symbols_by_file(imported_file_id)
                .map_err(|e| IndexError::TantivyError {
                    operation: "find_symbols_by_file for imports".to_string(),
                    cause: e.to_string(),
                })?;
            for symbol in imported_syms {
                if self.is_symbol_visible_from_file(&symbol, file_id) {
                    context.add_symbol(
                        symbol.name.to_string(),
                        symbol.id,
                        crate::parsing::ScopeLevel::Global,
                    );
                }
            }
        }

        Ok(Box::new(context))
    }

    // TypeScript-specific: Support hoisting
    fn is_resolvable_symbol(&self, symbol: &crate::Symbol) -> bool {
        use crate::SymbolKind;
        use crate::symbol::ScopeContext;

        // TypeScript hoists function declarations and class declarations
        // They can be used before their definition in the file
        let hoisted = matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
        );

        if hoisted {
            return true;
        }

        // Methods are always resolvable within their file
        if matches!(symbol.kind, SymbolKind::Method) {
            return true;
        }

        // Check scope_context for non-hoisted symbols
        if let Some(ref scope_context) = symbol.scope_context {
            match scope_context {
                ScopeContext::Module | ScopeContext::Global | ScopeContext::Package => true,
                ScopeContext::Local { .. } | ScopeContext::Parameter => false,
                ScopeContext::ClassMember => {
                    // Class members are resolvable if public or exported
                    matches!(symbol.visibility, Visibility::Public)
                }
            }
        } else {
            // Fallback for symbols without scope_context
            matches!(
                symbol.kind,
                SymbolKind::TypeAlias | SymbolKind::Constant | SymbolKind::Variable
            )
        }
    }

    // TypeScript-specific: Handle ES module imports with project resolution enhancement
    fn resolve_import(
        &self,
        import: &crate::parsing::Import,
        document_index: &DocumentIndex,
    ) -> Option<SymbolId> {
        // Step 1: Try to enhance the import path using project rules (tsconfig paths)
        let enhanced_path = if let Some(rules) = self.load_project_rules_for_file(import.file_id) {
            // Create an enhancer to transform the path
            let enhancer = super::resolution::TypeScriptProjectEnhancer::new(rules);
            use crate::parsing::resolution::ProjectResolutionEnhancer;
            enhancer
                .enhance_import_path(&import.path, import.file_id)
                .unwrap_or_else(|| import.path.clone())
        } else {
            import.path.clone()
        };

        // Prefer resolving by the imported local name when available (named/default imports)
        let importing_module = self
            .get_module_path_for_file(import.file_id)
            .unwrap_or_default();

        // Normalize import path to module path (relative to importing module)
        fn normalize_ts_import(import_path: &str, importing_mod: &str) -> String {
            fn parent_module(m: &str) -> String {
                let mut parts: Vec<&str> = if m.is_empty() {
                    Vec::new()
                } else {
                    m.split('.').collect()
                };
                if !parts.is_empty() {
                    parts.pop();
                }
                parts.join(".")
            }
            if import_path.starts_with("./") {
                let base_owned = parent_module(importing_mod);
                let base = base_owned.as_str();
                let rel = import_path.trim_start_matches("./").replace('/', ".");
                if base.is_empty() {
                    rel
                } else {
                    format!("{base}.{rel}")
                }
            } else if import_path.starts_with("../") {
                let base_owned = parent_module(importing_mod);
                let mut parts: Vec<&str> = base_owned.split('.').collect();
                let mut rest = import_path;
                while rest.starts_with("../") {
                    if !parts.is_empty() {
                        parts.pop();
                    }
                    rest = &rest[3..];
                }
                let rest = rest.trim_start_matches("./").replace('/', ".");
                let mut combined = parts.join(".");
                if !rest.is_empty() {
                    combined = if combined.is_empty() {
                        rest
                    } else {
                        format!("{combined}.{rest}")
                    };
                }
                combined
            } else {
                import_path.replace('/', ".")
            }
        }

        let target_module = normalize_ts_import(&enhanced_path, &importing_module);

        if let Some(local_name) = &import.alias {
            if let Ok(cands) = document_index.find_symbols_by_name(local_name, None) {
                for s in cands {
                    if let Some(module_path) = &s.module_path {
                        if module_path.as_ref() == target_module {
                            return Some(s.id);
                        }
                    }
                }
            }
            None
        } else {
            // Namespace or side-effect import: cannot map to a single symbol reliably
            None
        }
    }

    fn get_module_path_for_file(&self, file_id: FileId) -> Option<String> {
        // Use the BehaviorState to get module path (O(1) lookup)
        self.state.get_module_path(file_id)
    }

    fn import_matches_symbol(
        &self,
        import_path: &str,
        symbol_module_path: &str,
        importing_module: Option<&str>,
    ) -> bool {
        // Helper function to normalize path separators to dots
        fn normalize_path(path: &str) -> String {
            path.replace('/', ".")
        }

        // Helper function to resolve relative path to absolute module path
        fn resolve_relative_path(import_path: &str, importing_mod: &str) -> String {
            if import_path.starts_with("./") {
                // Same directory import
                let relative = import_path.trim_start_matches("./");
                let normalized = normalize_path(relative);

                if importing_mod.is_empty() {
                    normalized
                } else {
                    format!("{importing_mod}.{normalized}")
                }
            } else if import_path.starts_with("../") {
                // Parent directory import
                // Start with the importing module parts as owned strings
                let mut module_parts: Vec<String> =
                    importing_mod.split('.').map(|s| s.to_string()).collect();

                let mut path_remaining: &str = import_path;

                // Navigate up for each '../'
                while path_remaining.starts_with("../") {
                    if !module_parts.is_empty() {
                        module_parts.pop();
                    }
                    // If we've gone above the module root, we just continue
                    // This handles cases like ../../../some/path from a shallow module
                    path_remaining = &path_remaining[3..];
                }

                // Add the remaining path
                if !path_remaining.is_empty() {
                    let normalized = normalize_path(path_remaining);
                    module_parts.extend(
                        normalized
                            .split('.')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                    );
                }

                module_parts.join(".")
            } else {
                // Not a relative path, return as-is
                import_path.to_string()
            }
        }

        // Helper function to check if path matches with optional index resolution
        fn matches_with_index(candidate: &str, target: &str) -> bool {
            candidate == target || format!("{candidate}.index") == target
        }

        // Case 1: Exact match (most common case, check first for performance)
        if import_path == symbol_module_path {
            return true;
        }

        // Case 2: Only do complex matching if we have the importing module context
        if let Some(importing_mod) = importing_module {
            // TypeScript import resolution differs from Rust:
            // - Relative imports start with './' or '../'
            // - Absolute imports are package names or path aliases

            if import_path.starts_with("./") || import_path.starts_with("../") {
                // Resolve relative path to absolute module path
                let resolved = resolve_relative_path(import_path, importing_mod);

                // Check if it matches (with or without index)
                if matches_with_index(&resolved, symbol_module_path) {
                    return true;
                }
            }
            // else: bare module imports and scoped packages
            // These need exact match for now (TODO: implement proper resolution)
        }

        false
    }
}
