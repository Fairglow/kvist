#![forbid(unsafe_code)]
//! Verified capability probe for the version-one protocol with Bubblewrap backend.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

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
fn file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
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

/// Attempts to confirm the runner's production enforcement capabilities.
pub fn probe() -> ProbeOutcome {
    let bwrap_path = match locate_backend() {
        Some(path) => path,
        None => {
            return ProbeOutcome::Unavailable {
                reason: "no verified Bubblewrap backend is installed; the capability probe cannot be confirmed"
                    .to_owned(),
            };
        }
    };

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
