//! Pluggable execution backend that delegates approved effects to the installed
//! galla runner through its version-one protocol.
//!
//! [`ExecutionBackend`] is the concrete [`ToolExecutor`] a protected host injects
//! into the native runtime. It resolves and binds the installed galla-runner,
//! probes the runner's verified backend capabilities before any effect is
//! dispatched, and delegates every approved effect through the runner's
//! version-one request protocol. It never constructs isolation primitives
//! (namespaces, mounts, resource limits) in this crate: those are owned by the
//! runner. When the runner or its backend is unavailable, the backend refuses
//! before dispatch and never substitutes an unconstrained host execution.
//!
//! The approved effect is carried by the tool intent's arguments: an `argv`
//! array (the command line). The host directory mounted read-write at
//! `/workspace` and the extra environment variables are host-authority values
//! bound at construction ([`ExecutionBackend::new`]); the model supplies only
//! the command line and cannot redirect the workspace grant or inject
//! environment variables. The backend renders those into a closed
//! authoring-phase version-one request, binds the approval-bound identities,
//! and hands the request to the runner on standard input.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use sha2::{Digest, Sha256};

use super::error::{Error, Result};
use super::outcome::ToolOutcome;
use super::session::ToolExecutor;
use crate::CancellationToken;
use crate::ToolIntent;

/// The exact accepted runner request protocol identifier (version one).
const REQUEST_PROTOCOL: &str = "kvist-sandbox-request-v1";
/// The exact accepted runner probe protocol identifier (version one).
const PROBE_PROTOCOL: &str = "kvist-sandbox-probe-v1";
/// The single supported version of both protocol messages.
const PROTOCOL_VERSION: u32 = 1;
/// The probe argument the runner accepts for a capability attestation.
const PROBE_ARGUMENT: &str = "--kvist-sandbox-probe-v1";
/// The request argument the runner accepts for a one-shot execution.
const REQUEST_ARGUMENT: &str = "--kvist-sandbox-request-v1";
/// A fixed, short deadline for the capability probe so a hung runner cannot
/// block task startup indefinitely.
const PROBE_DEADLINE: Duration = Duration::from_secs(30);
/// The maximum accepted probe response size.
const MAX_PROBE_BYTES: usize = 64 * 1024;
/// The default wall-time limit for a delegated effect (15 minutes).
const DEFAULT_WALL_TIME_MS: u64 = 15 * 60 * 1000;
/// The default output capture limit for a delegated effect (1 MiB).
const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
/// The poll interval while supervising a delegated effect.
const SUPERVISION_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// The canonical absolute sandbox destination for the working directory.
const WORKSPACE_DESTINATION: &str = "/workspace";
/// The canonical absolute sandbox destination for the system tree.
const SYSTEM_DESTINATION: &str = "/usr";
/// The sandbox `PATH` exposing the system bin directories.
const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// The canonical identity of the trusted runner, bound before dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerIdentity {
    /// The canonical absolute runner path.
    pub canonical_path: String,
    /// The runner's content digest (`sha256:` prefixed).
    pub digest: String,
}

/// The identity of the enforcement backend confirmed by the version-one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendIdentity {
    /// The backend kind (always `bubblewrap` for a confirmed probe).
    pub kind: String,
    /// The canonical absolute backend path.
    pub path: String,
    /// The backend's content digest (`sha256:` prefixed).
    pub digest: String,
}

/// Kernel namespace capabilities verified by the runner's probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Namespaces {
    pub mount: bool,
    pub network: bool,
    pub pid: bool,
    pub ipc: bool,
    pub uts: bool,
    pub user: bool,
}

/// Verified backend capabilities confirmed by the runner's probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub namespaces: Namespaces,
    pub new_session: bool,
    pub parent_death_signal: bool,
}

impl Capabilities {
    /// Whether every capability required for complete mediation is confirmed.
    pub fn complete(&self) -> bool {
        self.namespaces.mount
            && self.namespaces.network
            && self.namespaces.pid
            && self.namespaces.ipc
            && self.namespaces.uts
            && self.namespaces.user
            && self.new_session
            && self.parent_death_signal
    }
}

/// A replaceable execution backend that resolves and binds the installed galla
/// runner, probes its verified backend capabilities, and delegates every
/// approved effect through the runner's version-one protocol.
///
/// Construct one with [`ExecutionBackend::new`], which resolves the runner and
/// runs the capability probe before any effect can be dispatched. The backend
/// then implements [`ToolExecutor`] for the native runtime.
///
/// ```rust
/// use std::collections::BTreeMap;
/// use std::path::Path;
/// use sav::runtime::ExecutionBackend;
///
/// // A missing runner fails closed before any effect; no unconstrained host
/// // execution is substituted.
/// let error = ExecutionBackend::new(
///     Path::new("/nonexistent/galla"),
///     "/workspace",
///     BTreeMap::new(),
/// )
/// .unwrap_err();
/// assert!(matches!(error, sav::runtime::Error::RunnerUnavailable { .. }));
/// ```
#[derive(Debug)]
pub struct ExecutionBackend {
    /// The installed runner's canonical path and content digest, bound before dispatch.
    runner: RunnerIdentity,
    /// The runner's verified enforcement backend, confirmed by the capability probe.
    backend: BackendIdentity,
    /// The verified namespace and session capabilities the runner confirmed.
    capabilities: Capabilities,
    /// The host directory mounted read-write at `/workspace`, bound by the host so
    /// the model cannot redirect the workspace grant to an arbitrary host path.
    workspace: String,
    /// Extra environment variables, bound by the host so the model cannot inject
    /// loader or credential variables into the sandbox environment.
    environment: BTreeMap<String, String>,
}

impl ExecutionBackend {
    /// Resolves the installed galla-runner at `runner_path`, probes its verified
    /// backend capabilities, and binds the runner and backend identities.
    ///
    /// `workspace` is the host directory the host mounts read-write at
    /// `/workspace`, and `environment` is the host-supplied set of extra
    /// environment variables. Both are host-authority values: binding them here,
    /// at construction, keeps the untrusted model from redirecting the workspace
    /// grant to an arbitrary host path or injecting loader or credential
    /// variables into the sandbox environment. The model supplies only the
    /// command line (`argv`).
    ///
    /// This fails closed before any effect: a missing or unreadable runner is a
    /// [`Error::RunnerUnavailable`], and a runner that cannot confirm the
    /// version-one Bubblewrap probe (or confirms incomplete capabilities) is an
    /// [`Error::BackendUnavailable`]. No unconstrained host execution is ever
    /// substituted.
    pub fn new(
        runner_path: &Path,
        workspace: impl Into<String>,
        environment: BTreeMap<String, String>,
    ) -> Result<Self> {
        let runner = resolve_runner(runner_path)?;
        let (backend, capabilities, probed_runner_digest) = probe(&runner)?;
        if probed_runner_digest != runner.digest {
            return Err(Error::RunnerUnavailable {
                reason: format!(
                    "the runner probe reported a digest ({probed_runner_digest}) that does not \
                     match the installed runner ({})",
                    runner.digest
                ),
            });
        }
        if !capabilities.complete() {
            return Err(Error::BackendUnavailable {
                reason: "the runner probe did not confirm the required namespace and session \
                         capabilities; no unconstrained host fallback is used"
                    .to_owned(),
            });
        }
        Ok(Self {
            runner,
            backend,
            capabilities,
            workspace: workspace.into(),
            environment,
        })
    }

    /// The bound runner identity.
    pub fn runner(&self) -> &RunnerIdentity {
        &self.runner
    }

    /// The bound, verified backend identity.
    pub fn backend(&self) -> &BackendIdentity {
        &self.backend
    }

    /// The verified backend capabilities confirmed by the probe.
    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    /// Re-hashes the installed runner and backend immediately before dispatch and
    /// fails closed when either identity has drifted since the probe bound it.
    fn rebind(&self) -> Result<()> {
        let current_runner =
            file_digest(Path::new(&self.runner.canonical_path)).map_err(|reason| {
                Error::RunnerUnavailable {
                    reason: format!(
                        "the installed galla runner is unreadable before dispatch: {reason}"
                    ),
                }
            })?;
        if current_runner != self.runner.digest {
            return Err(Error::RunnerUnavailable {
                reason: format!(
                    "the installed galla runner content changed after the capability probe \
                     (bound {}, observed {})",
                    self.runner.digest, current_runner
                ),
            });
        }
        let current_backend = file_digest(Path::new(&self.backend.path)).map_err(|reason| {
            Error::BackendUnavailable {
                reason: format!("the verified backend is unreadable before dispatch: {reason}"),
            }
        })?;
        if current_backend != self.backend.digest {
            return Err(Error::BackendUnavailable {
                reason: format!(
                    "the verified backend content changed after the capability probe \
                     (bound {}, observed {})",
                    self.backend.digest, current_backend
                ),
            });
        }
        Ok(())
    }
}

/// Resolves the installed galla-runner to a canonical path and content digest.
///
/// The runner must be a regular, non-symlink file; its exact bytes are hashed so
/// the approval binds the precise executable. A missing or unreadable runner is
/// a [`Error::RunnerUnavailable`].
pub fn resolve_runner(runner_path: &Path) -> Result<RunnerIdentity> {
    let metadata =
        std::fs::symlink_metadata(runner_path).map_err(|source| Error::RunnerUnavailable {
            reason: format!(
                "the installed galla runner `{}` cannot be inspected: {source}",
                runner_path.display()
            ),
        })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::RunnerUnavailable {
            reason: format!(
                "the installed galla runner `{}` must be a regular non-symlink file",
                runner_path.display()
            ),
        });
    }
    let canonical = runner_path
        .canonicalize()
        .map_err(|source| Error::RunnerUnavailable {
            reason: format!(
                "the installed galla runner `{}` cannot be canonicalized: {source}",
                runner_path.display()
            ),
        })?;
    let digest = file_digest(&canonical).map_err(|reason| Error::RunnerUnavailable {
        reason: format!("the installed galla runner cannot be read for its digest: {reason}"),
    })?;
    Ok(RunnerIdentity {
        canonical_path: canonical.to_string_lossy().into_owned(),
        digest,
    })
}

/// Computes the `sha256:` prefixed content digest of a regular file.
fn file_digest(path: &Path) -> std::result::Result<String, String> {
    let bytes = std::fs::read(path)
        .map_err(|source| format!("cannot read `{}`: {source}", path.display()))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(&bytes))))
}

/// Runs the runner's version-one capability probe and returns the confirmed
/// backend identity, capabilities, and the runner digest the probe self-attested.
///
/// The probe is bounded by [`PROBE_DEADLINE`] and [`MAX_PROBE_BYTES`]. A runner
/// that exits unsuccessfully, emits diagnostics on standard error, times out, or
/// reports an unsupported protocol, a non-Bubblewrap backend, or incomplete
/// capabilities is a [`Error::BackendUnavailable`].
fn probe(runner: &RunnerIdentity) -> Result<(BackendIdentity, Capabilities, String)> {
    let mut child = Command::new(&runner.canonical_path)
        .arg(PROBE_ARGUMENT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|source| Error::BackendUnavailable {
            reason: format!(
                "the installed galla runner `{}` could not be started for the capability probe: {source}",
                runner.canonical_path
            ),
        })?;
    let pgid = i32::try_from(child.id()).map_err(|_| Error::BackendUnavailable {
        reason: "the galla runner process ID exceeds the supported range".to_owned(),
    })?;
    let started = Instant::now();
    let output = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = drain_pipe(&mut child.stdout.take(), MAX_PROBE_BYTES);
                let stderr = drain_pipe(&mut child.stderr.take(), MAX_PROBE_BYTES);
                break (status, stdout, stderr);
            }
            Ok(None) if started.elapsed() < PROBE_DEADLINE => {
                std::thread::sleep(SUPERVISION_POLL_INTERVAL);
            }
            Ok(None) => {
                let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL);
                return Err(Error::BackendUnavailable {
                    reason: "the galla runner capability probe exceeded its deadline".to_owned(),
                });
            }
            Err(source) => {
                return Err(Error::BackendUnavailable {
                    reason: format!("the galla runner capability probe failed: {source}"),
                });
            }
        }
    };
    let (status, stdout, stderr) = output;
    if !status.success() {
        return Err(Error::BackendUnavailable {
            reason: format!(
                "the galla runner did not confirm the version-one capability probe ({status}); \
                 stderr: {}",
                String::from_utf8_lossy(&stderr)
            ),
        });
    }
    if !stderr.is_empty() {
        return Err(Error::BackendUnavailable {
            reason: format!(
                "the galla runner probe emitted diagnostics on standard error despite reporting \
                 success: {}",
                String::from_utf8_lossy(&stderr)
            ),
        });
    }
    if stdout.len() > MAX_PROBE_BYTES {
        return Err(Error::BackendUnavailable {
            reason: "the galla runner probe response exceeded the accepted size bound".to_owned(),
        });
    }
    let probe: ProbeWire =
        serde_json::from_slice(&stdout).map_err(|error| Error::BackendUnavailable {
            reason: format!(
                "the galla runner probe response is not a valid version-one object: {error}"
            ),
        })?;
    if probe.protocol != PROBE_PROTOCOL || probe.protocol_version != PROTOCOL_VERSION {
        return Err(Error::BackendUnavailable {
            reason: "the galla runner probe declared an unsupported protocol or version".to_owned(),
        });
    }
    if probe.backend.kind != "bubblewrap" {
        return Err(Error::BackendUnavailable {
            reason: "the galla runner probe did not confirm a Bubblewrap enforcement backend"
                .to_owned(),
        });
    }
    let capabilities = Capabilities {
        namespaces: Namespaces {
            mount: probe.capabilities.namespaces.mount,
            network: probe.capabilities.namespaces.network,
            pid: probe.capabilities.namespaces.pid,
            ipc: probe.capabilities.namespaces.ipc,
            uts: probe.capabilities.namespaces.uts,
            user: probe.capabilities.namespaces.user,
        },
        new_session: probe.capabilities.new_session,
        parent_death_signal: probe.capabilities.parent_death_signal,
    };
    let backend = BackendIdentity {
        kind: probe.backend.kind,
        path: probe.backend.path,
        digest: probe.backend.digest,
    };
    // The probe self-attests the runner path it ran from; the content digest is
    // what is compared against the bound identity in `ExecutionBackend::new`.
    let _reported_runner_path = probe.runner.path;
    Ok((backend, capabilities, probe.runner.digest))
}

/// Drains a piped stream to a bounded byte limit, closing it at EOF.
fn drain_pipe<R: std::io::Read>(pipe: &mut Option<R>, limit: usize) -> Vec<u8> {
    let mut captured = Vec::new();
    if let Some(stream) = pipe.as_mut() {
        let budget = limit.saturating_sub(captured.len());
        if budget > 0 {
            let _ = stream.take(budget as u64).read_to_end(&mut captured);
        }
    }
    *pipe = None;
    captured
}

/// The closed, capability-confirmed version-one probe response.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeWire {
    protocol: String,
    protocol_version: u32,
    runner: ProbeExecutable,
    backend: ProbeBackend,
    capabilities: ProbeCapabilities,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeExecutable {
    path: String,
    digest: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeBackend {
    kind: String,
    path: String,
    digest: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeCapabilities {
    namespaces: ProbeNamespaces,
    new_session: bool,
    parent_death_signal: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeNamespaces {
    mount: bool,
    network: bool,
    pid: bool,
    ipc: bool,
    uts: bool,
    user: bool,
}

/// The canonical digest of an approval-bound command, recomputed from the
/// request argv. The encoding is `command` followed by a NUL, then each argv
/// element joined by NUL separators (the first element is not preceded by a
/// separator). This is the exact byte sequence the runner re-digests to bind an
/// approved command to the argv that actually runs.
pub fn command_identity(argv: &[String]) -> String {
    let mut input = b"command\0".to_vec();
    for (index, argument) in argv.iter().enumerate() {
        if index > 0 {
            input.push(b'\0');
        }
        input.extend_from_slice(argument.as_bytes());
    }
    format!("sha256:{}", hex::encode(Sha256::digest(&input)))
}

/// The canonical digest of an approval-bound mount plan, recomputed from the
/// request grants. Each grant contributes its destination, a NUL, an access
/// byte (`0` read-only, `1` read-write), a purpose byte, and a NUL. The runner
/// re-digests the concatenation to bind the approved mount plan to the grants
/// that actually mount.
pub fn mount_plan_identity(grants: &[(String, u8, u8)]) -> String {
    let mut plan = Vec::new();
    for (destination, access, purpose) in grants {
        plan.extend_from_slice(destination.as_bytes());
        plan.push(b'\0');
        plan.push(*access);
        plan.push(*purpose);
        plan.push(b'\0');
    }
    format!("sha256:{}", hex::encode(Sha256::digest(&plan)))
}

/// A stable content digest over a label and an input, used for the approval-bound
/// policy and toolchain identities the runner carries without recomputing.
fn identity_digest(label: &[u8], input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(label);
    hasher.update(b"\0");
    hasher.update(input);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// The closed authoring-phase version-one execution request.
#[derive(Debug, serde::Serialize)]
struct SandboxRequest<'a> {
    protocol: &'static str,
    protocol_version: u32,
    phase: &'static str,
    argv: &'a [String],
    working_directory: &'static str,
    environment: &'a BTreeMap<String, String>,
    network: SandboxNetwork,
    resources: SandboxResources,
    identities: SandboxIdentities<'a>,
    grants: &'a [SandboxGrant],
    toolchain: SandboxToolchain,
    cache: Option<serde_json::Value>,
    scratch: Option<serde_json::Value>,
    home: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
struct SandboxNetwork {
    mode: &'static str,
    allowed_sources: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
struct SandboxResources {
    wall_time_ms: u64,
    max_output_bytes: u64,
    max_processes: u64,
    max_files: u64,
    max_file_bytes: u64,
    max_scratch_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_cache_bytes: Option<u64>,
}

#[derive(Debug, serde::Serialize)]
struct SandboxIdentities<'a> {
    runner: &'a str,
    backend: SandboxBackend,
    policy: String,
    toolchain: String,
    command: String,
    mount_plan: String,
}

#[derive(Debug, serde::Serialize)]
struct SandboxBackend {
    kind: &'static str,
    path: String,
    digest: String,
}

#[derive(Debug, serde::Serialize)]
struct SandboxGrant {
    source: String,
    destination: String,
    access: &'static str,
    purpose: &'static str,
    identity: String,
}

/// The approved toolchain for an execution, serialized to match the runner's
/// `system` toolchain shape.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SandboxToolchain {
    /// A generic system toolchain rooted at an approval-bound immutable root.
    System { identity: String, root: String },
}

/// Renders the approved effect carried by a tool intent into a closed
/// authoring-phase version-one request, bound to the installed runner and its
/// verified backend.
///
/// The intent's arguments carry only the non-empty `argv` array (the command
/// line). The workspace directory mounted read-write at `/workspace` and the
/// extra environment variables are host-authority values bound at construction
/// ([`ExecutionBackend::new`]); the model cannot redirect the workspace grant or
/// inject environment variables. A missing or malformed effect is a
/// [`Error::ToolRender`], which the runtime records as a not-executed refusal
/// before any dispatch.
fn render_request(
    intent: &ToolIntent,
    runner: &RunnerIdentity,
    backend: &BackendIdentity,
    workspace: &str,
    host_environment: &BTreeMap<String, String>,
) -> Result<Vec<u8>> {
    let arguments = intent
        .arguments
        .as_object()
        .ok_or_else(|| Error::ToolRender {
            tool: intent.name.clone(),
            reason: "the approved effect must be a JSON object".to_owned(),
        })?;
    let argv = arguments
        .get("argv")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::ToolRender {
            tool: intent.name.clone(),
            reason: "the approved effect must carry a non-empty `argv` array".to_owned(),
        })?;
    let argv: Vec<String> = argv
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::ToolRender {
                    tool: intent.name.clone(),
                    reason: "every `argv` entry must be a string".to_owned(),
                })
        })
        .collect::<Result<_>>()?;
    if argv.is_empty() {
        return Err(Error::ToolRender {
            tool: intent.name.clone(),
            reason: "the approved effect `argv` must not be empty".to_owned(),
        });
    }
    // The workspace directory and extra environment variables are host-authority
    // values bound at construction; the model supplies only the command line.
    // This keeps the model from redirecting the read-write workspace grant to an
    // arbitrary host path or injecting loader or credential variables into the
    // sandbox environment.
    let working_directory = workspace.to_owned();
    let mut environment: BTreeMap<String, String> = BTreeMap::from([
        ("HOME".to_owned(), WORKSPACE_DESTINATION.to_owned()),
        ("PATH".to_owned(), SYSTEM_PATH.to_owned()),
    ]);
    environment.extend(host_environment.clone());
    let grants = [
        SandboxGrant {
            source: SYSTEM_DESTINATION.to_owned(),
            destination: SYSTEM_DESTINATION.to_owned(),
            access: "read-only",
            purpose: "toolchain",
            identity: identity_digest(b"grant", SYSTEM_DESTINATION.as_bytes()),
        },
        SandboxGrant {
            source: working_directory.clone(),
            destination: WORKSPACE_DESTINATION.to_owned(),
            access: "read-write",
            purpose: "authoring",
            identity: identity_digest(b"grant", working_directory.as_bytes()),
        },
    ];
    // The command and mount-plan identities must match the runner's canonical
    // recomputation exactly, so derive them with the shared encodings.
    let command_identity = command_identity(&argv);
    let mount_plan = mount_plan_identity(&[
        (SYSTEM_DESTINATION.to_owned(), 0, 3),
        (WORKSPACE_DESTINATION.to_owned(), 1, 1),
    ]);
    let toolchain_identity = identity_digest(b"toolchain", SYSTEM_DESTINATION.as_bytes());
    let policy_identity = identity_digest(b"policy", command_identity.as_bytes());
    let request = SandboxRequest {
        protocol: REQUEST_PROTOCOL,
        protocol_version: PROTOCOL_VERSION,
        phase: "authoring",
        argv: &argv,
        working_directory: WORKSPACE_DESTINATION,
        environment: &environment,
        network: SandboxNetwork {
            mode: "deny",
            allowed_sources: Vec::new(),
        },
        resources: SandboxResources {
            wall_time_ms: DEFAULT_WALL_TIME_MS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_processes: 64,
            max_files: 4096,
            max_file_bytes: 256 * 1024 * 1024,
            max_scratch_bytes: 512 * 1024 * 1024,
            max_cache_bytes: None,
        },
        identities: SandboxIdentities {
            runner: &runner.digest,
            backend: SandboxBackend {
                kind: "bubblewrap",
                path: backend.path.clone(),
                digest: backend.digest.clone(),
            },
            policy: policy_identity,
            toolchain: toolchain_identity.clone(),
            command: command_identity,
            mount_plan,
        },
        grants: &grants,
        toolchain: SandboxToolchain::System {
            identity: toolchain_identity,
            root: SYSTEM_DESTINATION.to_owned(),
        },
        cache: None,
        scratch: None,
        home: None,
    };
    serde_json::to_vec(&request).map_err(|error| Error::ToolRender {
        tool: intent.name.clone(),
        reason: format!("cannot encode the version-one sandbox request: {error}"),
    })
}

/// Delegates one approved effect to the installed galla runner through its
/// version-one request protocol, supervising the runner with a bounded deadline
/// and output limit and reaping its process group on every outcome.
fn delegate(
    runner: &RunnerIdentity,
    request: &[u8],
    cancellation: &CancellationToken,
) -> Result<ToolOutcome> {
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let mut child = Command::new(&runner.canonical_path)
        .arg(REQUEST_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|source| Error::Host {
            reason: format!(
                "the installed galla runner `{}` could not be started for the delegated effect: {source}",
                runner.canonical_path
            ),
        })?;
    let pgid = i32::try_from(child.id()).map_err(|_| Error::Host {
        reason: "the galla runner process ID exceeds the supported range".to_owned(),
    })?;
    // Write the complete request before draining: the runner reads the whole
    // request from standard input before it produces output, so this cannot
    // deadlock against a full output pipe.
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(request)
            .and_then(|()| stdin.flush())
            .map_err(|source| Error::Host {
                reason: format!("cannot write the version-one sandbox request: {source}"),
            })?;
    }
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    // Non-blocking drains let the supervision loop poll cancellation and the
    // wall-time deadline even when the runner produces no output.
    if let Some(stream) = stdout.as_ref() {
        set_nonblocking(stream).map_err(|reason| Error::Host {
            reason: format!("cannot bound the galla runner standard output: {reason}"),
        })?;
    }
    if let Some(stream) = stderr.as_ref() {
        set_nonblocking(stream).map_err(|reason| Error::Host {
            reason: format!("cannot bound the galla runner standard error: {reason}"),
        })?;
    }
    let mut captured_stdout = Vec::new();
    let mut captured_stderr = Vec::new();
    let mut captured_total = 0usize;
    let mut output_limit_exceeded = false;
    let started = Instant::now();
    let mut timed_out = false;
    let mut cancelled = false;
    let deadline = started + Duration::from_millis(DEFAULT_WALL_TIME_MS);
    let status = loop {
        if cancellation.is_cancelled() {
            cancelled = true;
            break None;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break None;
        }
        output_limit_exceeded |= drain_bounded(
            &mut stdout,
            &mut captured_stdout,
            &mut captured_total,
            DEFAULT_MAX_OUTPUT_BYTES as usize,
        );
        output_limit_exceeded |= drain_bounded(
            &mut stderr,
            &mut captured_stderr,
            &mut captured_total,
            DEFAULT_MAX_OUTPUT_BYTES as usize,
        );
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => std::thread::sleep(SUPERVISION_POLL_INTERVAL),
            Err(source) => {
                return Err(Error::Host {
                    reason: format!("waiting for the galla runner failed: {source}"),
                });
            }
        }
    };
    // Reap the complete process group on every outcome, including timeout and
    // cancellation, so no runner descendant outlives the delegation.
    let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL);
    let status = match status {
        Some(status) => status,
        None => child.wait().map_err(|source| Error::Host {
            reason: format!("reaping the galla runner failed: {source}"),
        })?,
    };
    Ok(ToolOutcome {
        exited: status.code().is_some(),
        status: status.code(),
        stdout: captured_stdout,
        stderr: captured_stderr,
        timed_out,
        output_limit_exceeded,
        cancelled,
    })
}

/// Puts a piped stream into non-blocking mode so the supervision loop can poll
/// cancellation and the wall-time deadline without waiting on a silent runner.
fn set_nonblocking<F: std::os::fd::AsFd + ?Sized>(stream: &F) -> std::result::Result<(), String> {
    let current = nix::fcntl::fcntl(stream, nix::fcntl::FcntlArg::F_GETFL)
        .map_err(|error| format!("cannot read pipe flags: {error}"))?;
    nix::fcntl::fcntl(
        stream,
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::from_bits_truncate(
            current | nix::fcntl::OFlag::O_NONBLOCK.bits(),
        )),
    )
    .map(|_| ())
    .map_err(|error| format!("cannot set pipe non-blocking: {error}"))
}

/// Drains a piped stream into `captured` until the combined `captured_total`
/// reaches `limit`, returning whether the limit was exceeded.
fn drain_bounded<R: std::io::Read>(
    pipe: &mut Option<R>,
    captured: &mut Vec<u8>,
    captured_total: &mut usize,
    limit: usize,
) -> bool {
    let mut exceeded = false;
    if let Some(stream) = pipe.as_mut() {
        let remaining = limit.saturating_sub(*captured_total);
        if remaining == 0 {
            exceeded = true;
        } else {
            let mut buffer = [0u8; 8192];
            let budget = remaining.min(buffer.len());
            match stream.read(&mut buffer[..budget]) {
                Ok(0) => {
                    *pipe = None;
                }
                Ok(count) => {
                    captured.extend_from_slice(&buffer[..count]);
                    *captured_total = captured_total.saturating_add(count);
                    if *captured_total >= limit {
                        exceeded = true;
                    }
                }
                Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    *pipe = None;
                }
            }
        }
    }
    exceeded
}

impl ToolExecutor for ExecutionBackend {
    /// Host-facing description of the delegation, without granting authority.
    fn describe(&self, intent: &ToolIntent) -> String {
        format!(
            "delegate {} to the installed galla runner ({})",
            intent.name, self.runner.canonical_path
        )
    }

    /// Re-binds the runner and backend, renders the approved effect into a
    /// version-one request, and delegates it to the runner. A missing or
    /// drifted runner is a [`Error::RunnerUnavailable`], an incomplete backend
    /// is a [`Error::BackendUnavailable`], and both refuse before dispatch with
    /// no unconstrained host fallback.
    fn execute(
        &self,
        intent: &ToolIntent,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutcome> {
        self.rebind()?;
        let request = render_request(
            intent,
            &self.runner,
            &self.backend,
            &self.workspace,
            &self.environment,
        )?;
        delegate(&self.runner, &request, cancellation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_runner(contents: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galla-runner");
        std::fs::write(&path, contents).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        let _ = path;
        dir
    }

    #[test]
    fn resolves_and_hashes_a_regular_runner() {
        let dir = temp_runner(b"runner-bytes");
        let path = dir.path().join("galla-runner");
        let identity = resolve_runner(&path).unwrap();
        assert_eq!(
            identity.canonical_path,
            path.canonicalize().unwrap().to_string_lossy()
        );
        assert!(identity.digest.starts_with("sha256:"));
        assert_eq!(identity.digest.len(), "sha256:".len() + 64);
    }

    #[test]
    fn refuses_a_missing_runner_before_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent-runner");
        let error = resolve_runner(&missing).unwrap_err();
        assert!(matches!(error, Error::RunnerUnavailable { .. }));
    }

    #[test]
    fn refuses_a_symlinked_runner_before_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real-runner");
        std::fs::write(&target, b"runner-bytes").unwrap();
        let link = dir.path().join("linked-runner");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = resolve_runner(&link).unwrap_err();
        assert!(matches!(error, Error::RunnerUnavailable { .. }));
    }

    #[test]
    fn a_runner_that_fails_the_probe_is_backend_unavailable() {
        let dir = temp_runner(b"#!/bin/sh\nexit 3\n");
        let path = dir.path().join("galla-runner");
        let error = ExecutionBackend::new(&path, "/workspace", BTreeMap::new()).unwrap_err();
        assert!(matches!(error, Error::BackendUnavailable { .. }));
    }

    #[test]
    fn a_probe_that_reports_incomplete_capabilities_is_backend_unavailable() {
        // The probe self-attests the running executable's digest, which must
        // match the installed runner to reach the capabilities check. The script
        // computes its own digest at runtime (as a real galla probe does) and
        // reports incomplete namespace capabilities.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galla-runner");
        let script = "#!/bin/sh\ndigest=\"sha256:$(sha256sum \"$0\" | cut -d' ' -f1)\"\ncat <<EOF\n{\n  \"protocol\": \"kvist-sandbox-probe-v1\",\n  \"protocol_version\": 1,\n  \"runner\": { \"path\": \"/usr/bin/galla\", \"digest\": \"$digest\" },\n  \"backend\": { \"kind\": \"bubblewrap\", \"path\": \"/usr/bin/bwrap\", \"digest\": \"sha256:00\" },\n  \"capabilities\": {\n    \"namespaces\": {\n      \"mount\": true, \"network\": true, \"pid\": true,\n      \"ipc\": true, \"uts\": true, \"user\": false\n    },\n    \"new_session\": true,\n    \"parent_death_signal\": true\n  }\n}\nEOF\n";
        std::fs::write(&path, script).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        let error = ExecutionBackend::new(&path, "/workspace", BTreeMap::new()).unwrap_err();
        assert!(matches!(error, Error::BackendUnavailable { .. }));
    }

    #[test]
    fn command_identity_matches_the_runner_recomputation() {
        let argv = vec!["/usr/bin/true".to_owned()];
        let identity = command_identity(&argv);
        // Recompute independently: "command\0" + argv[0].
        let mut input = b"command\0".to_vec();
        input.extend_from_slice(b"/usr/bin/true");
        let expected = format!("sha256:{}", hex::encode(Sha256::digest(&input)));
        assert_eq!(identity, expected);
    }

    #[test]
    fn mount_plan_identity_encodes_destination_access_purpose() {
        let grants = [
            ("/usr".to_owned(), 0u8, 3u8),
            ("/workspace".to_owned(), 1, 1),
        ];
        let identity = mount_plan_identity(&grants);
        let mut plan = Vec::new();
        for (destination, access, purpose) in &grants {
            plan.extend_from_slice(destination.as_bytes());
            plan.push(b'\0');
            plan.push(*access);
            plan.push(*purpose);
            plan.push(b'\0');
        }
        let expected = format!("sha256:{}", hex::encode(Sha256::digest(&plan)));
        assert_eq!(identity, expected);
    }

    #[test]
    fn capabilities_complete_requires_every_namespace_and_session_flag() {
        let complete = Capabilities {
            namespaces: Namespaces {
                mount: true,
                network: true,
                pid: true,
                ipc: true,
                uts: true,
                user: true,
            },
            new_session: true,
            parent_death_signal: true,
        };
        assert!(complete.complete());
        let missing_user = Capabilities {
            namespaces: Namespaces {
                user: false,
                ..complete.namespaces
            },
            ..complete
        };
        assert!(!missing_user.complete());
    }

    #[test]
    fn render_request_requires_a_non_empty_argv() {
        let dir = temp_runner(b"runner-bytes");
        let path = dir.path().join("galla-runner");
        let runner = resolve_runner(&path).unwrap();
        let backend = BackendIdentity {
            kind: "bubblewrap".to_owned(),
            path: "/usr/bin/bwrap".to_owned(),
            digest: "sha256:00".to_owned(),
        };
        let intent = ToolIntent {
            id: "call-1".into(),
            provider_id: None,
            name: "run".into(),
            arguments: serde_json::json!({"working_directory": "/workspace"}),
        };
        let error =
            render_request(&intent, &runner, &backend, "/workspace", &BTreeMap::new()).unwrap_err();
        assert!(matches!(error, Error::ToolRender { .. }));
    }

    #[test]
    fn render_request_builds_a_closed_authoring_request() {
        let dir = temp_runner(b"runner-bytes");
        let path = dir.path().join("galla-runner");
        let runner = resolve_runner(&path).unwrap();
        let backend = BackendIdentity {
            kind: "bubblewrap".to_owned(),
            path: "/usr/bin/bwrap".to_owned(),
            digest: "sha256:00".to_owned(),
        };
        let intent = ToolIntent {
            id: "call-1".into(),
            provider_id: None,
            name: "run".into(),
            arguments: serde_json::json!({
                "argv": ["/usr/bin/true"],
                "working_directory": "/workspace"
            }),
        };
        let bytes =
            render_request(&intent, &runner, &backend, "/workspace", &BTreeMap::new()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["protocol"], REQUEST_PROTOCOL);
        assert_eq!(value["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(value["phase"], "authoring");
        assert_eq!(value["working_directory"], WORKSPACE_DESTINATION);
        assert_eq!(value["network"]["mode"], "deny");
        assert_eq!(value["identities"]["runner"], runner.digest);
        assert_eq!(value["identities"]["backend"]["kind"], "bubblewrap");
        assert_eq!(value["toolchain"]["kind"], "system");
        // The command and mount-plan identities must equal the canonical
        // recomputation the runner performs.
        let argv = vec!["/usr/bin/true".to_owned()];
        assert_eq!(value["identities"]["command"], command_identity(&argv));
        let grants = [
            (SYSTEM_DESTINATION.to_owned(), 0u8, 3u8),
            (WORKSPACE_DESTINATION.to_owned(), 1, 1),
        ];
        assert_eq!(
            value["identities"]["mount_plan"],
            mount_plan_identity(&grants)
        );
    }

    #[test]
    fn render_request_ignores_model_workspace_and_environment() {
        let dir = temp_runner(b"runner-bytes");
        let path = dir.path().join("galla-runner");
        let runner = resolve_runner(&path).unwrap();
        let backend = BackendIdentity {
            kind: "bubblewrap".to_owned(),
            path: "/usr/bin/bwrap".to_owned(),
            digest: "sha256:00".to_owned(),
        };
        // The model attempts to redirect the read-write workspace grant to an
        // arbitrary host path and to inject a loader variable into the sandbox
        // environment. Neither is host authority: the host-bound values win.
        let intent = ToolIntent {
            id: "call-1".into(),
            provider_id: None,
            name: "run".into(),
            arguments: serde_json::json!({
                "argv": ["/usr/bin/true"],
                "working_directory": "/etc",
                "environment": {"LD_PRELOAD": "/tmp/evil.so"}
            }),
        };
        let host_environment = BTreeMap::from([("HOST_VAR".to_owned(), "host".to_owned())]);
        let bytes = render_request(
            &intent,
            &runner,
            &backend,
            "/home/host/project",
            &host_environment,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // The workspace grant source is the host-bound directory, not the
        // model's `/etc`.
        let workspace_grant = value["grants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|grant| grant["destination"] == WORKSPACE_DESTINATION)
            .unwrap();
        assert_eq!(workspace_grant["source"], "/home/host/project");
        // The environment carries the host variable and the fixed HOME/PATH,
        // but not the model's injected loader variable.
        assert_eq!(value["environment"]["HOST_VAR"], "host");
        assert_eq!(value["environment"]["HOME"], WORKSPACE_DESTINATION);
        assert_eq!(value["environment"]["PATH"], SYSTEM_PATH);
        assert!(value["environment"].get("LD_PRELOAD").is_none());
    }
}
