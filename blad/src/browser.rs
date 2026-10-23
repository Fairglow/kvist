use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const MAX_DIAGNOSTIC: usize = 4096;

struct Launcher {
    child: Child,
    finished: bool,
}

impl Drop for Launcher {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = self.child.kill()
                && error.kind() != io::ErrorKind::InvalidInput
            {
                eprintln!("Failed to stop browser launcher: {error}");
            }
            if let Err(error) = self.child.wait() {
                eprintln!("Failed to reap browser launcher: {error}");
            }
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
    let status = loop {
        drain(&mut reader, &mut diagnostic, &mut truncated)?;
        if let Some(status) = launcher.child.try_wait()? {
            launcher.finished = true;
            // A launched browser may keep the socket open. Never wait for EOF.
            drain(&mut reader, &mut diagnostic, &mut truncated)?;
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let text = String::from_utf8_lossy(&diagnostic);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let omission = if truncated { " [output truncated]" } else { "" };
    let detail = if text.is_empty() {
        String::new()
    } else {
        format!(": {text}")
    };
    if status.success() {
        Ok(format!("Opened in browser{omission}{detail}"))
    } else {
        Err(io::Error::other(format!(
            "browser launcher exited with {status}{omission}{detail}"
        )))
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
}
