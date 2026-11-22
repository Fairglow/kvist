//! Host-side relay for the private debug PTY. No operator terminal FD is inherited.

use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use nix::sys::signal::{SigSet, SigmaskHow, Signal, kill};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::unistd::Pid;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{Mode, OFlags, fcntl_setfl, open};
use rustix::io::{self, Errno, fcntl_dupfd_cloexec};
use rustix::pty::{OpenptFlags, grantpt, ioctl_tiocgptpeer, openpt, unlockpt};
use rustix::termios::{OptionalActions, Termios, tcgetattr, tcgetwinsize, tcsetattr, tcsetwinsize};

use crate::protocol::Resources;

pub(super) const SETSID: &str = "/usr/bin/setsid";
const SAMPLE_INTERVAL: Duration = Duration::from_millis(10);
const POLL_INTERVAL: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 10_000_000,
};
const CLOSED: PollFlags = PollFlags::HUP.union(PollFlags::ERR).union(PollFlags::NVAL);

pub(super) struct Terminal {
    input: OwnedFd,
    output: OwnedFd,
    diagnostic: OwnedFd,
    master: OwnedFd,
}

impl Terminal {
    pub(super) fn attach(command: &mut Command) -> io::Result<Self> {
        // Reopen rather than changing flags on the operator's inherited open file descriptions.
        let flags = OFlags::NOCTTY | OFlags::CLOEXEC | OFlags::NONBLOCK;
        let input = open("/proc/self/fd/0", flags | OFlags::RDONLY, Mode::empty())?;
        let output = open("/proc/self/fd/1", flags | OFlags::WRONLY, Mode::empty())?;
        let diagnostic = open("/proc/self/fd/2", flags | OFlags::WRONLY, Mode::empty())?;
        let pty_flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
        let master = openpt(pty_flags)?;
        fcntl_setfl(&master, OFlags::NONBLOCK)?;
        grantpt(&master)?;
        unlockpt(&master)?;
        let slave = ioctl_tiocgptpeer(&master, pty_flags)?;
        tcsetattr(&slave, OptionalActions::Now, &tcgetattr(&input)?)?;
        tcsetwinsize(&slave, tcgetwinsize(&input)?)?;
        command
            .stdin(fcntl_dupfd_cloexec(&slave, 3)?)
            .stdout(fcntl_dupfd_cloexec(&slave, 3)?)
            .stderr(slave);
        Ok(Self {
            input,
            output,
            diagnostic,
            master,
        })
    }

    pub(super) fn supervise(&self, child: &mut Child, limits: Resources) -> Result<i32, String> {
        let result = self.with_raw_terminal(|signals| self.relay(child, limits, signals));
        if !matches!(result, Ok(End::Exited(_))) {
            self.stop(child);
        }
        match result {
            Ok(End::Exited(status)) => Ok(status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))),
            Ok(End::Interrupted(signal)) => Ok(128 + signal),
            Err(error) => Err(error),
        }
    }

    pub(super) fn report_error(&self, reason: &str) {
        // Diagnostics must not hang teardown if the operator isn't draining the terminal.
        let message = format!("galla: {reason}\n");
        let _ = io::write(&self.diagnostic, message.as_bytes());
    }

    fn stop(&self, child: &mut Child) {
        if !matches!(child.try_wait(), Ok(Some(_))) {
            let group = Pid::from_raw(-(child.id() as i32));
            match kill(group, Signal::SIGKILL) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
                Err(error) => self.report_error(&format!("cannot stop Bubblewrap group: {error}")),
            }
            if let Err(error) = child.wait() {
                self.report_error(&format!(
                    "cannot reap Bubblewrap after terminal shutdown: {error}"
                ));
            }
        }
    }

    fn with_raw_terminal(
        &self,
        relay: impl FnOnce(&SignalFd) -> Result<End, String>,
    ) -> Result<End, String> {
        let mut mask = SigSet::empty();
        for signal in [
            Signal::SIGHUP,
            Signal::SIGINT,
            Signal::SIGQUIT,
            Signal::SIGTERM,
            Signal::SIGTSTP,
        ] {
            mask.add(signal);
        }
        let signals = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
            .map_err(|error| format!("cannot prepare terminal cancellation: {error}"))?;
        // Block only in the host relay, after spawning Bubblewrap.
        let _mask = SignalMask {
            terminal: self,
            saved: mask
                .thread_swap_mask(SigmaskHow::SIG_BLOCK)
                .map_err(|error| format!("cannot block terminal cancellation signals: {error}"))?,
        };
        let saved = tcgetattr(&self.input)
            .map_err(|error| format!("cannot read terminal attributes: {error}"))?;
        let mut raw = saved.clone();
        raw.make_raw();
        tcsetattr(&self.input, OptionalActions::Now, &raw)
            .map_err(|error| format!("cannot set terminal to raw mode: {error}"))?;
        let _restore = RestoreTerminal {
            terminal: self,
            saved,
        };
        relay(&signals)
    }

    fn relay(
        &self,
        child: &mut Child,
        limits: Resources,
        signals: &SignalFd,
    ) -> Result<End, String> {
        let mut to_payload = Buffer::default();
        let mut to_host = Buffer::default();
        let mut master_open = true;
        let mut input_open = true;
        let mut total_output = 0;
        let mut status = None;
        let started = Instant::now();
        let deadline = started + Duration::from_millis(limits.wall_time_ms);
        let mut next_sample = started;
        let mut size = tcgetwinsize(&self.master)
            .map_err(|error| format!("cannot read sandbox terminal size: {error}"))?;
        loop {
            if status.is_none() {
                status = child
                    .try_wait()
                    .map_err(|error| format!("cannot wait for Bubblewrap: {error}"))?;
            }
            if let Some(status) = status
                && !master_open
                && to_host.is_empty()
            {
                return Ok(End::Exited(status));
            }
            if let Some(info) = signals
                .read_signal()
                .map_err(|error| format!("cannot read terminal cancellation: {error}"))?
            {
                return Ok(End::Interrupted(info.ssi_signo as i32));
            }
            let now = Instant::now();
            if now >= deadline {
                return Err("wall time limit exceeded".to_owned());
            }
            if now >= next_sample && status.is_none() {
                if super::count_processes_in_pgid(child.id()) > limits.max_processes {
                    return Err("process count limit exceeded".to_owned());
                }
                next_sample = now + SAMPLE_INTERVAL;
            }
            if master_open && status.is_none() {
                let current = tcgetwinsize(&self.input)
                    .map_err(|error| format!("cannot read operator terminal size: {error}"))?;
                if size != current {
                    tcsetwinsize(&self.master, current)
                        .map_err(|error| format!("cannot update sandbox terminal size: {error}"))?;
                    size = current;
                }
            }

            let accept_input = status.is_none() && master_open && input_open;
            let master_events = interest(to_host.is_empty(), PollFlags::IN)
                | interest(accept_input && !to_payload.is_empty(), PollFlags::OUT);
            let mut fds = [
                PollFd::new(signals, PollFlags::IN),
                PollFd::new(
                    &self.input,
                    interest(accept_input && to_payload.is_empty(), PollFlags::IN),
                ),
                PollFd::new(&self.output, interest(!to_host.is_empty(), PollFlags::OUT)),
                PollFd::new(&self.master, master_events),
            ];
            let fd_count = if master_open && !master_events.is_empty() {
                fds.len()
            } else {
                fds.len() - 1
            };
            match poll(&mut fds[..fd_count], Some(&POLL_INTERVAL)) {
                Ok(_) => {}
                Err(Errno::INTR) => continue,
                Err(error) => return Err(format!("cannot poll interactive terminal: {error}")),
            }
            let [_, input, output, master] = fds.map(|fd| fd.revents());
            if (input | output).intersects(CLOSED) {
                return Err("operator terminal disconnected".to_owned());
            }
            if output.contains(PollFlags::OUT) {
                to_host
                    .write_to(&self.output)
                    .map_err(|error| format!("cannot write to operator terminal: {error}"))?;
            }
            if master.intersects(CLOSED) {
                input_open = false;
            } else if master.contains(PollFlags::OUT) {
                match to_payload.write_to(&self.master) {
                    Ok(()) => {}
                    Err(Errno::IO) => input_open = false,
                    Err(error) => return Err(format!("cannot write to sandbox terminal: {error}")),
                }
            }
            if input.contains(PollFlags::IN)
                && to_payload
                    .read_from(&self.input)
                    .map_err(|error| format!("cannot read operator terminal: {error}"))?
                    == Some(0)
            {
                return Err("operator terminal disconnected".to_owned());
            }
            if to_host.is_empty() && master.intersects(PollFlags::IN | CLOSED) {
                match to_host.read_from(&self.master) {
                    Ok(Some(0)) | Err(Errno::IO) => master_open = false,
                    Ok(Some(count)) => {
                        total_output += count as u64;
                        if total_output > limits.max_output_bytes {
                            return Err("output limit exceeded".to_owned());
                        }
                    }
                    Ok(None) => {}
                    Err(error) => return Err(format!("cannot read sandbox terminal: {error}")),
                }
            }
        }
    }
}

fn interest(enabled: bool, event: PollFlags) -> PollFlags {
    if enabled { event } else { PollFlags::empty() }
}

enum End {
    Exited(ExitStatus),
    Interrupted(i32),
}

struct SignalMask<'a> {
    terminal: &'a Terminal,
    saved: SigSet,
}

impl Drop for SignalMask<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.saved.thread_set_mask() {
            self.terminal
                .report_error(&format!("cannot restore host signal mask: {error}"));
        }
    }
}

struct RestoreTerminal<'a> {
    terminal: &'a Terminal,
    saved: Termios,
}

impl Drop for RestoreTerminal<'_> {
    fn drop(&mut self) {
        if let Err(error) = tcsetattr(&self.terminal.input, OptionalActions::Now, &self.saved) {
            self.terminal.report_error(&format!(
                "cannot restore operator terminal attributes: {error}"
            ));
        }
    }
}

struct Buffer {
    bytes: [u8; 4096],
    pending: std::ops::Range<usize>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            bytes: [0; 4096],
            pending: 0..0,
        }
    }
}

impl Buffer {
    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn read_from(&mut self, fd: &impl AsFd) -> io::Result<Option<usize>> {
        match io::read(fd, &mut self.bytes) {
            Ok(count) => {
                self.pending = 0..count;
                Ok(Some(count))
            }
            Err(Errno::AGAIN | Errno::INTR) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_to(&mut self, fd: &impl AsFd) -> io::Result<()> {
        match io::write(fd, &self.bytes[self.pending.clone()]) {
            Ok(0) => Err(Errno::IO),
            Ok(count) => {
                self.pending.start += count;
                Ok(())
            }
            Err(Errno::AGAIN | Errno::INTR) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
