//! Exercise the real terminal entry point with deliberately blocked discovery.

use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{Winsize, openpty};
use serde_json::{Value, json};

struct Provider {
    endpoint: String,
    release: mpsc::Sender<()>,
    requests: mpsc::Receiver<(String, Value)>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Provider {
    fn blocked(block_path: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (release, gate) = mpsc::channel();
        let (requests, received) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut headers = Vec::new();
                let mut byte = [0];
                while !headers.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    headers.push(byte[0]);
                    assert!(headers.len() < 65536);
                }
                let headers = String::from_utf8(headers).unwrap();
                let path = headers.split_whitespace().nth(1).unwrap().to_owned();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                assert!(length < 1024 * 1024);
                let mut body = vec![0; length];
                socket.read_exact(&mut body).unwrap();
                let body = if body.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&body).unwrap()
                };
                requests.send((path.clone(), body.clone())).unwrap();
                if path.starts_with(block_path) {
                    loop {
                        if stopping.load(Ordering::SeqCst) {
                            return;
                        }
                        match gate.recv_timeout(Duration::from_millis(5)) {
                            Ok(()) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
                let response = match path.as_str() {
                    "/props" => json!({"role":"router","model_alias":"llama-server"}).to_string(),
                    "/models" => json!({"data":[
                        {"id":"Model-A","status":{"value":"unloaded"}},
                        {"id":"Model-B","status":{"value":"loaded"}}
                    ]})
                    .to_string(),
                    "/v1/chat/completions" => {
                        let delta = json!({"model":body["model"],"choices":[{"index":0,"delta":{"content":"answer-ready"},"finish_reason":null}]});
                        format!(
                            "data: {delta}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
                        )
                    }
                    path if path.starts_with("/props?model=") => {
                        json!({"default_generation_settings":{"n_ctx":32768}}).to_string()
                    }
                    other => panic!("unexpected startup request: {other}"),
                };
                let content_type = if path == "/v1/chat/completions" {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                // Cancellation may close the peer before the fixture responds.
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
            }
        });
        Self {
            endpoint,
            release,
            requests: received,
            stop,
            thread: Some(thread),
        }
    }

    fn next(&self) -> (String, Value) {
        self.requests
            .recv_timeout(Duration::from_secs(3))
            .expect("provider request")
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

struct Tui {
    child: Child,
    master: File,
    output: Vec<u8>,
    _root: tempfile::TempDir,
}

fn visible_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars();
    let mut visible = String::new();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for ch in chars.by_ref() {
                        if ('@'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    for ch in chars.by_ref() {
                        if ch == '\x07' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !ch.is_whitespace() {
            visible.push(ch);
        }
    }
    visible
}

impl Tui {
    fn start(provider: &Provider, explicit: Option<&str>, discover_capacity: bool) -> Self {
        Self::start_with_ignore(provider, explicit, discover_capacity, false)
    }

    fn start_with_ignore(
        provider: &Provider,
        explicit: Option<&str>,
        discover_capacity: bool,
        fifo: bool,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        if fifo {
            nix::unistd::mkfifo(
                &root.path().join(".gitignore"),
                nix::sys::stat::Mode::S_IRUSR,
            )
            .unwrap();
        }
        let config = root.path().join("skott.toml");
        let capacity = if discover_capacity {
            ""
        } else {
            "context_limit=32768\n"
        };
        std::fs::write(&config, format!(
            "schema_version=1\nworking_directory={:?}\ndefault_provider='llama-server'\n\
             default_thinking_effort='none'\n[tool_profiles]\n\
             rust='off'\npython='off'\njavascript='off'\ngo='off'\nc='off'\n\
             [[models]]\nid='a'\nprovider='llama-server'\nmodel='Model-A'\nbase_url={:?}\nis_default=true\n{capacity}\
             [[models]]\nid='b'\nprovider='llama-server'\nmodel='Model-B'\nbase_url={:?}\n{capacity}",
             root.path().to_str().unwrap(), provider.endpoint, provider.endpoint,
        )).unwrap();
        let pty = openpty(
            Some(&Winsize {
                ws_row: 30,
                ws_col: 120,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            None,
        )
        .unwrap();
        let master = File::from(pty.master);
        let slave = File::from(pty.slave);
        fcntl(&master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_skott"));
        command
            .arg("--config")
            .arg(config)
            .arg("--no-logs")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        if let Some(id) = explicit {
            command.args(["--model", id]);
        }
        let child = command.spawn().unwrap();
        Self {
            child,
            master,
            output: Vec::new(),
            _root: root,
        }
    }

    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let mut bytes = [0; 32768];
            match self.master.read(&mut bytes) {
                Ok(count) => self.output.extend_from_slice(&bytes[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("terminal read: {error}"),
            }
            if visible_text(&self.output).contains(
                &text
                    .chars()
                    .filter(|ch| !ch.is_whitespace())
                    .collect::<String>(),
            ) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "missing {text:?}: {}",
                String::from_utf8_lossy(&self.output)
            );
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "TUI exited: {}",
                String::from_utf8_lossy(&self.output)
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn type_text(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }

    fn quit(&mut self) {
        let started = Instant::now();
        self.type_text(b"\x11");
        let deadline = started + Duration::from_secs(1);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "quit did not cancel background work promptly"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut bytes = [0; 32768];
        while let Ok(count) = self.master.read(&mut bytes) {
            if count == 0 {
                break;
            }
            self.output.extend_from_slice(&bytes[..count]);
        }
        assert!(
            self.output
                .windows(b"\x1b[?1049l".len())
                .any(|bytes| bytes == b"\x1b[?1049l"),
            "terminal not restored"
        );
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
    }
}

#[test]
fn prompt_is_editable_and_quit_is_fast_while_model_discovery_is_blocked() {
    let provider = Provider::blocked("/props");
    let started = Instant::now();
    let mut tui = Tui::start(&provider, None, false);
    tui.wait_for("skott ready");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "prompt waited for discovery"
    );
    assert_eq!(provider.next().0, "/props");
    tui.type_text(b"typed-before-discovery");
    tui.wait_for("typed-before-discovery");
    assert!(
        provider.requests.try_recv().is_err(),
        "no inference during discovery"
    );
    tui.quit();
}

#[test]
fn gitignore_fifo_cannot_block_discovery_or_quit() {
    let provider = Provider::blocked("/props");
    let mut tui = Tui::start_with_ignore(&provider, None, false, true);
    tui.wait_for("skott ready");
    tui.wait_for("Toolchains:");
    tui.quit();
}

#[test]
fn submitted_prompts_wait_for_router_discovery_and_use_the_loaded_model() {
    let provider = Provider::blocked("/props");
    let mut tui = Tui::start(&provider, None, false);
    tui.wait_for("skott ready");
    assert_eq!(provider.next().0, "/props");
    tui.type_text(b"first-pending\r\r");
    tui.wait_for("first-pending");
    tui.type_text(b"second-pending\r\r");
    tui.wait_for("second-pending");
    assert!(provider.requests.try_recv().is_err());
    provider.release.send(()).unwrap();
    assert_eq!(provider.next().0, "/models");
    for prompt in ["first-pending", "second-pending"] {
        let (path, request) = provider.next();
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(request["model"], "Model-B");
        assert!(
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains(prompt)))
        );
    }
    tui.wait_for("answer-ready");
    tui.quit();
    let output = String::from_utf8_lossy(&tui.output);
    assert!(!output.contains("sha256:"));
    assert!(!output.contains("context/usage accounting"));
}

#[test]
fn manual_model_choice_survives_a_late_active_model_result() {
    let provider = Provider::blocked("/props");
    let mut tui = Tui::start(&provider, None, false);
    tui.wait_for("skott ready");
    assert_eq!(provider.next().0, "/props");
    // From an unselected model, Tab cycles to b, then a.
    tui.type_text(b"\t\tmanual-choice\r\r");
    tui.wait_for("manual-choice");
    provider.release.send(()).unwrap();
    let requests = [provider.next(), provider.next()];
    assert!(requests.iter().any(|(path, _)| path == "/models"));
    let (_, request) = requests
        .iter()
        .find(|(path, _)| path == "/v1/chat/completions")
        .unwrap();
    assert_eq!(request["model"], "Model-A");
    tui.wait_for("answer-ready");
    tui.type_text(b"after-discovery\r\r");
    let (_, request) = provider.next();
    assert_eq!(request["model"], "Model-A");
    tui.quit();
}

#[test]
fn explicit_selection_skips_active_probe_and_capacity_wait_is_cancellable() {
    let provider = Provider::blocked("/props?model=");
    let mut tui = Tui::start(&provider, Some("a"), true);
    tui.wait_for("skott ready");
    assert_eq!(provider.next().0, "/props?model=Model-A");
    tui.type_text(b"editable-during-capacity");
    tui.wait_for("editable-during-capacity");
    tui.quit();
}
