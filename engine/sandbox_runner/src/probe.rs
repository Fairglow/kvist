//! Fail-closed capability probe for the redefined version-one protocol.
//!
//! The probe *models* the canonical [`crate::protocol::SandboxProbe`] response
//! shape, but production capability confirmation depends on a verified
//! Bubblewrap backend and kernel namespace support that this intermediate
//! revision does not yet integrate. The probe therefore reports why it cannot
//! be confirmed and never emits a success response that would let the engine
//! treat an unenforced runner as ready.

use std::path::{Path, PathBuf};

/// The outcome of a capability probe attempt.
pub enum ProbeOutcome {
    /// The backend was located and every capability confirmed.
    ///
    /// This variant is reserved for the Bubblewrap integration work and is not
    /// produced by the current enforcement-unavailable revision.
    Confirmed(crate::protocol::SandboxProbe),
    /// The probe could not confirm production capabilities.
    Unavailable {
        /// A specific, non-secret explanation.
        reason: String,
    },
}

/// Attempts to confirm the runner's production enforcement capabilities.
///
/// The current revision locates the Bubblewrap backend for an actionable
/// diagnostic but always fails closed because namespace and resource
/// enforcement are not yet integrated.
pub fn probe() -> ProbeOutcome {
    match locate_backend() {
        Some(path) => ProbeOutcome::Unavailable {
            reason: format!(
                "Bubblewrap backend found at {} but namespace and resource enforcement are not yet integrated; the capability probe cannot be confirmed",
                path.display()
            ),
        },
        None => ProbeOutcome::Unavailable {
            reason: "no verified Bubblewrap backend is installed; the capability probe cannot be confirmed"
                .to_owned(),
        },
    }
}

/// Locates a candidate Bubblewrap executable without executing it.
fn locate_backend() -> Option<PathBuf> {
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
