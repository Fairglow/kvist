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
    reversed: Vec<Vec<bool>>,
    reverse: bool,
    underlined: Vec<Vec<bool>>,
    underline: bool,
    x: usize,
    y: usize,
    wrap: bool,
    scrolls: usize,
}

impl Screen {
    fn new(columns: usize, rows: usize) -> Self {
        Self {
            cells: vec![vec![' '; columns]; rows],
            reversed: vec![vec![false; columns]; rows],
            reverse: false,
            underlined: vec![vec![false; columns]; rows],
            underline: false,
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
            self.reversed.rotate_left(1);
            self.reversed.last_mut().unwrap().fill(false);
            self.underlined.rotate_left(1);
            self.underlined.last_mut().unwrap().fill(false);
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

    fn content_line(&self, row: usize) -> String {
        self.line(row).trim_end_matches([' ', '░', '█']).to_string()
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
        self.reversed[self.y][self.x] = self.reverse;
        self.underlined[self.y][self.x] = self.underline;
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
                for row in &mut self.reversed {
                    row.fill(false);
                }
                for row in &mut self.underlined {
                    row.fill(false);
                }
            }
            'm' => {
                let mut values = values.into_iter();
                while let Some(value) = values.next() {
                    match value {
                        0 => {
                            self.reverse = false;
                            self.underline = false;
                        }
                        27 => self.reverse = false,
                        7 => self.reverse = true,
                        4 => self.underline = true,
                        24 => self.underline = false,
                        38 | 48 => {
                            let count = if values.next() == Some(2) { 3 } else { 1 };
                            for _ in 0..count {
                                values.next();
                            }
                        }
                        _ => {}
                    }
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
            .args(["--ctty", "--wait", env!("CARGO_BIN_EXE_blad")])
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
        self.read_update(true);
    }

    fn read_update(&mut self, required: bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received = false;
        while Instant::now() < deadline {
            let mut descriptors = [PollFd::new(self.master.as_fd(), PollFlags::POLLIN)];
            let ready = poll(&mut descriptors, 100_u16).unwrap();
            if ready == 0 {
                if received || !required {
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
        self.read_update(false);
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
        self.exit_with(b"Q");
    }

    fn exit_with(&mut self, keys: &[u8]) {
        self.master.write_all(keys).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "viewer exited {status}");
                let terminal = nix::sys::termios::tcgetattr(&self.slave).unwrap();
                assert!(terminal.local_flags.contains(
                    nix::sys::termios::LocalFlags::ICANON | nix::sys::termios::LocalFlags::ECHO
                ));
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("viewer did not quit");
    }
}
#[test]
fn occurrence_search_is_precise_wraps_and_supports_reverse_and_regex() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("search.md");
    std::fs::write(&path, "before **hit** gap hit after\n\nhit[1] HIT2 hit3").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    viewer.keys(b"/hit\r");
    assert!(
        viewer.screen.line(11).contains("1/5"),
        "{}",
        viewer.screen.line(11)
    );
    for column in 0..28 {
        assert_eq!(
            viewer.screen.reversed[1][column],
            (7..10).contains(&column) || (15..18).contains(&column),
            "column {column}"
        );
    }
    viewer.keys(b"N");
    assert!(viewer.screen.line(11).contains("5/5"));
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("1/5"));
    viewer.keys(b"?hit\r");
    assert!(viewer.screen.line(11).contains("2/5"));
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("1/5"));
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("5/5"));
    viewer.keys(b"/hit[1]\r");
    assert!(viewer.screen.line(11).contains("1/1"));
    viewer.keys(b"/\x12hit[0-9]\r");
    assert!(viewer.screen.line(11).contains("1/2"));
    assert!(viewer.screen.line(11).contains("regex"));
    viewer.keys(b"/\x12[\r");
    assert!(viewer.screen.line(11).contains("Invalid regex"));
    viewer.keys(b"/absent\r");
    assert!(viewer.screen.line(11).contains("No matches"));
    viewer.quit();
}
#[test]
fn monochrome_search_inside_code_is_precise_and_esc_does_not_close_a_tab() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mono.md");
    std::fs::write(&path, "before `code needle after` tail").unwrap();
    let mut viewer = Viewer::start_with(&path, 60, 12, false, &["--color", "never"], &[]);
    viewer.keys(b"/needle\r");
    for column in 0..29 {
        assert_eq!(
            viewer.screen.underlined[1][column],
            (12..18).contains(&column),
            "column {column}"
        );
    }
    viewer.keys(b"/missing\x1b");
    assert!(viewer.child.try_wait().unwrap().is_none());
    assert!(viewer.screen.underlined[1].iter().all(|value| !value));
    viewer.exit_with(b"q");
}

#[test]
fn search_and_link_navigation_reveal_horizontal_positions_and_reflow() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wide.md");
    std::fs::write(&path, format!("```text\n{} needle\n```\n\n| left | right |\n| --- | --- |\n| [first](#top) | [{} target](#top) |\n\n# Top\n\n{}", "x".repeat(100), "long".repeat(20), "tail\n\n".repeat(20))).unwrap();
    let mut viewer = Viewer::start(&path, 35, 12, true);
    viewer.keys(b"/needle\r");
    assert!((1..11).any(|row| viewer.screen.line(row).contains("needle")));
    viewer.keys(b"\t\x1b[1;5C");
    assert!(viewer.screen.line(11).contains("Links: 2/2"));
    assert!((1..11).any(|row| viewer.screen.line(row).contains("long")));
    viewer.resize(45, 14);
    assert!(viewer.screen.line(13).contains("Links: 2/2"));
    assert!((1..13).any(|row| viewer.screen.line(row).contains("long")));
    viewer.quit();
}

#[test]
fn heading_jumps_and_full_half_pages_have_distinct_distances() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("headings.md");
    std::fs::write(
        &path,
        format!(
            "# First\n\n{}\n\n## Second\n\n{}",
            "filler\n\n".repeat(30),
            "tail\n\n".repeat(30)
        ),
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 60, 12, false);
    viewer.keys(b"]");
    assert_eq!(viewer.screen.content_line(1), "## Second");
    viewer.keys(b"[");
    assert_eq!(viewer.screen.content_line(1), "# First");
    viewer.keys(b"\x06");
    assert!(viewer.screen.line(11).contains("L11-"));
    viewer.keys(b"\x02");
    assert!(viewer.screen.line(11).contains("L1-"));
    viewer.keys(b"\x04");
    assert!(viewer.screen.line(11).contains("L6-"));
    viewer.keys(b"\x15");
    assert!(viewer.screen.line(11).contains("L1-"));
    viewer.quit();
}

#[test]
fn spatial_links_select_only_the_label_and_heading_keys_are_separate() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("links.md");
    std::fs::write(
        &path,
        "# Top\n\n[left](#top) gap [right](#bottom)\n\n[below](#top)\n\n## Bottom\n\nend",
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 60, 12, false);
    viewer.keys(b"\t");
    assert!(viewer.screen.line(11).contains("Links: 1/3"));
    for column in 0..14 {
        assert_eq!(viewer.screen.reversed[3][column], column < 4);
    }
    viewer.keys(b"\x1b[1;5C");
    assert!(viewer.screen.line(11).contains("Links: 2/3"));
    for column in 0..14 {
        assert_eq!(viewer.screen.reversed[3][column], (9..14).contains(&column));
    }
    viewer.keys(b"\x1b[1;5B");
    assert!(viewer.screen.line(11).contains("Links: 3/3"));
    viewer.keys(b"\x1b[1;5A");
    assert!(viewer.screen.line(11).contains("Links: 1/3"));
    viewer.keys(b"]");
    assert!(viewer.screen.line(11).contains("L"));
    viewer.keys(b"[");
    assert_eq!(viewer.screen.content_line(1), "# Top");
    viewer.quit();
}

#[test]
fn closing_tabs_repairs_history_without_terminal_churn() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.md");
    let second = temp.path().join("second.md");
    let third = temp.path().join("third.md");
    for (path, text) in [
        (&first, "# First"),
        (&second, "# Second"),
        (&third, "# Third"),
    ] {
        std::fs::write(path, text).unwrap();
    }
    let mut viewer = Viewer::start_with(
        &first,
        80,
        12,
        false,
        &[second.to_str().unwrap(), third.to_str().unwrap()],
        &[],
    );
    viewer.raw.clear();
    viewer.keys(b"\x1b[1;3Cq");
    assert!(viewer.child.try_wait().unwrap().is_none());
    assert_eq!(viewer.screen.content_line(1), "# Third");
    assert!(!viewer.screen.line(0).contains("second.md"));
    viewer.keys(b"b");
    assert_eq!(viewer.screen.content_line(1), "# First");
    viewer.keys(b"b");
    assert_eq!(viewer.screen.content_line(1), "# First");
    let raw = String::from_utf8_lossy(&viewer.raw);
    assert!(!raw.contains("\x1b[?1049"), "{raw:?}");
    viewer.keys(b"\x1b");
    assert_eq!(viewer.screen.content_line(1), "# Third");
    viewer.quit();
}

#[test]
fn scrolling_uses_changed_rows_without_full_clears_and_help_is_modal() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scroll.md");
    std::fs::write(
        &path,
        (0..80).map(|i| format!("line {i}\n\n")).collect::<String>(),
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 60, 12, false);
    viewer.raw.clear();
    viewer.keys(b"j");
    let raw = String::from_utf8_lossy(&viewer.raw);
    assert!(!raw.contains("\x1b[2J"), "{raw:?}");
    assert!(
        !raw.contains("=== Blad:"),
        "unchanged header was repainted: {raw:?}"
    );
    viewer.keys(b"g");
    viewer.keys(b"u");
    viewer.raw.clear();
    viewer.keys(b"u");
    assert!(viewer.raw.is_empty(), "unchanged frame produced output");
    viewer.keys(b"d");
    assert!(viewer.screen.line(11).contains("L6-"));
    let before_help = viewer.screen.content_line(1);
    viewer.keys(b"h");
    assert!((1..11).any(|row| viewer.screen.line(row).contains("Ctrl+R")));
    viewer.keys(b"\x1b");
    assert!(viewer.child.try_wait().unwrap().is_none());
    assert_eq!(viewer.screen.content_line(1), before_help);
    viewer.raw.clear();
    viewer.keys(b"\x0c");
    assert!(String::from_utf8_lossy(&viewer.raw).contains("=== Blad:"));
    viewer.quit();
}
impl Viewer {
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
    // Row 1 is the code panel's top frame line; the first code line is row 2.
    assert_eq!(viewer.screen.line(1).trim(), "");
    assert_eq!(viewer.screen.line(2), "0123456789ABCDEFGHIJKLMNOPQRS");
    assert_eq!(viewer.screen.line(3), "x");
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.keys(b"\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C");
    assert_eq!(viewer.screen.line(2), "ABCDEFGHIJKLMNOPQRSTUVWXYZabc");
    assert_eq!(viewer.screen.line(3), "");
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
            // The code panel's top frame line occupies line 1, so the code
            // line `X` is line 2 (and the gutter shows `2` with line numbers).
            let expected = if numbers { "2         X" } else { "        X" };
            assert_eq!(viewer.screen.line(2), expected);
            viewer.keys(&b"\x1b[C".repeat(10));
            assert_eq!(viewer.screen.line(2), expected);
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
        viewer.screen.line(23).contains("Links: 1/1"),
        "link mode header not shown: {}",
        viewer.screen.line(23)
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
        !viewer.screen.line(23).contains("Links:"),
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
    assert!(viewer.screen.line(23).contains("Links: 1/1"));
    viewer.keys(b"\x1b");
    assert!(
        viewer.screen.line(0).contains("withlink.md"),
        "Esc must exit link mode: {}",
        viewer.screen.line(0)
    );
    viewer.quit();
}

#[test]
fn enter_follows_the_link_at_the_cursor_without_link_mode() {
    let temp = tempfile::tempdir().unwrap();
    let linked = temp.path().join("linked.md");
    let source = temp.path().join("source.md");
    std::fs::write(&linked, "# Linked Title\n\nlinked body").unwrap();
    // The link is on the first content line, at the top of the viewport.
    std::fs::write(&source, "intro\n[open](linked.md)").unwrap();
    let mut viewer = Viewer::start(&source, 80, 24, false);
    assert!(viewer.screen.line(0).contains("source.md"));
    // Enter in normal mode follows the first link at or below the top of the
    // viewport — no `f` link mode required.
    viewer.keys(b"\r");
    assert!(
        viewer.screen.line(0).contains("linked.md"),
        "Enter must follow the link at the cursor, got: {}",
        viewer.screen.line(0)
    );
    assert_eq!(viewer.screen.line(1), "# Linked Title");
    // Back navigation returns to the source.
    viewer.keys(b"b");
    assert!(
        viewer.screen.line(0).contains("source.md"),
        "back must return to the source, got: {}",
        viewer.screen.line(0)
    );
    viewer.quit();
}

#[test]
fn anchors_tabs_and_back_preserve_document_position() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    let target = temp.path().join("other file.md");
    std::fs::write(
        &source,
        format!(
            "[jump](#destination)\n\n{}\n\n## Destination\n\n[other](other%20file.md#target)\n\n{}",
            "filler\n\n".repeat(40),
            "tail\n\n".repeat(20)
        ),
    )
    .unwrap();
    std::fs::write(
        &target,
        format!(
            "# Other\n\n{}\n\n## Target\n\n{}",
            "filler\n\n".repeat(30),
            "tail\n\n".repeat(20)
        ),
    )
    .unwrap();
    let mut viewer = Viewer::start(&source, 80, 12, false);
    viewer.keys(b"\r");
    assert_eq!(viewer.screen.content_line(1), "## Destination");
    viewer.keys(b"\t\r");
    assert_eq!(viewer.screen.content_line(1), "## Target");
    assert!(
        viewer.screen.line(0).contains("source.md")
            && viewer.screen.line(0).contains("other file.md")
    );
    viewer.keys(b"\x1b[1;3D");
    assert_eq!(viewer.screen.content_line(1), "## Destination");
    viewer.keys(b"\x1b[1;3C");
    assert_eq!(viewer.screen.content_line(1), "## Target");
    viewer.keys(b"b");
    assert_eq!(viewer.screen.content_line(1), "## Destination");
    viewer.quit();
}

#[test]
fn named_duplicate_and_wrapped_links_select_without_mode() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    std::fs::write(temp.path().join("first.md"), "# First").unwrap();
    std::fs::write(temp.path().join("second.md"), "# Second").unwrap();
    std::fs::write(
        &source,
        "same\n\n[same][first] and [same][second]\n\n[first]: first.md\n[second]: second.md",
    )
    .unwrap();
    let mut viewer = Viewer::start(&source, 40, 12, false);
    viewer.keys(b"\t\t\r");
    assert_eq!(viewer.screen.line(1), "# Second");
    viewer.keys(b"b");
    std::fs::write(
        &source,
        "[a very long label that wraps across multiple rows](first.md)",
    )
    .unwrap();
    viewer.quit();
    let mut viewer = Viewer::start(&source, 16, 10, false);
    viewer.keys(b"\r");
    assert_eq!(viewer.screen.line(1), "# First");
    viewer.quit();
}

#[test]
fn missing_anchor_is_visible_and_search_moves_the_viewport() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    std::fs::write(
        &source,
        format!(
            "[missing](#absent)\n\n{}\n\nneedle\n\n{}",
            "filler\n\n".repeat(30),
            "tail\n\n".repeat(20)
        ),
    )
    .unwrap();
    let mut viewer = Viewer::start(&source, 80, 12, false);
    viewer.keys(b"\r");
    assert!(
        viewer.screen.line(11).contains("anchor not found"),
        "{}",
        viewer.screen.line(11)
    );
    viewer.keys(b"/needle\r");
    assert!((1..11).any(|row| viewer.screen.line(row).contains("needle")));
    viewer.quit();
}

#[test]
fn mouse_follows_named_links_and_does_not_activate_plain_duplicates() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    std::fs::write(temp.path().join("target.md"), "# Target").unwrap();
    std::fs::write(&source, "same\n\n[same](target.md)").unwrap();
    let mut viewer = Viewer::start(&source, 40, 12, false);
    viewer.keys(b"\x1b[<0;2;2M");
    assert_eq!(viewer.screen.line(1), "same");
    viewer.keys(b"\x1b[<0;2;4M");
    assert_eq!(viewer.screen.line(1), "# Target");
    viewer.quit();
}

#[test]
fn theme_file_settings_survive_tabs_edit_and_resize() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    let theme = temp.path().join("theme.json");
    std::fs::write(&source, "# Source\n\n[target](target.md)").unwrap();
    std::fs::write(
        temp.path().join("target.md"),
        "# Target\n\n```python\nprint('yes')\n```",
    )
    .unwrap();
    std::fs::write(
        &theme,
        r##"{"theme":"light","palette":{"heading":"#010203"}}"##,
    )
    .unwrap();
    let mut viewer = Viewer::start_with(
        &source,
        50,
        14,
        false,
        &["--theme-file", theme.to_str().unwrap(), "--color", "always"],
        &[("COLORTERM", "truecolor"), ("EDITOR", "/bin/true")],
    );
    viewer.keys(b"\r");
    for action in [b"e".as_slice(), b"\x1b[1;3D\x1b[1;3C".as_slice()] {
        viewer.raw.clear();
        viewer.keys(action);
        let raw = String::from_utf8_lossy(&viewer.raw);
        assert!(
            raw.contains("\x1b[38;2;1;2;3m") && raw.contains("\x1b[48;5;254m"),
            "{raw:?}"
        );
        assert_eq!(viewer.screen.line(1), "# Target");
    }
    viewer.raw.clear();
    viewer.resize(70, 16);
    assert!(String::from_utf8_lossy(&viewer.raw).contains("\x1b[38;2;1;2;3m"));
    viewer.quit();
}

#[test]
fn multiple_initial_tabs_keep_their_order_and_deduplicate() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.md");
    let second = temp.path().join("second.md");
    std::fs::write(&first, "# First").unwrap();
    std::fs::write(&second, "# Second").unwrap();
    let mut viewer = Viewer::start_with(
        &first,
        80,
        12,
        false,
        &[second.to_str().unwrap(), first.to_str().unwrap()],
        &[],
    );
    assert!(
        viewer.screen.line(0).contains("[first.md] |  second.md"),
        "{}",
        viewer.screen.line(0)
    );
    let first_column = viewer.screen.line(0).find("first.md");
    let second_column = viewer.screen.line(0).find("second.md");
    viewer.keys(b"\x1b[1;3C");
    assert!(
        viewer.screen.line(0).contains("first.md  | [second.md]"),
        "{}",
        viewer.screen.line(0)
    );
    assert_eq!(viewer.screen.line(1), "# Second");
    assert_eq!(viewer.screen.line(0).find("first.md"), first_column);
    assert_eq!(viewer.screen.line(0).find("second.md"), second_column);
    viewer.keys(b"\x1b[1;3D");
    assert!(viewer.screen.line(0).contains("[first.md] |  second.md"));
    viewer.quit();
}

#[test]
fn browser_output_is_captured_and_retained_without_corrupting_the_screen() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("browser.md");
    let launcher = temp.path().join("xdg-open");
    let tools_path = format!(
        "{}:{}",
        temp.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::fs::write(&path, "# Document\n\n[web](https://example.test)").unwrap();
    for (script, expected) in [
        (
            "#!/bin/sh\nprintf 'launcher warning\\n' >&2\nprintf 'opened\\n'\n",
            "Opened in browser: launcher warning",
        ),
        (
            "#!/bin/sh\nprintf 'cannot launch\\n' >&2\nexit 7\n",
            "exit status: 7",
        ),
        (
            "#!/bin/sh\nif IFS= read -r line; then exit 9; fi\nprintf 'stdin closed; \\033[2Jinert warning\\n' >&2\n",
            "Opened in browser: stdin closed;",
        ),
        (
            "#!/bin/sh\ni=0; while [ \"$i\" -lt 9000 ]; do printf x; i=$((i+1)); done\n",
            "output truncated",
        ),
    ] {
        std::fs::write(&launcher, script).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut viewer = Viewer::start_with(&path, 100, 12, false, &[], &[("PATH", &tools_path)]);
        viewer.master.write_all(b"\r").unwrap();
        viewer.read_frame();
        assert!(
            viewer.screen.line(11).contains(expected),
            "{}",
            viewer.screen.line(11)
        );
        assert_eq!(viewer.screen.line(1), "# Document");
        assert_eq!(viewer.screen.scrolls, 0);
        viewer.resize(110, 14);
        assert!(
            viewer.screen.line(13).contains(expected),
            "{}",
            viewer.screen.line(13)
        );
        viewer.quit();
    }
}
