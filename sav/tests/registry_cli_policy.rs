use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    directory: tempfile::TempDir,
    registry: PathBuf,
    script: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir_in(".").expect("project-owned test directory");
        let root = directory
            .path()
            .canonicalize()
            .expect("absolute fixture path");
        Self {
            registry: root.join("registry.toml"),
            script: root.join("agent.sh"),
            marker: root.join("attempts"),
            directory,
        }
    }

    fn external(looping: bool) -> Self {
        let fixture = Self::new();
        let mut script = "printf 'attempt\\n' >> \"$1\"\n".to_owned();
        if looping {
            script.push_str(
                "printf 'same-line\\nsame-line\\nsame-line\\nsame-line\\n'\nexec /bin/sleep 5\n",
            );
        }
        fs::write(&fixture.script, script).expect("write literal test executable");
        fs::write(
            &fixture.registry,
            format!(
                "schema_version = 1\n[profiles.selected.backend]\nkind = \"external-agent\"\n\
                 provider = \"custom-script\"\nexecutable = \"/bin/sh\"\narguments = [{}, {}]\n",
                serde_json::to_string(&fixture.script).expect("literal script path"),
                serde_json::to_string(&fixture.marker).expect("literal marker path"),
            ),
        )
        .expect("write external registry");
        fixture
    }

    fn native(deadline_secs: u64) -> (Self, TcpListener) {
        let fixture = Self::new();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local fixture");
        fs::write(
            &fixture.registry,
            format!(
                "schema_version = 1\n[profiles.selected.backend]\nkind = \"native\"\n\
                 provider = \"llama-server\"\nmodel = \"pinned-model\"\n\
                 base_url = \"http://{}\"\ndeadline_secs = {deadline_secs}\n",
                listener.local_addr().expect("fixture address"),
            ),
        )
        .expect("write native registry");
        (fixture, listener)
    }

    fn command(&self, subcommand: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sav-run"));
        command
            .current_dir(self.directory.path())
            .args([subcommand, "--profile", "selected", "--registry"])
            .arg(&self.registry)
            .arg("hello");
        command
    }

    fn attempts(&self) -> usize {
        fs::read_to_string(&self.marker)
            .unwrap_or_default()
            .lines()
            .count()
    }
}

fn run_external(fixture: &Fixture, json: bool, retries: Option<&str>) -> Output {
    let mut command = fixture.command("run");
    command.args([
        "--allow-host-execution",
        "--detect-loops",
        "--idle-timeout",
        "2",
    ]);
    if json {
        command.arg("--json");
    }
    if let Some(retries) = retries {
        command.args(["--max-retries", retries]);
    }
    command.output().expect("run external fixture")
}

#[test]
fn external_profiles_default_to_one_attempt_even_after_retryable_failure() {
    for json in [false, true] {
        let fixture = Fixture::external(true);
        let output = run_external(&fixture, json, None);
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("repetition loop"),
            "{output:?}"
        );
        assert_eq!(fixture.attempts(), 1, "{output:?}");
    }
}

#[test]
fn external_profiles_reject_positive_retries_before_spawning() {
    for json in [false, true] {
        for retries in ["1", "3"] {
            let fixture = Fixture::external(false);
            let output = run_external(&fixture, json, Some(retries));
            assert_eq!(fixture.attempts(), 0, "{output:?}");
            assert!(!output.status.success(), "{output:?}");
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("--max-retries") && error.contains("external"),
                "{error}"
            );
        }
    }
}

#[test]
fn external_profiles_accept_explicit_zero_retries() {
    for json in [false, true] {
        let fixture = Fixture::external(false);
        let output = run_external(&fixture, json, Some("0"));
        assert!(output.status.success(), "{output:?}");
        assert_eq!(fixture.attempts(), 1, "{output:?}");
    }
}

#[test]
fn explicit_command_preserves_default_and_explicit_retry_policy() {
    for json in [false, true] {
        for (retries, expected_attempts) in [(None, 4), (Some("1"), 2), (Some("0"), 1)] {
            let fixture = Fixture::external(true);
            let mut command = Command::new(env!("CARGO_BIN_EXE_sav-run"));
            command.args(["run", "--command"]);
            command.arg(format!(
                "/bin/sh \"{}\" \"{}\"",
                fixture.script.display(),
                fixture.marker.display()
            ));
            command.args([
                "--allow-host-execution",
                "--detect-loops",
                "--idle-timeout",
                "2",
                "hello",
            ]);
            if json {
                command.arg("--json");
            }
            if let Some(retries) = retries {
                command.args(["--max-retries", retries]);
            }
            let output = command.output().expect("run explicit command");
            assert!(!output.status.success(), "{output:?}");
            assert_eq!(fixture.attempts(), expected_attempts, "{output:?}");
        }
    }
}

fn read_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bound fixture reads");
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).expect("request header");
        headers.push(byte[0]);
        assert!(headers.len() < 32 * 1024, "bounded request headers");
    }
    let headers = String::from_utf8(headers).expect("ASCII request headers");
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|length| length.trim().parse::<usize>().expect("content length"))
        })
        .expect("request body length");
    assert!(length < 32 * 1024, "bounded request body");
    let mut body = vec![0; length];
    stream.read_exact(&mut body).expect("request body");
}

#[test]
fn native_profiles_honor_cli_response_limit_for_unary_and_streaming() {
    for streaming in [false, true] {
        let (fixture, listener) = Fixture::native(5);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept native request");
            read_request(&mut stream);
            let text = "x".repeat(1_024);
            let (content_type, body) = if streaming {
                (
                    "text/event-stream",
                    format!(
                        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}},\
                         \"finish_reason\":null}}]}}\n\n\
                         data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
                         data: [DONE]\n\n"
                    ),
                )
            } else {
                (
                    "application/json",
                    format!(
                        "{{\"choices\":[{{\"message\":{{\"content\":\"{text}\"}},\
                         \"finish_reason\":\"stop\"}}]}}"
                    ),
                )
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        let mut command = fixture.command("model");
        command.args(["--max-response-bytes", "128"]);
        if streaming {
            command.arg("--stream");
        }
        let output = command.output().expect("request native profile");
        server.join().expect("native server");
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("128-byte response limit"),
            "{output:?}"
        );
    }
}

#[test]
fn native_profiles_honor_short_cli_deadlines_without_implicit_watchdog_conflicts() {
    for streaming in [false, true] {
        let (fixture, listener) = Fixture::native(5);
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let done = Arc::new(AtomicBool::new(false));
        let server_done = Arc::clone(&done);
        let server = thread::spawn(move || {
            while !server_done.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        read_request(&mut stream);
                        let mut byte = [0];
                        let _ = stream.read(&mut byte);
                        return true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept native request: {error}"),
                }
            }
            false
        });
        let mut command = fixture.command("model");
        command.args(["--timeout", "1"]);
        if streaming {
            command.arg("--stream");
        }
        let started = Instant::now();
        let output = command.output().expect("request bounded native profile");
        let elapsed = started.elapsed();
        done.store(true, Ordering::Release);
        assert!(server.join().expect("native server"), "{output:?}");
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("timed out"),
            "{output:?}"
        );
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    }
}

#[test]
fn native_profiles_reject_inapplicable_watchdogs_and_invalid_limits_before_contact() {
    for (arguments, expected_error) in [
        (["--slot-timeout", "1"], "cannot be used with"),
        (["--ttft-timeout", "180"], "cannot be used with"),
        (["--timeout", "0"], "--timeout"),
        (["--timeout", "86401"], "--timeout"),
        (["--max-response-bytes", "0"], "response limit"),
        (["--max-response-bytes", "16777217"], "response limit"),
    ] {
        let (fixture, listener) = Fixture::native(1);
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let output = fixture
            .command("model")
            .args(arguments)
            .output()
            .expect("reject unsupported overrides");
        assert!(!output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "{arguments:?}: {output:?}"
        );
        let error = listener
            .accept()
            .expect_err("must reject before provider contact");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    }
}
