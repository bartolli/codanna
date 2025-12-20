//! Unified logging for debug output.
//!
//! Provides compact timestamped logging with per-module level configuration.
//! Supports `RUST_LOG` environment variable for runtime overrides.
//!
//! # Configuration
//!
//! ```toml
//! [logging]
//! default = "warn"  # quiet by default
//!
//! [logging.modules]
//! cli = "debug"     # enable CLI debug logs
//! ```
//!
//! # Environment Variable
//!
//! `RUST_LOG` takes precedence over config:
//! ```bash
//! RUST_LOG=debug codanna index
//! RUST_LOG=cli=debug,indexer=trace codanna mcp
//! ```

use std::sync::Once;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::config::LoggingConfig;

static INIT: Once = Once::new();

/// Compact time format: HH:MM:SS.mmm
struct CompactTime;

impl FormatTime for CompactTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%H:%M:%S%.3f"))
    }
}

/// Modules that use explicit `target: "name"` or are external crates.
/// These don't need the `codanna::` prefix in filter strings.
const EXTERNAL_TARGETS: &[&str] = &["cli", "tantivy"];

/// Initialize logging with configuration.
///
/// Call once at startup. Safe to call multiple times (only first call takes effect).
///
/// Log levels control visibility:
/// - `error` - errors only (quietest)
/// - `warn` - errors + warnings (default, quiet operation)
/// - `info` - normal operation logs
/// - `debug` - detailed debugging
/// - `trace` - everything
///
/// The `RUST_LOG` environment variable takes precedence over config settings.
///
/// # Arguments
/// * `config` - Logging configuration with default level and per-module overrides
pub fn init_with_config(config: &LoggingConfig) {
    INIT.call_once(|| {
        // RUST_LOG env var takes precedence over config
        let filter = if std::env::var("RUST_LOG").is_ok() {
            EnvFilter::from_default_env()
        } else {
            // Build filter string from config
            let mut filter_str = config.default.clone();
            for (module, level) in &config.modules {
                // Internal modules need codanna:: prefix to match module paths
                let target = if EXTERNAL_TARGETS.contains(&module.as_str()) {
                    module.clone()
                } else {
                    format!("codanna::{module}")
                };
                filter_str.push_str(&format!(",{target}={level}"));
            }
            EnvFilter::new(&filter_str)
        };

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_target(true) // Show target for filtering visibility
            .with_timer(CompactTime)
            .with_level(true)
            .with_filter(filter);

        tracing_subscriber::registry().with(fmt_layer).init();
    });
}

/// Initialize logging to stderr (for MCP stdio mode).
///
/// MCP stdio protocol requires stdout for JSON-RPC only.
/// All logging must go to stderr to avoid breaking the protocol.
pub fn init_with_config_stderr(config: &LoggingConfig) {
    INIT.call_once(|| {
        let filter = if std::env::var("RUST_LOG").is_ok() {
            EnvFilter::from_default_env()
        } else {
            let mut filter_str = config.default.clone();
            for (module, level) in &config.modules {
                let target = if EXTERNAL_TARGETS.contains(&module.as_str()) {
                    module.clone()
                } else {
                    format!("codanna::{module}")
                };
                filter_str.push_str(&format!(",{target}={level}"));
            }
            EnvFilter::new(&filter_str)
        };

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_timer(CompactTime)
            .with_level(true)
            .with_filter(filter);

        tracing_subscriber::registry().with(fmt_layer).init();
    });
}

/// Initialize logging with default configuration.
///
/// Uses `LoggingConfig::default()` which sets `default = "warn"` for quiet operation.
/// Use `RUST_LOG=debug` environment variable for verbose output.
pub fn init() {
    init_with_config(&LoggingConfig::default());
}

/// Log an event with handler context.
///
/// # Examples
/// ```ignore
/// log_event!("document", "modified", "{}", path.display());
/// log_event!("code", "reindexed");
/// ```
#[macro_export]
macro_rules! log_event {
    ($handler:expr, $event:expr) => {
        tracing::info!("[{}] {}", $handler, $event)
    };
    ($handler:expr, $event:expr, $($arg:tt)*) => {
        tracing::info!("[{}] {}: {}", $handler, $event, format!($($arg)*))
    };
}

/// Debug-only event logging.
///
/// # Examples
/// ```ignore
/// debug_event!("watcher", "broadcast", "FileReindexed");
/// ```
#[macro_export]
macro_rules! debug_event {
    ($handler:expr, $event:expr) => {
        tracing::debug!("[{}] {}", $handler, $event)
    };
    ($handler:expr, $event:expr, $($arg:tt)*) => {
        tracing::debug!("[{}] {}: {}", $handler, $event, format!($($arg)*))
    };
}
