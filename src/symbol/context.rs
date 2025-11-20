//! Symbol context aggregation for comprehensive metadata display

use crate::relationship::RelationshipMetadata;
use crate::{Symbol, Visibility};
use bitflags::bitflags;
use serde::Serialize;
use std::fmt;

/// Comprehensive context for a symbol including all relationships
#[derive(Debug, Clone, Serialize)]
pub struct SymbolContext {
    /// The symbol itself with all its metadata
    pub symbol: Symbol,
    /// Resolved file path for easy navigation
    pub file_path: String,
    /// All relationships this symbol has
    pub relationships: SymbolRelationships,
}

/// Container for all types of symbol relationships
#[derive(Debug, Clone, Default, Serialize)]
pub struct SymbolRelationships {
    /// What traits this type implements
    pub implements: Option<Vec<Symbol>>,
    /// What types implement this trait
    pub implemented_by: Option<Vec<Symbol>>,
    /// What base class(es) this class extends
    pub extends: Option<Vec<Symbol>>,
    /// What classes extend this base class
    pub extended_by: Option<Vec<Symbol>>,
    /// What types this symbol uses
    pub uses: Option<Vec<Symbol>>,
    /// What symbols use this type
    pub used_by: Option<Vec<Symbol>>,
    /// What methods/fields this symbol defines
    pub defines: Option<Vec<Symbol>>,
    /// What this symbol calls (with relationship metadata including call site location)
    pub calls: Option<Vec<(Symbol, Option<RelationshipMetadata>)>>,
    /// What calls this symbol (with relationship metadata including call site location)
    pub called_by: Option<Vec<(Symbol, Option<RelationshipMetadata>)>>,
}

bitflags! {
    /// Flags to control what context to include
    pub struct ContextIncludes: u8 {
        const IMPLEMENTATIONS = 0b00000001;
        const DEFINITIONS    = 0b00000010;
        const CALLS         = 0b00000100;
        const CALLERS       = 0b00001000;
        const EXTENDS       = 0b00010000;
        const USES          = 0b00100000;
        const ALL           = 0b00111111;
    }
}

impl fmt::Display for SymbolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let formatted = self.format_full("");

        if formatted.ends_with('\n') {
            write!(f, "{}", &formatted[..formatted.len() - 1])
        } else {
            write!(f, "{formatted}")
        }
    }
}

impl SymbolContext {
    /// Format just the location line
    pub fn format_location(&self) -> String {
        format!(
            "{} at {}",
            self.symbol.name,
            Self::symbol_location(&self.symbol)
        )
    }

    /// Format location with type info
    pub fn format_location_with_type(&self) -> String {
        format!(
            "{:?} {} at {} [symbol_id:{}]",
            self.symbol.kind,
            self.symbol.name,
            Self::symbol_location(&self.symbol),
            self.symbol.id.value()
        )
    }

    /// Format comprehensive output
    pub fn format_full(&self, indent: &str) -> String {
        let mut output = String::new();
        self.append_header(&mut output, indent);
        self.append_metadata(&mut output, indent);
        self.append_relationships(&mut output, indent);
        output
    }

    fn append_header(&self, output: &mut String, indent: &str) {
        output.push_str(&format!(
            "{}{} ({:?}) at {} [symbol_id:{}]\n",
            indent,
            self.symbol.name,
            self.symbol.kind,
            Self::symbol_location(&self.symbol),
            self.symbol.id.value()
        ));
    }

    fn append_metadata(&self, output: &mut String, indent: &str) {
        // Module path
        if let Some(module) = self.symbol.as_module_path() {
            output.push_str(&format!("{indent}Module: {module}\n"));
        }

        if let Some(sig) = self.symbol.as_signature() {
            output.push_str(&format!("{indent}Signature:\n"));
            Self::write_multiline(output, sig, indent, 2);
        }

        // Visibility for appropriate symbols
        if !matches!(self.symbol.visibility, Visibility::Private) {
            output.push_str(&format!(
                "{}Visibility: {:?}\n",
                indent, self.symbol.visibility
            ));
        }

        // Documentation preview
        if let Some(doc) = self.symbol.as_doc_comment() {
            let preview: Vec<&str> = doc.lines().take(2).collect();
            if !preview.is_empty() {
                output.push_str(&format!("{}Doc: {}", indent, preview.join(" ")));
                if doc.lines().count() > 2 {
                    output.push_str("...");
                }
                output.push('\n');
            }
        }
    }

    fn append_relationships(&self, output: &mut String, indent: &str) {
        // Implementations
        if let Some(impls) = &self.relationships.implements {
            if !impls.is_empty() {
                output.push_str(&format!("{indent}Implements:\n"));
                for symbol in impls {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}\n",
                        indent,
                        symbol.name,
                        symbol.kind,
                        SymbolContext::symbol_location(symbol)
                    ));
                }
            }
        }

        // Implemented by
        if let Some(impl_by) = &self.relationships.implemented_by {
            if !impl_by.is_empty() {
                output.push_str(&format!(
                    "{}Implemented by {} symbol(s):\n",
                    indent,
                    impl_by.len()
                ));
                for impl_type in impl_by {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}\n",
                        indent,
                        impl_type.name,
                        impl_type.kind,
                        SymbolContext::symbol_location(impl_type)
                    ));
                }
            }
        }

        // Extends (what base class this extends)
        if let Some(extends) = &self.relationships.extends {
            if !extends.is_empty() {
                output.push_str(&format!("{indent}Extends:\n"));
                for base_class in extends {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}\n",
                        indent,
                        base_class.name,
                        base_class.kind,
                        SymbolContext::symbol_location(base_class)
                    ));
                }
            }
        }

        // Extended by (what classes extend this base class)
        if let Some(extended_by) = &self.relationships.extended_by {
            if !extended_by.is_empty() {
                output.push_str(&format!(
                    "{}Extended by {} class(es):\n",
                    indent,
                    extended_by.len()
                ));
                for derived_class in extended_by {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}\n",
                        indent,
                        derived_class.name,
                        derived_class.kind,
                        SymbolContext::symbol_location(derived_class)
                    ));
                }
            }
        }

        // Uses (what types this symbol uses)
        if let Some(uses) = &self.relationships.uses {
            if !uses.is_empty() {
                output.push_str(&format!("{indent}Uses {} type(s):\n", uses.len()));
                for used_type in uses {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}\n",
                        indent,
                        used_type.name,
                        used_type.kind,
                        SymbolContext::symbol_location(used_type)
                    ));
                }
            }
        }

        // Used by (what symbols use this type)
        if let Some(used_by) = &self.relationships.used_by {
            if !used_by.is_empty() {
                output.push_str(&format!("{}Used by {} symbol(s):\n", indent, used_by.len()));
                for using_symbol in used_by {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}\n",
                        indent,
                        using_symbol.name,
                        using_symbol.kind,
                        SymbolContext::symbol_location(using_symbol)
                    ));
                }
            }
        }

        // Methods defined
        if let Some(defines) = &self.relationships.defines {
            if !defines.is_empty() {
                output.push_str(&format!("{}Defines {} symbol(s):\n", indent, defines.len()));
                for defined in defines {
                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {}",
                        indent,
                        defined.name,
                        defined.kind,
                        SymbolContext::symbol_location(defined)
                    ));
                    if let Some(sig) = defined.as_signature() {
                        output.push('\n');
                        Self::write_multiline(output, sig, indent, 4);
                    }
                    output.push('\n');
                }
            }
        }

        // Calls
        if let Some(calls) = &self.relationships.calls {
            if !calls.is_empty() {
                output.push_str(&format!("{}Calls {} function(s):\n", indent, calls.len()));
                for (called, metadata) in calls {
                    // Use call site location from metadata if available, otherwise definition location
                    let location = if let Some(meta) = metadata {
                        if let Some(line) = meta.line {
                            format!("{}:{}", called.file_path, line.saturating_add(1))
                        } else {
                            Self::symbol_location(called)
                        }
                    } else {
                        Self::symbol_location(called)
                    };

                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {} [symbol_id:{}]",
                        indent,
                        called.name,
                        called.kind,
                        location,
                        called.id.value()
                    ));

                    // Show receiver info if available
                    if let Some(meta) = metadata {
                        if let Some(context) = &meta.context {
                            if !context.is_empty() {
                                output.push_str(&format!(" [{context}]"));
                            }
                        }
                    }
                    output.push('\n');
                }
            }
        }

        // Called by
        if let Some(callers) = &self.relationships.called_by {
            if !callers.is_empty() {
                output.push_str(&format!(
                    "{}Called by {} function(s):\n",
                    indent,
                    callers.len()
                ));
                for (caller, metadata) in callers {
                    // Use call site location from metadata if available, otherwise definition location
                    let location = if let Some(meta) = metadata {
                        if let Some(line) = meta.line {
                            format!("{}:{}", caller.file_path, line.saturating_add(1))
                        } else {
                            Self::symbol_location(caller)
                        }
                    } else {
                        Self::symbol_location(caller)
                    };

                    output.push_str(&format!(
                        "{}  - {} ({:?}) at {} [symbol_id:{}]",
                        indent,
                        caller.name,
                        caller.kind,
                        location,
                        caller.id.value()
                    ));

                    // Show receiver info if available
                    if let Some(meta) = metadata {
                        if let Some(context) = &meta.context {
                            if !context.is_empty() {
                                output.push_str(&format!(" [{context}]"));
                            }
                        }
                    }
                    output.push('\n');
                }
            }
        }
    }

    pub(crate) fn symbol_location(symbol: &Symbol) -> String {
        let start = symbol.range.start_line.saturating_add(1);
        let end = symbol.range.end_line.saturating_add(1);
        if start == end {
            format!("{}:{start}", symbol.file_path)
        } else {
            format!("{}:{start}-{end}", symbol.file_path)
        }
    }
}

impl SymbolContext {
    fn write_multiline(output: &mut String, text: &str, indent: &str, extra_spaces: usize) {
        let padding = format!("{indent}{:width$}", "", width = extra_spaces);
        for line in text.lines() {
            output.push_str(&padding);
            output.push_str(line);
            output.push('\n');
        }
    }
}
