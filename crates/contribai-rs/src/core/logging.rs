//! Structured JSON logging for observability.
//!
//! Configurable via `log` section in config:
//! ```yaml
//! log:
//!   level: "info"
//!   format: "json"
//!   file: "~/.contribai/logs/pipeline.log"
//! ```

use serde::Serialize;
use std::fs::create_dir_all;
use std::path::Path;

/// Structured log entry.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub target: Option<String>,
}

/// Initialize structured JSON logging.
pub fn init_json_logging(level: &str, log_file: Option<&Path>) {
    // Set RUST_LOG env var for tracing
    std::env::set_var("RUST_LOG", level);

    if let Some(path) = log_file {
        let log_dir = dirs::home_dir()
            .map(|mut d| {
                d.push(".contribai");
                d.push("logs");
                d
            })
            .unwrap_or_else(|| Path::new(".").to_path_buf());

        let canonical_path = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => log_dir.join(path.file_name().unwrap_or_default()),
        };

        if !canonical_path.starts_with(&log_dir) {
            return;
        }

        if let Some(parent) = canonical_path.parent() {
            let _ = create_dir_all(parent);
        }
        tracing::info!(file = %canonical_path.display(), "JSON logging initialized");
    }
}
