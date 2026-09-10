use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::pager::display_output;
use crate::{KvistError, Result, cli};

/// A non-blocking background terminal spinner for transient command execution.
pub struct ProgressSpinner {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressSpinner {
    /// Starts an asynchronous spinner on stderr displaying `message`.
    pub fn start(message: String) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let mut idx = 0;
            while running_clone.load(Ordering::Relaxed) {
                let char = spinner_chars[idx % spinner_chars.len()];
                let _ = io::stderr().write_all(format!("\r  {char} {message}...").as_bytes());
                let _ = io::stderr().flush();
                idx += 1;
                thread::sleep(Duration::from_millis(80));
            }
            // Erase the transient spinner line completely using ANSI codes
            let _ = io::stderr().write_all(b"\r\x1b[2K");
            let _ = io::stderr().flush();
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    /// Stops the spinner and erases the transient line from the terminal.
    pub fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ProgressSpinner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Manages transient output streams and log link replacement for execution.
pub struct StreamManager {
    project_root: PathBuf,
    current_log: Mutex<Option<PathBuf>>,
}

impl StreamManager {
    /// Creates a new stream manager for the given project root.
    pub fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            current_log: Mutex::new(None),
        }
    }

    /// Starts a non-blocking progress spinner for a long-running command.
    pub fn start_spinner(&self, command_name: &str) -> ProgressSpinner {
        ProgressSpinner::start(format!("Running {command_name}"))
    }

    /// Prepares a timestamped log path in `.kvist/logs/`.
    pub fn prepare_log_path(&self, program: &str) -> Result<PathBuf> {
        let log_dir = self.project_root.join(".kvist/logs");
        fs::create_dir_all(&log_dir).map_err(|e| KvistError::Io {
            operation: "create logs directory",
            path: log_dir.clone(),
            source: e,
        })?;
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let log_name = format!("{}_{}.log", program.replace('-', "_"), timestamp);
        let log_path = log_dir.join(log_name);
        if let Ok(mut guard) = self.current_log.lock() {
            *guard = Some(log_path.clone());
        }
        Ok(log_path)
    }

    /// Replaces transient progress display with the finalized command output and log link.
    pub fn finish_stream(
        &self,
        _program: &str,
        result: &std::result::Result<cli::CommandOutput, crate::KvistError>,
        _prompt_line: &str,
    ) {
        match result {
            Ok(output) => {
                let text = output.to_string();
                if !text.is_empty() {
                    display_output(&text);
                }
                if let Some(log_path) = self.current_log.lock().ok().and_then(|mut g| g.take())
                    && log_path.exists()
                {
                    let relative_log = log_path
                        .strip_prefix(&self.project_root)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| log_path.display().to_string());
                    println!("\nLogs: ./{relative_log}");
                }
            }
            Err(error) => {
                let _ = error.print();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn spinner_starts_and_stops_cleanly() {
        let spinner = ProgressSpinner::start("Testing".into());
        thread::sleep(Duration::from_millis(150));
        spinner.stop();
    }

    #[test]
    fn prepare_log_path_creates_directory_and_returns_valid_path() {
        let dir = tempdir().unwrap();
        let manager = StreamManager::new(dir.path());
        let log_path = manager.prepare_log_path("task-run").unwrap();
        assert!(log_path.starts_with(dir.path().join(".kvist/logs")));
        assert!(log_path.to_str().unwrap().contains("task_run"));
    }
}
