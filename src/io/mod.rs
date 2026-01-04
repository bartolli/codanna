//! Input/Output handling for CLI and tool integration.
//!
//! This module provides:
//! - Unified output formatting (text, JSON)
//! - Consistent error handling and exit codes
//! - Future: JSON-RPC 2.0 support for IDE integration

pub mod args;
pub mod exit_code;
pub mod format;
pub mod guidance;
pub mod guidance_engine;
pub mod input;
pub mod output;
pub mod parse;
pub mod schema;
pub mod status_line;
#[cfg(test)]
mod test;

pub use exit_code::ExitCode;
pub use format::{ErrorDetails, JsonResponse, OutputFormat, ResponseMeta};
pub use output::OutputManager;
pub use schema::{EntityType, OutputData, OutputStatus, UnifiedOutput, UnifiedOutputBuilder};
pub use status_line::{
    DualProgressBar, ProgressBar, ProgressBarOptions, ProgressBarStyle, Spinner, SpinnerOptions,
};
// Future: pub use input::{JsonRpcRequest, JsonRpcResponse};
