//! End-to-end smoke tests for the interactive workspace shell (`kvist shell`).
//!
//! `shell_rejects_a_non_interactive_stdin` covers the piped-stdin rejection
//! with its actionable hint. `shell_on_a_pty_shows_the_banner_and_exits_cleanly`
//! drives the shell over a real pseudo-terminal through the full line-editor
//! stack: help, selected-component insertion, task completion, deletion,
//! paste, history, policy refusal without confirmation, and a clean exit. The
//! test answers the line editor's terminal queries (cursor position reports)
//! like a minimal terminal emulator, since a bare pty has no emulator behind
//! it.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Initializes a Kvist project in `dir` so context-dependent commands have a
/// project to resolve.
fn init_project(dir: &std::path::Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["init", dir.to_str().expect("UTF-8 project path")])
        .status()
        .expect("initialize the project");
    assert!(status.success(), "kvist init must succeed in {dir:?}");
}

#[cfg(unix)]
struct EditorQueues {
    root_path: std::path::PathBuf,
    root_before: String,
    child_path: std::path::PathBuf,
    child_before: String,
}

#[cfg(unix)]
fn add_editor_task(dir: &tempfile::TempDir) -> EditorQueues {
    let queue_path = dir.path().join("src/TODOS.yaml");
    let mut queue =
        kvist::task_queue::parse(&std::fs::read_to_string(&queue_path).unwrap()).unwrap();
    let timestamp: kvist::task_queue::Timestamp =
        serde_json::from_str("\"2026-10-28T23:00:00Z\"").unwrap();
    queue.tasks.push(kvist::task_queue::Task {
        id: "prefill-test".into(),
        title: "Test editor prefill".into(),
        description: "Never execute this task in the terminal fixture.".into(),
        context: "The terminal fixture has no execution policy.".into(),
        purpose: "Keep terminal input visible.".into(),
        expected_outcome: "Policy refusal has no durable effects.".into(),
        kind: kvist::task_queue::TaskKind::Test,
        status: kvist::task_queue::TaskStatus::Pending,
        depends_on: vec![],
        requirements: vec!["REQUIREMENTS.md#Task-selection-and-state-updates".into()],
        timestamps: kvist::task_queue::TaskTimestamps {
            created_at: timestamp.clone(),
            updated_at: timestamp,
            completed_at: None,
            recovered: false,
        },
        blocked_reason: None,
        recovery_state: None,
        acceptance_id: None,
        disposition: None,
    });
    assert!(
        queue
            .tasks
            .iter()
            .all(|task| task.id != "dash-prefill-test")
    );
    std::fs::write(&queue_path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
    for path in ["src/other folder", "src/-component"] {
        assert!(
            Command::new(env!("CARGO_BIN_EXE_kvist"))
                .args(["component", "new", path])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success()
        );
    }
    for (path, task_id) in [
        ("src/other folder", None),
        ("src/-component", Some("dash-prefill-test")),
    ] {
        let mut child_queue = queue.clone();
        if let Some(task_id) = task_id {
            child_queue.tasks[0].id = task_id.into();
        } else {
            child_queue.tasks.clear();
        }
        std::fs::write(
            dir.path().join(path).join("TODOS.yaml"),
            kvist::task_queue::serialize(&child_queue).unwrap(),
        )
        .unwrap();
        std::fs::copy(
            dir.path().join("src/IMPL.md"),
            dir.path().join(path).join("IMPL.md"),
        )
        .unwrap();
    }
    let config_path = dir.path().join("kvist.toml");
    let configuration = std::fs::read_to_string(&config_path).unwrap();
    // These inert executables must never be reached without execution approval.
    std::fs::write(
        config_path,
        format!(
            "{configuration}\n\
             [sandbox]\n\
             schema_version = 1\n\
             runner = \"/usr/bin/false\"\n\
             backend = \"/usr/bin/false\"\n\
             network = \"deny\"\n\
             environment_allowlist = []\n\
             mount = \"component\"\n\
             [test_policy]\n\
             schema_version = 1\n\
             working_directory = \"component\"\n\
             environment_allowlist = []\n\
             timeout_seconds = 1\n\
             max_output_bytes = 1000\n\
             [[test_policy.commands]]\n\
             component = \"-component\"\n\
             command = \"/usr/bin/false\"\n"
        ),
    )
    .unwrap();
    for args in [vec!["init", "--quiet"], vec!["add", "."]] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success()
        );
    }
    for component in [".", "src/other folder", "src/-component"] {
        let accepted = Command::new(env!("CARGO_BIN_EXE_kvist"))
            .args(["component", "accept", component])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            accepted.status.success(),
            "fixture acceptance failed for {component}: {}",
            String::from_utf8_lossy(&accepted.stderr)
        );
    }
    assert!(
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    let inspection = kvist::project_state::inspect(dir.path()).unwrap();
    assert_eq!(
        inspection.state,
        kvist::project_state::ProjectState::Current
    );
    assert!(
        inspection
            .components
            .iter()
            .all(|component| component.state == kvist::project_state::ComponentState::Current)
    );
    let child_path = dir.path().join("src/-component/TODOS.yaml");
    EditorQueues {
        root_before: std::fs::read_to_string(&queue_path).unwrap(),
        root_path: queue_path,
        child_before: std::fs::read_to_string(&child_path).unwrap(),
        child_path,
    }
}

fn fixture_directory() -> tempfile::TempDir {
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.kvist");
    std::fs::create_dir_all(&parent).expect("create project-local test storage");
    tempfile::Builder::new()
        .prefix("shell-smoke-")
        .tempdir_in(parent)
        .expect("project-local fixture directory")
}

#[test]
fn shell_rejects_a_non_interactive_stdin() {
    let dir = fixture_directory();
    init_project(dir.path());
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
        while i < chars.len() {
            let ch = chars[i];
            match ch {
                '\x1b' => match self.try_parse_sequence(&chars[i..], reply) {
                    Some(consumed) => {
                        i += consumed;
                        continue;
                    }
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
            i += 1;
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

    let dir = fixture_directory();
    init_project(dir.path());
    let queues = add_editor_task(&dir);
    // Isolate the child from the user's real state and configuration.
    let homes = fixture_directory();
    let state_home = homes.path().join("state");
    let config_home = homes.path().join("config");
    std::fs::create_dir_all(&state_home).expect("create state home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    let state_home = state_home.canonicalize().unwrap();
    let config_home = config_home.canonicalize().unwrap();
    let mut paused = kvist::task_queue::parse(&queues.child_before).unwrap();
    paused.tasks[0].status = kvist::task_queue::TaskStatus::Blocked;
    paused.tasks[0].blocked_reason = Some("Review this fixture retry.".into());
    std::fs::write(
        &queues.child_path,
        kvist::task_queue::serialize(&paused).unwrap(),
    )
    .unwrap();
    let draft = dir.path().join("fixture-decision.json");
    std::fs::write(&draft, r#"{"schema_version":1,"rationale":"Fixture retry.","documents":{},"tasks":[],"disposition":"resolve"}"#).unwrap();
    let prepared = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args([
            "decision",
            "./-component",
            "plan",
            "--import",
            draft.to_str().unwrap(),
            "--review-exception",
            "Fixture reviewed offline.",
        ])
        .current_dir(dir.path())
        .env("XDG_STATE_HOME", &state_home)
        .env("XDG_CONFIG_HOME", &config_home)
        .env_remove("KVIST_CONFIG_PATH")
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    // Retain a real authenticated response, but stale its queue so selecting it cannot apply.
    std::fs::write(&queues.child_path, &queues.child_before).unwrap();
    let absolute_component = dir.path().join("src/-component").canonicalize().unwrap();
    let absolute_command = format!(
        "task run \"{}\" dash-prefill-test",
        absolute_component
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    let absolute_input = format!("{absolute_command}\r").into_bytes();

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

    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_kvist"))
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
            .expect("spawn the shell on the pty"),
    );

    let flags = fcntl(master.as_fd(), FcntlArg::F_GETFL).expect("read the pty flags");
    fcntl(
        master.as_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .expect("make the pty master non-blocking");

    let deadline = Instant::now() + Duration::from_secs(45);
    let mut terminal = FakeTerminal::new(100);
    let mut bytes = Vec::new();
    let mut consumed = 0usize;
    let mut buffer = [0u8; 8192];
    let mut phase = 0;
    let mut phase_start = 0;
    let mut phase_time = Instant::now();

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
                "timed out in shell phase {phase}; output so far:\n{}",
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

        let fresh = &output[phase_start..];
        let entries: Vec<serde_json::Value> =
            std::fs::read_to_string(dir.path().join(".kvist/session.log"))
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .collect();
        let commands: Vec<&str> = entries
            .iter()
            .filter_map(|entry| entry["command"].as_str())
            .collect();
        let assert_policy_refusal = |command: &str| {
            let result = entries
                .iter()
                .find(|entry| entry["command"].as_str() == Some(command))
                .and_then(|entry| entry["result"].as_str())
                .expect("submitted command has a journal result");
            assert!(
                result.contains("approval record") && result.contains("is missing"),
                "{command} did not reach the missing-approval gate: {result}"
            );
        };
        let next: Option<&[u8]> = match phase {
            0 if output.contains("Kvist Interactive Workspace Shell") => {
                assert!(output.contains("Branch:"), "banner: {output}");
                assert!(output.contains("Component:  . (root)"), "banner: {output}");
                assert!(output.contains("Ctrl+L clear"), "banner: {output}");
                assert!(
                    output.contains('╭') && output.contains('╰'),
                    "banner: {output}"
                );
                Some(b"help\r")
            }
            1 if fresh.contains("Kvist shell — builtins") => {
                assert!(output.contains("kvist ❯"), "prompt: {output}");
                assert!(output.contains("cd [COMPONENT]"), "help output: {output}");
                Some(b"cd .\r")
            }
            2 if commands.contains(&"cd .") => Some(b"task run "),
            // This must appear before Enter, without a prior task command in
            // history. A later journal assertion proves it was real buffer text.
            3 if fresh.contains("task run . ") => Some(b"\t"),
            4 if fresh.contains("prefill-test") => Some(b"\r"),
            5 if fresh.contains("task run . prefill-test") => Some(b"\r"),
            6 if commands.contains(&"task run . prefill-test") => {
                assert!(!fresh.contains("Run suggested"), "{fresh}");
                assert_policy_refusal("task run . prefill-test");
                Some(b"component validate ")
            }
            7 if fresh.contains("component validate . ") => Some(b"\x17"),
            8 if phase_time.elapsed() >= Duration::from_millis(200) => {
                assert!(!fresh.contains("component validate . "), "{fresh}");
                Some(b"\"other folder\"\r")
            }
            9 if commands.contains(&"component validate \"other folder\"") => Some(b"\x1b[A"),
            10 if fresh.contains("component validate \"other folder\"") => Some(b"\r"),
            11 if commands
                .iter()
                .filter(|command| **command == "component validate \"other folder\"")
                .count()
                == 2 =>
            {
                Some(b"\x1b[200~component validate --json \x1b[201~")
            }
            12 if fresh.contains("component validate --json . ") => Some(b"\x03"),
            13 if fresh.contains("^C") => Some(b"cd \"other folder\"\r"),
            14 if commands.contains(&"cd \"other folder\"") => Some(b"component validate "),
            15 if fresh.contains("component validate \"other folder\" ") => Some(b"\r"),
            16 if commands
                .iter()
                .filter(|command| **command == "component validate \"other folder\"")
                .count()
                == 3 =>
            {
                Some(b"cd .\r")
            }
            17 if commands
                .iter()
                .filter(|command| **command == "cd .")
                .count()
                == 2 =>
            {
                Some(b"cd \"-component\"\r")
            }
            18 if commands.contains(&"cd \"-component\"") => Some(b"task run "),
            19 if fresh.contains("task run ./-component ") => Some(b"\t"),
            20 if fresh.contains("dash-prefill-test") => Some(b"\r"),
            21 if fresh.contains("task run ./-component dash-prefill-test") => Some(b"\r"),
            22 if commands.contains(&"task run ./-component dash-prefill-test") => {
                assert_policy_refusal("task run ./-component dash-prefill-test");
                assert_eq!(
                    std::fs::read_to_string(&queues.child_path).unwrap(),
                    queues.child_before
                );
                Some(b"cd .\r")
            }
            23 if commands
                .iter()
                .filter(|command| **command == "cd .")
                .count()
                == 3 =>
            {
                Some(absolute_input.as_slice())
            }
            24 if commands.contains(&absolute_command.as_str()) => {
                assert_policy_refusal(&absolute_command);
                Some(b"task run\r")
            }
            25 if commands.contains(&"task run .")
                && (fresh.to_ascii_lowercase().contains("error")
                    || fresh.to_ascii_lowercase().contains("failed")) =>
            {
                assert!(!fresh.contains("Run suggested"), "{fresh}");
                assert!(!fresh.contains("[Y/n]"), "{fresh}");
                let diagnostic = fresh.to_ascii_lowercase();
                assert!(
                    diagnostic.contains("error") || diagnostic.contains("failed"),
                    "{fresh}"
                );
                assert_eq!(
                    std::fs::read_to_string(&queues.root_path).unwrap(),
                    queues.root_before
                );
                Some(b"decision ")
            }
            26 if fresh.contains("decision . ") => Some(b"\r"),
            27 if commands.contains(&"decision .") && fresh.contains("0 paused task(s)") => {
                assert!(!fresh.contains("Select component"), "{fresh}");
                Some(b"cd \"-component\"\r")
            }
            28 if commands
                .iter()
                .filter(|command| **command == "cd \"-component\"")
                .count()
                == 2 =>
            {
                Some(b"decision ")
            }
            29 if fresh.contains("decision ./-component ") => Some(b"\r"),
            30 if commands.contains(&"decision ./-component")
                && fresh.contains("0 paused task(s)") =>
            {
                Some(b"decision ")
            }
            31 if fresh.contains("decision ./-component ") => Some(b"do \t"),
            32 if fresh.contains("Approve exact") && fresh.contains("application") => Some(b"\r"),
            33 if fresh.contains("decision ./-component do 1") => Some(b"\r"),
            34 if commands.contains(&"decision ./-component do 1")
                && (fresh.to_ascii_lowercase().contains("error")
                    || fresh.to_ascii_lowercase().contains("failed")) =>
            {
                assert_eq!(
                    std::fs::read_to_string(&queues.child_path).unwrap(),
                    queues.child_before
                );
                assert!(!fresh.contains("[Y/n]"), "{fresh}");
                Some(b"exit\r")
            }
            _ => None,
        };
        if let Some(input) = next {
            phase += 1;
            phase_start = output.len();
            phase_time = Instant::now();
            write(master.as_fd(), input).expect("send the next editor interaction");
        }
    }

    assert!(
        phase == 35,
        "all editor phases must complete (stopped at {phase}); output so far:\n{}",
        utf8_prefix(&bytes)
    );
    let status = child.0.wait().expect("wait for the shell");
    assert!(
        status.success(),
        "the shell must exit cleanly; output:\n{}",
        utf8_prefix(&bytes)
    );
    assert_eq!(
        std::fs::read_to_string(queues.root_path).unwrap(),
        queues.root_before
    );
    assert_eq!(
        std::fs::read_to_string(queues.child_path).unwrap(),
        queues.child_before
    );
    for component in ["src", "src/-component"] {
        assert!(!dir.path().join(component).join(".kvist-task.lock").exists());
        assert!(!dir.path().join(component).join(".kvist-attempts").exists());
    }
}
