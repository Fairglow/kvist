//! Diagnostic logging infrastructure for Kvist.
//!
//! Kvist uses structured, level-filtered logging over [`tracing`]. Diagnostic
//! events use standard error for headless commands and the Markdown transcript
//! while the workspace owns the terminal, keeping command and JSON output clean.
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

fn filter(default_level: &str) -> EnvFilter {
    match std::env::var(KVIST_LOG_ENV).or_else(|_| std::env::var(RUST_LOG_ENV)) {
        Ok(value) if !value.trim().is_empty() => EnvFilter::builder()
            .with_default_directive(tracing::level_filters::LevelFilter::WARN.into())
            .parse_lossy(value),
        _ => EnvFilter::new(default_level),
    }
}

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
        let filter = filter(default_level);

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

pub(crate) struct WorkspaceDiagnostics {
    receiver: std::sync::mpsc::Receiver<String>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[derive(Clone)]
struct DiagnosticWriter {
    sender: std::sync::mpsc::SyncSender<String>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl std::io::Write for DiagnosticWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let mut end = text.len().min(16_384);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut event = text[..end].to_owned();
        if bytes.len() > event.len() {
            event.push_str("\n[host diagnostic truncated after 16384 bytes]\n");
        }
        if self.sender.try_send(event).is_err() {
            self.dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl WorkspaceDiagnostics {
    pub(crate) fn enter() -> (Self, tracing::subscriber::DefaultGuard) {
        Self::with_filter(filter(DEFAULT_LOG_LEVEL))
    }

    fn with_filter(filter: EnvFilter) -> (Self, tracing::subscriber::DefaultGuard) {
        let (sender, receiver) = std::sync::mpsc::sync_channel(32);
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let writer = DiagnosticWriter {
            sender,
            dropped: dropped.clone(),
        };
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_env_filter(filter)
            .with_ansi(false)
            .compact()
            .finish();
        (
            Self { receiver, dropped },
            tracing::subscriber::set_default(subscriber),
        )
    }

    pub(crate) fn drain(&self) -> String {
        let mut output = String::new();
        for event in self.receiver.try_iter().take(32) {
            output.push_str(&event);
        }
        let dropped = self.dropped.swap(0, std::sync::atomic::Ordering::Relaxed);
        if dropped > 0 {
            use std::fmt::Write;
            let _ = writeln!(
                output,
                "{dropped} host diagnostic fragments were not retained because the workspace log channel was full."
            );
        }
        output
    }
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

    #[test]
    fn workspace_diagnostics_are_captured_and_saturation_is_explicit() {
        let (diagnostics, _guard) = WorkspaceDiagnostics::with_filter(EnvFilter::new("error"));
        tracing::error!("workspace diagnostic fixture");
        assert!(diagnostics.drain().contains("workspace diagnostic fixture"));
        for _ in 0..100 {
            tracing::error!("burst diagnostic fixture");
        }
        assert!(diagnostics.drain().contains("not retained"));
        tracing::error!("{}", "x".repeat(32_768));
        let output = diagnostics.drain();
        assert!(output.contains("truncated after 16384 bytes"));
        assert!(output.len() < 17_000);

        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut writer = DiagnosticWriter {
            sender,
            dropped: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        use std::io::Write;
        writer
            .write_all(format!("{}é", "x".repeat(16_383)).as_bytes())
            .unwrap();
        let output = receiver.recv().unwrap();
        assert!(output.starts_with(&"x".repeat(16_383)));
        assert!(output.contains("truncated after 16384 bytes"));
        assert!(!output.contains('\u{fffd}'));
        assert_eq!(
            writer.write(&[0xff]).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
