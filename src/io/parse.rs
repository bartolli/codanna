//! Parse command output module for AST visualization
//!
//! Outputs tree-sitter AST nodes in JSON Lines format for external analysis.

use crate::io::ExitCode;
use serde::Serialize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during parse command execution
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("File not found: {path}\nSuggestion: Check if the file exists and the path is correct")]
    FileNotFound { path: String },

    #[error(
        "Unable to detect language from file extension: {extension}\nSuggestion: Use a supported file extension (rs, py, ts, tsx, js, jsx, php, go, c, cpp)"
    )]
    UnsupportedLanguage { extension: String },

    #[error(
        "Failed to read file: {path}\n{source}\nSuggestion: Check file permissions and ensure it's a valid text file"
    )]
    FileReadError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "Failed to create output file: {path}\n{source}\nSuggestion: Check write permissions for the directory"
    )]
    OutputCreateError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "Failed to write output\n{source}\nSuggestion: Check disk space and output file permissions"
    )]
    OutputWriteError {
        #[source]
        source: std::io::Error,
    },

    #[error(
        "Failed to set parser language\n{source}\nSuggestion: This is an internal error, please report it"
    )]
    LanguageSetupError {
        #[source]
        source: tree_sitter::LanguageError,
    },

    #[error(
        "Failed to parse file\nSuggestion: Check if the file has valid syntax for the detected language"
    )]
    ParseFailure,

    #[error(
        "Failed to serialize node to JSON\n{source}\nSuggestion: This is an internal error, please report it"
    )]
    SerializationError {
        #[from]
        source: serde_json::Error,
    },
}

impl ParseError {
    /// Convert parse error to appropriate exit code
    pub fn exit_code(&self) -> ExitCode {
        match self {
            ParseError::FileNotFound { .. } => ExitCode::NotFound,
            ParseError::UnsupportedLanguage { .. } => ExitCode::UnsupportedOperation,
            ParseError::FileReadError { .. } => ExitCode::IoError,
            ParseError::OutputCreateError { .. } => ExitCode::IoError,
            ParseError::OutputWriteError { .. } => ExitCode::IoError,
            ParseError::LanguageSetupError { .. } => ExitCode::ParseError,
            ParseError::ParseFailure => ExitCode::ParseError,
            ParseError::SerializationError { .. } => ExitCode::GeneralError,
        }
    }
}

/// Information about a single AST node
#[derive(Debug, Serialize)]
pub struct NodeInfo {
    /// Node type name (e.g., "function_declaration", "identifier")
    pub node: String,
    /// Start position [line, column]
    pub start: [usize; 2],
    /// End position [line, column]  
    pub end: [usize; 2],
    /// Tree-sitter node kind ID
    pub kind_id: u16,
    /// Depth in the AST (0 = root)
    pub depth: usize,
    /// Unique node ID within the file
    pub id: usize,
    /// Parent node ID (omitted for root)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// Optional name for identifiers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Output handler for parse command
pub struct ParseOutput {
    writer: Box<dyn Write>,
}

impl ParseOutput {
    /// Create a new parse output handler
    pub fn new(output_path: Option<PathBuf>) -> Result<Self, ParseError> {
        let writer: Box<dyn Write> = if let Some(path) = output_path {
            Box::new(
                std::fs::File::create(&path).map_err(|e| ParseError::OutputCreateError {
                    path: path.display().to_string(),
                    source: e,
                })?,
            )
        } else {
            Box::new(io::stdout())
        };

        Ok(Self { writer })
    }

    /// Write a node to the output in JSONL format
    pub fn write_node(&mut self, node: &NodeInfo) -> Result<(), ParseError> {
        // Serialize to JSON and write as a single line
        let json = serde_json::to_string(node)?;
        writeln!(self.writer, "{json}").map_err(|e| ParseError::OutputWriteError { source: e })?;
        self.writer
            .flush()
            .map_err(|e| ParseError::OutputWriteError { source: e })?;
        Ok(())
    }
}

/// Walk the AST and stream nodes with hierarchy tracking
pub fn walk_and_stream(
    node: tree_sitter::Node,
    code: &str,
    writer: &mut ParseOutput,
    depth: usize,
    parent_id: Option<usize>,
    node_counter: &mut usize,
    max_depth: Option<usize>,
    all_nodes: bool,
) -> Result<(), ParseError> {
    let current_id = *node_counter;
    *node_counter += 1;

    // Skip anonymous nodes unless all_nodes is true
    // Anonymous nodes are typically punctuation, operators, and keywords
    // Named nodes follow the pattern: lowercase letters, underscores, or longer keywords
    let node_kind = node.kind();
    let is_named_node = node.is_named();

    // Only output if we're showing all nodes OR this is a named node
    if all_nodes || is_named_node {
        // Extract optional name for identifier nodes
        let name = if node_kind == "identifier"
            || node_kind == "property_identifier"
            || node_kind == "type_identifier"
            || node_kind == "field_identifier"
        {
            node.utf8_text(code.as_bytes()).ok().map(String::from)
        } else {
            None
        };

        let info = NodeInfo {
            node: node_kind.to_string(),
            start: [node.start_position().row, node.start_position().column],
            end: [node.end_position().row, node.end_position().column],
            kind_id: node.kind_id(),
            depth,
            id: current_id,
            parent: parent_id,
            name,
        };

        writer.write_node(&info)?;
    }

    // Stop traversing if we've reached max depth
    if let Some(max) = max_depth {
        if depth >= max {
            return Ok(());
        }
    }

    // Recursively walk children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_and_stream(
            child,
            code,
            writer,
            depth + 1,
            Some(current_id),
            node_counter,
            max_depth,
            all_nodes,
        )?;
    }

    Ok(())
}

/// Execute the parse command with proper error handling
pub fn execute_parse(
    file_path: &Path,
    output_path: Option<PathBuf>,
    max_depth: Option<usize>,
    all_nodes: bool,
) -> Result<(), ParseError> {
    use crate::parsing::Language;

    // Check if file exists
    if !file_path.exists() {
        return Err(ParseError::FileNotFound {
            path: file_path.display().to_string(),
        });
    }

    // Detect language from file extension
    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    let language =
        Language::from_extension(extension).ok_or_else(|| ParseError::UnsupportedLanguage {
            extension: extension.to_string(),
        })?;

    // Read file content with lossy UTF-8 conversion
    // This handles files with invalid UTF-8 sequences by replacing them with �
    let bytes = std::fs::read(file_path).map_err(|e| ParseError::FileReadError {
        path: file_path.display().to_string(),
        source: e,
    })?;
    let code = String::from_utf8_lossy(&bytes).into_owned();

    // Create tree-sitter parser for the language
    let mut parser = tree_sitter::Parser::new();
    let ts_language = match language {
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::C => tree_sitter_c::LANGUAGE.into(),
        Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Language::Gdscript => tree_sitter_gdscript::LANGUAGE.into(),
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::Kotlin => tree_sitter_kotlin::language(),
        Language::Swift => tree_sitter_swift::LANGUAGE.into(),
    };

    parser
        .set_language(&ts_language)
        .map_err(|e| ParseError::LanguageSetupError { source: e })?;

    // Parse the code
    let tree = parser.parse(&code, None).ok_or(ParseError::ParseFailure)?;

    // Create output handler
    let mut output_handler = ParseOutput::new(output_path)?;

    // Walk the tree and output nodes
    let mut node_counter = 0;
    walk_and_stream(
        tree.root_node(),
        &code,
        &mut output_handler,
        0,
        None,
        &mut node_counter,
        max_depth,
        all_nodes,
    )?;

    Ok(())
}
