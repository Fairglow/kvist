use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::pty::{OpenptFlags, grantpt, ioctl_tiocgptpeer, openpt, unlockpt};
use rustix::termios::{LocalModes, Winsize, tcgetattr, tcsetwinsize};

const GALLA: &str = env!("CARGO_BIN_EXE_galla");
const WAIT: Duration = Duration::from_secs(10);

struct Terminal {
    child: Child,
    master: std::fs::File,
    slave: std::fs::File,
    output: Vec<u8>,
}

impl Terminal {
    fn spawn(workspace: &Path, args: &[&str]) -> Self {
        let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
        let master = openpt(flags).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let slave = std::fs::File::from(ioctl_tiocgptpeer(&master, flags).unwrap());
        set_size(&slave, 24, 80);
        let child = Command::new(GALLA)
            .args(args)
            .current_dir(workspace)
            .env("TERM", "xterm")
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave.try_clone().unwrap())
            .spawn()
            .unwrap();
        Self {
            child,
            master: master.into(),
            slave,
            output: Vec::new(),
        }
    }

    fn read(&mut self, timeout_ns: i64) -> bool {
        let mut descriptors = [PollFd::new(&self.master, PollFlags::IN)];
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: timeout_ns,
        };
        if poll(&mut descriptors, Some(&timeout)).unwrap() == 0 {
            return false;
        }
        let mut bytes = [0; 8192];
        let count = self.master.read(&mut bytes).unwrap();
        self.output.extend_from_slice(&bytes[..count]);
        count != 0
    }

    fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.output)
    }

    fn expect(&mut self, text: &str) {
        let start = Instant::now();
        while !self.text().contains(text) {
            assert!(
                start.elapsed() < WAIT && self.child.try_wait().unwrap().is_none(),
                "expected {text:?}, got {:?}",
                self.text()
            );
            self.read(50_000_000);
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }

    fn wait(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                while self.read(0) {}
                return status;
            }

            assert!(start.elapsed() < WAIT, "Galla did not exit");
            self.read(50_000_000);
        }
    }

    fn assert_cooked(&self) {
        assert!(
            tcgetattr(&self.slave)
                .unwrap()
                .local_modes
                .contains(LocalModes::ICANON | LocalModes::ECHO)
        );
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn set_size(terminal: &std::fs::File, rows: u16, cols: u16) {
    tcsetwinsize(
        terminal,
        Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        },
    )
    .unwrap();
}

#[test]
fn bash_prompt_uses_a_private_controlling_terminal() {
    let workspace = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn(
        workspace.path(),
        &["/usr/bin/bash", "--noprofile", "--norc"],
    );
    let host_node = terminal.slave.metadata().unwrap();
    terminal.expect("bash-");
    terminal.send(b"test -t 0 && test -t 1 && test -t 2 && : </dev/tty && printf 'term=%s\\n' \"$TERM\" && stty size && stat -Lc 'pty=%d:%i' /proc/self/fd/0 && printf 'tty-%s\\n' checked\n");
    terminal.expect("tty-checked\r\n");
    terminal.expect("term=xterm\r\n24 80\r\npty=");
    assert!(
        !terminal
            .text()
            .contains(&format!("pty={}:{}\r\n", host_node.dev(), host_node.ino()))
    );
    assert!(!terminal.text().contains("no job control"));

    set_size(&terminal.slave, 31, 97);
    terminal.send(b"stty size\n");
    terminal.expect("31 97\r\n");

    terminal.send(b"bash -c 'printf \"job-%s\\n\" ready; exec sleep 60'\n");
    terminal.expect("job-ready\r\n");
    terminal.output.clear();
    terminal.send(b"\x03");
    terminal.expect("bash-");
    terminal.send(b"printf 'still-%s\\n' alivx\x7fe\n");
    terminal.expect("still-alive\r\n");
    terminal.send(b"trap 'exit 23' EXIT; printf 'trap-%s\\n' ready\n");
    terminal.expect("trap-ready\r\n");
    terminal.send(b"\x04");
    assert_eq!(terminal.wait().code(), Some(23));
    terminal.assert_cooked();
}

#[test]
fn terminal_output_is_bounded() {
    let workspace = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn(workspace.path(), &["exec", "/usr/bin/yes"]);
    assert_eq!(terminal.wait().code(), Some(3));
    let diagnostic = "galla: output limit exceeded";
    assert!(terminal.text().contains(diagnostic));
    assert!(terminal.output.len() <= (1 << 20) + diagnostic.len() + 2);
    terminal.assert_cooked();
}

#[test]
fn host_termination_restores_the_terminal() {
    let workspace = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn(
        workspace.path(),
        &["/usr/bin/bash", "--noprofile", "--norc"],
    );
    terminal.expect("bash-");
    kill(Pid::from_raw(terminal.child.id() as i32), Signal::SIGTERM).unwrap();
    assert_eq!(terminal.wait().code(), Some(143));
    terminal.assert_cooked();
}

#[test]
fn redirected_exec_keeps_closed_input_and_separate_output() {
    let workspace = tempfile::tempdir().unwrap();
    let input = workspace.path().join("input");
    std::fs::write(&input, b"must not reach the payload\n").unwrap();
    let output = Command::new(GALLA)
        .current_dir(workspace.path())
        .args([
            "exec",
            "/usr/bin/bash",
            "-c",
            "if read -r line || test -t 0; then exit 99; fi; printf out; printf err >&2; exit 17",
        ])
        .stdin(Stdio::from(std::fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");
}
