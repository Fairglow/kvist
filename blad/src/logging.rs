//! Diagnostic logging for Blad.
//!
//! Structured, level-filtered logging over `tracing`. Diagnostic events are
//! directed exclusively to standard error so they never interfere with the
//! viewer screen or any structured output on standard output.
//!
//! The interactive viewer uses this for soft, recoverable failures — a link
//! that cannot be opened, a reload that fails, a terminal that will not
//! restore — where the user already sees the problem in the status line and
//! does not need it repeated on stderr. The two fatal entry points in
//! [`crate::main`] (`report_io` and `fail`) stay on unconditional stderr: an
//! exit status of 1 must always carry a message, even when logging is
//! filtered to a level that would suppress the event.
//!
//! Level filtering is controlled by `BLAD_LOG` (falling back to `RUST_LOG`),
//! with a default of `warn`. When `BLAD_LOG_FILE` is set, diagnostic output is
//! redirected to that file instead of stderr, so a user can keep the terminal
//! quiet while still capturing diagnostics for later inspection.

use std::fs::File;
use std::path::Path;
use std::sync::Once;
use tracing_subscriber::EnvFilter;

static INIT: Once = Once::new();

/// Environment variable for Blad-specific log filtering.
pub const BLAD_LOG_ENV: &str = "BLAD_LOG";
/// Standard fallback environment variable for Rust log filtering.
pub const RUST_LOG_ENV: &str = "RUST_LOG";
/// Environment variable selecting a file to receive diagnostic output.
pub const BLAD_LOG_FILE_ENV: &str = "BLAD_LOG_FILE";
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
///
/// The filter is read from [`BLAD_LOG_ENV`], falling back to [`RUST_LOG_ENV`],
/// and defaults to [`DEFAULT_LOG_LEVEL`] when neither is set. When
/// [`BLAD_LOG_FILE_ENV`] points at a writable path, diagnostics are written to
/// that file; otherwise they go to stderr.
pub fn init_logging_with_default(default_level: &str) {
    INIT.call_once(|| {
        let filter = match std::env::var(BLAD_LOG_ENV).or_else(|_| std::env::var(RUST_LOG_ENV)) {
            Ok(env_val) if !env_val.trim().is_empty() => EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::WARN.into())
                .parse_lossy(env_val),
            _ => EnvFilter::new(default_level),
        };

        // The log file is a convenience, not a contract failure: if it cannot
        // be opened we fall back to stderr so diagnostics are never lost.
        let writer = match std::env::var(BLAD_LOG_FILE_ENV) {
            Ok(path) if !path.trim().is_empty() => match open_log_file(&path) {
                Ok(_) => LogFileWriter::new(std::path::PathBuf::from(&path)),
                Err(error) => {
                    tracing::warn!(%error, log_file = %path, "cannot open log file; using stderr");
                    LogFileWriter::stderr()
                }
            },
            _ => LogFileWriter::stderr(),
        };

        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(writer)
            .compact()
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Open a log file for appending, creating it if necessary.
fn open_log_file(path: &str) -> std::io::Result<File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(path))
}

/// A `MakeWriter` that opens a log file once and reuses the handle, falling
/// back to stderr if the file cannot be opened. Opening lazily keeps
/// diagnostics out of the live viewer and lets a user point `BLAD_LOG_FILE`
/// at a file to capture them for later inspection.
struct LogFileWriter {
    path: std::path::PathBuf,
    cache: std::sync::Mutex<Option<File>>,
}

impl LogFileWriter {
    fn new(path: std::path::PathBuf) -> Self {
        LogFileWriter {
            path,
            cache: std::sync::Mutex::new(None),
        }
    }

    fn stderr() -> Self {
        // A sentinel path that never matches; make_writer ignores it.
        LogFileWriter {
            path: std::path::PathBuf::from("-"),
            cache: std::sync::Mutex::new(None),
        }
    }
}

/// A log destination that is either a file handle or stderr.
enum LogWriter {
    File(File),
    Stderr,
}

impl std::io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            LogWriter::File(file) => file.write(buf),
            LogWriter::Stderr => std::io::stderr().write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            LogWriter::File(file) => file.flush(),
            LogWriter::Stderr => std::io::stderr().flush(),
        }
    }
}

impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for LogFileWriter {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> <Self as tracing_subscriber::fmt::writer::MakeWriter<'a>>::Writer {
        if self.path.as_os_str() == "-" {
            return LogWriter::Stderr;
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some(file) = cache.as_ref() {
            return match file.try_clone() {
                Ok(f) => LogWriter::File(f),
                Err(_) => LogWriter::Stderr,
            };
        }
        match open_log_file(&self.path.to_string_lossy()) {
            Ok(file) => {
                *cache = file.try_clone().ok();
                LogWriter::File(file)
            }
            Err(error) => {
                tracing::warn!(%error, path = %self.path.display(), "cannot open log file; using stderr");
                LogWriter::Stderr
            }
        }
    }
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
