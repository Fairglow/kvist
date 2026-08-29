//! Real-time prompt supervisor with idle watchdog and loop detection.

use std::{
    io::Write,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use crate::{KvistError, Result};

enum StreamType {
    Stdout,
    Stderr,
}

enum SupervisionEvent {
    ProcessExited(ExitStatus),
    IdleTimeout,
    LoopDetected,
}

/// Runs a command under real-time supervision, supporting idle timeouts, loop detection, and automatic restarts.
pub fn run_supervised_prompt(
    program: &str,
    args: &[String],
    idle_timeout: u64,
    detect_loops: bool,
    max_restarts: u32,
) -> Result<()> {
    let mut restarts = 0;
    loop {
        if restarts > 0 {
            println!(
                "\n[SUPERVISOR INFO] Restarting prompt execution (attempt {}/{})...",
                restarts + 1,
                max_restarts + 1
            );
        } else {
            println!("[SUPERVISOR INFO] Executing custom prompt under supervision...");
        }

        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| KvistError::Io {
                operation: "spawn prompt process",
                path: PathBuf::from(program),
                source,
            })?;

        let event = supervise_process(&mut child, idle_timeout, detect_loops)?;
        match event {
            SupervisionEvent::ProcessExited(status) => {
                if status.success() {
                    break;
                } else {
                    return Err(KvistError::ImportFailed {
                        reason: format!("prompt command failed with exit status: {status}"),
                    });
                }
            }
            SupervisionEvent::IdleTimeout => {
                println!(
                    "\n[SUPERVISOR WARNING] Idle timeout of {idle_timeout} seconds exceeded with no output. Terminating..."
                );
                let _ = child.kill();
                let _ = child.wait();
                if restarts < max_restarts {
                    restarts += 1;
                    continue;
                } else {
                    return Err(KvistError::ImportFailed {
                        reason: "maximum automatic restarts exceeded due to idle timeout"
                            .to_owned(),
                    });
                }
            }
            SupervisionEvent::LoopDetected => {
                println!(
                    "\n[SUPERVISOR WARNING] Repetition loop detected in output! Terminating..."
                );
                let _ = child.kill();
                let _ = child.wait();
                if restarts < max_restarts {
                    restarts += 1;
                    continue;
                } else {
                    return Err(KvistError::ImportFailed {
                        reason: "maximum automatic restarts exceeded due to repetition loop"
                            .to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn supervise_process(
    child: &mut std::process::Child,
    idle_timeout: u64,
    detect_loops: bool,
) -> Result<SupervisionEvent> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Spawn stdout forwarding thread
    let tx_out = tx.clone();
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| KvistError::ImportFailed {
            reason: "failed to capture child stdout".to_owned(),
        })?;
    std::thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while let Ok(count) = std::io::Read::read(&mut stdout, &mut buffer) {
            if count == 0 {
                break;
            }
            if tx_out
                .send((StreamType::Stdout, buffer[..count].to_vec()))
                .is_err()
            {
                break;
            }
        }
    });

    // Spawn stderr forwarding thread
    let tx_err = tx;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| KvistError::ImportFailed {
            reason: "failed to capture child stderr".to_owned(),
        })?;
    std::thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while let Ok(count) = std::io::Read::read(&mut stderr, &mut buffer) {
            if count == 0 {
                break;
            }
            if tx_err
                .send((StreamType::Stderr, buffer[..count].to_vec()))
                .is_err()
            {
                break;
            }
        }
    });

    let mut last_output = Instant::now();
    let mut rolling_buffer = String::new();
    let idle_timeout_dur = Duration::from_secs(idle_timeout);

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok((stream_type, data)) => {
                last_output = Instant::now();

                match stream_type {
                    StreamType::Stdout => {
                        std::io::stdout()
                            .write_all(&data)
                            .map_err(|source| KvistError::Io {
                                operation: "write prompt stdout to console",
                                path: PathBuf::from("stdout"),
                                source,
                            })?;
                        let _ = std::io::stdout().flush();

                        if detect_loops {
                            if let Ok(text) = std::str::from_utf8(&data) {
                                rolling_buffer.push_str(text);
                                if rolling_buffer.len() > 4096 {
                                    let skip = rolling_buffer.len() - 4096;
                                    let mut idx = skip;
                                    while !rolling_buffer.is_char_boundary(idx) {
                                        idx += 1;
                                    }
                                    rolling_buffer = rolling_buffer[idx..].to_owned();
                                }

                                if check_for_loops(&rolling_buffer) {
                                    return Ok(SupervisionEvent::LoopDetected);
                                }
                            }
                        }
                    }
                    StreamType::Stderr => {
                        std::io::stderr()
                            .write_all(&data)
                            .map_err(|source| KvistError::Io {
                                operation: "write prompt stderr to console",
                                path: PathBuf::from("stderr"),
                                source,
                            })?;
                        let _ = std::io::stderr().flush();
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Check idle timeout
                if last_output.elapsed() > idle_timeout_dur {
                    return Ok(SupervisionEvent::IdleTimeout);
                }

                // Check if process has exited
                if let Ok(Some(status)) = child.try_wait() {
                    // Drain any remaining items in channel
                    while let Ok((stream_type, data)) = rx.try_recv() {
                        match stream_type {
                            StreamType::Stdout => {
                                let _ = std::io::stdout().write_all(&data);
                            }
                            StreamType::Stderr => {
                                let _ = std::io::stderr().write_all(&data);
                            }
                        }
                    }
                    let _ = std::io::stdout().flush();
                    let _ = std::io::stderr().flush();
                    return Ok(SupervisionEvent::ProcessExited(status));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Thread disconnected, wait for exit
                let status = child.wait().map_err(|source| KvistError::Io {
                    operation: "wait for prompt process exit",
                    path: PathBuf::from("process"),
                    source,
                })?;
                return Ok(SupervisionEvent::ProcessExited(status));
            }
        }
    }
}

fn check_for_loops(buffer: &str) -> bool {
    let bytes = buffer.as_bytes();
    let len = bytes.len();

    // 1. Cycle-pattern repetition loop detection (cycles from 10 to 512 bytes)
    for l in 10..=512 {
        if len >= l * 3 {
            let cycle1 = &bytes[len - l..];
            let cycle2 = &bytes[len - 2 * l..len - l];
            let cycle3 = &bytes[len - 3 * l..len - 2 * l];
            if cycle1 == cycle2 && cycle2 == cycle3 {
                return true;
            }
        }
    }

    // 2. Line-pattern repetition loop detection (single line repeating >= 4 times)
    let lines: Vec<&str> = buffer
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();

    if lines.len() >= 4 {
        let last_idx = lines.len() - 1;
        let last_line = lines[last_idx];
        if lines[last_idx - 1] == last_line
            && lines[last_idx - 2] == last_line
            && lines[last_idx - 3] == last_line
        {
            return true;
        }
    }

    // 3. Dual-line oscillation pattern detection (A-B-A-B-A-B)
    if lines.len() >= 6 {
        let last_idx = lines.len() - 1;
        let line_a = lines[last_idx];
        let line_b = lines[last_idx - 1];
        if lines[last_idx - 2] == line_a
            && lines[last_idx - 3] == line_b
            && lines[last_idx - 4] == line_a
            && lines[last_idx - 5] == line_b
        {
            return true;
        }
    }

    false
}
