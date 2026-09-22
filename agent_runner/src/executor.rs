//! The sandbox-backed tool executor.
//!
//! [`SandboxExecutor`] renders a tool intent, stages any write content inside the
//! working directory, builds one sandbox request, and executes it. It is the only
//! place that turns model tool intents into process effects, and every effect goes
//! through the sandbox.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_runtime::{CancellationToken, ToolIntent};

use crate::config::SandboxPaths;
use crate::error::{Result, io_error};
use crate::sandbox::{BuildRequest, ToolOutcome, build_request, default_resources, execute};
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
        let context = ExecContext::new(self.working_directory.clone(), intent.id.clone());
        let rendered = self.registry.render(intent, &context)?;

        // Stage write content on the host, inside the working directory, before
        // building the request so the staged file is already mounted read-write.
        let staged_host_path = match &rendered.staged_write {
            Some(staged) => {
                let path = self.stage_content(staged.host_path.clone(), intent)?;
                Some(path)
            }
            None => None,
        };

        let argv = rendered.argv.clone();
        let build = BuildRequest {
            argv: &argv,
            working_directory: &self.working_directory,
            read_roots: &[],
            environment: BTreeMap::new(),
            policy: self.registry.policy(),
            resources: default_resources(),
        };
        let request = build_request(&self.sandbox, &build)?;

        let outcome = execute(&self.sandbox, &request, cancellation);

        // Best-effort cleanup of any staged content a failed write left behind.
        if let Some(path) = staged_host_path {
            let _ = std::fs::remove_file(&path);
        }

        outcome
    }
}

impl SandboxExecutor {
    fn stage_content(&self, host_path: PathBuf, intent: &ToolIntent) -> Result<PathBuf> {
        let content = intent
            .arguments
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if let Some(parent) = host_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| {
                io_error(
                    "create staging directory for the write",
                    Some(&parent.to_string_lossy()),
                    source,
                )
            })?;
        }
        std::fs::write(&host_path, content.as_bytes()).map_err(|source| {
            io_error(
                "stage write content",
                Some(&host_path.to_string_lossy()),
                source,
            )
        })?;
        tracing::debug!(path = ?host_path, "staged write content in the working directory");
        Ok(host_path)
    }
}
