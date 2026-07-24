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
        let parser = TypeScriptParser::new().map_err(|e| IndexError::General(e.to_string()))?;
        let parser = match settings
            .languages
            .get("typescript")
            .and_then(|c| c.parser_options.get("function_wrappers"))
        {
            None => parser,
            Some(value) => {
                let wrappers = value
                    .as_array()
                    .and_then(|items| {
                        items
                            .iter()
                            .map(|i| i.as_str().map(String::from))
                            .collect::<Option<Vec<String>>>()
                    })
                    .ok_or_else(|| {
                        IndexError::General(
                            "languages.typescript.parser_options.function_wrappers must be an array of strings".to_string(),
                        )
                    })?;
                parser.with_function_wrappers(wrappers)
            }
        };
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
