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
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vte::{Params, Parser, Perform};

struct Screen {
    cells: Vec<Vec<String>>,
    reversed: Vec<Vec<bool>>,
    reverse: bool,
    underlined: Vec<Vec<bool>>,
    underline: bool,
    emphasis: Vec<Vec<[bool; 3]>>,
    current_emphasis: [bool; 3],
    x: usize,
    y: usize,
    wrap: bool,
    scrolls: usize,
    last_grapheme: Option<(usize, usize)>,
}

impl Screen {
    fn new(columns: usize, rows: usize) -> Self {
        Self {
            cells: vec![vec![" ".to_owned(); columns]; rows],
            reversed: vec![vec![false; columns]; rows],
            reverse: false,
            underlined: vec![vec![false; columns]; rows],
            underline: false,
            emphasis: vec![vec![[false; 3]; columns]; rows],
            current_emphasis: [false; 3],
            x: 0,
            y: 0,
            wrap: false,
            scrolls: 0,
            last_grapheme: None,
        }
    }

    fn down(&mut self) {
        self.last_grapheme = None;
        if self.y + 1 == self.cells.len() {
            self.cells.rotate_left(1);
            self.cells.last_mut().unwrap().fill(" ".to_owned());
            self.reversed.rotate_left(1);
            self.reversed.last_mut().unwrap().fill(false);
            self.underlined.rotate_left(1);
            self.underlined.last_mut().unwrap().fill(false);
            self.emphasis.rotate_left(1);
            self.emphasis.last_mut().unwrap().fill([false; 3]);
            self.scrolls += 1;
        } else {
            self.y += 1;
        }
    }

    fn line(&self, row: usize) -> String {
        self.cells[row].concat().trim_end().to_string()
    }

    fn content_line(&self, row: usize) -> String {
        self.line(row).trim_end_matches([' ', '░', '█']).to_string()
    }

    fn put_grapheme(&mut self, row: usize, column: usize, text: String) {
        let width = text.width();
        let end = column + width;
        assert!(
            end <= self.cells[row].len(),
            "grapheme exceeds the terminal edge"
        );
        self.cells[row][column] = text;
        for continuation in column + 1..end {
            self.cells[row][continuation].clear();
        }
        for cell in column..end {
            self.reversed[row][cell] = self.reverse;
            self.underlined[row][cell] = self.underline;
            self.emphasis[row][cell] = self.current_emphasis;
        }
        self.wrap = end == self.cells[row].len();
        self.x = end.min(self.cells[row].len() - 1);
        self.y = row;
        self.last_grapheme = Some((row, column));
    }
}

impl Perform for Screen {
    fn print(&mut self, c: char) {
        if let Some((row, column)) = self.last_grapheme {
            let mut joined = self.cells[row][column].clone();
            joined.push(c);
            if joined.graphemes(true).count() == 1 {
                self.put_grapheme(row, column, joined);
                return;
            }
        }
        let text = c.to_string();
        if self.wrap || self.x + text.width() > self.cells[0].len() {
            self.x = 0;
            self.down();
            self.wrap = false;
        }
        self.put_grapheme(self.y, self.x, text);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => {
                self.last_grapheme = None;
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
                self.last_grapheme = None;
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
                self.last_grapheme = None;
                for row in &mut self.cells {
                    row.fill(" ".to_owned());
                }
                for row in &mut self.reversed {
                    row.fill(false);
                }
                for row in &mut self.underlined {
                    row.fill(false);
                }
                for row in &mut self.emphasis {
                    row.fill([false; 3]);
                }
            }
            'm' => {
                let mut values = values.into_iter();
                while let Some(value) = values.next() {
                    match value {
                        0 => {
                            self.reverse = false;
                            self.underline = false;
                            self.current_emphasis = [false; 3];
                        }
                        1 => self.current_emphasis[0] = true,
                        3 => self.current_emphasis[1] = true,
                        9 => self.current_emphasis[2] = true,
                        22 => self.current_emphasis[0] = false,
                        23 => self.current_emphasis[1] = false,
                        29 => self.current_emphasis[2] = false,
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
    input: Option<File>,
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
        Self::start_sources(&[path], columns, rows, numbers, args, environment, false)
    }

    fn start_stream(columns: u16, rows: u16) -> Self {
        Self::start_sources(&[], columns, rows, false, &[], &[], true)
    }

    fn start_sources(
        paths: &[&std::path::Path],
        columns: u16,
        rows: u16,
        numbers: bool,
        args: &[&str],
        environment: &[(&str, &str)],
        stream: bool,
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
        command.args(["--ctty", "--wait"]);
        let pipe = stream.then(|| nix::unistd::pipe2(OFlag::O_CLOEXEC).unwrap());
        if let Some((reader, _)) = &pipe {
            use std::os::fd::AsRawFd;
            command
                .args(["sh", "-c", "exec \"$@\" < \"$BLAD_TEST_STDIN\"", "blad-pty"])
                .env(
                    "BLAD_TEST_STDIN",
                    format!("/proc/{}/fd/{}", std::process::id(), reader.as_raw_fd()),
                );
        }
        command.arg(env!("CARGO_BIN_EXE_blad")).args(paths);
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
            input: pipe
                .as_ref()
                .map(|(_, writer)| File::from(writer.try_clone().unwrap())),
        };
        viewer.read_frame();
        viewer
    }

    fn send_input(&mut self, bytes: &[u8]) {
        self.input.as_mut().unwrap().write_all(bytes).unwrap();
    }

    fn wait_for(&mut self, predicate: impl Fn(&Screen) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate(&self.screen) && Instant::now() < deadline {
            self.read_with_timeout(false, deadline.saturating_duration_since(Instant::now()));
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "viewer exited early"
            );
        }
        assert!(
            predicate(&self.screen),
            "unexpected screen: {:?}",
            (0..self.screen.cells.len())
                .map(|row| self.screen.line(row))
                .collect::<Vec<_>>()
        );
    }

    fn read_frame(&mut self) {
        self.read_update(true);
    }

    fn read_update(&mut self, required: bool) {
        self.read_with_timeout(required, Duration::from_secs(5));
    }

    fn read_with_timeout(&mut self, required: bool, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut received = false;
        let start = self.raw.len();
        let mut complete = false;
        while Instant::now() < deadline {
            let mut descriptors = [PollFd::new(self.master.as_fd(), PollFlags::POLLIN)];
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .clamp(1, 100) as u16;
            let ready = poll(&mut descriptors, wait).unwrap();
            if ready == 0 {
                if complete || (!required && !received) {
                    return;
                }
                if self.child.try_wait().unwrap().is_some() {
                    break;
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
                    complete = self.raw[start..]
                        .windows(b"\x1b[?2026l".len())
                        .any(|bytes| bytes == b"\x1b[?2026l");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("PTY read failed: {error}"),
            }
        }
        assert!(
            !required || complete,
            "viewer did not produce a complete synchronized frame"
        );
    }

    fn keys(&mut self, keys: &[u8]) {
        self.master.write_all(keys).unwrap();
        self.read_update(false);
    }

    fn keys_with_frame(&mut self, keys: &[u8]) {
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
        self.exit_with(b"Q");
    }

    fn exit_with(&mut self, keys: &[u8]) {
        self.exit_with_status(keys, 0);
    }

    fn exit_with_status(&mut self, keys: &[u8], expected: i32) {
        self.master.write_all(keys).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.read_update(false);
                assert_eq!(status.code(), Some(expected), "viewer exited {status}");
                let terminal = nix::sys::termios::tcgetattr(&self.slave).unwrap();
                assert!(terminal.local_flags.contains(
                    nix::sys::termios::LocalFlags::ICANON | nix::sys::termios::LocalFlags::ECHO
                ));
                assert!(self.raw.windows(8).any(|bytes| bytes == b"\x1b[?1049l"));
                return;
            }

            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("viewer did not quit");
    }
}

#[test]
fn frontmatter_search_overflow_resize_and_body_navigation_work_in_the_viewer() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("frontmatter.md");
    let source = format!(
        "---\n# Duplicate\nurl: https://metadata.test\nwide: \x1b[2J{} NEEDLE\n---\n\n# Duplicate\n\n[body](#duplicate)\n\n{}",
        "x".repeat(75),
        "tail paragraph\n\n".repeat(30)
    );
    std::fs::write(&path, format!("\u{feff}{}", source.replace('\n', "\r\n"))).unwrap();
    let mut viewer = Viewer::start(&path, 45, 18, true);
    assert!(viewer.screen.line(2).contains("---"));
    assert!((1..17).any(|row| {
        viewer
            .screen
            .line(row)
            .contains("url: https://metadata.test")
    }));
    viewer.keys(b"/NEEDLE\r");
    let (row, column) = (1..17)
        .find_map(|row| {
            viewer
                .screen
                .line(row)
                .find("NEEDLE")
                .map(|column| (row, column))
        })
        .unwrap();
    assert!(
        viewer.screen.underlined[row][column..column + 6]
            .iter()
            .all(|value| *value)
    );
    viewer.resize(55, 20);
    assert!((1..19).any(|row| viewer.screen.line(row).contains("NEEDLE")));
    viewer.keys(b"\t");
    assert!(viewer.screen.line(19).contains("Links: 1/1"));
    viewer.keys(b"\r");
    assert!(viewer.screen.line(1).contains("# Duplicate"));
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
}

#[test]
fn frontmatter_streaming_completes_split_delimiters_and_matches_finite_viewing() {
    let temp = tempfile::tempdir().unwrap();
    let mut viewer = Viewer::start_stream(90, 24);
    let first = "---\nname: **literal**";
    viewer.send_input(first.as_bytes());
    viewer.wait_for(|screen| (1..23).any(|row| screen.line(row).contains("name: literal")));
    let rest = "\nmetadata:\n  url: https://metadata.test\n--";
    viewer.send_input(rest.as_bytes());
    viewer.wait_for(|screen| (1..23).any(|row| screen.line(row).contains("https://metadata.test")));
    let last = "-\n\n# Body\n\n[body](#body)\n";
    viewer.send_input(last.as_bytes());
    viewer.wait_for(|screen| (1..23).any(|row| screen.line(row).contains("name: **literal**")));
    let tail = "\nTail\n\n".repeat(30);
    viewer.send_input(tail.as_bytes());
    viewer.input.take();
    viewer.wait_for(|screen| screen.line(23).contains("EOF"));
    viewer.keys_with_frame(b"g");
    let path = temp.path().join("complete.md");
    std::fs::write(&path, format!("{first}{rest}{last}{tail}")).unwrap();
    let mut complete = Viewer::start(&path, 90, 24, false);
    for row in 1..23 {
        assert_eq!(
            viewer.screen.content_line(row),
            complete.screen.content_line(row),
            "row {row}"
        );
    }
    viewer.keys(b"\t");
    assert!(viewer.screen.line(23).contains("Links: 1/1"));
    viewer.keys(b"\r");
    assert!(viewer.screen.line(1).starts_with("# Body"));
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
    complete.quit();
}

#[test]
fn live_stdin_starts_without_bytes_and_quits_without_waiting_for_eof() {
    let mut viewer = Viewer::start_stream(90, 14);
    assert!(viewer.screen.line(13).contains("LIVE"));
    viewer.keys_with_frame(b"?");
    assert!(viewer.screen.line(0).contains("help"));
    viewer.exit_with(b"\x03");
}

#[test]
fn live_stdin_renders_unfinished_paragraphs_and_fences_then_keeps_eof_open() {
    let mut viewer = Viewer::start_stream(90, 18);
    viewer.send_input(b"EARLY_WITHOUT_NEWLINE");
    viewer.wait_for(|screen| screen.line(1).contains("EARLY_WITHOUT_NEWLINE"));
    viewer.send_input(b"\n\n```rust\nlet unfinished = 1;");
    viewer.wait_for(|screen| (1..17).any(|row| screen.line(row).contains("let unfinished = 1;")));
    viewer.input.take();
    viewer.wait_for(|screen| screen.line(17).contains("EOF"));
    assert!(viewer.child.try_wait().unwrap().is_none());
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
}

#[test]
fn live_stdin_preserves_split_unicode_controls_and_search_refresh() {
    let mut viewer = Viewer::start_stream(100, 18);
    viewer.send_input(b"hit ");
    viewer.wait_for(|screen| screen.line(1).contains("hit"));
    viewer.keys_with_frame(b"/hit");
    assert!(viewer.screen.line(17).contains("1/1"));
    viewer.send_input(&[0xe7, 0x95]);
    viewer.read_update(false);
    assert!(!viewer.screen.line(17).contains("error"));
    viewer.send_input(&[0x8c]);
    viewer.send_input(b" hit \x1b");
    viewer.send_input(b"[2J INERT");
    viewer.wait_for(|screen| screen.line(17).contains("1/2"));
    assert!(viewer.screen.line(1).contains("界"));
    assert!(viewer.screen.line(1).contains("INERT"));
    assert!(viewer.screen.line(0).contains("Blad"));
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.keys(b"\r");
    viewer.input.take();
    viewer.wait_for(|screen| screen.line(17).contains("EOF") && screen.line(17).contains("1/2"));
    viewer.quit();
}

#[test]
fn live_stdin_follow_pause_resume_and_resize_retain_their_state() {
    let mut viewer = Viewer::start_stream(70, 12);
    for batch in 0..3 {
        let source: String = (batch * 20..batch * 20 + 20)
            .map(|index| format!("row {index:03}\n\n"))
            .collect();
        viewer.send_input(source.as_bytes());
        viewer.wait_for(|screen| {
            (1..11).any(|row| {
                screen
                    .line(row)
                    .contains(&format!("row {:03}", batch * 20 + 19))
            })
        });
    }
    viewer.keys_with_frame(b"g");
    assert!(viewer.screen.line(1).starts_with("row 000"));
    assert!(viewer.screen.line(11).contains("PAUSED"));
    viewer.send_input(b"row 060\n\n");
    viewer.wait_for(|screen| screen.line(11).contains("of 121"));
    assert!(viewer.screen.line(1).starts_with("row 000"));
    viewer.resize(55, 14);
    assert!(viewer.screen.line(1).starts_with("row 000"));
    assert!(viewer.screen.line(13).contains("PAUSED"));
    viewer.keys_with_frame(b"\x1b[1;5F");
    assert!(viewer.screen.line(13).contains("FOLLOW"));
    viewer.send_input(b"row 061\n\n");
    viewer.wait_for(|screen| (1..13).any(|row| screen.line(row).contains("row 061")));
    viewer.resize(80, 10);
    assert!((1..9).any(|row| viewer.screen.line(row).contains("row 061")));
    assert!(viewer.screen.line(9).contains("FOLLOW"));
    viewer.quit();
}

#[test]
fn live_stdin_revises_late_references_and_matches_one_shot_rendering_at_eof() {
    let temp = tempfile::tempdir().unwrap();
    let mut viewer = Viewer::start_stream(90, 35);
    let first = "# Heading\n\n[jump][target]\n\nUnicode 界 e\u{301}\n\n";
    viewer.send_input(first.as_bytes());
    viewer.wait_for(|screen| screen.line(3).contains("[jump][target]"));
    let rest = "[target]: #heading\n\n| A | B |\n| - | - |\n| one | two |\n\nText $x^2$ with **bold**.\n\n```rust\nlet value = 1;";
    for chunk in rest.as_bytes().chunks(3) {
        viewer.send_input(chunk);
    }
    viewer.input.take();
    viewer.wait_for(|screen| screen.line(34).contains("EOF"));
    assert!(viewer.screen.line(3).starts_with("jump"));
    let path = temp.path().join("complete.md");
    std::fs::write(&path, format!("{first}{rest}")).unwrap();
    let mut complete = Viewer::start(&path, 90, 35, false);
    for row in 1..34 {
        assert_eq!(
            viewer.screen.content_line(row),
            complete.screen.content_line(row),
            "row {row}"
        );
    }
    viewer.keys_with_frame(b"\t");
    assert!(viewer.screen.line(34).contains("Links:") || viewer.screen.line(0).contains("Links:"));
    viewer.keys_with_frame(b"\r");
    assert!(viewer.screen.line(1).starts_with("# Heading"));
    viewer.quit();
    complete.quit();
}

#[test]
fn live_stdin_ingests_inactive_tabs_and_deduplicates_then_stops_when_closed() {
    let temp = tempfile::tempdir().unwrap();
    let named = temp.path().join("named.md");
    std::fs::write(&named, "# Named").unwrap();
    let mut viewer = Viewer::start_sources(
        &[&named, std::path::Path::new("-"), std::path::Path::new("-")],
        90,
        14,
        false,
        &[],
        &[],
        true,
    );
    viewer.send_input(b"INACTIVE_INPUT");
    viewer.read_update(false);
    viewer.keys_with_frame(b"\x1b[6;5~");
    viewer.wait_for(|screen| screen.line(1).contains("INACTIVE_INPUT"));
    viewer.keys_with_frame(b"q");
    assert!(viewer.screen.line(1).contains("Named"));
    assert_eq!(
        viewer
            .input
            .as_mut()
            .unwrap()
            .write(b"closed")
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::BrokenPipe,
    );
    viewer.exit_with(b"q");
}

#[test]
fn live_stdin_editor_reads_keyboard_without_consuming_the_document_pipe() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let named = temp.path().join("named.md");
    std::fs::write(&named, "# Named").unwrap();
    let editor = temp.path().join("editor");
    std::fs::write(&editor, "#!/bin/sh\nprintf 'EDITOR_READY\\n'\nIFS= read -r line || exit 1\nprintf 'EDITOR_INPUT:%s\\n' \"$line\"\n").unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut viewer = Viewer::start_sources(
        &[&named, std::path::Path::new("-")],
        100,
        16,
        false,
        &[],
        &[("EDITOR", editor.to_str().unwrap())],
        true,
    );
    viewer.master.write_all(b"e").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !viewer.raw.windows(12).any(|bytes| bytes == b"EDITOR_READY") && Instant::now() < deadline
    {
        viewer.read_with_timeout(false, Duration::from_millis(50));
    }
    assert!(viewer.raw.windows(12).any(|bytes| bytes == b"EDITOR_READY"));
    viewer.send_input(b"STREAM_MUST_SURVIVE\n");
    std::thread::sleep(Duration::from_millis(50));
    viewer.master.write_all(b"EDITOR_KEYBOARD\n").unwrap();
    viewer.read_frame();
    assert!(
        viewer
            .raw
            .windows(b"EDITOR_INPUT:EDITOR_KEYBOARD".len())
            .any(|bytes| bytes == b"EDITOR_INPUT:EDITOR_KEYBOARD")
    );
    viewer.keys_with_frame(b"\x1b[6;5~");
    viewer.wait_for(|screen| screen.line(1).contains("STREAM_MUST_SURVIVE"));
    viewer.quit();
}

#[test]
fn live_stdin_errors_remain_visible_and_fail_after_terminal_restoration() {
    for bytes in [b"valid \xff".as_slice(), b"valid \xe7\x95".as_slice()] {
        let mut viewer = Viewer::start_stream(100, 14);
        viewer.send_input(b"LAST_VALID");
        viewer.wait_for(|screen| screen.line(1).contains("LAST_VALID"));
        viewer.send_input(bytes);
        viewer.input.take();
        viewer.wait_for(|screen| screen.line(13).contains("stdin error"));
        viewer.keys(b"g");
        assert!(viewer.screen.line(13).contains("stdin error"));
        assert!(viewer.screen.line(1).contains("LAST_VALID"));
        viewer.exit_with_status(b"q", 1);
    }
}
#[test]
fn live_stdin_continuous_input_updates_without_idle_debounce_and_coalesces_frames() {
    let mut viewer = Viewer::start_stream(80, 12);
    viewer.raw.clear();
    for index in 0..80 {
        viewer.send_input(format!("tick {index:03}\n\n").as_bytes());
        viewer.read_with_timeout(false, Duration::from_millis(10));
        if index == 40 {
            assert!((1..11).any(|row| viewer.screen.line(row).contains("tick 0")));
            assert!(viewer.screen.line(11).contains("LIVE"));
        }
    }
    viewer.wait_for(|screen| (1..11).any(|row| screen.line(row).contains("tick 079")));
    let frames = viewer
        .raw
        .windows(8)
        .filter(|bytes| *bytes == b"\x1b[?2026l")
        .count();
    assert!(
        (2..80).contains(&frames),
        "expected coalescing, got {frames} frames"
    );
    viewer.quit();
}

#[test]
fn live_stdin_empty_eof_and_color_opt_outs_still_leave_a_pager() {
    for (args, environment) in [
        (vec![], vec![]),
        (vec!["--color", "never"], vec![]),
        (vec![], vec![("NO_COLOR", "1")]),
        (vec![], vec![("TERM", "dumb")]),
    ] {
        let mut viewer = Viewer::start_sources(&[], 80, 12, false, &args, &environment, true);
        viewer.input.take();
        viewer.wait_for(|screen| screen.line(11).contains("EOF"));
        assert!(viewer.screen.content_line(1).is_empty());
        viewer.quit();
    }
}

#[test]
fn live_stdin_updates_and_errors_remain_observable_while_help_is_open() {
    for fail in [false, true] {
        let mut viewer = Viewer::start_stream(100, 20);
        viewer.keys_with_frame(b"?");
        viewer.send_input(b"ARRIVED_DURING_HELP");
        if fail {
            viewer.send_input(b"\xff");
        }
        viewer.input.take();
        viewer.wait_for(|screen| {
            screen
                .line(19)
                .contains(if fail { "stdin error" } else { "EOF" })
        });
        assert!(viewer.screen.line(0).contains("help"));
        viewer.keys_with_frame(b"?");
        assert!(viewer.screen.line(1).contains("ARRIVED_DURING_HELP"));
        viewer.exit_with_status(b"q", i32::from(fail));
    }
}

#[test]
fn live_stdin_follow_pin_survives_tab_switches_in_both_states() {
    let temp = tempfile::tempdir().unwrap();
    let named = temp.path().join("named.md");
    std::fs::write(&named, "# Other").unwrap();
    let mut viewer = Viewer::start_sources(
        &[std::path::Path::new("-"), &named],
        80,
        12,
        false,
        &[],
        &[],
        true,
    );
    let source: String = (0..30).map(|index| format!("row {index:03}\n\n")).collect();
    viewer.send_input(source.as_bytes());
    viewer.wait_for(|screen| (1..11).any(|row| screen.line(row).contains("row 029")));
    viewer.keys_with_frame(b"\x1b[6;5~");
    viewer.send_input(b"row 030\n\n");
    viewer.read_update(false);
    viewer.keys_with_frame(b"\x1b[5;5~");
    viewer.wait_for(|screen| (1..11).any(|row| screen.line(row).contains("row 030")));
    assert!(viewer.screen.line(11).contains("FOLLOW"));
    viewer.keys_with_frame(b"g");
    viewer.keys_with_frame(b"\x1b[6;5~");
    viewer.send_input(b"row 031\n\n");
    viewer.read_update(false);
    viewer.keys_with_frame(b"\x1b[5;5~");
    assert!(viewer.screen.line(1).starts_with("row 000"));
    assert!(viewer.screen.line(11).contains("PAUSED"));
    viewer.quit();
}

#[test]
fn live_stdin_limit_is_inclusive_and_overflow_fails_without_losing_the_viewer() {
    const LIMIT: usize = 16 * 1024 * 1024;
    for overflow in [false, true] {
        let mut viewer = Viewer::start_stream(100, 12);
        let mut producer = viewer.input.take().unwrap();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                producer.write_all(b"VALID_PREFIX\n\n").unwrap();
                let count = LIMIT - b"VALID_PREFIX\n\n".len() + usize::from(overflow);
                let bytes = [b' '; 64 * 1024];
                for _ in 0..count / bytes.len() {
                    producer.write_all(&bytes).unwrap();
                }
                producer.write_all(&bytes[..count % bytes.len()]).unwrap();
            });
            viewer.wait_for(|screen| {
                screen
                    .line(11)
                    .contains(if overflow { "stdin error" } else { "EOF" })
            });
        });
        assert!(viewer.screen.line(1).contains("VALID_PREFIX"));
        if overflow {
            assert!(viewer.screen.line(11).contains("16777216"));
        }
        viewer.exit_with_status(b"q", i32::from(overflow));
    }
}

#[test]
fn live_stdin_failure_is_retained_across_closing_the_stdin_tab() {
    let temp = tempfile::tempdir().unwrap();
    let named = temp.path().join("named.md");
    std::fs::write(&named, "# Named").unwrap();
    let mut viewer = Viewer::start_sources(
        &[&named, std::path::Path::new("-")],
        100,
        12,
        false,
        &[],
        &[],
        true,
    );
    viewer.send_input(b"valid\xff");
    viewer.wait_for(|screen| screen.line(11).contains("stdin error"));
    viewer.keys_with_frame(b"\x1b[6;5~");
    viewer.keys_with_frame(b"q");
    assert!(viewer.screen.line(1).contains("Named"));
    assert!(viewer.screen.line(11).contains("stdin error"));
    viewer.exit_with_status(b"Q", 1);
}

#[test]
fn live_stdin_quit_while_a_fast_producer_is_active_never_drains_the_pipe() {
    let mut viewer = Viewer::start_stream(80, 12);
    let mut producer = viewer.input.take().unwrap();
    fcntl(&producer, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                match producer.write(b"busy producer\n\n") {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => return,
                    Err(error) => panic!("producer failed: {error}"),
                }
            }
            panic!("viewer did not close its read end");
        });
        viewer.wait_for(|screen| (1..11).any(|row| screen.line(row).contains("busy producer")));
        viewer.exit_with(b"\x03");
    });
}

#[test]
#[ignore = "explicit sustained-input performance characterization, not a latency guarantee"]
fn live_stdin_sustained_input_characterization() {
    let mut viewer = Viewer::start_stream(100, 30);
    viewer.raw.clear();
    let source: String = (0..200).map(|index| format!(
        "# Section {index:04}\n\nReadable **Markdown** with 界, $x^2$ and [a reference][ref].\n\n\
         - item one\n- item two\n\n```rust\nlet value = {index};\n```\n\n\
         | Name | Value |\n| ---- | ----- |\n| width | {index} |\n\n"
    )).collect();
    let started = Instant::now();
    viewer.send_input(b"# Early\n\n");
    viewer.wait_for(|screen| screen.line(1).contains("Early"));
    let first = started.elapsed();
    for bytes in source.as_bytes().chunks(1024) {
        viewer.send_input(bytes);
        viewer.read_with_timeout(false, Duration::from_millis(10));
    }
    viewer.send_input(b"\n[ref]: #early\n\nFINAL_SENTINEL");
    viewer.input.take();
    viewer.wait_for(|screen| {
        screen.line(29).contains("EOF")
            && (1..29).any(|row| screen.line(row).contains("FINAL_SENTINEL"))
    });
    let elapsed = started.elapsed();
    let frames = viewer
        .raw
        .windows(8)
        .filter(|bytes| *bytes == b"\x1b[?2026l")
        .count();
    let memory = std::fs::read_to_string(format!("/proc/{}/status", viewer.child.id())).unwrap();
    let peak = memory
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .unwrap();
    let quit = Instant::now();
    viewer.quit();
    eprintln!(
        "live stdin: source={} bytes; first={first:?}; total={elapsed:?}; frames={frames}; \
         terminal={} bytes; quit={:?}; {peak}",
        source.len() + b"# Early\n\n".len() + b"\n[ref]: #early\n\nFINAL_SENTINEL".len(),
        viewer.raw.len(),
        quit.elapsed(),
    );
}

#[test]
fn redraw_actions_wait_for_delayed_input_processing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("delayed.md");
    std::fs::write(&path, "hit").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    let pid = Pid::from_raw(viewer.child.id().try_into().unwrap());
    kill(pid, Signal::SIGSTOP).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            kill(pid, Signal::SIGCONT).unwrap();
        });
        viewer.keys_with_frame(b"/hit");
    });
    assert!(
        viewer.screen.line(11).contains("[literal] 1/1"),
        "{}",
        viewer.screen.line(11)
    );
    viewer.exit_with(b"\x03");
}

#[test]
fn toggling_regex_recomputes_an_existing_query_without_an_extra_keystroke() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("toggle.md");
    std::fs::write(&path, "hit[1] hit1 hit1").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    viewer.keys(b"/hit[1]");
    assert!(viewer.screen.line(11).contains("[literal] 1/1"));
    viewer.keys(b"\x12");
    assert!(
        viewer.screen.line(11).contains("[regex] 1/2"),
        "{}",
        viewer.screen.line(11)
    );
    viewer.keys(b"\x12");
    assert!(viewer.screen.line(11).contains("[literal] 1/1"));
    viewer.keys(b"\r");
    viewer.quit();
}

#[test]
fn question_mark_opens_help_and_reverse_search_uses_only_n_or_angle_bracket() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("keys.md");
    std::fs::write(&path, "hit one\n\nhit two\n\nhit three").unwrap();
    let mut viewer = Viewer::start(&path, 100, 30, false);
    viewer.keys_with_frame(b"?");
    assert!(viewer.screen.line(0).contains("help"));
    let help = (1..29)
        .map(|row| viewer.screen.line(row))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(help.contains("? / F1 / h"));
    assert!(!help.contains("backward"));
    assert!(!help.contains("N/p"));
    assert!(!help.contains("F5"));
    viewer.keys_with_frame(b"?");
    viewer.keys_with_frame(b"/hit\r");
    assert!(
        viewer
            .screen
            .line(29)
            .starts_with("SEARCH [literal] 1/3 Esc/q:exit n/N:move Enter:next"),
        "{}",
        viewer.screen.line(29)
    );
    viewer.keys_with_frame(b"n");
    assert!(viewer.screen.line(29).contains("2/3"));
    viewer.raw.clear();
    viewer.keys(b"p");
    assert!(viewer.raw.is_empty());
    assert!(viewer.screen.line(29).contains("2/3"));
    viewer.keys_with_frame(b"<");
    assert!(viewer.screen.line(29).contains("1/3"));
    viewer.keys_with_frame(b"N");
    assert!(viewer.screen.line(29).contains("3/3"));
    viewer.keys_with_frame(b">");
    assert!(viewer.screen.line(29).contains("1/3"));
    viewer.keys_with_frame(b"\x1bOP");
    assert!(viewer.screen.line(0).contains("help"));
    viewer.keys(b"Q");
    assert!(viewer.child.try_wait().unwrap().is_none());
    viewer.exit_with(b"\x03");
}

#[test]
fn f5_is_inert_and_search_still_treats_question_mark_as_text() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("keys.md");
    std::fs::write(&path, "hit? hit1").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    viewer.raw.clear();
    viewer.keys(b"\x1b[15~");
    assert!(viewer.raw.is_empty(), "retired reload key produced output");
    assert!(viewer.child.try_wait().unwrap().is_none());
    viewer.keys_with_frame(b"/hit?");
    assert!(
        viewer
            .screen
            .line(11)
            .starts_with("FIND [literal] 1/1 Esc:cancel Enter:search")
    );
    viewer.keys_with_frame(b"\x12");
    assert!(
        viewer
            .screen
            .line(11)
            .starts_with("FIND [regex] 1/2 Esc:cancel Enter:search")
    );
    viewer.exit_with(b"\x03");
}

#[test]
fn ctrl_r_starts_a_fresh_regex_search_and_toggles_in_the_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("regex.md");
    std::fs::write(&path, "hit? hit1\n\n[link](#target)\n\n# Target").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    viewer.keys_with_frame(b"\t");
    assert!(viewer.screen.line(11).contains("Links: 1/1"));
    viewer.keys_with_frame(b"\x12hit?");
    assert!(
        viewer
            .screen
            .line(11)
            .starts_with("FIND [regex] 1/2 Esc:cancel Enter:search")
    );
    viewer.keys_with_frame(b"\x12");
    assert!(
        viewer
            .screen
            .line(11)
            .starts_with("FIND [literal] 1/1 Esc:cancel Enter:search")
    );
    viewer.keys(b"\r");
    viewer.keys_with_frame(b"\x12hit[0-9]");
    assert!(
        viewer
            .screen
            .line(11)
            .starts_with("FIND [regex] 1/1 Esc:cancel Enter:search")
    );
    viewer.keys(b"\r");
    viewer.keys_with_frame(b"/hit?");
    assert!(
        viewer
            .screen
            .line(11)
            .starts_with("FIND [literal] 1/1 Esc:cancel Enter:search")
    );
    viewer.keys_with_frame(b"\x1b");
    assert!(!viewer.screen.line(11).contains("Links:"));
    viewer.quit();
}

#[test]
fn search_feedback_distinguishes_current_matches_even_in_reversed_source() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("current.md");
    std::fs::write(
        &path,
        "before **hit** gap `hit` after\n\n| hit | hit |\n|---|---|\n| hit | hit |\n\n界 hit e\u{301} hit",
    )
    .unwrap();
    for args in [
        vec!["--color", "never"],
        vec!["--theme", "dark"],
        vec!["--theme", "light"],
        vec!["--theme", "terminal"],
    ] {
        let mut viewer = Viewer::start_with(&path, 100, 20, false, &args, &[]);
        viewer.keys_with_frame(b"/hit");
        assert!(viewer.screen.reversed[1][7..10].iter().all(|value| *value));
        assert!(viewer.screen.reversed[1][15..18].iter().all(|value| !value));
        for range in [7..10, 15..18] {
            assert!(
                viewer.screen.underlined[1][range]
                    .iter()
                    .all(|value| *value)
            );
        }
        viewer.keys_with_frame(b"\rn");
        assert!(viewer.screen.reversed[1][7..10].iter().all(|value| !value));
        assert!(viewer.screen.reversed[1][15..18].iter().all(|value| *value));
        assert!(
            viewer.screen.emphasis[1][7..10]
                .iter()
                .all(|value| value[0])
        );
        viewer.keys_with_frame(b"n");
        let row = (1..19)
            .find(|row| viewer.screen.content_line(*row) == "hit | hit")
            .unwrap();
        assert!(viewer.screen.reversed[row][0..3].iter().all(|value| *value));
        assert!(viewer.screen.reversed[row][6..9].iter().all(|value| !value));
        assert!(viewer.screen.reversed[row][3..6].iter().all(|value| *value));
        viewer.keys_with_frame(b"n");
        assert!(viewer.screen.reversed[row][0..3].iter().all(|value| !value));
        assert!(viewer.screen.reversed[row][6..9].iter().all(|value| *value));
        viewer.resize(60, 20);
        assert!(viewer.screen.reversed[row][0..3].iter().all(|value| !value));
        assert!(viewer.screen.reversed[row][6..9].iter().all(|value| *value));
        viewer.keys_with_frame(b"nnnn");
        assert!(viewer.screen.line(19).contains("8/8"));
        viewer.keys_with_frame(b"n");
        assert!(viewer.screen.line(19).contains("1/8"));
        assert!(viewer.screen.reversed[1][7..10].iter().all(|value| *value));
        assert!(viewer.screen.reversed[1][15..18].iter().all(|value| !value));
        viewer.keys_with_frame(b"\x1b");
        assert!(viewer.screen.reversed[row][0..9].iter().all(|value| *value));
        assert!(
            viewer.screen.underlined[row][0..9]
                .iter()
                .all(|value| !value)
        );
        viewer.quit();
    }
}

#[test]
fn search_feedback_enter_and_safe_exit_preserve_tabs_until_normal_quit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("search-mode.md");
    let other = temp.path().join("other.md");
    std::fs::write(&path, "hit hit\n\n[link](#target)\n\n# Target").unwrap();
    std::fs::write(&other, "other document").unwrap();
    for exit in [b"q".as_slice(), b"\x1b"] {
        let mut viewer = Viewer::start_sources(&[&path, &other], 100, 18, false, &[], &[], false);
        viewer.keys_with_frame(b"/hit");
        assert!(viewer.screen.line(17).starts_with("FIND "));
        assert!(viewer.screen.line(17).contains("Enter:search"));
        assert!(viewer.screen.line(17).contains("Esc:cancel"));
        viewer.keys_with_frame(b"\r");
        assert!(viewer.screen.line(17).starts_with("SEARCH "));
        assert!(viewer.screen.line(17).contains("Esc/q:exit"));
        assert!(viewer.screen.line(17).contains("n/N"));
        viewer.keys_with_frame(b"\r");
        assert!(
            viewer.screen.line(17).contains("2/2"),
            "{}",
            viewer.screen.line(17)
        );
        viewer.keys_with_frame(exit);
        assert!(viewer.child.try_wait().unwrap().is_none());
        assert!(viewer.screen.line(0).contains("[search-mode.md]"));
        assert!(!viewer.screen.line(17).contains("SEARCH"));
        assert!(viewer.screen.underlined[1].iter().all(|value| !value));
        viewer.keys_with_frame(b"q");
        assert!(viewer.screen.line(0).contains("[other.md]"));
        viewer.quit();
    }
}

#[test]
fn search_feedback_handles_empty_errors_literal_q_and_help_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("search-errors.md");
    std::fs::write(&path, "q q hit").unwrap();
    let mut viewer = Viewer::start(&path, 100, 18, false);
    viewer.keys_with_frame(b"/q");
    assert!(viewer.screen.line(17).contains("1/2"));
    viewer.keys_with_frame(b"\r");
    viewer.keys_with_frame(b"?");
    assert!(viewer.screen.line(0).contains("help"));
    viewer.keys_with_frame(b"q");
    assert!(viewer.screen.line(17).starts_with("SEARCH "));
    viewer.keys_with_frame(b"q");
    assert!(!viewer.screen.line(17).contains("SEARCH"));
    for (query, diagnostic) in [
        (b"/absent\r".as_slice(), "No matches"),
        (b"/\x12[\r".as_slice(), "Invalid regex"),
    ] {
        viewer.keys_with_frame(query);
        assert!(viewer.screen.line(17).starts_with("SEARCH "));
        assert!(viewer.screen.line(17).contains(diagnostic));
        viewer.keys(b"\r");
        assert!(viewer.screen.line(17).contains(diagnostic));
        viewer.keys_with_frame(b"\x1b");
        assert!(viewer.child.try_wait().unwrap().is_none());
        assert!(!viewer.screen.line(17).contains("SEARCH"));
    }
    viewer.keys_with_frame(b"/\r");
    assert!(!viewer.screen.line(17).contains("FIND"));
    assert!(!viewer.screen.line(17).contains("SEARCH"));
    viewer.exit_with(b"q");
    let mut viewer = Viewer::start(&path, 100, 18, false);
    viewer.keys_with_frame(b"/hit\r");
    viewer.exit_with(b"\x11");
}

#[test]
fn search_feedback_enter_pauses_live_follow_and_reveals_the_next_match() {
    let mut viewer = Viewer::start_stream(100, 12);
    viewer.send_input(format!("hit top\n\n{}hit bottom", "filler\n\n".repeat(30)).as_bytes());
    viewer.wait_for(|screen| (1..11).any(|row| screen.line(row).contains("hit bottom")));
    viewer.keys_with_frame(b"/hit\r");
    assert!(viewer.screen.line(11).contains("2/2"));
    viewer.keys_with_frame(b"G");
    assert!(viewer.screen.line(11).contains("FOLLOW"));
    viewer.keys_with_frame(b"\r");
    assert!(viewer.screen.line(11).contains("PAUSED"));
    assert!(viewer.screen.line(11).contains("1/2"));
    assert!(viewer.screen.line(1).contains("hit top"));
    viewer.quit();
}

#[test]
fn search_feedback_resize_keeps_a_visible_current_match_in_view() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("search-reflow.md");
    std::fs::write(&path, format!("{}hit", "word ".repeat(50))).unwrap();
    let mut viewer = Viewer::start(&path, 100, 18, false);
    viewer.keys_with_frame(b"/hit\r");
    assert!((1..17).any(|row| viewer.screen.line(row).contains("hit")));
    viewer.resize(25, 10);
    assert!((1..9).any(|row| viewer.screen.line(row).contains("hit")));
    assert!(viewer.screen.line(9).contains("1/1"));
    viewer.keys_with_frame(b"g");
    viewer.resize(20, 10);
    assert!(viewer.screen.line(1).starts_with("word"));
    assert!(!(1..9).any(|row| viewer.screen.line(row).contains("hit")));
    viewer.quit();
}

#[test]
fn search_feedback_remains_current_after_live_input_and_file_reload() {
    let mut viewer = Viewer::start_stream(100, 18);
    viewer.send_input(b"hit hit");
    viewer.wait_for(|screen| screen.line(1).contains("hit hit"));
    viewer.keys_with_frame(b"/hit\rn");
    assert!(viewer.screen.line(17).contains("2/2"));
    assert!(viewer.screen.reversed[1][0..3].iter().all(|value| !value));
    assert!(viewer.screen.reversed[1][4..7].iter().all(|value| *value));
    viewer.send_input(b" hit");
    viewer.wait_for(|screen| screen.line(17).contains("1/3"));
    assert!(viewer.screen.line(17).contains("SEARCH"));
    assert!(viewer.screen.reversed[1][0..3].iter().all(|value| *value));
    assert!(viewer.screen.reversed[1][4..11].iter().all(|value| !value));
    viewer.keys_with_frame(b"q");
    assert!(viewer.screen.line(17).contains("PAUSED"));
    assert!(!viewer.screen.line(17).contains("SEARCH"));
    viewer.quit();

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("search-reload.md");
    std::fs::write(&path, "hit hit").unwrap();
    let mut viewer = Viewer::start(&path, 100, 18, true);
    viewer.keys_with_frame(b"/hit\rn");
    std::fs::write(&path, "hit hit hit").unwrap();
    viewer.wait_for(|screen| screen.line(17).contains("1/3"));
    assert!(viewer.screen.line(17).contains("SEARCH"));
    assert!(viewer.screen.reversed[1][2..5].iter().all(|value| *value));
    assert!(viewer.screen.reversed[1][6..13].iter().all(|value| !value));
    viewer.quit();
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
            (7..10).contains(&column),
            "column {column}"
        );
        assert_eq!(
            viewer.screen.underlined[1][column],
            (7..10).contains(&column) || (15..18).contains(&column),
            "column {column}"
        );
    }
    viewer.keys(b"N");
    assert!(viewer.screen.line(11).contains("5/5"));
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("1/5"));
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
fn file_links_reject_special_and_oversized_inputs_without_adding_tabs() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    let target = temp.path().join("target.md");
    let recovered = temp.path().join("recovered.md");
    std::fs::write(&source, "# Source\n\n[open](target.md)").unwrap();
    std::fs::write(&recovered, "# Recovered").unwrap();
    for kind in ["fifo", "directory", "oversized"] {
        match kind {
            "fifo" => nix::unistd::mkfifo(&target, nix::sys::stat::Mode::S_IRUSR).unwrap(),
            "directory" => std::fs::create_dir(&target).unwrap(),
            _ => {
                std::fs::write(&target, [0xff]).unwrap();
                File::options()
                    .write(true)
                    .open(&target)
                    .unwrap()
                    .set_len(16 * 1024 * 1024 + 1)
                    .unwrap();
            }
        }
        let mut viewer = Viewer::start(&source, 100, 12, false);
        viewer.keys(b"\t");
        viewer.master.write_all(b"\r").unwrap();
        viewer.read_frame();
        let footer = viewer.screen.line(11);
        assert!(footer.contains("Error opening file"), "{footer}");
        assert!(
            footer.contains(if kind == "oversized" {
                "16777216"
            } else {
                "requires a regular file"
            }),
            "{footer}"
        );
        assert_eq!(viewer.screen.content_line(1), "# Source");
        viewer.keys(b"\x1b[1;3Cb");
        assert_eq!(viewer.screen.content_line(1), "# Source");
        assert!(viewer.screen.line(0).contains("[source.md]"));
        if kind == "directory" {
            std::fs::remove_dir(&target).unwrap();
        } else {
            std::fs::remove_file(&target).unwrap();
        }
        std::os::unix::fs::symlink(&recovered, &target).unwrap();
        viewer.keys(b"\t\r");
        assert_eq!(viewer.screen.content_line(1), "# Recovered");
        viewer.keys(b"b");
        assert_eq!(viewer.screen.content_line(1), "# Source");
        viewer.quit();
        std::fs::remove_file(&target).unwrap();
    }
}

#[test]
fn oversized_reload_retains_search_position_and_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bounded.md");
    std::fs::write(
        &path,
        format!(
            "# Original\n\n{}needle needle",
            "retained content\n\n".repeat(30)
        ),
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 100, 12, false);
    viewer.keys(b"/needle\r");
    assert!(viewer.screen.line(11).contains("1/2"));
    let before: Vec<_> = (1..11).map(|row| viewer.screen.content_line(row)).collect();
    std::fs::write(&path, [0xff]).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    viewer.raw.clear();
    viewer.read_frame();
    assert!(viewer.screen.line(11).contains("16777216"));
    assert_eq!(
        (1..11)
            .map(|row| viewer.screen.content_line(row))
            .collect::<Vec<_>>(),
        before
    );
    assert!(!String::from_utf8_lossy(&viewer.raw).contains("\x1b[?1049"));
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("2/2"));
    std::fs::write(&path, "# Recovered needle").unwrap();
    viewer.read_frame();
    assert_eq!(viewer.screen.content_line(1), "# Recovered needle");
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("1/1"));
    viewer.quit();
}

#[test]
fn editor_rereads_reject_special_oversized_and_substituted_symlink_inputs() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("edited.md");
    let replacement = temp.path().join("edited.md.replacement");
    let other = temp.path().join("other.md");
    let editor = temp.path().join("replace-editor");
    std::fs::write(&other, "# Not the edited file").unwrap();
    std::fs::write(&editor, "#!/bin/sh\nmv -- \"$1.replacement\" \"$1\"\n").unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    for kind in ["fifo", "oversized", "symlink"] {
        std::fs::write(&path, "# Original").unwrap();
        match kind {
            "fifo" => nix::unistd::mkfifo(&replacement, nix::sys::stat::Mode::S_IRUSR).unwrap(),
            "symlink" => std::os::unix::fs::symlink(&other, &replacement).unwrap(),
            _ => {
                std::fs::write(&replacement, [0xff]).unwrap();
                File::options()
                    .write(true)
                    .open(&replacement)
                    .unwrap()
                    .set_len(16 * 1024 * 1024 + 1)
                    .unwrap();
            }
        }
        let mut viewer = Viewer::start_with(
            &path,
            100,
            12,
            false,
            &[],
            &[("EDITOR", editor.to_str().unwrap())],
        );
        viewer.keys(b"e");
        viewer.wait_failure();
        let output = String::from_utf8_lossy(&viewer.raw);
        assert!(output.contains("Failed to display markdown"), "{output:?}");
        if kind == "oversized" {
            assert!(output.contains("16777216"), "{output:?}");
        }
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn automatic_reload_preserves_offsets_search_and_terminal_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("watch.md");
    let source = |word: &str| {
        format!(
            "```text\n{}\n```",
            format!("{} {word} needle\n", "x".repeat(90)).repeat(40)
        )
    };
    std::fs::write(&path, source("before")).unwrap();
    let mut viewer = Viewer::start(&path, 60, 12, false);
    viewer.keys(b"ddd");
    viewer.keys(&b"\x1b[C".repeat(80));
    viewer.keys(b"/needle\r");
    let search_footer = viewer.screen.line(11);
    assert!(search_footer.contains("/40"), "{search_footer}");
    let before: Vec<_> = (1..11).map(|row| viewer.screen.content_line(row)).collect();
    viewer.raw.clear();
    std::fs::write(&path, source("after!")).unwrap();
    viewer.read_frame();
    let after: Vec<_> = (1..11).map(|row| viewer.screen.content_line(row)).collect();
    for (old, new) in before.iter().zip(&after) {
        assert_eq!(old.replace("before", "after!"), *new);
    }
    assert_eq!(viewer.screen.line(11), search_footer);
    let raw = String::from_utf8_lossy(&viewer.raw);
    assert!(
        !raw.contains("\x1b[?1049") && !raw.contains("\x1b[2J"),
        "{raw:?}"
    );
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("/40"));
    assert_ne!(viewer.screen.line(11), search_footer);
    viewer.quit();
}

#[test]
fn automatic_reload_handles_atomic_saves_inactive_tabs_and_shrink() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("first.md");
    let other = temp.path().join("second.md");
    let replacement = temp.path().join("save.md");
    std::fs::write(&path, "# First\n\n".repeat(40)).unwrap();
    std::fs::write(&other, "# Second").unwrap();
    let mut viewer = Viewer::start_with(&path, 70, 12, false, &[other.to_str().unwrap()], &[]);
    viewer.keys(b"G");
    viewer.keys(b"\x1b[1;3C");
    std::fs::write(&replacement, "# Atomic replacement").unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    viewer.keys(b"\x1b[1;3D");
    viewer.read_frame();
    assert_eq!(viewer.screen.content_line(1), "# Atomic replacement");
    assert!(
        viewer.screen.line(0).find("first.md").unwrap()
            < viewer.screen.line(0).find("second.md").unwrap()
    );
    std::fs::write(&other, "# Updated inactive tab").unwrap();
    viewer.keys(b"\x1b[1;3C");
    viewer.read_frame();
    assert_eq!(viewer.screen.content_line(1), "# Updated inactive tab");
    viewer.quit();
}

#[test]
fn automatic_reload_retains_usable_content_on_failure_and_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recover.md");
    std::fs::write(&path, "# Original").unwrap();
    let mut viewer = Viewer::start(&path, 90, 12, false);
    std::fs::remove_file(&path).unwrap();
    nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap();
    viewer.read_frame();
    assert_eq!(viewer.screen.content_line(1), "# Original");
    assert!(viewer.screen.line(11).contains("Error reloading"));
    viewer.keys(b"h");
    assert!(viewer.screen.line(0).contains("help"));
    viewer.keys(b"h");
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, "# Recovered").unwrap();
    viewer.read_frame();
    assert_eq!(viewer.screen.content_line(1), "# Recovered");
    assert!(!viewer.screen.line(11).contains("Error reloading"));
    viewer.quit();
}

#[test]
fn reload_preserves_position_policy_tabs_and_recomputes_search_and_links() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("reload.md");
    let other = temp.path().join("other.md");
    let body = "filler\n\n".repeat(40);
    std::fs::write(&path, format!("# Original\n\n{body}needle\n\n[old](#old)")).unwrap();
    std::fs::write(&other, "# Other").unwrap();
    let mut viewer = Viewer::start_with(
        &path,
        60,
        12,
        false,
        &["--math", "source", other.to_str().unwrap()],
        &[],
    );
    viewer.keys(b"ddd");
    let before = viewer.screen.content_line(1);
    std::fs::write(
        &path,
        format!(
            "# Updated\n\n{}needle needle\n\n[new](#updated)\n\n$x^2$",
            body.replace("filler", "updated filler")
        ),
    )
    .unwrap();
    viewer.raw.clear();
    viewer.read_frame();
    assert_eq!(
        viewer.screen.content_line(1),
        before.replace("filler", "updated filler")
    );
    assert!(!viewer.screen.line(11).contains("Reloaded"));
    let raw = String::from_utf8_lossy(&viewer.raw);
    assert!(!raw.contains("\x1b[?1049"));
    assert!(!raw.contains("\x1b[2J"));
    viewer.keys(b"/needle\r");
    assert!(viewer.screen.line(11).contains("1/2"));
    std::fs::write(
        &path,
        format!(
            "# Updated\n\n{}needle\n\n[new](#updated)\n\n$x^2$",
            body.replace("filler", "updated filler")
        ),
    )
    .unwrap();
    viewer.read_frame();
    viewer.keys(b"n");
    assert!(viewer.screen.line(11).contains("1/1"));
    viewer.keys(b"\t\r");
    assert_eq!(viewer.screen.content_line(1), "# Updated");
    viewer.keys(b"\x1b[1;3C");
    assert_eq!(viewer.screen.content_line(1), "# Other");
    viewer.keys(b"b");
    assert_eq!(viewer.screen.content_line(1), "# Updated");
    viewer.keys(b"G");
    assert!((1..11).any(|row| viewer.screen.line(row).contains("$x^2$")));
    viewer.quit();
}

#[test]
fn reload_failure_retains_the_old_document_and_shrink_clamps_both_offsets() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("shrink.md");
    std::fs::write(
        &path,
        format!(
            "```text\n{}\n```",
            format!("{}\n", "x".repeat(150)).repeat(40)
        ),
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 50, 12, false);
    viewer.keys(&b"\x1b[C".repeat(70));
    viewer.keys(b"G");
    let before: Vec<_> = (1..11).map(|row| viewer.screen.content_line(row)).collect();
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    viewer.read_frame();
    assert!(viewer.screen.line(11).contains("Error reloading"));
    assert_eq!(
        (1..11)
            .map(|row| viewer.screen.content_line(row))
            .collect::<Vec<_>>(),
        before
    );
    std::fs::remove_file(&path).unwrap();
    viewer.read_frame();
    assert!(viewer.screen.line(11).contains("Error reloading"));
    assert_eq!(
        (1..11)
            .map(|row| viewer.screen.content_line(row))
            .collect::<Vec<_>>(),
        before
    );
    std::fs::write(&path, "# Short").unwrap();
    viewer.read_frame();
    assert_eq!(viewer.screen.content_line(1), "# Short");
    assert!((2..11).all(|row| viewer.screen.line(row).is_empty()));
    viewer.resize(40, 10);
    assert_eq!(viewer.screen.content_line(1), "# Short");
    viewer.quit();
}

#[test]
fn unicode_search_link_clicks_and_clipping_use_display_columns() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("unicode.md");
    std::fs::write(
        &path,
        format!(
            "界e\u{301}👩‍💻 [go](#target) tail\n\n# Target\n\n{}",
            "end\n\n".repeat(10)
        ),
    )
    .unwrap();
    let mut viewer = Viewer::start(&path, 40, 12, true);
    assert_eq!(viewer.screen.cells[1][3].to_string(), "界");
    assert_eq!(viewer.screen.cells[1][4].to_string(), "");
    viewer.keys("/e\u{301}👩‍💻\r".as_bytes());
    for column in 2..18 {
        assert_eq!(
            viewer.screen.underlined[1][column],
            (5..8).contains(&column),
            "column {column}"
        );
    }
    viewer.resize(30, 12);
    assert!(viewer.screen.line(11).contains("1/1"));
    viewer.keys(b"\x1b[<0;10;2M");
    assert!(viewer.screen.line(1).contains("# Target"));
    viewer.quit();
}

#[test]
fn terminal_model_joins_graphemes_and_keeps_wide_continuations() {
    let mut screen = Screen::new(20, 3);
    let mut parser = Parser::new();
    for part in ["界", "e", "\x1b[7m", "\u{301}", "👩", "\u{200d}", "💻", "z"] {
        parser.advance(&mut screen, part.as_bytes());
    }
    assert_eq!(screen.x, 6);
    assert_eq!(screen.cells[0][0].to_string(), "界");
    assert_eq!(screen.cells[0][1].to_string(), "");
    assert_eq!(screen.cells[0][2].to_string(), "e\u{301}");
    assert_eq!(screen.cells[0][3].to_string(), "👩‍💻");
    assert_eq!(screen.cells[0][4].to_string(), "");
    assert_eq!(screen.cells[0][5].to_string(), "z");
    let mut edge = Screen::new(2, 2);
    parser.advance(&mut edge, "界\u{301}".as_bytes());
    assert_eq!(edge.y, 0);
    assert_eq!(edge.scrolls, 0);
    parser.advance(&mut edge, b"x");
    assert_eq!(edge.y, 1);
}

#[test]
fn browser_timeout_retains_the_document_diagnostics_and_live_input() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("browser.md");
    let launcher = temp.path().join("xdg-open");
    std::fs::write(&path, "# Document\n\n[web](https://example.test)").unwrap();
    std::fs::write(
        &launcher,
        "#!/bin/sh\nprintf 'launch hint\\n' >&2\nexec sleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    let tools_path = format!(
        "{}:{}",
        temp.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut viewer = Viewer::start_with(&path, 160, 12, false, &[], &[("PATH", &tools_path)]);
    viewer.raw.clear();
    let started = Instant::now();
    viewer.master.write_all(b"\r").unwrap();
    viewer.read_with_timeout(true, Duration::from_secs(8));
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_secs(8));
    assert!(viewer.screen.line(11).contains("timed out after 5 seconds"));
    assert!(viewer.screen.line(11).contains("launch hint"));
    assert_eq!(viewer.screen.content_line(1), "# Document");
    let raw = String::from_utf8_lossy(&viewer.raw);
    assert!(!raw.contains("\x1b[?1049") && !raw.contains("\x1b[2J"));
    viewer.keys(b"j");
    assert!(viewer.screen.line(11).contains("L1-"));
    viewer.quit();
}

#[test]
fn reload_rejects_fifo_replacements_without_blocking() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("file.md");
    std::fs::write(&path, "# Retained").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    std::fs::remove_file(&path).unwrap();
    nix::unistd::mkfifo(
        &path,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    viewer.read_frame();
    assert!(viewer.screen.line(11).contains("requires a regular file"));
    assert_eq!(viewer.screen.content_line(1), "# Retained");
    viewer.quit();
}

#[test]
fn unicode_horizontal_clipping_keeps_graphemes_and_reflows_after_resize() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wide-unicode.md");
    std::fs::write(&path, "```text\n界e\u{301}👩‍💻abcdefghijklmnopqrstuv\n```").unwrap();
    let mut viewer = Viewer::start(&path, 20, 10, false);
    viewer.keys(b"\x1b[C");
    assert!(viewer.screen.content_line(2).starts_with(" e\u{301}👩‍💻"));
    assert_eq!(viewer.screen.cells[2][2], "👩‍💻");
    assert_eq!(viewer.screen.cells[2][3], "");
    viewer.keys(b"\x1b[C");
    assert!(viewer.screen.content_line(2).starts_with("e\u{301}👩‍💻"));
    viewer.resize(50, 12);
    assert!(viewer.screen.content_line(2).starts_with("界e\u{301}👩‍💻"));
    assert_eq!(viewer.screen.scrolls, 0);
    viewer.quit();
}

#[test]
fn reload_does_not_follow_a_replaced_final_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("file.md");
    let replacement = temp.path().join("replacement.md");
    std::fs::write(&path, "# Retained").unwrap();
    std::fs::write(&replacement, "# Not loaded").unwrap();
    let mut viewer = Viewer::start(&path, 80, 12, false);
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&replacement, &path).unwrap();
    viewer.read_frame();
    assert!(viewer.screen.line(11).contains("Error reloading"));
    assert_eq!(viewer.screen.content_line(1), "# Retained");
    viewer.quit();
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
        viewer.keys_with_frame(b"e");
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
    viewer.keys_with_frame(b"\t");
    assert!(
        viewer.screen.line(23).contains("Links: 1/1"),
        "link selection not shown: {}",
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
fn f_is_inert_with_or_without_links_and_does_not_clear_search_or_block_navigation() {
    let temp = tempfile::tempdir().unwrap();
    let no_links = temp.path().join("nolinks.md");
    std::fs::write(&no_links, "# No Links Here\n\njust plain prose").unwrap();
    let mut viewer = Viewer::start(&no_links, 80, 24, false);
    viewer.raw.clear();
    viewer.keys(b"f");
    assert!(viewer.raw.is_empty());
    assert!(
        !viewer.screen.line(23).contains("Links:"),
        "f must not select links: {}",
        viewer.screen.line(0)
    );
    viewer.quit();
    let with_link = temp.path().join("withlink.md");
    std::fs::write(&with_link, "# Titled\n\n[link](#titled)").unwrap();
    let mut viewer = Viewer::start(&with_link, 80, 24, false);
    viewer.keys_with_frame(b"/Titled\r");
    let footer = viewer.screen.line(23);
    viewer.raw.clear();
    viewer.keys(b"f");
    assert!(viewer.raw.is_empty());
    assert_eq!(viewer.screen.line(23), footer);
    viewer.keys_with_frame(b"\t");
    assert!(viewer.screen.line(23).contains("Links: 1/1"));
    viewer.raw.clear();
    viewer.keys(b"f");
    assert!(viewer.raw.is_empty());
    viewer.keys_with_frame(b"h");
    assert!(viewer.screen.line(0).contains("help"));
    viewer.keys_with_frame(b"h");
    viewer.exit_with(b"\x1b");
}

#[test]
fn enter_follows_the_link_at_the_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let linked = temp.path().join("linked.md");
    let source = temp.path().join("source.md");
    std::fs::write(&linked, "# Linked Title\n\nlinked body").unwrap();
    // The link is on the first content line, at the top of the viewport.
    std::fs::write(&source, "intro\n[open](linked.md)").unwrap();
    let mut viewer = Viewer::start(&source, 80, 24, false);
    assert!(viewer.screen.line(0).contains("source.md"));
    // Enter in normal mode follows the first link at or below the top of the
    // viewport.
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
fn aligned_table_links_keep_selection_and_mouse_hitboxes_after_resize() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.md");
    std::fs::write(temp.path().join("target.md"), "# Target").unwrap();
    for (delimiter, leading) in [(":---:", 7), ("---:", 14)] {
        std::fs::write(
            &source,
            format!(
                "| Alignment |\n| {delimiter} |\n| [**界** *x* ~~y~~](target.md) |\n| 01234567890123456789 |"
            ),
        )
        .unwrap();
        for numbers in [false, true] {
            let mut viewer = Viewer::start(&source, 40, 10, numbers);
            let column = leading + if numbers { 2 } else { 0 };
            assert_eq!(viewer.screen.cells[2][column], "界");
            assert_eq!(viewer.screen.emphasis[2][column], [true, false, false]);
            assert_eq!(viewer.screen.emphasis[2][column + 3], [false, true, false]);
            assert_eq!(viewer.screen.emphasis[2][column + 5], [false, false, true]);
            viewer.keys(format!("\x1b[<0;{};3M", column).as_bytes());
            assert!(viewer.screen.line(0).contains("source.md"));
            viewer.keys_with_frame("/界 x y\r".as_bytes());
            assert_eq!(viewer.screen.emphasis[2][column], [true, false, false]);
            assert_eq!(viewer.screen.emphasis[2][column + 3], [false, true, false]);
            assert_eq!(viewer.screen.emphasis[2][column + 5], [false, false, true]);
            assert!(viewer.screen.underlined[2][column]);
            viewer.keys_with_frame(b"/absent\r");
            assert_eq!(viewer.screen.emphasis[2][column], [true, false, false]);
            assert!(!viewer.screen.underlined[2][column]);
            viewer.keys_with_frame(b"\t");
            assert!(
                viewer.screen.underlined[2]
                    .iter()
                    .enumerate()
                    .all(|(index, &value)| value == (column..column + 6).contains(&index))
            );
            viewer.resize(16, 10);
            viewer.keys(&b"\x1b[C".repeat(20));
            let clipped = leading - 5;
            assert_eq!(viewer.screen.cells[2][clipped], "界");
            assert_eq!(viewer.screen.emphasis[2][clipped], [true, false, false]);
            assert_eq!(viewer.screen.emphasis[2][clipped + 3], [false, true, false]);
            assert_eq!(viewer.screen.emphasis[2][clipped + 5], [false, false, true]);
            assert!(
                viewer.screen.underlined[2][clipped..clipped + 6]
                    .iter()
                    .all(|&v| v)
            );
            viewer.resize(50, 12);
            assert_eq!(viewer.screen.cells[2][column], "界");
            assert_eq!(viewer.screen.emphasis[2][column], [true, false, false]);
            viewer.keys(format!("\x1b[<0;{};3M", column + 1).as_bytes());
            assert_eq!(
                viewer.screen.line(1),
                if numbers { "1 # Target" } else { "# Target" }
            );
            assert_eq!(viewer.screen.scrolls, 0);
            viewer.quit();
        }
    }
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
    viewer.keys_with_frame(b"\r");
    for (action, heading) in [
        (b"e".as_slice(), "# Target"),
        (b"\x1b[1;3D".as_slice(), "# Source"),
        (b"\x1b[1;3C".as_slice(), "# Target"),
    ] {
        viewer.raw.clear();
        viewer.keys_with_frame(action);
        let raw = String::from_utf8_lossy(&viewer.raw);
        assert!(raw.contains("\x1b[38;2;1;2;3m"), "{raw:?}");
        if heading == "# Target" {
            assert!(raw.contains("\x1b[48;5;254m"), "{raw:?}");
        }
        assert_eq!(viewer.screen.line(1), heading);
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
