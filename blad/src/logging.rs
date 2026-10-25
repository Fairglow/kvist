//! Diagnostic logging for Blad.
//!
//! Structured, level-filtered logging over `tracing`, directed to stderr or
//! an explicitly selected file, never document stdout.
//!
//! Footer notices are not automatically logged. The CLI's fatal diagnostics
//! stay on unconditional stderr so filtering cannot hide a reported failure.
//!
//! Level filtering is controlled by `BLAD_LOG` (falling back to `RUST_LOG`),
//! with a default of `warn`. When `BLAD_LOG_FILE` is set, diagnostic output is
//! redirected to that file instead of stderr, so a user can keep the terminal
//! quiet while still capturing diagnostics for later inspection.

use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
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

        let writer = match std::env::var_os(BLAD_LOG_FILE_ENV) {
            Some(path) if !path.is_empty() => match open_log_file(Path::new(&path)) {
                Ok(file) => LogFileWriter(Some(file)),
                Err(error) => {
                    eprintln!(
                        "blad: cannot open diagnostic log {}: {}; using stderr",
                        path.to_string_lossy().escape_debug(),
                        error.to_string().escape_debug()
                    );
                    LogFileWriter(None)
                }
            },
            _ => LogFileWriter(None),
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
fn open_log_file(path: &Path) -> std::io::Result<File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "diagnostic log requires a regular file",
        ));
    }
    Ok(file)
}

struct LogFileWriter(Option<File>);

/// A log destination that is either a file handle or stderr.
enum LogWriter<'a> {
    File(&'a File),
    Stderr,
}

impl std::io::Write for LogWriter<'_> {
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
    type Writer = LogWriter<'a>;

    fn make_writer(&'a self) -> <Self as tracing_subscriber::fmt::writer::MakeWriter<'a>>::Writer {
        match &self.0 {
            Some(file) => LogWriter::File(file),
            None => LogWriter::Stderr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LogFileWriter, init_logging_with_default, open_log_file};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use tracing_subscriber::fmt::writer::MakeWriter;

    #[test]
    fn log_writers_retain_the_opened_descriptor_and_append_privately() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("log");
        let retained = temp.path().join("retained");
        let destination = LogFileWriter(Some(open_log_file(&path).unwrap()));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
            0
        );
        destination.make_writer().write_all(b"first\n").unwrap();
        std::fs::rename(&path, &retained).unwrap();
        std::fs::write(&path, "replacement").unwrap();
        destination.make_writer().write_all(b"second\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&retained).unwrap(),
            "first\nsecond\n"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
    }

    // Logging is initialized over a `Once`; repeated calls must stay safe and
    // must not panic, double-initialize, or error out.
    #[test]
    fn init_logging_is_idempotent() {
        init_logging_with_default("debug");
        init_logging_with_default("info");
        init_logging_with_default("warn");
    }
}
