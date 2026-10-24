use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const MAX_DIAGNOSTIC: usize = 4096;
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(5);

struct Launcher {
    child: Child,
    finished: bool,
}

impl Launcher {
    fn stop(&mut self) -> io::Result<()> {
        match self.child.kill() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("cannot stop browser launcher: {error}"),
                ));
            }
        }
        self.child.wait().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot reap browser launcher: {error}"),
            )
        })?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = self.stop()
        {
            tracing::error!("Failed to clean up browser launcher: {error}");
        }
    }
}

/// Explicit user action only; no shell or inherited viewer terminal streams.
pub(crate) fn open(url: &str) -> io::Result<String> {
    let (mut reader, writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    let child = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(writer.try_clone()?)))
        .stderr(Stdio::from(OwnedFd::from(writer)))
        .spawn()?;
    let mut launcher = Launcher {
        child,
        finished: false,
    };
    let mut diagnostic = Vec::new();
    let mut truncated = false;
    let status = wait_for_exit(
        &mut launcher,
        &mut reader,
        LAUNCH_TIMEOUT,
        &mut diagnostic,
        &mut truncated,
    );
    let text = String::from_utf8_lossy(&diagnostic);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let omission = if truncated { " [output truncated]" } else { "" };
    let detail = if text.is_empty() {
        String::new()
    } else {
        format!(": {text}")
    };
    let status = status
        .map_err(|error| io::Error::new(error.kind(), format!("{error}{omission}{detail}")))?;
    if status.success() {
        Ok(format!("Opened in browser{omission}{detail}"))
    } else {
        Err(io::Error::other(format!(
            "browser launcher exited with {status}{omission}{detail}"
        )))
    }
}

fn wait_for_exit(
    launcher: &mut Launcher,
    reader: &mut UnixStream,
    timeout: Duration,
    diagnostic: &mut Vec<u8>,
    truncated: &mut bool,
) -> io::Result<ExitStatus> {
    let started = Instant::now();
    loop {
        drain(reader, diagnostic, truncated)?;
        if let Some(status) = launcher.child.try_wait()? {
            launcher.finished = true;
            // A launched browser may keep the socket open. Never wait for EOF.
            drain(reader, diagnostic, truncated)?;
            return Ok(status);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            launcher.stop()?;
            drain(reader, diagnostic, truncated)?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "browser launcher timed out after {} seconds; launcher stopped",
                    timeout.as_secs_f64()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(10).min(remaining));
    }
}

fn drain(reader: &mut UnixStream, output: &mut Vec<u8>, truncated: &mut bool) -> io::Result<()> {
    let mut buffer = [0; 4096];
    for _ in 0..16 {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let retained = count.min(MAX_DIAGNOSTIC.saturating_sub(output.len()));
                output.extend_from_slice(&buffer[..retained]);
                *truncated |= retained < count;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn diagnostic_reads_are_bounded_and_do_not_require_eof() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        writer.write_all(&[b'x'; MAX_DIAGNOSTIC + 100]).unwrap();
        let mut output = Vec::new();
        let mut truncated = false;
        drain(&mut reader, &mut output, &mut truncated).unwrap();
        assert_eq!(output.len(), MAX_DIAGNOSTIC);
        assert!(truncated);
        drain(&mut reader, &mut output, &mut truncated).unwrap();
        assert_eq!(output.len(), MAX_DIAGNOSTIC);
        drop(writer);
    }

    fn test_launcher(script: &str) -> (Launcher, UnixStream, UnixStream) {
        let (reader, writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let held_writer = writer.try_clone().unwrap();
        let child = Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::from(OwnedFd::from(writer.try_clone().unwrap())))
            .stderr(Stdio::from(OwnedFd::from(writer)))
            .spawn()
            .unwrap();
        (
            Launcher {
                child,
                finished: false,
            },
            reader,
            held_writer,
        )
    }

    #[test]
    fn launcher_timeout_is_typed_bounded_and_reaps_the_owned_child() {
        let (mut launcher, mut reader, _held_writer) =
            test_launcher("printf 'launch hint\\n'; exec sleep 60");
        let pid = nix::unistd::Pid::from_raw(launcher.child.id().try_into().unwrap());
        let mut output = Vec::new();
        let mut truncated = false;
        let timeout = Duration::from_millis(30);
        let start = std::time::Instant::now();
        let error = wait_for_exit(
            &mut launcher,
            &mut reader,
            timeout,
            &mut output,
            &mut truncated,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() >= timeout);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(launcher.finished);
        assert_eq!(
            nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
            Err(nix::errno::Errno::ECHILD)
        );
        assert_eq!(output, b"launch hint\n");
    }

    #[test]
    fn flooding_launcher_cannot_extend_the_deadline_or_capture_limit() {
        let (mut launcher, mut reader, _held_writer) =
            test_launcher("while :; do printf 'diagnostic output diagnostic output\\n'; done");
        let mut output = Vec::new();
        let mut truncated = false;
        let start = std::time::Instant::now();
        let error = wait_for_exit(
            &mut launcher,
            &mut reader,
            Duration::from_millis(50),
            &mut output,
            &mut truncated,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(output.len(), MAX_DIAGNOSTIC);
        assert!(truncated);
        assert!(launcher.finished);
    }

    #[test]
    fn completed_launcher_does_not_wait_for_an_open_capture_writer() {
        let (mut launcher, mut reader, _still_open) = test_launcher("exit 7");
        let mut output = Vec::new();
        let mut truncated = false;
        let status = wait_for_exit(
            &mut launcher,
            &mut reader,
            Duration::from_secs(1),
            &mut output,
            &mut truncated,
        )
        .unwrap();
        assert_eq!(status.code(), Some(7));
        assert!(launcher.finished);
    }
}
