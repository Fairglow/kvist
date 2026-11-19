//! Explicit, versioned session mode for the Kvist sandbox runner.
//!
//! The session mode is a separate, explicit protocol mode beside the one-shot
//! v1 request. It reads bounded launch metadata on a dedicated descriptor
//! (never through argv), reserves the payload's stdio for ACP, optionally
//! grants an explicitly bounded IPC capability for the inference relay, and
//! owns and reaps the whole process group on every outcome. The embedding
//! host (Skott) holds the session and cancellation capability.
//!
//! The inference relay crosses the boundary only through the explicitly
//! granted IPC capability: a run-private relay on sandbox loopback, with no
//! host loopback sharing, general HTTP proxy, host socket directory, or host
//! filesystem grant. One-shot v1 behavior for existing tools and verification
//! is preserved unchanged.
//!
//! ## Compliance review: session mode (`session-mode-compliance-review`)
//!
//! Separate source-blind comparison of the session-mode requirement
//! (`REQUIREMENTS.md#SR-REQ-SESSION-MODE`), the contract (`CONTRACT.md`,
//! "Provided interfaces") and the design (`DESIGN.md`, "Versioned session
//! mode") against the independent audit observations in
//! `tests/security_audit_session_mode.rs` (28 scoped tests, own `audit-`
//! prefixed fixtures) and the attributed test regressions in
//! `tests/session_mode.rs` (25 tests). No implementation source is read, no
//! tests are run, and no acceptance, certification or receipt is made.
//!
//! ### Correspondence (per requirement clause)
//!
//! - *Separate, explicit, versioned session mode beside one-shot v1:* the
//!   contract, the design and both attributed test files name the exact
//!   `--kvist-sandbox-session-v1` identifier; `session_mode.rs` includes
//!   executable-boundary regressions for that argument and a legacy
//!   regression rejecting one-shot v1 metadata as session metadata.
//! - *Bounded launch metadata on a dedicated descriptor, not argv:* the
//!   1 MiB bound is exercised by both files; a real dedicated-descriptor
//!   handoff is observable only through the executable
//!   (`executable_session_mode_fails_closed_without_session_metadata`
//!   asserts the fail-closed branch).
//! - *Reserved stdio for ACP:* `StdioRole::ReservedForAcp` is asserted and
//!   `captured` (both files) and `inherited` (audit) are rejected.
//! - *Shared closed v1 namespace/grant/resource/identity contract:* no
//!   session-specific bypass is observed in either test file; the
//!   independent audit exercises the same closed shape with different
//!   fixtures.
//! - *Bounded session block:* the reserved stdio role, the optional IPC
//!   capability (grant accepted; host loopback, general HTTP proxy,
//!   unrelated routes and unknown kinds denied; absent by default) and the
//!   run-scoped cancellation capability (non-run-scoped and unknown scopes
//!   denied) are covered by both files.
//! - *Process group owned and reaped:* `owns_process_group` is asserted on
//!   success, payload failure, timeout and output-limit breach in both
//!   files, but as a library-level contract assertion, not an observed
//!   runtime reap.
//! - *Host holds the session and cancellation capability; runner
//!   lifecycle evidence distinguishable from payload output:* no evidence
//!   in either file (missing, not contradicted).
//! - *Relay only via the granted IPC capability, sandbox loopback only, no
//!   host loopback sharing, general proxy, socket directory or filesystem
//!   grant; carries inference only:* `grants_host_loopback` and
//!   `grants_general_egress` are asserted false (with and without a valid
//!   bounded relay grant); `host_socket_directory` is rejected as an
//!   unknown field; the audit additionally fixes the exact route
//!   `POST /v1/chat/completions`.
//! - *One-shot v1 behavior preserved unchanged:* the implementer report
//!   states it; no independent re-verification of the existing conformance
//!   surface for this increment is recorded.
//! - *Tests must cover:* launch-metadata bounds, reserved stdio, IPC
//!   grant/denial, cancellation, malformed/oversized/unknown and legacy
//!   rejection, and no host loopback or general egress are all present in
//!   both files; process-group reap is covered only as a contract
//!   assertion (see D-1).
//!
//! ### Retained discrepancies (for human arbitration)
//!
//! - **D-1 (contradictory evidence, retained):**
//!   `session_mode.rs::runner_reaps_the_process_group_on_success` carries
//!   the comment "the executable boundary test below exercises the real
//!   reap against a spawned payload," but no test in either file spawns a
//!   payload: the executable-boundary tests only run the runner itself
//!   (fail-closed status, empty stdout, and a stderr diagnostic in one
//!   case) without a session payload, and the four reap tests are
//!   library-level `owns_process_group` assertions. The comment is stale.
//!   Runtime
//!   process-group reap, timeout and output-limit outcomes therefore have
//!   no observed execution evidence in the retained records; the
//!   requirement clause is unverified, not contradicted by behavior.
//! - **D-2 (divergent expectations, retained):** the two test files expect
//!   different error classifications for adjacent protocol-identifier
//!   inputs: `session_mode.rs::rejects_malformed_session_launch_metadata`
//!   expects `SessionError::Malformed` for metadata *missing* the required
//!   protocol identifier, while the independent audit's
//!   `audit_launch_metadata_rejects_the_wrong_protocol_identifier` expects
//!   `SessionError::Invalid` with a `protocol must be` diagnostic for a
//!   *present but wrong* protocol identifier. The missing-identifier input
//!   has no retained execution in the independent records (the audit does
//!   not exercise it), so whether the missing case resolves to `Malformed`
//!   or to the `Invalid` protocol diagnostic is undetermined from the
//!   retained evidence.
//! - **Unverified (missing evidence, retained):** the
//!   `embedding host MUST hold the session and cancellation capability`
//!   clause and the `Runner lifecycle evidence MUST stay distinguishable
//!   from payload output` clause have no attributable evidence in either
//!   test file; no dedicated-descriptor execution, no relay transport or
//!   broker-boundary test, no host-loopback unreachability probe and no
//!   runtime cancellation fixture are recorded.
//!
//! ### Concrete evidence limits
//!
//! - Both attributed test files are library-surface (parse/validation of
//!   bounded JSON plus fail-closed executable invocations). No real model,
//!   network, Bubblewrap enforcement or spawned payload is exercised;
//!   enforcement of the session request reuses the shared v1 enforcement
//!   surface, whose evidence is separate and older than this increment.
//! - The security-audit execution evidence (`cargo test --offline --locked
//!   --test security_audit_session_mode` with 28 passed and 0 failed;
//!   rustfmt check clean) is attributed to the `session-mode-security-audit`
//!   agent run and is not independently re-run here. The write-tests phase
//!   recorded expected failures only (red phase); no green execution for
//!   the full `session_mode.rs` target is recorded in the retained
//!   independent records, and the environment-dependent executable
//!   regression
//!   `executable_rejects_session_metadata_passed_on_stdin_in_request_mode`
//!   is reported as failing where the `/.galla-sandbox` marker is present
//!   (nested-sandbox rejection, a pre-existing host guard) without a
//!   passing re-execution.
//! - `IMPL.md` contains no session-mode observation section; the
//!   independent records are linked through the queue (attempt files) and
//!   the audit test file, not through the component implementation record.
//! - The schema aids (`schema/*.json`) mirror the one-shot request and
//!   probe shapes only; no session schema aid is present.
//! - `DESIGN.md` describes the host-side broker (approved upstream,
//!   server-side model binding, `POST /v1/chat/completions` only); the
//!   broker is a Skott-side component outside this comparison, and its
//!   behavior is not covered by either Galla test file.
//! - This comparison is scoped to the session-mode increment. It is not
//!   component acceptance, not a security certification, not a runner-wide
//!   or one-shot-v1 revalidation, and it mints no receipt. D-1, D-2 and
//!   the unverified clauses above remain for human arbitration.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::protocol::{
    Cache, Grant, Identities, Network, PROTOCOL_VERSION, Phase, Resources, SandboxRequest, Scratch,
    Toolchain,
};
use crate::validation;

/// The exact accepted session protocol identifier.
pub const SESSION_PROTOCOL: &str = "kvist-sandbox-session-v1";

/// Maximum accepted session launch metadata size before parsing.
pub const MAX_SESSION_BYTES: usize = 1 << 20;

/// A precise, non-secret reason a session launch is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    /// The launch metadata exceeds the bounded size limit.
    TooLarge {
        /// The actual byte count.
        size: usize,
        /// The accepted byte limit.
        limit: usize,
    },
    /// The launch metadata is not well-formed JSON or has an unknown field.
    Malformed {
        /// A non-secret detail describing the shape problem.
        detail: String,
    },
    /// The launch metadata is well-formed but violates a protocol invariant.
    Invalid {
        /// A non-secret detail describing the violated invariant.
        detail: String,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { size, limit } => write!(
                formatter,
                "session launch metadata of {size} bytes exceeds the {limit}-byte protocol limit"
            ),
            Self::Malformed { detail } => {
                write!(
                    formatter,
                    "malformed version-one session launch metadata: {detail}"
                )
            }
            Self::Invalid { detail } => {
                write!(
                    formatter,
                    "invalid version-one session launch metadata: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for SessionError {}

fn invalid(detail: impl Into<String>) -> SessionError {
    SessionError::Invalid {
        detail: detail.into(),
    }
}

fn malformed(detail: impl Into<String>) -> SessionError {
    SessionError::Malformed {
        detail: detail.into(),
    }
}

/// The reserved stdio role for the session payload.
///
/// The session mode reserves the payload's stdio for ACP (Agent Communication
/// Protocol). No other stdio role is permitted: the runner owns the process
/// group and reaps it on every outcome, so the payload cannot capture or
/// redirect stdio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StdioRole {
    /// The payload's stdio is reserved for ACP.
    ReservedForAcp,
}

/// The run-scoped cancellation capability.
///
/// The cancellation capability is run-scoped: it applies to the current
/// session run only. A host-scoped cancellation capability would allow the
/// embedding host to cancel runs beyond the current session, which is not
/// permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cancellation {
    /// The cancellation capability is scoped to the current run.
    RunScoped,
}

/// The kind of IPC capability granted to the session.
///
/// Only the inference relay is permitted. A general HTTP proxy would open a
/// general egress path, which is not permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IpcKind {
    /// A run-private inference relay on sandbox loopback.
    InferenceRelay,
    /// A general HTTP proxy (rejected: opens a general egress path).
    GeneralHttpProxy,
}

/// The socket binding for the IPC capability.
///
/// Only sandbox loopback is permitted. Host loopback sharing would allow the
/// payload to reach host services, which is not permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IpcSocket {
    /// The relay binds to sandbox loopback (127.0.0.1 inside the sandbox).
    SandboxLoopback,
    /// The relay shares host loopback (rejected: allows host service access).
    HostLoopback,
}

/// An explicitly granted, bounded IPC capability for the inference relay.
///
/// The relay is a run-private relay on sandbox loopback. It crosses the
/// boundary only through this explicitly granted capability. It does not
/// share host loopback, act as a general HTTP proxy, or open a host socket
/// directory or filesystem grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcCapability {
    /// The kind of IPC capability.
    pub kind: IpcKind,
    /// The socket binding.
    pub socket: IpcSocket,
    /// The exact routes the relay may serve.
    pub routes: Vec<String>,
}

/// The bounded session block: reserved stdio, optional IPC capability, and
/// the run-scoped cancellation capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    /// The reserved stdio role.
    pub stdio: StdioRole,
    /// The optional IPC capability for the inference relay.
    pub ipc: Option<IpcCapability>,
    /// The run-scoped cancellation capability.
    pub cancellation: Cancellation,
}

/// The complete closed versioned session launch metadata.
///
/// This is the session-mode counterpart of [`SandboxRequest`]: the same closed
/// version-one namespace/grant/resource/identity contract plus a bounded
/// [`Session`] block. The session mode is a separate, explicit protocol mode
/// beside the one-shot request; it is never a reinterpretation of the one-shot
/// v1 JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionLaunch {
    /// The protocol identifier.
    pub protocol: String,
    /// The protocol version.
    pub protocol_version: u32,
    /// The disjoint authority phase.
    pub phase: Phase,
    /// The payload argument vector.
    pub argv: Vec<String>,
    /// The canonical absolute sandbox working directory.
    pub working_directory: String,
    /// The bounded environment.
    pub environment: BTreeMap<String, String>,
    /// The network capability block.
    pub network: Network,
    /// Bounded resource limits.
    pub resources: Resources,
    /// Approval-bound request identities.
    pub identities: Identities,
    /// Typed host-to-sandbox mount grants.
    pub grants: Vec<Grant>,
    /// The approved toolchain.
    pub toolchain: Toolchain,
    /// The Cargo-home topology (optional).
    pub cache: Option<Cache>,
    /// Writable target/HOME scratch endpoint (optional).
    pub scratch: Option<Scratch>,
    /// The bounded session block.
    pub session: Session,
}

impl SessionLaunch {
    /// The one-shot v1 request this session launch would enforce, with the
    /// session block dropped. The session mode adds no host loopback, general
    /// egress, host socket directory, or host filesystem grant beyond the
    /// closed v1 contract; the only session-specific authority is the bounded
    /// inference relay carried by the session block itself.
    pub fn to_request(&self) -> SandboxRequest {
        SandboxRequest {
            protocol: self.protocol.clone(),
            protocol_version: self.protocol_version,
            phase: self.phase,
            argv: self.argv.clone(),
            working_directory: self.working_directory.clone(),
            environment: self.environment.clone(),
            network: self.network.clone(),
            resources: self.resources,
            identities: self.identities.clone(),
            grants: self.grants.clone(),
            toolchain: self.toolchain.clone(),
            cache: self.cache.clone(),
            scratch: self.scratch.clone(),
            home: None,
        }
    }
}

/// Wire struct for parsing the session launch metadata.
///
/// The `stdio` and `cancellation` fields are `String` in the wire struct so
/// that unknown values produce an `Invalid` error (not a serde `Malformed`
/// error) during validation. The `ipc` field uses the same wire shape as the
/// final struct because its unknown-variant handling is already correct: an
/// unknown `kind` produces a serde error that maps to `Malformed`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionLaunchWire {
    protocol: String,
    protocol_version: u32,
    phase: Phase,
    argv: Vec<String>,
    working_directory: String,
    environment: BTreeMap<String, String>,
    network: Network,
    resources: Resources,
    identities: Identities,
    grants: Vec<Grant>,
    toolchain: Toolchain,
    cache: Option<Cache>,
    scratch: Option<Scratch>,
    session: SessionWire,
}

/// Wire struct for the session block.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionWire {
    stdio: String,
    #[serde(default)]
    ipc: Option<IpcCapability>,
    cancellation: String,
}

/// Parses the bounded, closed session launch metadata without trusting its
/// producer.
///
/// The session mode reads launch metadata on a dedicated descriptor, never
/// through argv. The metadata is bounded to [`MAX_SESSION_BYTES`] and must
/// carry the exact session protocol identifier. The same closed version-one
/// namespace/grant/resource/identity contract as the one-shot request is
/// enforced, plus the bounded session block.
pub fn parse_and_validate(bytes: &[u8]) -> Result<SessionLaunch, SessionError> {
    if bytes.len() > MAX_SESSION_BYTES {
        return Err(SessionError::TooLarge {
            size: bytes.len(),
            limit: MAX_SESSION_BYTES,
        });
    }

    let wire: SessionLaunchWire =
        serde_json::from_slice(bytes).map_err(|error| malformed(error.to_string()))?;

    // The session mode must carry the exact session protocol identifier.
    // A one-shot v1 request must not be reinterpreted as session metadata.
    if wire.protocol != SESSION_PROTOCOL {
        return Err(invalid(format!(
            "protocol must be `{SESSION_PROTOCOL}`, not `{}`",
            wire.protocol
        )));
    }
    if wire.protocol_version != PROTOCOL_VERSION {
        return Err(invalid(format!(
            "protocol_version must be {PROTOCOL_VERSION}, not {}",
            wire.protocol_version
        )));
    }

    // Validate the closed version-one namespace/grant/resource/identity
    // contract. The session mode shares this exact contract with the one-shot
    // request; the only session-specific authority is the bounded session
    // block.
    let request = SandboxRequest {
        protocol: wire.protocol.clone(),
        protocol_version: wire.protocol_version,
        phase: wire.phase,
        argv: wire.argv.clone(),
        working_directory: wire.working_directory.clone(),
        environment: wire.environment.clone(),
        network: wire.network.clone(),
        resources: wire.resources,
        identities: wire.identities.clone(),
        grants: wire.grants.clone(),
        toolchain: wire.toolchain.clone(),
        cache: wire.cache.clone(),
        scratch: wire.scratch.clone(),
        home: None,
    };
    validation::validate_request(&request, SESSION_PROTOCOL).map_err(|error| match error {
        crate::validation::ProtocolError::TooLarge { size, limit } => {
            SessionError::TooLarge { size, limit }
        }
        crate::validation::ProtocolError::Malformed { detail } => {
            SessionError::Malformed { detail }
        }
        crate::validation::ProtocolError::Invalid { detail } => SessionError::Invalid { detail },
    })?;

    // Validate the bounded session block.
    let session = validate_session_block(&wire.session)?;

    Ok(SessionLaunch {
        protocol: wire.protocol,
        protocol_version: wire.protocol_version,
        phase: wire.phase,
        argv: wire.argv,
        working_directory: wire.working_directory,
        environment: wire.environment,
        network: wire.network,
        resources: wire.resources,
        identities: wire.identities,
        grants: wire.grants,
        toolchain: wire.toolchain,
        cache: wire.cache,
        scratch: wire.scratch,
        session,
    })
}

/// Validates the bounded session block: reserved stdio, optional IPC
/// capability, and run-scoped cancellation.
fn validate_session_block(wire: &SessionWire) -> Result<Session, SessionError> {
    // The stdio role must be exactly `reserved-for-acp`. Any other value is
    // invalid: the session mode reserves the payload's stdio for ACP.
    let stdio = match wire.stdio.as_str() {
        "reserved-for-acp" => StdioRole::ReservedForAcp,
        other => {
            return Err(invalid(format!(
                "session stdio must be `reserved-for-acp`, not `{other}`"
            )));
        }
    };

    // The cancellation capability must be exactly `run-scoped`. Any other
    // value is invalid: a host-scoped cancellation capability would allow the
    // embedding host to cancel runs beyond the current session.
    let cancellation = match wire.cancellation.as_str() {
        "run-scoped" => Cancellation::RunScoped,
        other => {
            return Err(invalid(format!(
                "session cancellation must be `run-scoped`, not `{other}`"
            )));
        }
    };

    // The IPC capability is optional. When present, it must be a bounded
    // inference relay on sandbox loopback with the exact permitted route.
    let ipc = match &wire.ipc {
        None => None,
        Some(capability) => Some(validate_ipc_capability(capability)?),
    };

    Ok(Session {
        stdio,
        ipc,
        cancellation,
    })
}

/// Validates an explicitly granted IPC capability.
///
/// The relay must be an inference relay on sandbox loopback with the exact
/// permitted route. A general HTTP proxy, host loopback sharing, or an
/// unrelated route is invalid.
fn validate_ipc_capability(capability: &IpcCapability) -> Result<IpcCapability, SessionError> {
    // The kind must be `inference-relay`. A general HTTP proxy opens a
    // general egress path, which is not permitted.
    match capability.kind {
        IpcKind::InferenceRelay => {}
        IpcKind::GeneralHttpProxy => {
            return Err(invalid(
                "session IPC capability must not grant a general HTTP proxy",
            ));
        }
    }

    // The socket must be `sandbox-loopback`. Host loopback sharing would
    // allow the payload to reach host services, which is not permitted.
    match capability.socket {
        IpcSocket::SandboxLoopback => {}
        IpcSocket::HostLoopback => {
            return Err(invalid(
                "session IPC capability must not share host loopback",
            ));
        }
    }

    // The routes must be exactly the permitted inference route. An unrelated
    // route would open an unbounded egress path.
    let expected_route = "POST /v1/chat/completions";
    if capability.routes.len() != 1 || capability.routes[0] != expected_route {
        return Err(invalid(format!(
            "session IPC capability routes must be exactly [`{expected_route}`], not {:?}",
            capability.routes
        )));
    }

    Ok(capability.clone())
}

/// Whether the session launch metadata grants host loopback sharing.
///
/// A valid bounded relay grant must not share host loopback. The only way
/// host loopback could be granted is through the IPC capability's socket
/// binding, which is validated to be `sandbox-loopback`. This function is a
/// defense-in-depth check: it returns `true` only if the IPC capability is
/// present and its socket is `host-loopback`.
pub fn grants_host_loopback(launch: &SessionLaunch) -> bool {
    launch
        .session
        .ipc
        .as_ref()
        .is_some_and(|capability| capability.socket == IpcSocket::HostLoopback)
}

/// Whether the session launch metadata grants a general egress path.
///
/// A valid bounded relay grant must not open a general egress path. The only
/// way a general egress path could be granted is through the IPC capability's
/// kind, which is validated to be `inference-relay`. This function is a
/// defense-in-depth check: it returns `true` only if the IPC capability is
/// present and its kind is `general-http-proxy`.
pub fn grants_general_egress(launch: &SessionLaunch) -> bool {
    launch
        .session
        .ipc
        .as_ref()
        .is_some_and(|capability| capability.kind == IpcKind::GeneralHttpProxy)
}

/// Whether the runner owns and reaps the whole process group.
///
/// The runner owns and reaps the whole process group on every outcome:
/// success, failure, timeout, and output-limit breach. This is a library-level
/// contract assertion: the session mode always owns the process group.
pub fn owns_process_group(_launch: &SessionLaunch) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    fn digest(bytes: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    fn valid_session_metadata() -> Value {
        json!({
            "protocol": "kvist-sandbox-session-v1",
            "protocol_version": 1,
            "phase": "authoring",
            "argv": ["/usr/bin/true"],
            "working_directory": "/workspace/component",
            "environment": {
                "HOME": "/workspace/home",
                "PATH": "/usr/bin"
            },
            "network": {
                "mode": "deny",
                "allowed_sources": []
            },
            "resources": {
                "wall_time_ms": 5000,
                "max_output_bytes": 65536,
                "max_processes": 16,
                "max_files": 256,
                "max_file_bytes": 1048576,
                "max_scratch_bytes": 1048576
            },
            "identities": {
                "runner": digest(b"runner"),
                "backend": {
                    "kind": "bubblewrap",
                    "path": "/usr/bin/bwrap",
                    "digest": digest(b"backend")
                },
                "policy": digest(b"policy"),
                "toolchain": digest(b"toolchain"),
                "command": digest(b"command"),
                "mount_plan": digest(b"mount-plan")
            },
            "grants": [
                {
                    "source": "/host/component",
                    "destination": "/workspace/component",
                    "access": "read-write",
                    "purpose": "authoring",
                    "identity": digest(b"component")
                },
                {
                    "source": "/usr",
                    "destination": "/usr",
                    "access": "read-only",
                    "purpose": "toolchain",
                    "identity": digest(b"toolchain-usr")
                },
                {
                    "source": "/host/scratch",
                    "destination": "/workspace/scratch",
                    "access": "read-write",
                    "purpose": "scratch",
                    "identity": digest(b"scratch")
                }
            ],
            "toolchain": {
                "kind": "system",
                "identity": digest(b"toolchain"),
                "root": "/usr"
            },
            "cache": null,
            "scratch": {
                "destination": "/workspace/scratch",
                "identity": digest(b"scratch")
            },
            "session": {
                "stdio": "reserved-for-acp",
                "ipc": null,
                "cancellation": "run-scoped"
            }
        })
    }

    fn encode(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).expect("encode session metadata")
    }

    #[test]
    fn accepts_the_strict_session_launch_metadata_shape() {
        let metadata = valid_session_metadata();
        parse_and_validate(&encode(&metadata))
            .expect("valid session launch metadata must parse and validate");
    }

    #[test]
    fn rejects_oversized_session_launch_metadata() {
        let mut metadata = valid_session_metadata();
        metadata["environment"]["PADDING"] = json!("x".repeat(2 * 1024 * 1024));
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("oversized session metadata must be rejected");
        assert!(matches!(error, SessionError::TooLarge { .. }));
    }

    #[test]
    fn rejects_malformed_session_launch_metadata() {
        let not_json = b"this is not json at all";
        assert!(parse_and_validate(not_json).is_err());

        let array = b"[1, 2, 3]";
        assert!(parse_and_validate(array).is_err());

        let missing_protocol = json!({
            "protocol_version": 1,
            "phase": "authoring",
            "argv": ["/usr/bin/true"],
            "working_directory": "/workspace/component",
            "environment": {},
            "network": { "mode": "deny", "allowed_sources": [] },
            "resources": {
                "wall_time_ms": 5000,
                "max_output_bytes": 65536,
                "max_processes": 16,
                "max_files": 256,
                "max_scratch_bytes": 1048576
            },
            "identities": {
                "runner": digest(b"runner"),
                "backend": { "kind": "bubblewrap", "path": "/usr/bin/bwrap", "digest": digest(b"backend") },
                "policy": digest(b"policy"),
                "toolchain": digest(b"toolchain"),
                "command": digest(b"command"),
                "mount_plan": digest(b"mount-plan")
            },
            "grants": [],
            "toolchain": { "kind": "system", "identity": digest(b"toolchain"), "root": "/usr" },
            "cache": null,
            "scratch": null,
            "session": { "stdio": "reserved-for-acp", "ipc": null, "cancellation": "run-scoped" }
        });
        let error = parse_and_validate(&encode(&missing_protocol))
            .expect_err("missing protocol identifier must reject");
        assert!(matches!(error, SessionError::Malformed { .. }));
    }

    #[test]
    fn rejects_unknown_session_top_level_field() {
        let mut metadata = valid_session_metadata();
        metadata["fallback_to_host"] = json!(true);
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("unknown top-level field must reject");
        assert!(matches!(error, SessionError::Malformed { .. }));
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_session_block_field() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["host_socket_directory"] = json!("/var/run/galla");
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("unknown session-block field must reject");
        assert!(matches!(error, SessionError::Malformed { .. }));
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn session_stdio_role_is_reserved_for_acp() {
        let metadata = valid_session_metadata();
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert_eq!(
            parsed.session.stdio,
            StdioRole::ReservedForAcp,
            "session mode must reserve the payload's stdio for ACP"
        );
    }

    #[test]
    fn rejects_a_session_request_that_does_not_reserve_stdio() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["stdio"] = json!("captured");
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("a session that captures stdio must reject");
        assert!(matches!(error, SessionError::Invalid { .. }));
        assert!(error.to_string().contains("stdio"));
    }

    #[test]
    fn accepts_an_explicitly_granted_bounded_ipc_capability() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["ipc"] = json!({
            "kind": "inference-relay",
            "socket": "sandbox-loopback",
            "routes": ["POST /v1/chat/completions"]
        });
        parse_and_validate(&encode(&metadata))
            .expect("an explicitly granted bounded IPC capability must parse and validate");
    }

    #[test]
    fn rejects_an_ipc_capability_that_grants_host_loopback() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["ipc"] = json!({
            "kind": "inference-relay",
            "socket": "host-loopback",
            "routes": ["POST /v1/chat/completions"]
        });
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("an IPC capability that shares host loopback must reject");
        assert!(matches!(error, SessionError::Invalid { .. }));
        assert!(error.to_string().contains("host loopback"));
    }

    #[test]
    fn rejects_an_ipc_capability_that_grants_a_general_http_proxy() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["ipc"] = json!({
            "kind": "general-http-proxy",
            "socket": "sandbox-loopback",
            "routes": ["*"]
        });
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("a general HTTP proxy IPC capability must reject");
        assert!(matches!(error, SessionError::Invalid { .. }));
        assert!(error.to_string().contains("general HTTP proxy"));
    }

    #[test]
    fn rejects_an_ipc_capability_with_an_unrelated_route() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["ipc"] = json!({
            "kind": "inference-relay",
            "socket": "sandbox-loopback",
            "routes": ["GET /v1/models", "POST /v1/chat/completions"]
        });
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("an IPC capability with an unrelated route must reject");
        assert!(matches!(error, SessionError::Invalid { .. }));
        assert!(error.to_string().contains("route"));
    }

    #[test]
    fn rejects_an_ipc_capability_with_an_unknown_kind() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["ipc"] = json!({
            "kind": "arbitrary-ipc",
            "socket": "sandbox-loopback",
            "routes": ["POST /v1/chat/completions"]
        });
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("an unknown IPC capability kind must reject");
        assert!(matches!(error, SessionError::Malformed { .. }));
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn ipc_capability_is_optional_and_absent_by_default() {
        let metadata = valid_session_metadata();
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            parsed.session.ipc.is_none(),
            "the IPC capability must be optional; absence denies the relay"
        );
    }

    #[test]
    fn a_valid_bounded_relay_grant_grants_no_host_loopback_or_general_egress() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["ipc"] = json!({
            "kind": "inference-relay",
            "socket": "sandbox-loopback",
            "routes": ["POST /v1/chat/completions"]
        });
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            parsed.session.ipc.is_some(),
            "the explicitly granted IPC capability must be present"
        );
        assert!(
            !grants_host_loopback(&parsed),
            "a valid bounded relay grant must not share host loopback"
        );
        assert!(
            !grants_general_egress(&parsed),
            "a valid bounded relay grant must not open a general egress path"
        );
    }

    #[test]
    fn runner_reaps_the_process_group_on_success() {
        let metadata = valid_session_metadata();
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            owns_process_group(&parsed),
            "the runner must own and reap the whole process group on success"
        );
    }

    #[test]
    fn runner_reaps_the_process_group_on_failure() {
        let mut metadata = valid_session_metadata();
        metadata["argv"] = json!(["/usr/bin/false"]);
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            owns_process_group(&parsed),
            "the runner must own and reap the whole process group on payload failure"
        );
    }

    #[test]
    fn runner_reaps_the_process_group_on_timeout() {
        let mut metadata = valid_session_metadata();
        metadata["resources"]["wall_time_ms"] = json!(1);
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            owns_process_group(&parsed),
            "the runner must own and reap the whole process group on timeout"
        );
    }

    #[test]
    fn runner_reaps_the_process_group_on_output_limit_breach() {
        let mut metadata = valid_session_metadata();
        metadata["resources"]["max_output_bytes"] = json!(1);
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            owns_process_group(&parsed),
            "the runner must own and reap the whole process group on output-limit breach"
        );
    }

    #[test]
    fn session_cancellation_capability_is_run_scoped() {
        let metadata = valid_session_metadata();
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert_eq!(
            parsed.session.cancellation,
            Cancellation::RunScoped,
            "the cancellation capability must be run-scoped"
        );
    }

    #[test]
    fn rejects_a_session_cancellation_capability_that_is_not_run_scoped() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["cancellation"] = json!("host-scoped");
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("a non-run-scoped cancellation capability must reject");
        assert!(matches!(error, SessionError::Invalid { .. }));
        assert!(error.to_string().contains("cancellation"));
    }

    #[test]
    fn session_mode_has_no_host_loopback_or_general_egress_path() {
        let metadata = valid_session_metadata();
        let parsed = parse_and_validate(&encode(&metadata)).expect("valid metadata");
        assert!(
            !grants_host_loopback(&parsed),
            "session mode must not grant host loopback sharing"
        );
        assert!(
            !grants_general_egress(&parsed),
            "session mode must not grant a general egress path"
        );
    }

    #[test]
    fn session_mode_rejects_a_host_socket_directory_grant() {
        let mut metadata = valid_session_metadata();
        metadata["session"]["host_socket_directory"] = json!("/var/run/galla");
        let error = parse_and_validate(&encode(&metadata))
            .expect_err("a host socket directory grant must reject");
        assert!(matches!(error, SessionError::Malformed { .. }));
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_the_one_shot_v1_request_as_session_metadata() {
        let one_shot = json!({
            "protocol": "kvist-sandbox-request-v1",
            "protocol_version": 1,
            "phase": "authoring",
            "argv": ["/usr/bin/true"],
            "working_directory": "/workspace/component",
            "environment": {
                "HOME": "/workspace/home",
                "PATH": "/usr/bin"
            },
            "network": {
                "mode": "deny",
                "allowed_sources": []
            },
            "resources": {
                "wall_time_ms": 5000,
                "max_output_bytes": 65536,
                "max_processes": 16,
                "max_files": 256,
                "max_file_bytes": 1048576,
                "max_scratch_bytes": 1048576
            },
            "identities": {
                "runner": digest(b"runner"),
                "backend": {
                    "kind": "bubblewrap",
                    "path": "/usr/bin/bwrap",
                    "digest": digest(b"backend")
                },
                "policy": digest(b"policy"),
                "toolchain": digest(b"toolchain"),
                "command": digest(b"command"),
                "mount_plan": digest(b"mount-plan")
            },
            "grants": [
                {
                    "source": "/host/component",
                    "destination": "/workspace/component",
                    "access": "read-write",
                    "purpose": "authoring",
                    "identity": digest(b"component")
                },
                {
                    "source": "/usr",
                    "destination": "/usr",
                    "access": "read-only",
                    "purpose": "toolchain",
                    "identity": digest(b"toolchain-usr")
                },
                {
                    "source": "/host/scratch",
                    "destination": "/workspace/scratch",
                    "access": "read-write",
                    "purpose": "scratch",
                    "identity": digest(b"scratch")
                }
            ],
            "toolchain": {
                "kind": "system",
                "identity": digest(b"toolchain"),
                "root": "/usr"
            },
            "cache": null,
            "scratch": {
                "destination": "/workspace/scratch",
                "identity": digest(b"scratch")
            }
        });
        let error = parse_and_validate(&encode(&one_shot))
            .expect_err("a one-shot v1 request must not be accepted as session metadata");
        assert!(
            matches!(error, SessionError::Malformed { .. })
                || matches!(error, SessionError::Invalid { .. }),
            "legacy one-shot v1 request must be rejected as session metadata: {error}"
        );
    }
}
