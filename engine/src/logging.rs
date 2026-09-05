//! Diagnostic logging infrastructure for Kvist.
//!
//! Kvist uses structured, level-filtered logging over [`tracing`]. Diagnostic
//! events are directed exclusively to standard error to prevent interfering with
//! deterministic command output or structured JSON output on standard output.
//!
//! # Log level principles
//!
//! - **ERROR (`tracing::error!`)**: Failures that prevent command completion,
//!   task execution, or violate security/integrity boundaries. Must contain
//!   the target resource, operation, root cause, and actionable remediation context.
//! - **WARN (`tracing::warn!`)**: Recoverable anomalies, process retries, fallback
//!   behaviors, or non-fatal configuration discrepancies. Must explain *why*
//!   the condition occurred and what fallback/recovery action is being taken.
//! - **INFO (`tracing::info!`)**: High-level workflow milestones and operator
//!   lifecycle overviews. Answers *what* is happening at a glance (e.g. project
//!   initialization, discovery summary, task phase transitions, verification outcome).
//! - **DEBUG (`tracing::debug!`)**: Contextual diagnostics explaining *why* decisions
//!   and transitions were made (e.g. resolved paths, configuration values, sandbox mounts,
//!   VCS commit digests, command arguments, HTTP statuses).
//! - **TRACE (`tracing::trace!`)**: Fine-grained internal mechanics for low-level
//!   investigation (e.g. per-file hashing, stream chunk sizes, state machine ticks).
//!
//! # Anti-spamming and deduplication
//!
//! - Repetitive loops, stream transfers, and recursive directory walks must never
//!   emit repetitive events at `INFO` or `WARN` levels.
//! - Aggregate summaries or rate-limited progress must be preferred.
//! - Third-party prompt/response sentinels and sensitive credentials must never be
//!   emitted in tracing logs.

use std::io::IsTerminal;
use std::sync::Once;
use tracing_subscriber::EnvFilter;

static INIT: Once = Once::new();

/// Environment variable for Kvist-specific log filtering.
pub const KVIST_LOG_ENV: &str = "KVIST_LOG";

/// Standard fallback environment variable for Rust log filtering.
pub const RUST_LOG_ENV: &str = "RUST_LOG";

/// Default log level when no environment filter is specified in production.
pub const DEFAULT_LOG_LEVEL: &str = "warn";

/// Default log level for testing environments.
pub const DEFAULT_TEST_LOG_LEVEL: &str = "debug";

/// Initializes the global tracing subscriber for Kvist if not already initialized.
///
/// Logging is directed to [`std::io::stderr`]. Filter configuration checks
/// `KVIST_LOG` first, then `RUST_LOG`, and defaults to `warn` if neither is set.
/// This ensures clean standard output for deterministic CLI scripts and `--json`
/// modes while allowing rich diagnostic exploration on demand.
pub fn init_logging() {
    let default_level = if cfg!(test) {
        DEFAULT_TEST_LOG_LEVEL
    } else {
        DEFAULT_LOG_LEVEL
    };
    init_logging_with_default(default_level);
}

/// Initializes the global tracing subscriber configured for test execution.
///
/// Tests run on `debug` level by default unless overridden by `KVIST_LOG` or `RUST_LOG`.
pub fn init_test_logging() {
    init_logging_with_default(DEFAULT_TEST_LOG_LEVEL);
}

/// Initializes the global tracing subscriber with a specified default level.
pub fn init_logging_with_default(default_level: &str) {
    INIT.call_once(|| {
        let filter = match std::env::var(KVIST_LOG_ENV).or_else(|_| std::env::var(RUST_LOG_ENV)) {
            Ok(env_val) if !env_val.trim().is_empty() => EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::WARN.into())
                .parse_lossy(env_val),
            _ => EnvFilter::new(default_level),
        };

        let is_terminal = std::io::stderr().is_terminal();

        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .with_target(true)
            .with_ansi(is_terminal)
            .compact()
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_is_safe_and_idempotent() {
        init_logging();
        init_logging();
        init_logging_with_default("info");
    }

    #[test]
    fn constants_are_defined_consistently() {
        assert_eq!(KVIST_LOG_ENV, "KVIST_LOG");
        assert_eq!(RUST_LOG_ENV, "RUST_LOG");
        assert_eq!(DEFAULT_LOG_LEVEL, "warn");
        assert_eq!(DEFAULT_TEST_LOG_LEVEL, "debug");
    }
}
