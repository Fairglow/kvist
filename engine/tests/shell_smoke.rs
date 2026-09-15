//! End-to-end smoke tests for the interactive workspace shell (`kvist shell`).
//!
//! `shell_rejects_a_non_interactive_stdin` covers the piped-stdin rejection
//! with its actionable hint. `shell_on_a_pty_shows_the_banner_and_exits_cleanly`
//! drives the shell over a real pseudo-terminal through the full line-editor
//! stack: the welcome banner, the `help` builtin, and a clean `exit`. The
//! test answers the line editor's terminal queries (cursor position reports)
//! like a minimal terminal emulator, since a bare pty has no emulator behind
//! it.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn shell_rejects_a_non_interactive_stdin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .arg("shell")
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run kvist shell");
    assert!(
        !output.status.success(),
        "the shell must fail without an interactive terminal"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("interactive shell requires an interactive terminal"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("run `kvist shell` from an interactive terminal"),
        "stderr: {stderr}"
    );
}

/// The longest valid UTF-8 prefix of `bytes`, so multi-byte characters split
/// across pty reads are never misdecoded.
fn utf8_prefix(bytes: &[u8]) -> &str {
    let mut len = bytes.len();
    while len > 0 && std::str::from_utf8(&bytes[..len]).is_err() {
        len -= 1;
    }
    std::str::from_utf8(&bytes[..len]).expect("prefix is valid UTF-8")
}

/// A minimal terminal emulator: it tracks where the cursor is after the
/// shell's output and answers the line editor's cursor-position queries, so
/// a bare pty behaves like a real terminal.
struct FakeTerminal {
    row: u16,
    col: u16,
    width: u16,
}

/// The last screen row (0-based) for the 30-row test window.
const LAST_ROW: u16 = 29;

impl FakeTerminal {
    fn new(width: u16) -> Self {
        Self {
            row: 0,
            col: 0,
            width,
        }
    }

    /// Processes the characters the shell printed since the last call and
    /// appends any owed terminal replies (e.g. cursor position reports).
    /// Returns how many characters were fully processed; an incomplete
    /// escape sequence at the end is held back for the next call.
    fn feed(&mut self, chars: &[char], reply: &mut Vec<u8>) -> usize {
        let mut i = 0;
        for &ch in chars {
            match ch {
                '\x1b' => match self.try_parse_sequence(&chars[i..], reply) {
                    Some(consumed) => i += consumed,
                    None => return i, // incomplete escape: wait for more bytes
                },
                '\r' => self.col = 0,
                '\n' | '\u{0c}' => self.row = self.row.saturating_add(1).min(LAST_ROW),
                '\u{8}' => self.col = self.col.saturating_sub(1),
                '\t' => {
                    self.col = ((self.col / 8) + 1) * 8;
                    if self.col >= self.width {
                        self.col = 0;
                        self.row = self.row.saturating_add(1).min(LAST_ROW);
                    }
                }
                c if c.is_ascii_control() => {}
                _ => self.advance(1),
            }
        }
        i
    }

    fn advance(&mut self, columns: u16) {
        self.col = self.col.saturating_add(columns);
        if self.col >= self.width {
            self.col = 0;
            self.row = self.row.saturating_add(1).min(LAST_ROW);
        }
    }

    /// Interprets one complete escape sequence starting at `chars[0]` (which
    /// must be the escape character) and returns its length in characters,
    /// or `None` when the sequence is not complete yet.
    fn try_parse_sequence(&mut self, chars: &[char], reply: &mut Vec<u8>) -> Option<usize> {
        match chars.get(1) {
            None => None,
            Some('[') => {
                // CSI: parameters run until the final byte (0x40..=0x7E).
                let end = chars[2..]
                    .iter()
                    .position(|c| ('\x40'..='\x7e').contains(c))?
                    + 2;
                let params: String = chars[2..end].iter().collect();
                if let Some(response) = self.apply_csi(chars[end], &params) {
                    reply.extend_from_slice(response.as_bytes());
                }
                Some(end + 1)
            }
            Some(']') => {
                // OSC: ends at BEL or the ST sequence (ESC \).
                for j in 2..chars.len() {
                    if chars[j] == '\x07' {
                        return Some(j + 1);
                    }
                    if chars[j] == '\x1b' && chars.get(j + 1) == Some(&'\\') {
                        return Some(j + 2);
                    }
                }
                None
            }
            Some(_) => Some(2), // two-byte escape (e.g. save/restore cursor)
        }
    }

    /// Applies one CSI sequence and returns a reply owed to the terminal, if
    /// any (cursor position and device status reports).
    fn apply_csi(&mut self, final_byte: char, params: &str) -> Option<String> {
        let num = |value: &str| value.parse::<u16>().unwrap_or(1);
        match final_byte {
            'n' if params == "6" => Some(format!("\x1b[{};{}R", self.row + 1, self.col + 1)),
            'n' if params == "5" => Some("\x1b[0n".to_owned()),
            'c' => Some("\x1b[?62;1;6c".to_owned()),
            'H' | 'f' => {
                let mut parts = params.split(';');
                let row = parts
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(1);
                let col = parts
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(1);
                self.row = row.saturating_sub(1).min(LAST_ROW);
                self.col = col.saturating_sub(1).min(self.width.saturating_sub(1));
                None
            }
            'A' => {
                self.row = self.row.saturating_sub(num(params));
                None
            }
            'B' => {
                self.row = self.row.saturating_add(num(params)).min(LAST_ROW);
                None
            }
            'C' => {
                self.col = self
                    .col
                    .saturating_add(num(params))
                    .min(self.width.saturating_sub(1));
                None
            }
            'D' => {
                self.col = self.col.saturating_sub(num(params));
                None
            }
            _ => None, // clear, scroll, SGR, DEC private: no cursor movement
        }
    }
}

#[test]
#[cfg(unix)]
fn shell_on_a_pty_shows_the_banner_and_exits_cleanly() {
    use nix::errno::Errno;
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    use nix::pty::{Winsize, openpty};
    use nix::unistd::{read, write};
    use std::os::fd::AsFd;

    let dir = tempfile::tempdir().expect("tempdir");
    // Isolate the child from the user's real state and configuration.
    let state_home = dir.path().join("state");
    let config_home = dir.path().join("config");
    std::fs::create_dir_all(&state_home).expect("create state home");
    std::fs::create_dir_all(&config_home).expect("create config home");

    // A real window size so the height-aware pager never engages: the test
    // drives the shell programmatically and cannot press `q`.
    let winsize = Winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = openpty(Some(&winsize), None).expect("open a pty pair");
    let master = pty.master;
    let slave = pty.slave;

    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .arg("shell")
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-256color")
        .env("KVIST_NO_PAGER", "1")
        .env("XDG_STATE_HOME", &state_home)
        .env("XDG_CONFIG_HOME", &config_home)
        .env_remove("KVIST_CONFIG_PATH")
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .stdin(Stdio::from(
            slave.try_clone().expect("clone the slave for stdin"),
        ))
        .stdout(Stdio::from(
            slave.try_clone().expect("clone the slave for stdout"),
        ))
        .stderr(Stdio::from(slave))
        .spawn()
        .expect("spawn the shell on the pty");

    let flags = fcntl(master.as_fd(), FcntlArg::F_GETFL).expect("read the pty flags");
    fcntl(
        master.as_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .expect("make the pty master non-blocking");

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut terminal = FakeTerminal::new(100);
    let mut bytes = Vec::new();
    let mut consumed = 0usize;
    let mut buffer = [0u8; 8192];
    let mut help_sent = false;
    let mut exit_sent = false;

    loop {
        match read(master.as_fd(), &mut buffer) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(error) => match error {
                Errno::EIO => break, // the slave side closed: the shell exited
                Errno::EWOULDBLOCK => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                other => panic!(
                    "read from the pty master failed: {other}; output so far:\n{}",
                    utf8_prefix(&bytes)
                ),
            },
        }

        if Instant::now() > deadline {
            panic!(
                "timed out waiting for the shell; output so far:\n{}",
                utf8_prefix(&bytes)
            );
        }

        let output = utf8_prefix(&bytes);
        // Answer the line editor's terminal queries like a real terminal.
        let mut reply = Vec::new();
        let fresh: Vec<char> = output.chars().skip(consumed).collect();
        consumed += terminal.feed(&fresh, &mut reply);
        if !reply.is_empty() {
            write(master.as_fd(), &reply).expect("answer the terminal query");
        }

        if !help_sent && output.contains("Kvist Interactive Workspace Shell") {
            // Phase 1: the welcome banner.
            assert!(output.contains("Branch:"), "banner: {output}");
            assert!(output.contains("Component:  . (root)"), "banner: {output}");
            assert!(
                output.contains("Ctrl+L clear"),
                "the key hints must advertise Ctrl+L; banner: {output}"
            );
            assert!(
                output.contains('╭') && output.contains('╰'),
                "banner: {output}"
            );
            write(master.as_fd(), b"help\n").expect("send `help`");
            help_sent = true;
        }
        if help_sent && !exit_sent && output.contains("Kvist shell — builtins") {
            // Phase 2: the `help` builtin, with the themed prompt redrawn.
            assert!(output.contains("kvist ❯"), "prompt: {output}");
            assert!(output.contains("cd [COMPONENT]"), "help output: {output}");
            write(master.as_fd(), b"exit\n").expect("send `exit`");
            exit_sent = true;
        }
    }

    assert!(
        help_sent && exit_sent,
        "the banner and help phases must complete; output so far:\n{}",
        utf8_prefix(&bytes)
    );
    let status = child.wait().expect("wait for the shell");
    assert!(
        status.success(),
        "the shell must exit cleanly; output:\n{}",
        utf8_prefix(&bytes)
    );
}
