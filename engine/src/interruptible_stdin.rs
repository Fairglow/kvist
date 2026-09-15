//! Interruptible standard-input reader for interactive prompts.
//!
//! Interactive commands (the agent setup wizard, `confirm`, ...) read the
//! terminal while the command runs, so the shared SIGINT/SIGTERM handler is the
//! only one active. That handler is installed with `SA_RESTART` by design (see
//! [`agent_runtime::interrupt`]): an in-flight blocking system call resumes
//! instead of failing, which means a plain blocked `read` would swallow
//! Ctrl-C and make a prompt impossible to cancel.
//!
//! This reader instead watches stdin with a bounded `select` and checks the
//! interrupt flag between waits, mirroring the supervision loops. A pending
//! interrupt surfaces as end-of-stream (a `read` returning `Ok(0)`), which
//! `read_line` propagates promptly so callers observe a clean cancellation
//! instead of hanging. It is a safe wrapper over nix primitives, so the crate
//! keeps `#![forbid(unsafe_code)].
//!
//! Wrap this in a [`std::io::BufReader`] to recover [`std::io::BufRead::read_line`]
//! for prompt input.

use std::io::{self, Read};
use std::os::unix::io::{AsFd, AsRawFd};

use agent_runtime::take_interrupted;
use nix::errno::Errno;
use nix::sys::select::{FdSet, select};
use nix::sys::time::TimeVal;
use nix::unistd::read;

/// How long a single read waits on stdin before re-checking for an interrupt.
///
/// Long enough that the wait is effectively idle (a blocked `select` sleeps),
/// short enough that Ctrl-C is always observed within a fraction of a second.
const POLL_INTERVAL_MICROS: i64 = 100_000;

/// Reads bytes from an interruptible descriptor.
///
/// The descriptor is stored as a trait object so the production stdin handle
/// and test pipes share one implementation. Only its file descriptor is ever
/// watched/read; the handle is never read through directly, so it does not
/// double-buffer with the outer [`BufReader`].
pub struct InterruptibleStdin {
    source: Box<dyn AsFd>,
    /// Zero-cost interrupt probe; the shared handler's flag in production.
    check_interrupt: fn() -> bool,
    /// Once an interrupt is observed, keep surfacing it so a partial read is
    /// never mistaken for input after the user cancels.
    aborted: bool,
}

impl InterruptibleStdin {
    /// Opens the process standard input for interruptible reads.
    pub fn new() -> Self {
        Self {
            source: Box::new(io::stdin()),
            check_interrupt: take_interrupted,
            aborted: false,
        }
    }

    /// Wraps an arbitrary readable descriptor (for example a test pipe) so the
    /// same interruptible read logic can be exercised without a terminal.
    #[cfg(test)]
    pub fn from_source<S: AsFd + 'static>(source: S) -> Self {
        Self {
            source: Box::new(source),
            check_interrupt: take_interrupted,
            aborted: false,
        }
    }
}

impl Default for InterruptibleStdin {
    fn default() -> Self {
        Self::new()
    }
}

impl Read for InterruptibleStdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.aborted || (self.check_interrupt)() {
                self.aborted = true;
                // Signal cancellation to BufRead consumers as end-of-stream.
                // std's `read_until` (used by `read_line`) retries on
                // ErrorKind::Interrupted, so returning that kind would loop
                // forever instead of propagating the cancellation. Ok(0) lets
                // `read_line` return promptly; callers treat the empty read as
                // a user cancellation.
                return Ok(0);
            }

            // `select`'s first argument counts descriptors up to and including
            // this fd, so it must be `fd + 1`; hardcoding it would ignore the
            // actual descriptor and hang waiting for input that never arrives.
            let source_fd = self.source.as_fd();
            let nfds = source_fd.as_raw_fd() + 1;
            let mut fds = FdSet::new();
            fds.insert(source_fd);
            let mut timeout = TimeVal::new(0, POLL_INTERVAL_MICROS);

            match select(nfds, Some(&mut fds), None, None, Some(&mut timeout)) {
                // `SA_RESTART` resumes the blocked `select` on a signal, so a
                // pending interrupt is observed on the next loop iteration.
                Ok(ready) if ready > 0 && fds.contains(source_fd) => {
                    match read(source_fd, buf) {
                        Ok(0) => return Ok(0),
                        Ok(count) => return Ok(count),
                        // A signal can still interrupt the read itself; treat it
                        // the same as "re-check the interrupt flag".
                        Err(Errno::EINTR) => continue,
                        Err(error) => return Err(io::Error::from(error)),
                    }
                }
                Ok(_) => continue,
                Err(Errno::EINTR) => continue,
                Err(error) => return Err(io::Error::from(error)),
            }
        }
    }
}

/// A [`BufRead`] over an interruptible stdin, ready for prompt input.
pub type InterruptibleStdinReader = std::io::BufReader<InterruptibleStdin>;

/// Builds an interruptible stdin reader for interactive prompts.
pub fn interruptible_reader() -> InterruptibleStdinReader {
    std::io::BufReader::new(InterruptibleStdin::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};

    fn reader_with_check(check: fn() -> bool) -> InterruptibleStdinReader {
        // A closed pipe keeps `select` pending; the injected check stands in
        // for a Ctrl-C so the read must unblock as end-of-stream.
        let (reader, _writer) = io::pipe().expect("create pipe");
        drop(_writer);
        InterruptibleStdinReader::new(InterruptibleStdin {
            source: Box::new(reader),
            check_interrupt: check,
            aborted: false,
        })
    }

    #[test]
    fn reads_a_line_from_a_pipe() {
        let (reader, mut writer) = io::pipe().expect("create pipe");
        writer.write_all(b"hello\n").expect("write to pipe");

        let mut buf = String::new();
        let mut source = InterruptibleStdinReader::new(InterruptibleStdin::from_source(reader));
        let n = source.read_line(&mut buf).expect("read line");
        assert_eq!(n, 6);
        assert_eq!(buf, "hello\n");
        drop(writer);
    }

    #[test]
    fn returns_zero_at_eof() {
        let (reader, _writer) = io::pipe().expect("create pipe");
        drop(_writer);
        let mut source = InterruptibleStdinReader::new(InterruptibleStdin::from_source(reader));
        let mut buf = String::new();
        assert_eq!(source.read_line(&mut buf).expect("read to eof"), 0);
    }

    #[test]
    fn interrupt_surfaces_as_end_of_stream() {
        // A pending interrupt must make `read_line` return Ok(0) so callers
        // observe a clean cancellation instead of looping forever.
        let mut source = reader_with_check(|| true);
        assert_eq!(
            source.read_line(&mut String::new()).expect("eof on cancel"),
            0
        );
    }

    #[test]
    fn read_returns_zero_directly_on_interrupt() {
        // The underlying `Read` contract: a cancelled read yields Ok(0).
        let (reader, _writer) = io::pipe().expect("create pipe");
        drop(_writer);
        let mut source = InterruptibleStdin::from_source(reader);
        source.check_interrupt = || true;
        let mut buf = [0u8; 16];
        assert_eq!(source.read(&mut buf).expect("eof on cancel"), 0);
    }

    #[test]
    fn interrupt_stays_surfaced_until_resumed() {
        // Once cancelled, the reader keeps reporting end-of-stream until a fresh
        // reader is constructed (the abort is remembered, not re-triggered).
        let mut source = reader_with_check(|| true);
        for _ in 0..2 {
            assert_eq!(
                source.read_line(&mut String::new()).expect("eof on cancel"),
                0
            );
        }
    }
}
