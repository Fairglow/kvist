use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::{PollFd, PollFlags, poll};
use nix::pty::{Winsize, openpty};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use vte::{Params, Parser, Perform};

struct Screen {
    cells: Vec<Vec<char>>,
    x: usize,
    y: usize,
    wrap: bool,
    scrolls: usize,
}

impl Screen {
    fn new(columns: usize, rows: usize) -> Self {
        Self {
            cells: vec![vec![' '; columns]; rows],
            x: 0,
            y: 0,
            wrap: false,
            scrolls: 0,
        }
    }

    fn down(&mut self) {
        if self.y + 1 == self.cells.len() {
            self.cells.rotate_left(1);
            self.cells.last_mut().unwrap().fill(' ');
            self.scrolls += 1;
        } else {
            self.y += 1;
        }
    }

    fn line(&self, row: usize) -> String {
        self.cells[row]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

impl Perform for Screen {
    fn print(&mut self, c: char) {
        if self.wrap {
            self.x = 0;
            self.down();
            self.wrap = false;
        }
        self.cells[self.y][self.x] = c;
        if self.x + 1 == self.cells[0].len() {
            self.wrap = true;
        } else {
            self.x += 1;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => {
                self.x = 0;
                self.wrap = false;
            }
            b'\n' => {
                self.down();
                self.wrap = false;
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        let values: Vec<usize> = params.iter().map(|param| usize::from(param[0])).collect();
        match action {
            'H' | 'f' => {
                self.y = values
                    .first()
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(self.cells.len() - 1);
                self.x = values
                    .get(1)
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(self.cells[0].len() - 1);
                self.wrap = false;
            }
            'J' if values.first() == Some(&2) => {
                for row in &mut self.cells {
                    row.fill(' ');
                }
            }
            _ => {}
        }
    }
}

struct Viewer {
    child: Child,
    master: File,
    slave: OwnedFd,
    parser: Parser,
    screen: Screen,
}

impl Viewer {
    fn start(path: &std::path::Path, columns: u16, rows: u16, numbers: bool) -> Self {
        let pty = openpty(
            Some(&Winsize {
                ws_row: rows,
                ws_col: columns,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            None,
        )
        .unwrap();
        fcntl(&pty.master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
        // Crossterm prefers /dev/tty over stdin for terminal state and size.
        let mut command = Command::new("setsid");
        command
            .args(["--ctty", "--wait", env!("CARGO_BIN_EXE_blad"), "view"])
            .arg(path);
        if numbers {
            command.arg("--line-numbers");
        }
        let child = command
            .stdin(Stdio::from(pty.slave.try_clone().unwrap()))
            .stdout(Stdio::from(pty.slave.try_clone().unwrap()))
            .stderr(Stdio::from(pty.slave.try_clone().unwrap()))
            .spawn()
            .unwrap();
        let mut viewer = Self {
            child,
            master: File::from(pty.master),
            slave: pty.slave,
            parser: Parser::new(),
            screen: Screen::new(usize::from(columns), usize::from(rows)),
        };
        viewer.read_frame();
        viewer
    }

    fn read_frame(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received = false;
        while Instant::now() < deadline {
            let mut descriptors = [PollFd::new(self.master.as_fd(), PollFlags::POLLIN)];
            let ready = poll(&mut descriptors, 100_u16).unwrap();
            if ready == 0 {
                if received {
                    return;
                }
                continue;
            }
            let mut bytes = [0; 16384];
            match self.master.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    self.parser.advance(&mut self.screen, &bytes[..count]);
                    received = true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("PTY read failed: {error}"),
            }
        }
        assert!(received, "viewer did not produce a frame");
    }

    fn keys(&mut self, keys: &[u8]) {
        self.master.write_all(keys).unwrap();
        self.read_frame();
    }

    fn resize(&mut self, columns: u16, rows: u16) {
        assert!(
            Command::new("stty")
                .args(["cols", &columns.to_string(), "rows", &rows.to_string()])
                .stdin(Stdio::from(self.slave.try_clone().unwrap()))
                .status()
                .unwrap()
                .success()
        );
        self.screen = Screen::new(usize::from(columns), usize::from(rows));
        kill(
            Pid::from_raw(self.child.id().try_into().unwrap()),
            Signal::SIGWINCH,
        )
        .unwrap();
        self.read_frame();
    }

    fn quit(&mut self) {
        self.master.write_all(b"q").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "viewer exited {status}");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("viewer did not quit");
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn tagged_document_keeps_its_first_line_without_screen_scrolling() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tagged_document.md");
    let mut viewer = Viewer::start(&path, 80, 24, false);
    assert_eq!(viewer.screen.scrolls, 0);
    assert_eq!(
        viewer.screen.line(1).trim_end_matches([' ', '░', '█']),
        "# Final Artifact-Model Redesign Compliance Review"
    );
    assert!(viewer.screen.line(0).contains("Blad"));
    viewer.quit();
}

#[test]
fn heading_prose_and_gutter_share_the_terminal_width() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp
        .path()
        .join("a-deliberately-long-document-file-name.md");
    std::fs::write(
        &path,
        format!(
            "# FIRST TITLE with a very long heading\n\n{}\n\nend",
            "123456789 123456789 123456789\n\n".repeat(30)
        ),
    )
    .unwrap();
    for numbers in [false, true] {
        let mut viewer = Viewer::start(&path, 30, 12, numbers);
        assert_eq!(viewer.screen.scrolls, 0);
        assert!(viewer.screen.line(1).contains("# FIRST TITLE"));
        assert!(viewer.screen.line(0).starts_with("=== Blad:"));
        viewer.quit();
    }
}

#[test]
fn code_overflow_is_clipped_and_horizontal_offset_survives_short_lines() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("code.md");
    std::fs::write(
        &path,
        "```text\n0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\nx\n```\n\nsmall",
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 30, 10, false);
    assert_eq!(viewer.screen.line(1), "0123456789ABCDEFGHIJKLMNOPQRS");
    assert_eq!(viewer.screen.line(2), "x");
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.keys(b"\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C");
    assert_eq!(viewer.screen.line(1), "ABCDEFGHIJKLMNOPQRSTUVWXYZabc");
    assert_eq!(viewer.screen.line(2), "");
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
}

#[test]
fn resize_reflows_content_and_tiny_viewport_remains_quittable() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("resize.md");
    std::fs::write(
        &path,
        "# FIRST TITLE\n\n123456789 123456789 123456789 123456789",
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 30, 10, false);
    assert_eq!(viewer.screen.line(3), "123456789 123456789 123456789");
    assert_eq!(viewer.screen.line(4), "123456789");
    viewer.resize(50, 12);
    assert_eq!(
        viewer.screen.line(3),
        "123456789 123456789 123456789 123456789"
    );
    assert_eq!(viewer.screen.line(4), "");
    assert!(viewer.screen.line(0).starts_with("=== Blad:"));
    assert!(viewer.screen.line(11).contains("TOP"));
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
    let mut tiny = Viewer::start(&path, 1, 1, false);
    assert_eq!(tiny.screen.scrolls, 0);
    tiny.quit();
    std::fs::write(&path, "").unwrap();
    let mut empty = Viewer::start(&path, 30, 10, false);
    assert_eq!(empty.screen.line(1), "");
    assert!(empty.screen.line(9).contains("L0-0 of 0"));
    assert_eq!(empty.screen.scrolls, 0);
    empty.quit();
}
