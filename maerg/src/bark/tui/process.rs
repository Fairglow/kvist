//! Owned argument-vector subprocesses and bounded, backpressured output.

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

const STREAM_BYTES: usize = 16_777_216;

pub(super) enum Output {
    Text { stderr: bool, text: String },
    Error(io::Error),
    Eof,
}

pub(super) struct Running {
    child: Child,
    input: Option<SyncSender<String>>,
    output: Option<Receiver<Output>>,
    readers: Vec<JoinHandle<()>>,
    pub(super) started: Instant,
    cancelled: Option<Instant>,
    ended: Option<(ExitStatus, Instant)>,
    eof: usize,
}

fn reader(
    mut source: impl Read + Send + 'static,
    stderr: bool,
    sender: SyncSender<Output>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        let mut pending = Vec::new();
        loop {
            let size = match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => size,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = sender.send(Output::Error(error));
                    break;
                }
            };
            pending.extend_from_slice(&buffer[..size]);
            let valid = match std::str::from_utf8(&pending) {
                Ok(_) => pending.len(),
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                Err(error) => {
                    let _ = sender.send(Output::Error(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("command output is not UTF-8: {error}"),
                    )));
                    break;
                }
            };
            if valid > 0 {
                match String::from_utf8(pending.drain(..valid).collect()) {
                    Ok(text) => {
                        if sender.send(Output::Text { stderr, text }).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Output::Error(io::Error::other(error)));
                        break;
                    }
                }
            }
        }
        if !pending.is_empty() {
            let _ = sender.send(Output::Error(io::Error::new(
                io::ErrorKind::InvalidData,
                "command output ends in incomplete UTF-8",
            )));
        }
        let _ = sender.send(Output::Eof);
    })
}

impl Running {
    pub(super) fn spawn(directory: &Path, arguments: &[String]) -> io::Result<Self> {
        Self::spawn_program(&std::env::current_exe()?, directory, arguments)
    }

    fn spawn_program(program: &Path, directory: &Path, arguments: &[String]) -> io::Result<Self> {
        let mut child = Command::new(program)
            .args(arguments)
            .current_dir(directory)
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env_remove("CLICOLOR_FORCE")
            .env("KVIST_NO_PAGER", "1")
            .env("TERM", "dumb")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing child stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing child stderr"))?;
        let (sender, receiver) = mpsc::sync_channel(32);
        let (answers, answer_receiver) = mpsc::sync_channel::<String>(2);
        let writer = sender.clone();
        let readers = vec![
            thread::spawn(move || {
                for answer in answer_receiver {
                    if let Err(error) = writeln!(input, "{answer}").and_then(|_| input.flush()) {
                        let _ = writer.send(Output::Error(io::Error::other(format!(
                            "command input failed: {error}"
                        ))));
                        break;
                    }
                }
            }),
            reader(stdout, false, sender.clone()),
            reader(stderr, true, sender),
        ];
        Ok(Self {
            child,
            input: Some(answers),
            output: Some(receiver),
            readers,
            started: Instant::now(),
            cancelled: None,
            ended: None,
            eof: 0,
        })
    }

    pub(super) fn drain(&mut self) -> Vec<Output> {
        let mut events = Vec::new();
        if let Some(output) = &self.output {
            for event in output.try_iter().take(64) {
                match event {
                    Output::Eof => self.eof += 1,
                    event => events.push(event),
                }
            }
        }
        events
    }

    fn signal(&self, signal: Signal) -> io::Result<()> {
        let pid = i32::try_from(self.child.id()).map_err(io::Error::other)?;
        match killpg(Pid::from_raw(pid), signal) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(error) => Err(io::Error::other(error)),
        }
    }

    pub(super) fn cancel(&mut self) -> io::Result<()> {
        if self.cancelled.is_none() {
            self.signal(Signal::SIGINT)?;
            self.cancelled = Some(Instant::now());
        }
        Ok(())
    }

    pub(super) fn answer(&mut self, text: &str) -> io::Result<()> {
        let input = self
            .input
            .as_ref()
            .ok_or_else(|| io::Error::other("command stdin is closed"))?;
        input.try_send(text.to_owned()).map_err(|error| {
            io::Error::other(format!(
                "command has not consumed its pending answers: {error}"
            ))
        })
    }

    pub(super) fn close_input(&mut self) {
        self.input.take();
    }

    pub(super) fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.ended.is_none()
            && let Some(status) = self.child.try_wait()?
        {
            self.ended = Some((status, Instant::now()));
        }
        if self
            .cancelled
            .is_some_and(|time| time.elapsed() > Duration::from_secs(5))
            || self
                .ended
                .is_some_and(|(_, time)| time.elapsed() > Duration::from_secs(2) && self.eof < 2)
        {
            self.signal(Signal::SIGKILL)?;
        }
        if self.eof == 2 {
            Ok(self.ended.map(|(status, _)| status))
        } else {
            Ok(None)
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // A terminal/recording failure must not leave the command executing.
        let _ = self.signal(Signal::SIGKILL);
        self.input.take();
        self.output.take();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

/// Reassembles report JSON without mistaking chunk boundaries for documents.
pub(super) struct Stream {
    pending: String,
    redactions: Vec<String>,
    redactor: crate::agent::redaction::StreamRedactor,
}

/// Render a captured line with terminal carriage-return semantics.
///
/// A `\r` returns the cursor to column zero and the following characters
/// overwrite the line in place; the visible result is what a terminal would
/// show. This collapses the agent's in-place progress updates (emitted as
/// `\r`-prefixed rewrites) to their final state so the transcript records one
/// clean line instead of every intermediate update. A bare `\r` immediately
/// followed by a newline leaves the existing text untouched, matching a
/// terminal.
fn collapse_cr(line: &str) -> String {
    let mut visible: Vec<char> = Vec::new();
    let mut position = 0usize;
    let mut newline = "";
    for ch in line.chars() {
        match ch {
            '\r' => position = 0,
            '\n' => {
                newline = "\n";
                break;
            }
            _ => {
                if position == visible.len() {
                    visible.push(ch);
                } else {
                    visible[position] = ch;
                }
                position += 1;
            }
        }
    }
    let mut rendered: String = visible.into_iter().collect();
    rendered.push_str(newline);
    rendered
}

impl Default for Stream {
    fn default() -> Self {
        Self::with_redactions(Vec::new())
    }
}

impl Stream {
    pub(super) fn with_redactions(redactions: Vec<String>) -> Self {
        let redactor = crate::agent::redaction::StreamRedactor::new(&redactions);
        Self {
            pending: String::new(),
            redactions,
            redactor,
        }
    }

    pub(super) fn push(&mut self, text: &str) -> io::Result<String> {
        if self.pending.len() + text.len() > STREAM_BYTES {
            return Err(io::Error::other(
                "one command output record exceeds the 16-MiB presentation limit; inspect the retained task log",
            ));
        }
        self.pending.push_str(text);
        let mut output = String::new();
        loop {
            if matches!(self.pending.trim_start().chars().next(), Some('{' | '[')) {
                let mut stream = serde_json::Deserializer::from_str(&self.pending)
                    .into_iter::<serde_json::Value>();
                match stream.next() {
                    Some(Ok(_)) => {
                        let end = stream.byte_offset();
                        let record: String = self.pending.drain(..end).collect();
                        output.push_str(&crate::presentation::report_redacted(
                            &record,
                            &self.redactions,
                        ));
                        output.push_str("\n\n");
                        continue;
                    }
                    Some(Err(error)) if error.is_eof() => break,
                    _ => {}
                }
            }
            let Some(end) = self.pending.find('\n') else {
                break;
            };
            let line: String = self.pending.drain(..=end).collect();
            output.push_str(&crate::presentation::report_redacted(
                &collapse_cr(&line),
                &self.redactions,
            ));
        }
        Ok(self.redactor.absorb(&output))
    }

    pub(super) fn flush(&mut self, finished: bool) -> String {
        if !finished
            && matches!(self.pending.trim_start().chars().next(), Some('{' | '['))
            && serde_json::from_str::<serde_json::Value>(&self.pending)
                .is_err_and(|error| error.is_eof())
        {
            return String::new();
        }
        let pending = std::mem::take(&mut self.pending);
        let output = crate::presentation::report_redacted(&collapse_cr(&pending), &self.redactions);
        let mut output = self.redactor.absorb(&output);
        if finished {
            output.push_str(&self.redactor.finish());
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_split_across_reads_decode_at_record_boundary() {
        let mut stream = Stream::default();
        assert!(stream.push("{\"report\":\"# Rev").unwrap().is_empty());
        assert!(stream.flush(false).is_empty());
        let rendered = stream.push("iew\\n\\n**Done**\",\"tokens\":12}\n").unwrap();
        assert!(rendered.contains("# Review\n\n**Done**"));
        assert!(rendered.contains("**tokens:** 12"));
    }

    #[test]
    fn ordinary_markdown_is_preserved_exactly_across_every_read_boundary() {
        let source = "# Heading\n\n| A | B |\n|---|---|\n| x | y |\n\n```yaml\nkey: value\n```\n";
        for size in [1, 7, source.len()] {
            let mut stream = Stream::default();
            let mut output = String::new();
            for chunk in source.as_bytes().chunks(size) {
                output.push_str(&stream.push(std::str::from_utf8(chunk).unwrap()).unwrap());
                output.push_str(&stream.flush(false));
            }
            output.push_str(&stream.flush(true));
            assert_eq!(output, source, "chunk size {size}");
        }
    }

    #[test]
    fn carriage_return_progress_rewrites_collapse_to_the_final_update() {
        // The agent rewrites one status line in place with a bare carriage
        // return each turn. The transcript must record the final update, not
        // every intermediate rewrite, and a bare \r before a newline must not
        // erase the line that precedes it.
        let mut stream = Stream::default();
        let output = stream
            .push("\r\u{27f3} Turn 1 \u{2502} reads: 0\r\u{27f3} Turn 2 \u{2502} reads: 5\n")
            .unwrap();
        assert_eq!(output, "\u{27f3} Turn 2 \u{2502} reads: 5\n");
        let output = stream.push("plain line\r\n").unwrap();
        assert_eq!(output, "plain line\n");
    }

    #[test]
    fn ordinary_multiline_secrets_remain_redacted_across_lines_and_chunks() {
        for chunks in [vec!["line\nsecret\n"], vec!["li", "ne\nse", "cret\n"]] {
            let mut stream = Stream::with_redactions(vec!["line\nsecret".into()]);
            let mut output = String::new();
            for chunk in chunks {
                output.push_str(&stream.push(chunk).unwrap());
                output.push_str(&stream.flush(false));
            }
            output.push_str(&stream.flush(true));
            assert_eq!(output, "[REDACTED]\n");
        }
    }

    #[test]
    fn pretty_json_and_cross_delta_secrets_remain_readable_and_redacted() {
        let mut stream = Stream::with_redactions(vec!["top_secret".into(), "line\nsecret".into()]);
        assert!(
            stream
                .push("{\n \"report\": \"# Review\\nline\\n")
                .unwrap()
                .is_empty()
        );
        let output = stream
            .push("secret\", \"other\":\"top_secret\"\n}\n")
            .unwrap();
        assert!(output.contains("# Review"));
        assert!(!output.contains("secret"), "{output}");
        assert!(stream.push("top_").unwrap().is_empty());
        assert!(stream.flush(false).is_empty());
        stream.push("secret").unwrap();
        assert_eq!(stream.flush(true), "[REDACTED]");
    }

    #[test]
    fn command_io_and_exit_status_are_owned() {
        let mut run = Running::spawn_program(
            Path::new("/usr/bin/printf"),
            Path::new("/"),
            &["# Hello\n".into()],
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = String::new();
        loop {
            for event in run.drain() {
                if let Output::Text { text, .. } = event {
                    output.push_str(&text);
                }
            }
            if let Some(status) = run.poll().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(output, "# Hello\n");
    }

    #[test]
    fn cancellation_reaps_a_command_without_waiting_for_its_timeout() {
        let mut run =
            Running::spawn_program(Path::new("/usr/bin/sleep"), Path::new("/"), &["60".into()])
                .unwrap();
        run.cancel().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            run.drain();
            if let Some(status) = run.poll().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
    }
}
