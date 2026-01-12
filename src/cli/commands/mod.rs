//! Command implementations for the CLI.
//!
//! Each command is implemented in its own module.
//! Commands are progressively migrated from main.rs.

pub mod benchmark;
pub mod completions;
pub mod directories;
pub mod documents;
pub mod index;
pub mod index_parallel;
pub mod init;
pub mod mcp;
pub mod parse;
pub mod plugin;
pub mod profile;
pub mod retrieve;
pub mod serve;
