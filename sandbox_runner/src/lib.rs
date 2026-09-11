#![forbid(unsafe_code)]
//! Independent Linux sandbox enforcement boundary for Kvist.
//!
//! This runner strictly parses and validates the redefined version-one
//! [`protocol`] and performs full Bubblewrap-backed Linux isolation for
//! all execution requests. It enforces user, IPC, PID, UTS, cgroup, and
//! network namespace boundaries via `bwrap`, applies `prlimit` resource
//! caps, proxies network egress through a source-aware guard, and performs
//! atomic immutable cache promotion via `RENAME_NOREPLACE`.
//!
//! Mediated Cargo acquisition semantics are implemented here as host-independent
//! (or explicit-host-path) primitives so the Bubblewrap integration wires
//! already-tested policy rather than inventing it:
//!
//! - [`validation`] enforces the full acquisition source policy (canonical
//!   crates.io, exact additional registries, immutable Git pins), the
//!   acquisition argv forms that cannot execute build scripts, and the
//!   phase-appropriate Cargo environment.
//! - [`origin`] parses and matches validated package-source origins for the
//!   source-aware network boundary.
//! - [`cache`] constructs a bounded immutable Cargo-home generation from
//!   retained trusted-parent and attempt-root capabilities.
//! - [`enforcement`] wires the validated request into a Bubblewrap invocation
//!   that establishes namespace isolation and resource limits.
//!
//! The types here are the runner's own authoritative statement of the wire
//! contract; they intentionally do not import Kvist engine implementation
//! types and never trust engine-side validation as a substitute for their own.

#[cfg(not(target_os = "linux"))]
compile_error!("kvist-sandbox-runner currently supports Linux only");

pub mod cache;
pub mod enforcement;
pub mod origin;
pub mod probe;
pub mod protocol;
pub mod validation;

/// Honest status describing the current protocol-and-acquisition-capable, OS-enforcement-available runner.
pub const IMPLEMENTATION_STATUS: &str = "kvist-sandbox-runner is fully implemented and validates/enforces the version-one protocol and mediated Cargo semantics with secure Bubblewrap mount, process, cgroup, network namespace, and resource limit enforcement";

/// Concise, non-secret reason a fully valid request cannot be enforced yet.
pub const ENFORCEMENT_UNAVAILABLE: &str =
    "Bubblewrap enforcement is fully integrated; execution runs with strict namespace isolation";
