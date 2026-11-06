//! The sandbox-backed tool executor.
//!
//! [`SandboxExecutor`] renders a tool intent, privately stages a bounded request
//! outside the writable workspace, builds one sandbox request, and executes it.
//! Rendering and validation never create model-selected host files. It is the only
//! place that turns model tool intents into process effects, and every effect goes
//! through the sandbox.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sav::loop_detection::ProgressEvidence;
use sav::{CancellationToken, ToolIntent};

use crate::config::SandboxPaths;
use crate::error::{Error, Result, io_error};
use crate::file_tools::{FileRequest, canonical_path};
use crate::sandbox::{BuildRequest, ToolOutcome, default_resources, execute};
use crate::session::ToolExecutor;
use crate::tools::{ExecContext, ToolRegistry};

mod progress;
pub(crate) use progress::NativeProgress;

/// Executes tool intents inside the sandbox.
pub struct SandboxExecutor {
    registry: ToolRegistry,
    sandbox: SandboxPaths,
    working_directory: PathBuf,
    progress: NativeProgress,
}

impl SandboxExecutor {
    /// Builds an executor from a registry, sandbox paths, and the working
    /// directory the agent may write to.
    pub fn new(registry: ToolRegistry, sandbox: SandboxPaths, working_directory: PathBuf) -> Self {
        SandboxExecutor {
            registry,
            sandbox,
            working_directory,
            progress: NativeProgress::default(),
        }
    }

    /// The working directory this executor writes to.
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// The enabled tool profiles.
    pub fn profiles(&self) -> Vec<&'static str> {
        self.registry.profiles()
    }

    fn execute_prepared(
        &self,
        intent: &ToolIntent,
        file_request: Option<&FileRequest>,
        request: &galla::protocol::SandboxRequest,
        cancellation: &CancellationToken,
        execute: impl FnOnce() -> Result<ToolOutcome>,
    ) -> Result<ToolOutcome> {
        check_cancelled(cancellation)?;
        let prepared = file_request.map(|file_request| {
            let mut authoring = request.grants.iter().filter(|grant| {
                grant.purpose == galla::protocol::Purpose::Authoring
                    && grant.access == galla::protocol::Access::ReadWrite
            });
            let grant = authoring.next().ok_or_else(|| Error::SandboxBuild {
                reason: "prepared sandbox request has no writable authoring scope".into(),
            })?;
            if authoring.next().is_some() || grant.destination != request.working_directory {
                return Err(Error::SandboxBuild {
                    reason: "prepared sandbox authoring scope is ambiguous or differs from its working directory".into(),
                });
            }
            file_request.sandbox_mapped(&request.working_directory, Path::new(&grant.source))
        }).transpose().map_err(|error| Error::SandboxExec {
            tool: Some(intent.name.clone()),
            reason: format!(
                "native sandbox observation preparation failed (not executed): {error}; \
                 reconcile the prepared scope before further work; no automatic replay"
            ),
        })?;
        self.progress
            .execute_prepared(intent, prepared.as_ref(), cancellation, execute)
    }
}

impl<T: ToolExecutor + ?Sized> ToolExecutor for Arc<T> {
    fn execute(
        &self,
        intent: &ToolIntent,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutcome> {
        self.as_ref().execute(intent, cancellation)
    }

    fn progress(&self, intent: &ToolIntent, outcome: &ToolOutcome) -> Result<ProgressEvidence> {
        self.as_ref().progress(intent, outcome)
    }
}

impl ToolExecutor for SandboxExecutor {
    fn execute(
        &self,
        intent: &ToolIntent,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutcome> {
        check_cancelled(cancellation)?;
        let context = ExecContext::new(self.working_directory.clone(), intent.id.clone());
        let rendered = self.registry.render(intent, &context)?;
        let helper = if rendered.file_request.is_some() {
            Some(
                resolve_file_helper(rendered.file_helper.as_deref(), &self.working_directory)
                    .map_err(|error| prerequisite_error(intent, error, cancellation))?,
            )
        } else {
            None
        };
        check_cancelled(cancellation)?;
        // These RAII owners survive building/spawning and drop on every error.
        // Neither the model's path nor its call ID chooses a staging name.
        let payload = rendered
            .file_request
            .as_ref()
            .map(|request| stage_file_request(request, &self.working_directory, cancellation))
            .transpose()?;
        // A shell command too large for one argv entry travels as the /context/0
        // read-only script file its argv references; the render contract keeps
        // it mutually exclusive with the native file payload.
        let script = rendered
            .shell_script
            .as_deref()
            .map(|script| stage_bytes(script.as_bytes(), &self.working_directory, cancellation))
            .transpose()?;
        let mut read_roots: Vec<PathBuf> = Vec::new();
        if let (Some(payload), Some(helper)) = (&payload, &helper) {
            read_roots.push(payload.path().to_owned());
            read_roots.push(helper.clone());
        } else if let Some(script) = &script {
            read_roots.push(script.path().to_owned());
        }
        let argv = rendered.argv.clone();
        let build = BuildRequest {
            argv: &argv,
            working_directory: &self.working_directory,
            read_roots: &read_roots,
            environment: BTreeMap::new(),
            policy: self.registry.policy(),
            resources: default_resources(),
        };
        let request = crate::sandbox::build_request_with_rust(
            &self.sandbox,
            &build,
            self.registry.rust_environment(),
            cancellation,
        )
        .map_err(|error| prerequisite_error(intent, error, cancellation))?;
        check_cancelled(cancellation)?;
        self.execute_prepared(
            intent,
            rendered.file_request.as_ref(),
            &request,
            cancellation,
            || {
                check_cancelled(cancellation)?;
                execute(&self.sandbox, &request, cancellation)
            },
        )
    }

    fn progress(&self, intent: &ToolIntent, outcome: &ToolOutcome) -> Result<ProgressEvidence> {
        self.progress.progress(intent, outcome)
    }
}

pub(crate) fn check_cancelled(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(Error::SandboxExec {
            tool: None,
            reason: "cancelled before staging or process execution".to_owned(),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn prerequisite_error(
    intent: &ToolIntent,
    error: Error,
    cancellation: &CancellationToken,
) -> Error {
    if cancellation.is_cancelled()
        || matches!(
            error,
            Error::SandboxUnavailable { .. }
                | Error::SandboxBuild { .. }
                | Error::ToolchainUnavailable { .. }
        )
    {
        // Direct callers retain established domains; the native bridge owns
        // their adaptation to its typed prerequisite diagnostic.
        error
    } else {
        Error::ToolPrerequisite {
            tool: intent.name.clone(),
            reason: error.describe(),
        }
    }
}

pub(crate) fn resolve_file_helper(configured: Option<&Path>, workdir: &Path) -> Result<PathBuf> {
    let helper = match configured {
        Some(path) => path.to_owned(),
        None => crate::tools::default_file_helper_path()?,
    };
    let display = helper.to_str().ok_or_else(|| Error::SandboxBuild {
        reason: "helper path must be UTF-8".to_owned(),
    })?;
    canonical_path(display)?;
    let mut prefix = PathBuf::from("/");
    for part in helper.components().filter_map(|c| match c {
        std::path::Component::Normal(part) => Some(part),
        _ => None,
    }) {
        prefix.push(part);
        if std::fs::symlink_metadata(&prefix)
            .map_err(|e| {
                io_error(
                    "inspect native file helper path",
                    Some(&prefix.to_string_lossy()),
                    e,
                )
            })?
            .file_type()
            .is_symlink()
        {
            return Err(Error::SandboxBuild {
                reason: "native file helper and its ancestors must not be symbolic links"
                    .to_owned(),
            });
        }
    }
    let metadata = std::fs::symlink_metadata(&helper)
        .map_err(|e| io_error("inspect native file helper", Some(display), e))?;
    if !metadata.is_file() {
        return Err(Error::SandboxBuild {
            reason: "install skott-file-tool as a regular non-link file".to_owned(),
        });
    }
    let workdir = workdir
        .canonicalize()
        .map_err(|e| io_error("resolve file-tool workspace", None, e))?;
    if helper.starts_with(&workdir) {
        return Err(Error::SandboxBuild {
            reason: "native file helper must be installed outside the writable workspace"
                .to_owned(),
        });
    }
    Ok(helper)
}

pub(crate) struct StagedFileRequest {
    file: tempfile::NamedTempFile,
    _directory: tempfile::TempDir,
}

impl StagedFileRequest {
    pub(crate) fn path(&self) -> &Path {
        self.file.path()
    }
}

pub(crate) fn stage_file_request(
    request: &FileRequest,
    workdir: &Path,
    cancellation: &CancellationToken,
) -> Result<StagedFileRequest> {
    let bytes = request.to_bytes()?;
    stage_bytes(&bytes, workdir, cancellation)
}

/// Privately stages bounded bytes outside the writable workspace as a
/// mode-0600 regular file inside a mode-0700 directory, with RAII cleanup.
/// The directory is the workspace's canonical parent, so the staged file can
/// never overlap the writable mount.
pub(crate) fn stage_bytes(
    bytes: &[u8],
    workdir: &Path,
    cancellation: &CancellationToken,
) -> Result<StagedFileRequest> {
    check_cancelled(cancellation)?;
    let workdir = workdir
        .canonicalize()
        .map_err(|e| io_error("resolve payload staging scope", None, e))?;
    // The writable workspace's canonical parent is outside its mount. Refuse
    // an unwritable parent rather than staging inside the workspace or falling
    // back to a model-selected location. No global temporary directory is used.
    let parent = workdir.parent().ok_or_else(|| Error::SandboxBuild {
        reason: "cannot privately stage a file request outside a root filesystem workspace"
            .to_owned(),
    })?;
    check_cancelled(cancellation)?;
    let directory = tempfile::Builder::new()
        .prefix(".skott-payload-")
        .tempdir_in(parent)
        .map_err(|e| {
            io_error(
                "create private payload outside workspace (parent must be writable)",
                None,
                e,
            )
        })?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|e| io_error("set private native payload directory permissions", None, e))?;
    check_cancelled(cancellation)?;
    let mut file = tempfile::Builder::new()
        .prefix("request-")
        .tempfile_in(directory.path())
        .map_err(|e| io_error("create native payload in its private directory", None, e))?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|e| io_error("set private native payload file permissions", None, e))?;
    file.write_all(bytes)
        .map_err(|e| io_error("stage bounded native file payload", None, e))?;
    file.as_file()
        .sync_all()
        .map_err(|e| io_error("synchronize native file payload", None, e))?;
    Ok(StagedFileRequest {
        file,
        _directory: directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct PreparedSelection {
        _directory: tempfile::TempDir,
        executor: SandboxExecutor,
        sandbox_request: galla::protocol::SandboxRequest,
        file_request: FileRequest,
        intent: ToolIntent,
        original: PathBuf,
        rebound: PathBuf,
    }

    fn prepared_selection() -> PreparedSelection {
        let directory = tempfile::tempdir_in(".").unwrap();
        let parent = directory.path().canonicalize().unwrap();
        let original = parent.join("original");
        let rebound = parent.join("rebound");
        let selected = parent.join("selected");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(&rebound).unwrap();
        std::fs::write(original.join("text"), "before").unwrap();
        std::fs::write(rebound.join("text"), "unrelated").unwrap();
        std::os::unix::fs::symlink(&original, &selected).unwrap();
        let sandbox = SandboxPaths {
            runner: parent.join("runner"),
            backend: parent.join("backend"),
        };
        std::fs::write(&sandbox.runner, "fixture runner").unwrap();
        std::fs::write(&sandbox.backend, "fixture backend").unwrap();
        let policy = crate::config::ToolPolicy::minimum();
        let argv = vec!["/usr/bin/true".into()];
        let sandbox_request = crate::sandbox::build_request(
            &sandbox,
            &BuildRequest {
                argv: &argv,
                working_directory: &selected,
                read_roots: &[],
                environment: BTreeMap::new(),
                policy: &policy,
                resources: default_resources(),
            },
        )
        .unwrap();
        let grant = sandbox_request
            .grants
            .iter()
            .find(|grant| grant.purpose == galla::protocol::Purpose::Authoring)
            .unwrap();
        assert_eq!(grant.source, original.to_str().unwrap());
        std::fs::remove_file(&selected).unwrap();
        std::os::unix::fs::symlink(&rebound, &selected).unwrap();
        let intent = ToolIntent {
            id: "prepared-sandbox".into(),
            provider_id: None,
            name: "write_file".into(),
            arguments: json!({"path":"/workspace/text","content":"changed"}),
        };
        let file_request =
            FileRequest::new("/workspace", &intent.name, intent.arguments.clone()).unwrap();
        PreparedSelection {
            _directory: directory,
            executor: SandboxExecutor::new(ToolRegistry::new(policy), sandbox, selected),
            sandbox_request,
            file_request,
            intent,
            original,
            rebound,
        }
    }

    fn successful_fixture_outcome() -> ToolOutcome {
        ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"completed".to_vec(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    #[test]
    fn sandbox_prepared_scope_observes_the_executed_grant_not_a_rebound_selection() {
        let fixture = prepared_selection();
        let actual = fixture.file_request.host_mapped(&fixture.original).unwrap();
        let out = fixture
            .executor
            .execute_prepared(
                &fixture.intent,
                Some(&fixture.file_request),
                &fixture.sandbox_request,
                &CancellationToken::new(),
                || {
                    crate::file_tools::execute_file_request(&actual).unwrap();
                    Ok(successful_fixture_outcome())
                },
            )
            .unwrap();
        assert_eq!(
            fixture.executor.progress(&fixture.intent, &out).unwrap(),
            ProgressEvidence::StateChanged
        );
        assert_eq!(
            std::fs::read(fixture.original.join("text")).unwrap(),
            b"changed"
        );
        assert_eq!(
            std::fs::read(fixture.rebound.join("text")).unwrap(),
            b"unrelated"
        );
    }

    #[test]
    fn sandbox_prepared_scope_never_certifies_a_change_in_the_rebound_selection() {
        let fixture = prepared_selection();
        let out = fixture
            .executor
            .execute_prepared(
                &fixture.intent,
                Some(&fixture.file_request),
                &fixture.sandbox_request,
                &CancellationToken::new(),
                || {
                    std::fs::write(fixture.rebound.join("text"), "unrelated external edit")
                        .unwrap();
                    Ok(successful_fixture_outcome())
                },
            )
            .unwrap();
        assert_eq!(
            fixture.executor.progress(&fixture.intent, &out).unwrap(),
            ProgressEvidence::None
        );
        assert_eq!(
            std::fs::read(fixture.original.join("text")).unwrap(),
            b"before"
        );
    }

    #[test]
    fn sandbox_prepared_scope_link_rebind_is_rejected_before_effects() {
        let fixture = prepared_selection();
        let moved = fixture.original.with_file_name("moved");
        std::fs::rename(&fixture.original, &moved).unwrap();
        std::os::unix::fs::symlink(&fixture.rebound, &fixture.original).unwrap();
        let executed = std::cell::Cell::new(false);
        let result = fixture.executor.execute_prepared(
            &fixture.intent,
            Some(&fixture.file_request),
            &fixture.sandbox_request,
            &CancellationToken::new(),
            || {
                executed.set(true);
                Ok(successful_fixture_outcome())
            },
        );
        assert!(result.is_err(), "prepared source alias must be refused");
        let error = result.unwrap_err();
        assert!(error.describe().contains("not executed"), "{error}");
        assert!(error.describe().contains("reconcile"), "{error}");
        assert!(!executed.get());
        assert_eq!(std::fs::read(moved.join("text")).unwrap(), b"before");
        assert_eq!(
            std::fs::read(fixture.rebound.join("text")).unwrap(),
            b"unrelated"
        );
    }

    #[test]
    fn stage_bytes_creates_private_payload_outside_workspace() {
        let parent = tempfile::tempdir().unwrap();
        let workdir = parent.path().join("work");
        std::fs::create_dir(&workdir).unwrap();
        let cancellation = CancellationToken::new();
        let staged = stage_bytes(b"payload-bytes", &workdir, &cancellation).unwrap();
        let path = staged.path().to_owned();
        assert_eq!(std::fs::read(&path).unwrap(), b"payload-bytes");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert!(!path.starts_with(&workdir));
    }
}
