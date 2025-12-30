//! CLI module for the codebase intelligence system.
//!
//! Provides command-line interface parsing and command dispatch.
//! Extracted from main.rs for better modularity.

pub mod args;
pub mod commands;

pub use args::{Cli, Commands, DocumentAction, PluginAction, RetrieveQuery};
