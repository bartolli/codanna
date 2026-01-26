//! Lua language parser implementation
//!
//! This module provides Lua language support for Codanna's code intelligence system,
//! enabling symbol extraction, relationship tracking, and semantic analysis of Lua codebases.
//!
//! ## Overview
//!
//! The Lua parser uses tree-sitter-lua to provide support for Lua language features
//! including functions, tables, metatables, and module patterns.
//!
//! ## Key Features
//!
//! ### Symbol Extraction
//! - **Functions**: Global and local function declarations
//! - **Methods**: Colon-syntax methods with implicit self
//! - **Tables**: Table constructors and class-like patterns
//! - **Variables**: Local and global variable declarations
//! - **Fields**: Table field definitions
//!
//! ### Lua-Specific Language Features
//! - **Module System**: require() pattern for imports
//! - **Visibility**: local keyword and underscore prefix conventions
//! - **Tables as Classes**: Metatable-based OOP patterns
//! - **Colon Syntax**: Method calls with implicit self parameter
//!
//! ## Module Components
//!
//! - [`parser`]: Core tree-sitter integration and symbol extraction
//! - [`behavior`]: Lua-specific language behaviors and formatting rules
//! - [`definition`]: Language registration and tree-sitter node mappings
//! - [`resolution`]: Symbol resolution and scope management
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use codanna::parsing::lua::{LuaParser, LuaBehavior};
//! use codanna::parsing::{LanguageParser, LanguageBehavior};
//!
//! let parser = LuaParser::new();
//! let behavior = LuaBehavior::new();
//! ```
//!
//! ## Documentation References
//!
//! - [`definition`] module for complete AST node mappings
//! - `contributing/parsers/lua/NODE_MAPPING.md` for tree-sitter node types
//! - `tests/fixtures/lua/` for comprehensive code examples

pub mod audit;
pub mod behavior;
pub mod definition;
pub mod parser;
pub mod resolution;

pub use behavior::LuaBehavior;
pub use definition::LuaLanguage;
pub use parser::LuaParser;
pub use resolution::{LuaInheritanceResolver, LuaResolutionContext};

pub(crate) use definition::register;
