//! Structured logging for NASA-grade audit trails using **tracing**.
//!
//! Performance characteristics:
//! - Non-blocking: tracing macros push events to a queue, not directly to I/O
//! - Async-compatible: Works efficiently with Rayon's parallel workers
//! - Rich context: Automatically captures level, timestamp, target, and thread ID
//!
//! The JSON subscriber provides machine-readable output for observability platforms.

use tracing::{error, info, warn};

/// Initializes the global tracing collector (subscriber).
///
/// This should be called *once* at the beginning of the application's runtime.
/// It configures structured JSON output to stderr.
///
/// # Environment Variables
/// - `RUST_LOG`: Controls log filtering (e.g., `RUST_LOG=deadmod=debug`)
pub fn init_structured_logging() {
    tracing_subscriber::fmt()
        .json() // Output logs in JSON format
        .with_ansi(false) // Disable ANSI codes in JSON output
        .with_level(true) // Include the log level field
        .with_target(true) // Include the module path (target)
        .with_current_span(true) // Include tracing span context
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()) // Allow RUST_LOG env var
        .with_writer(std::io::stderr) // Write to stderr (keeps stdout clean for tool output)
        .init();
}

/// Logs a warning event.
///
/// Uses tracing macro for non-blocking, structured output.
pub fn log_warn(message: &str) {
    warn!(detail = %message);
}

/// Logs an info event.
///
/// Uses tracing macro for non-blocking, structured output.
pub fn log_info(message: &str) {
    info!(detail = %message);
}

/// Logs an error event.
///
/// Uses tracing macro for non-blocking, structured output.
pub fn log_error(message: &str) {
    error!(detail = %message);
}

/// Logs a custom event with a specific event name.
///
/// Preserved for backwards compatibility with existing call sites.
/// Maps to appropriate log level based on event name.
pub fn log_event(event: &str, detail: &str) {
    match event.to_uppercase().as_str() {
        "ERROR" => error!(event = %event, detail = %detail),
        "WARN" | "WARNING" => warn!(event = %event, detail = %detail),
        _ => info!(event = %event, detail = %detail),
    }
}
