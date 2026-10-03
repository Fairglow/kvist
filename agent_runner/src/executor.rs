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

use agent_runtime::{CancellationToken, ToolIntent};

use crate::config::SandboxPaths;
use crate::error::{Error, Result, io_error};
use crate::file_tools::{FileRequest, canonical_path};
use crate::sandbox::{BuildRequest, ToolOutcome, default_resources, execute};
use crate::session::ToolExecutor;
use crate::tools::{ExecContext, ToolRegistry};

/// Executes tool intents inside the sandbox.
pub struct SandboxExecutor {
    registry: ToolRegistry,
    sandbox: SandboxPaths,
    working_directory: PathBuf,
}

impl SandboxExecutor {
    /// Builds an executor from a registry, sandbox paths, and the working
    /// directory the agent may write to.
    pub fn new(registry: ToolRegistry, sandbox: SandboxPaths, working_directory: PathBuf) -> Self {
        SandboxExecutor {
            registry,
            sandbox,
            working_directory,
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
}

impl<T: ToolExecutor + ?Sized> ToolExecutor for Arc<T> {
    fn execute(
        &self,
        intent: &ToolIntent,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutcome> {
        self.as_ref().execute(intent, cancellation)
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
            Some(resolve_file_helper(
                rendered.file_helper.as_deref(),
                &self.working_directory,
            )?)
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
        )?;
        check_cancelled(cancellation)?;
        execute(&self.sandbox, &request, cancellation)
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
            reason: "install agent-runner-file-tool as a regular non-link file".to_owned(),
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
        .prefix(".agent-runner-payload-")
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
