//! Diagnostic logging for agent-runner.
//!
//! Structured, level-filtered logging over `tracing`. Diagnostic events are
//! directed exclusively to standard error so they never interfere with the
//! transcript or structured output on standard output.

use std::io::IsTerminal;
use std::sync::Once;
use tracing_subscriber::EnvFilter;

static INIT: Once = Once::new();

/// Environment variable for agent-runner-specific log filtering.
pub const AGENT_RUNNER_LOG_ENV: &str = "AGENT_RUNNER_LOG";
/// Standard fallback environment variable for Rust log filtering.
pub const RUST_LOG_ENV: &str = "RUST_LOG";
/// Default production log level when no filter is specified.
pub const DEFAULT_LOG_LEVEL: &str = "warn";
/// Default test log level.
pub const DEFAULT_TEST_LOG_LEVEL: &str = "debug";

/// Initializes the global tracing subscriber if not already initialized.
pub fn init_logging() {
    let default_level = if cfg!(test) {
        DEFAULT_TEST_LOG_LEVEL
    } else {
        DEFAULT_LOG_LEVEL
    };
    init_logging_with_default(default_level);
}

/// Initializes the global tracing subscriber for tests on `debug` by default.
///
/// Retained as a public test entry point; the production entry point
/// (`init_logging`) already selects the debug level when the library itself is
/// compiled for tests.
#[allow(dead_code)]
pub fn init_test_logging() {
    init_logging_with_default(DEFAULT_TEST_LOG_LEVEL);
}

/// Initializes the global tracing subscriber with a specific default level.
pub fn init_logging_with_default(default_level: &str) {
    INIT.call_once(|| {
        let filter =
            match std::env::var(AGENT_RUNNER_LOG_ENV).or_else(|_| std::env::var(RUST_LOG_ENV)) {
                Ok(env_val) if !env_val.trim().is_empty() => EnvFilter::builder()
                    .with_default_directive(tracing::level_filters::LevelFilter::WARN.into())
                    .parse_lossy(env_val),
                _ => EnvFilter::new(default_level),
            };

        let is_terminal = std::io::stderr().is_terminal();

        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .with_ansi(is_terminal)
            .compact()
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

#[cfg(test)]
mod tests {
    use super::init_logging_with_default;

    // Logging is initialized over a `Once`; repeated calls must stay safe and
    // must not panic, double-initialize, or error out.
    #[test]
    fn init_logging_is_idempotent() {
        init_logging_with_default("debug");
        init_logging_with_default("info");
        init_logging_with_default("warn");
    }
}
