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
    raw: Vec<u8>,
}

impl Viewer {
    fn start(path: &std::path::Path, columns: u16, rows: u16, numbers: bool) -> Self {
        Self::start_with(path, columns, rows, numbers, &[], &[])
    }

    fn start_with(
        path: &std::path::Path,
        columns: u16,
        rows: u16,
        numbers: bool,
        args: &[&str],
        environment: &[(&str, &str)],
    ) -> Self {
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
        command.args(args).envs(environment.iter().copied());
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
            raw: Vec::new(),
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
                    self.raw.extend_from_slice(&bytes[..count]);
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

    fn wait_failure(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(1), "{status}");
                let terminal = nix::sys::termios::tcgetattr(&self.slave).unwrap();
                assert!(
                    terminal
                        .local_flags
                        .contains(nix::sys::termios::LocalFlags::ICANON)
                );
                assert!(
                    terminal
                        .local_flags
                        .contains(nix::sys::termios::LocalFlags::ECHO)
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("viewer did not report edit failure");
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
    // Prefer the run-time `CARGO_MANIFEST_DIR` (set by cargo) so the fixture
    // path stays correct when a previously built test binary is reused after
    // the repository has moved; the compile-time value is the fallback.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
    let path = std::path::Path::new(&manifest).join("tests/fixtures/tagged_document.md");
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
fn table_overflow_is_accessible_and_resize_preserves_the_first_row() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("table.md");
    std::fs::write(
        &path,
        "| A | B |\n|---|---|\n| x | 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz |",
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 30, 10, false);
    assert_eq!(viewer.screen.line(1), "A | B");
    assert_eq!(viewer.screen.line(2), "x | 0123456789ABCDEFGHIJKLMNO");
    viewer.keys(b"\x1b[C\x1b[C\x1b[C\x1b[C");
    assert_eq!(viewer.screen.line(2), "0123456789ABCDEFGHIJKLMNOPQRS");
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.resize(80, 12);
    assert!(viewer.screen.line(1).starts_with('A'));
    assert!(
        viewer
            .screen
            .line(2)
            .ends_with("abcdefghijklmnopqrstuvwxyz")
    );
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

#[test]
fn tty_color_policy_and_light_theme_survive_resize() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("appearance.md");
    std::fs::write(&path, "# Heading\n\n```rust\nlet x = 1;\n```").unwrap();
    for (no_color, term, colors) in [
        ("", "xterm-256color", true),
        ("1", "xterm-256color", false),
        ("", "dumb", false),
    ] {
        let mut viewer = Viewer::start_with(
            &path,
            40,
            12,
            false,
            &["--theme", "light", "--color", "auto"],
            &[("NO_COLOR", no_color), ("TERM", term), ("COLORTERM", "")],
        );
        for resized in [false, true] {
            if resized {
                viewer.raw.clear();
                viewer.resize(60, 14);
            }
            let raw = String::from_utf8_lossy(&viewer.raw);
            assert_eq!(raw.contains("\x1b[34m"), colors, "{raw:?}");
            assert_eq!(raw.contains("\x1b[48;5;254m"), colors, "{raw:?}");
            assert_eq!(viewer.screen.line(1), "# Heading");
        }
        viewer.quit();
    }
}

#[test]
fn ascii_diagram_overflow_remains_scrollable_and_canonical_on_resize() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("diagram.md");
    std::fs::write(
        &path,
        "```mermaid\nflowchart LR\nA[First long label] --> B[Second long label]\n```",
    )
    .unwrap();
    let mut viewer = Viewer::start_with(&path, 30, 10, false, &["--diagram-charset", "ascii"], &[]);
    let label_row = (1..9)
        .find(|&row| viewer.screen.line(row).contains("First long label"))
        .unwrap();
    assert!(!viewer.screen.line(label_row).contains("Second long label"));
    viewer.keys(&b"\x1b[C".repeat(60));
    assert!(
        viewer.screen.line(label_row).contains("Second long label"),
        "{}",
        viewer.screen.line(label_row)
    );
    viewer.resize(80, 12);
    assert!(
        viewer.screen.line(label_row).contains("First long label")
            && viewer.screen.line(label_row).contains("Second long label")
    );
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
}

#[test]
fn editing_reenters_without_losing_projection_or_appearance_options() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("edit.md");
    std::fs::write(&path, "# Initial\n\n$x^2$").unwrap();
    let editor = temp.path().join("editor");
    std::fs::write(&editor, "#!/bin/sh\ncat > \"$1\" <<'EOF'\n# Edited H~2~O\n\n$x^2$\n\n```rust\nlet x = 1;\n```\n\n```mermaid\nflowchart LR\nA --> B\n```\nEOF\n").unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut viewer = Viewer::start_with(
        &path,
        80,
        24,
        false,
        &[
            "--theme",
            "light",
            "--color",
            "always",
            "--math",
            "source",
            "--mermaid",
            "source",
        ],
        &[
            ("EDITOR", editor.to_str().unwrap()),
            ("TERM", "xterm-256color"),
            ("COLORTERM", ""),
        ],
    );
    for _ in 0..5 {
        viewer.raw.clear();
        viewer.keys(b"e");
        assert_eq!(viewer.screen.line(1), "# Edited H₂O");
        assert_eq!(viewer.screen.line(3), "$x^2$");
        let raw = String::from_utf8_lossy(&viewer.raw);
        assert!(
            raw.contains("\x1b[48;5;254m") && raw.contains("flowchart LR"),
            "{raw:?}"
        );
    }
    viewer.resize(60, 20);
    assert_eq!(viewer.screen.line(3), "$x^2$");
    viewer.quit();
}

#[test]
fn editor_failures_propagate_and_restore_terminal_state() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let moving_editor = temp.path().join("move-editor");
    std::fs::write(&moving_editor, "#!/bin/sh\nmv \"$1\" \"$1.moved\"\n").unwrap();
    std::fs::set_permissions(&moving_editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    for editor in [
        "/does-not-exist-blad-editor",
        "/bin/false",
        moving_editor.to_str().unwrap(),
    ] {
        let path = temp.path().join("failure.md");
        std::fs::write(&path, "# Initial").unwrap();
        let mut viewer = Viewer::start_with(&path, 40, 12, false, &[], &[("EDITOR", editor)]);
        viewer.keys(b"e");
        viewer.wait_failure();
        assert!(String::from_utf8_lossy(&viewer.raw).contains("Failed to display markdown"));
    }
}

#[test]
fn fitting_code_tabs_do_not_acquire_phantom_horizontal_scroll() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tabs.md");
    for syntax in ["text", "rust"] {
        std::fs::write(&path, format!("```{syntax}\n\tX\n```")).unwrap();
        for numbers in [false, true] {
            let mut viewer = Viewer::start(&path, 20, 8, numbers);
            let expected = if numbers { "1         X" } else { "        X" };
            assert_eq!(viewer.screen.line(1), expected);
            viewer.keys(&b"\x1b[C".repeat(10));
            assert_eq!(viewer.screen.line(1), expected);
            viewer.quit();
        }
    }
}

#[test]
fn following_a_file_link_opens_the_target_and_back_returns() {
    let temp = tempfile::tempdir().unwrap();
    let linked = temp.path().join("linked.md");
    let source = temp.path().join("source.md");
    std::fs::write(&linked, "# Linked Title\n\nlinked body").unwrap();
    std::fs::write(
        &source,
        "# Source Title\n\n[open the linked file](linked.md)",
    )
    .unwrap();
    let mut viewer = Viewer::start(&source, 80, 24, false);
    assert!(viewer.screen.line(0).contains("source.md"));
    // Enter link mode; the header announces the link count and first link.
    viewer.keys(b"f");
    assert!(
        viewer.screen.line(0).contains("Links: 1/1"),
        "link mode header not shown: {}",
        viewer.screen.line(0)
    );
    // Follow the link; the viewer must open the linked file.
    viewer.keys(b"\r");
    assert!(
        viewer.screen.line(0).contains("linked.md"),
        "expected the linked file header, got: {}",
        viewer.screen.line(0)
    );
    assert_eq!(viewer.screen.line(1), "# Linked Title");
    // Back navigation must return to the original document.
    viewer.keys(b"b");
    assert!(
        viewer.screen.line(0).contains("source.md"),
        "expected the source file header, got: {}",
        viewer.screen.line(0)
    );
    assert_eq!(viewer.screen.line(1), "# Source Title");
    viewer.quit();
}

#[test]
fn link_mode_is_inert_without_links_and_esc_exits_it() {
    let temp = tempfile::tempdir().unwrap();
    // A document with no links: `f` must not enter link mode.
    let no_links = temp.path().join("nolinks.md");
    std::fs::write(&no_links, "# No Links Here\n\njust plain prose").unwrap();
    let mut viewer = Viewer::start(&no_links, 80, 24, false);
    viewer.keys(b"f");
    assert!(
        !viewer.screen.line(0).contains("Links:"),
        "link mode must not start without links: {}",
        viewer.screen.line(0)
    );
    viewer.quit();
    // A document with a link: `f` enters link mode, Esc leaves it without
    // following, and the header reverts to the file title.
    let with_link = temp.path().join("withlink.md");
    std::fs::write(&with_link, "# Titled\n\n[link](https://example.com)").unwrap();
    let mut viewer = Viewer::start(&with_link, 80, 24, false);
    viewer.keys(b"f");
    assert!(viewer.screen.line(0).contains("Links: 1/1"));
    viewer.keys(b"\x1b");
    assert!(
        viewer.screen.line(0).contains("withlink.md"),
        "Esc must exit link mode: {}",
        viewer.screen.line(0)
    );
    viewer.quit();
}
