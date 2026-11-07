#![forbid(unsafe_code)]
//! Verified capability probe for the version-one protocol with Bubblewrap backend.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::protocol::{
    BackendKind, BackendReference, Capabilities, ExecutableReference, Namespaces, PROBE_PROTOCOL,
    PROTOCOL_VERSION, SandboxProbe,
};

/// The outcome of a capability probe attempt.
pub enum ProbeOutcome {
    /// The backend was located and every capability confirmed.
    Confirmed(SandboxProbe),
    /// The probe could not confirm production capabilities.
    Unavailable {
        /// A specific, non-secret explanation.
        reason: String,
    },
}

/// Helper to compute the SHA256 digest of a file.
pub(crate) fn file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut file = file.take(length);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn qualify_backend(backend: &Path) -> Result<(), String> {
    let directory = crate::enforcement::source::pin(Path::new("/usr"))?;
    let shell = Path::new("/bin/sh").canonicalize().map_err(|error| {
        format!("cannot locate the system shell for namespace qualification: {error}")
    })?;
    let executable = crate::enforcement::source::pin(&shell)?;
    let writable_bind = nix::unistd::dup(&directory)
        .map_err(|error| format!("cannot prepare descriptor-bind qualification: {error}"))?;
    for descriptor in [&directory, &executable] {
        nix::fcntl::fcntl(
            descriptor,
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        )
        .map_err(|error| format!("cannot pass qualification source descriptors: {error}"))?;
    }
    let mut args = [
        "--clearenv",
        "--unshare-all",
        "--new-session",
        "--die-with-parent",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
    ]
    .map(str::to_owned)
    .to_vec();
    crate::enforcement::system_layout::add_host_system_layout(&mut args)?;
    args.extend([
        "--ro-bind-fd".into(), directory.as_raw_fd().to_string(), "/usr".into(),
        "--ro-bind-fd".into(), executable.as_raw_fd().to_string(), "/probe-command".into(),
        "--bind-fd".into(), writable_bind.as_raw_fd().to_string(), "/probe-source".into(),
        "--remount-ro".into(), "/probe-source".into(),
        "--cap-drop".into(), "ALL".into(), "--".into(), "/probe-command".into(), "-c".into(),
        format!("test ! -e /proc/self/fd/{} && test ! -e /proc/self/fd/{} && test ! -e /proc/self/fd/{}", directory.as_raw_fd(), executable.as_raw_fd(), writable_bind.as_raw_fd()),
    ]);
    let mut child = Command::new(backend)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| {
            format!("cannot start namespace/descriptor-bind qualification: {error}")
        })?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            result => {
                let pid = nix::unistd::Pid::from_raw(-(child.id() as i32));
                let killed = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
                let waited = child.wait();
                return Err(format!(
                    "namespace/descriptor-bind qualification did not complete within its supervised budget ({result:?}; cleanup: {killed:?}, {waited:?}); repair the installed backend or namespace permissions and retry"
                ));
            }
        }
    };
    if status.success() {
        return Ok(());
    }
    let mut diagnostic = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        nix::fcntl::fcntl(
            &stderr,
            nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
        )
        .map_err(|error| format!("cannot bound backend qualification diagnostic reads: {error}"))?;
        match stderr.take(4096).read_to_end(&mut diagnostic) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => {
                return Err(format!(
                    "cannot read backend qualification diagnostic: {error}"
                ));
            }
        }
    }
    Err(format!(
        "native namespace/descriptor-bind qualification failed ({status}): {}; install Bubblewrap supporting --bind-fd/--ro-bind-fd and ensure namespaces are permitted; no unconfined fallback is used",
        String::from_utf8_lossy(&diagnostic)
    ))
}

/// Attempts to confirm the runner's production enforcement capabilities.
pub fn probe() -> ProbeOutcome {
    if let Err(reason) = crate::context::require_host() {
        return ProbeOutcome::Unavailable { reason };
    }

    let bwrap_path = match locate_backend() {
        Some(path) => path,
        None => {
            return ProbeOutcome::Unavailable {
                reason: "no verified Bubblewrap backend is installed; the capability probe cannot be confirmed"
                    .to_owned(),
            };
        }
    };
    if let Err(reason) = qualify_backend(&bwrap_path) {
        return ProbeOutcome::Unavailable { reason };
    }
    // Calculate the SHA256 of the bwrap executable
    let bwrap_digest = match file_sha256(&bwrap_path) {
        Ok(digest) => digest,
        Err(error) => {
            return ProbeOutcome::Unavailable {
                reason: format!("failed to read Bubblewrap binary to compute digest: {error}"),
            };
        }
    };

    // Locate and hash the current runner executable itself
    let runner_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return ProbeOutcome::Unavailable {
                reason: format!("failed to resolve current runner executable path: {error}"),
            };
        }
    };

    let runner_digest = match file_sha256(&runner_path) {
        Ok(digest) => digest,
        Err(error) => {
            return ProbeOutcome::Unavailable {
                reason: format!("failed to read current runner binary to compute digest: {error}"),
            };
        }
    };

    // Determine namespace availability on the host kernel
    // We check existence of /proc/self/ns/{mnt,net,pid,ipc,uts,user}
    let namespaces = Namespaces {
        mount: Path::new("/proc/self/ns/mnt").exists(),
        network: Path::new("/proc/self/ns/net").exists(),
        pid: Path::new("/proc/self/ns/pid").exists(),
        ipc: Path::new("/proc/self/ns/ipc").exists(),
        uts: Path::new("/proc/self/ns/uts").exists(),
        user: Path::new("/proc/self/ns/user").exists(),
    };

    let capabilities = Capabilities {
        namespaces,
        new_session: true,
        parent_death_signal: true,
    };

    let response = SandboxProbe {
        protocol: PROBE_PROTOCOL.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        runner: ExecutableReference {
            path: runner_path.to_string_lossy().into_owned(),
            digest: runner_digest,
        },
        backend: BackendReference {
            kind: BackendKind::Bubblewrap,
            path: bwrap_path.to_string_lossy().into_owned(),
            digest: bwrap_digest,
        },
        capabilities,
    };

    ProbeOutcome::Confirmed(response)
}

/// Locates a candidate Bubblewrap executable without executing it.
pub(crate) fn locate_backend() -> Option<PathBuf> {
    let direct = ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"];
    for candidate in direct {
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    let raw = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&raw) {
        let candidate = directory.join("bwrap");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
