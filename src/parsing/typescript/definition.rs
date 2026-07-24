//! TypeScript language definition and registration

use crate::parsing::{
    LanguageBehavior, LanguageDefinition, LanguageId, LanguageParser, LanguageRegistry,
};
use crate::{IndexError, IndexResult, Settings};
use std::sync::Arc;

use super::{TypeScriptBehavior, TypeScriptParser};

/// TypeScript language definition
pub struct TypeScriptLanguage;

impl LanguageDefinition for TypeScriptLanguage {
    fn id(&self) -> LanguageId {
        LanguageId::new("typescript")
    }

    fn name(&self) -> &'static str {
        "TypeScript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "mts", "cts"]
    }

    fn create_parser(&self, settings: &Settings) -> IndexResult<Box<dyn LanguageParser>> {
        let mut parser = TypeScriptParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        // Wire the configurable function-wrapper list
        // ([languages.typescript].parser_options.function_wrappers) so
        // higher-order-wrapped functions (Effect.fn/gen/sync, React memo, etc.)
        // are indexed as callable functions. Empty by default.
        if let Some(wrappers) = settings
            .languages
            .get("typescript")
            .and_then(|c| c.parser_options.get("function_wrappers"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
        {
            parser.set_function_wrappers(wrappers);
        }
        Ok(Box::new(parser))
    }

    fn create_behavior(&self) -> Box<dyn LanguageBehavior> {
        Box::new(TypeScriptBehavior::new())
    }

    fn default_enabled(&self) -> bool {
        true // Enable TypeScript by default
    }

    fn is_enabled(&self, settings: &Settings) -> bool {
        settings
            .languages
            .get("typescript")
            .map(|config| config.enabled)
            .unwrap_or(self.default_enabled())
    }
}

/// Register TypeScript language with the registry
pub(crate) fn register(registry: &mut LanguageRegistry) {
    registry.register(Arc::new(TypeScriptLanguage));
}
