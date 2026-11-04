//! Real-terminal entry, completion, child output and component-history regression.
#![cfg(unix)]

use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{Winsize, openpty};
use nix::unistd::{read, write};

struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pump(master: &impl AsFd, output: &mut Vec<u8>) {
    let mut buffer = [0; 65536];
    loop {
        match read(master, &mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(nix::errno::Errno::EAGAIN | nix::errno::Errno::EIO) => break,
            Err(error) => panic!("read terminal: {error}"),
        }
    }
}

fn wait_for(master: &impl AsFd, output: &mut Vec<u8>, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(master, output);
        if predicate() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal timeout:\n{}",
            String::from_utf8_lossy(output)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn transcript(path: &std::path::Path) -> String {
    match std::fs::read_to_string(path.join(".kvist/transcript/00000000000000000000.md")) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => panic!("read transcript: {error}"),
    }
}

fn escape(master: &impl AsFd, levels: usize) {
    for _ in 0..levels {
        write(master, b"\x1b").unwrap();
        std::thread::sleep(Duration::from_millis(60));
    }
}

#[test]
fn bare_kvist_hosts_completion_reports_and_restorable_component_history() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new(env!("CARGO_BIN_EXE_kvist"))
            .arg("init")
            .current_dir(dir.path())
            .output()
            .unwrap()
            .status
            .success()
    );
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
    let stub = dir.path().join("stub-skott");
    std::fs::write(&stub, "#!/bin/sh\nprintf '%s\\n' '{\"report\":\"# Agent report\\n\\n**Readable** output.\\n\\n| Key | Value |\\n|---|---|\\n| mode | test |\",\"decision_required\":false}'\nread answer || :\nprintf 'Accepted default\\n'\nprintf '\\033]52;c;inert\\007' >&2\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
    let user_config = dir.path().join("user-config/kvist");
    std::fs::create_dir_all(&user_config).unwrap();
    std::fs::write(
        user_config.join("theme.toml"),
        "name = \"fixture-user\"\nmode = \"dark\"\n[colors]\nagent_background = \"#123456\"\n",
    )
    .unwrap();
    std::fs::write(user_config.join("theme"), "fixture-user").unwrap();
    let config = dir.path().join("kvist.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::create_dir(dir.path().join(".kvist")).unwrap();
    std::fs::write(dir.path().join(".kvist/providers.toml"),
        "schema_version = 1\n[profiles.fixture-a.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"fixture-a\"\n\
        [profiles.fixture-b.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"fixture-b\"\n").unwrap();
    std::fs::write(
        config,
        format!(
            "{text}\n[agent.registry]\npath = \".kvist/providers.toml\"\n\
        [agent.roles.developer]\nprofile = \"fixture-a\"\n\
        [agent.roles.architect]\nprofile = \"fixture-a\"\n\
        [agent.roles.security-reviewer]\nprofile = \"fixture-a\"\n"
        ),
    )
    .unwrap();
    assert!(
        Command::new(env!("CARGO_BIN_EXE_kvist"))
            .args(["component", "accept", "."])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    let size = Winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = openpty(Some(&size), None).unwrap();
    let flags = fcntl(&pty.master, FcntlArg::F_GETFL).unwrap();
    fcntl(
        &pty.master,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .unwrap();
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_kvist"))
            .current_dir(dir.path())
            .env_remove("KVIST_THEME")
            .env("KVIST_LOG", "debug")
            .env("TERM", "xterm-256color")
            .env("KVIST_SKOTT", &stub)
            .env("XDG_CONFIG_HOME", dir.path().join("user-config"))
            .env("XDG_STATE_HOME", dir.path().join("user-state"))
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR")
            .env_remove("CLICOLOR_FORCE")
            .env_remove("KVIST_CONFIG_PATH")
            .stdin(Stdio::from(pty.slave.try_clone().unwrap()))
            .stdout(Stdio::from(pty.slave.try_clone().unwrap()))
            .stderr(Stdio::from(pty.slave))
            .spawn()
            .unwrap(),
    );
    let mut output = Vec::new();
    wait_for(&pty.master, &mut output, || {
        transcript(dir.path()).contains("# Kvist workspace")
    });
    wait_for(&pty.master, &mut output, || {
        transcript(dir.path()).contains("### Host diagnostics")
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(&pty.master, &mut output);
        if String::from_utf8_lossy(&output).contains("48;2;18;52;86") {
            break;
        }
        assert!(Instant::now() < deadline, "XDG user theme was not loaded");
        std::thread::sleep(Duration::from_millis(20));
    }
    write(&pty.master, b"cd .\r").unwrap();
    let component = dir.path().join("src");
    wait_for(&pty.master, &mut output, || {
        transcript(&component).contains("# Focus:")
    });
    write(&pty.master, b"task ne\t").unwrap();
    std::thread::sleep(Duration::from_millis(150));
    write(&pty.master, b"\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).contains("no ready task")
    });
    wait_for(&pty.master, &mut output, || {
        transcript(&component).contains("Command finished")
    });
    write(&pty.master, b"prompt \"fixture markdown\"\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).contains("# Agent report")
    });
    write(&pty.master, b"\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).matches("Command finished").count() >= 2
    });
    // Theme, model and effort use the actual menu dispatch, not a test shortcut.
    escape(&pty.master, 1);
    write(&pty.master, b"\x1b[B\x1b[B\x1b[B\r\r\x1b[H\x1b[B\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        std::fs::read_to_string(dir.path().join(".kvist/theme"))
            .is_ok_and(|text| text.trim() == "light")
    });
    escape(&pty.master, 2);
    write(&pty.master, b"\x1bOQ\x1b[B\r\r\r\x1b[B\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).matches("Command finished").count() >= 3
    });
    escape(&pty.master, 3);
    write(
        &pty.master,
        b"\x1bOQ\x1b[B\r\r\x1b[B\r\x1b[B\x1b[B\x1b[B\x1b[B\x1b[B\r",
    )
    .unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).matches("Command finished").count() >= 4
    });
    assert!(
        std::fs::read_to_string(dir.path().join("kvist.toml"))
            .unwrap()
            .contains("thinking_effort = \"high\"")
    );
    assert!(
        std::fs::read_to_string(dir.path().join("kvist.toml"))
            .unwrap()
            .contains("profile = \"fixture-b\"")
    );
    escape(&pty.master, 3);
    write(&pty.master, b"prompt --file -\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).contains("prompt --file -")
    });
    write(&pty.master, b"stdin prompt body\r\x04").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).matches("Command finished").count() >= 5
    });
    assert!(
        !std::fs::read_to_string(component.join(".kvist/transcript/commands.json"))
            .unwrap()
            .contains("stdin prompt body")
    );
    write(&pty.master, b"status\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).matches("Command finished").count() >= 6
    });
    assert!(transcript(&component).contains("## Component: .\n"));
    assert!(
        std::fs::read_to_string(component.join(".kvist/transcript/commands.json"))
            .unwrap()
            .contains("\"status .\"")
    );
    write(&pty.master, b"cd\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(dir.path()).contains("# Focus: (project)")
    });
    write(&pty.master, b"status\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(dir.path()).contains("| Component | State | Artifacts | Tasks | Next action |")
    });
    wait_for(&pty.master, &mut output, || {
        transcript(dir.path()).contains("Command finished")
    });
    write(&pty.master, b"cd .\r").unwrap();
    wait_for(&pty.master, &mut output, || {
        transcript(&component).matches("# Focus:").count() >= 2
    });
    write(&pty.master, b"\x1b[5~").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    write(&pty.master, b"\x1b[1;5F").unwrap();
    escape(&pty.master, 1);
    write(&pty.master, b"\x1b[F\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        pump(&pty.master, &mut output);
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "workspace did not exit:\n{}",
            String::from_utf8_lossy(&output[output.len().saturating_sub(12_000)..])
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let stored = transcript(&component);
    assert!(stored.contains("task next ."), "{stored}");
    assert!(
        stored.contains("**Readable** output.") && stored.contains("| Key | Value |"),
        "{stored}"
    );
    assert!(
        !stored.contains("\"report\":") && !stored.contains('\x1b'),
        "{stored}"
    );
    assert!(stored.contains("Accepted default"));
    assert!(stored.contains("answer not retained"));
    assert!(
        output
            .windows(b"\x1b[?1049h".len())
            .any(|bytes| bytes == b"\x1b[?1049h")
    );
    assert!(
        output
            .windows(b"\x1b[?1049l".len())
            .any(|bytes| bytes == b"\x1b[?1049l")
    );
}
