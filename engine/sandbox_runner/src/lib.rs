#![forbid(unsafe_code)]
//! Independent Linux sandbox enforcement boundary for Kvist.
//!
//! This revision is a *protocol-capable, enforcement-unavailable* intermediate
//! state. The runner strictly parses and validates the redefined version-one
//! [`protocol`] and rejects every superseded ("legacy") request shape with an
//! actionable diagnostic, but it does not yet perform Bubblewrap-backed Linux
//! isolation. Any valid execution request therefore fails closed: the runner
//! never falls back to unconstrained host execution and never reports a request
//! as enforced.
//!
//! The types here are the runner's own authoritative statement of the wire
//! contract; they intentionally do not import Kvist engine implementation
//! types and never trust engine-side validation as a substitute for their own.

#[cfg(not(target_os = "linux"))]
compile_error!("kvist-sandbox-runner currently supports Linux only");

pub mod probe;
pub mod protocol;
pub mod validation;

/// Honest status describing the current protocol-capable, enforcement-unavailable runner.
pub const IMPLEMENTATION_STATUS: &str = "kvist-sandbox-runner validates the version-one protocol but Bubblewrap enforcement is not implemented; execution requests fail closed";

/// Concise, non-secret reason a fully valid request cannot be enforced yet.
pub const ENFORCEMENT_UNAVAILABLE: &str =
    "Bubblewrap enforcement is not yet integrated; the runner refuses to execute without isolation";
