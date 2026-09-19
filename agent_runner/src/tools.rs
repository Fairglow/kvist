//! Model-facing tools and their mapping to sandbox-executed commands.
//!
//! Tools never execute directly. Each [`ToolRegistry::render`] call produces an
//! argv (with an absolute canonical program) that the sandbox executes, plus an
//! optional staged write that lets the agent create files larger than the
//! sandbox argv byte limit without leaving the write root.

use std::path::PathBuf;

use agent_runtime::{ToolDefinition, ToolIntent};
use serde_json::Value;

// Re-export the policy type so consumers can refer to it as `agent_runner::tools::ToolPolicy`.
pub use crate::config::ToolPolicy;

/// The sandbox path every staged write lands under before it is moved into
/// place. Kept hidden under the write root so it does not collide with user
/// files and stays within the writable scope.
const STAGING_DIR: &str = "/.agent-writes";
/// The number of leading characters kept in a tool summary.
const MAX_SUMMARY_BYTES: usize = 120;
/// The number of leading characters kept in a path summary.
const MAX_PATH_SUMMARY_CHARS: usize = 60;

/// Per-call execution context shared by rendering and execution.
#[derive(Debug, Clone)]
pub struct ExecContext {
    /// The host working directory, source of the sandbox write root.
    pub workdir: PathBuf,
    /// A unique identifier for the current tool call.
    pub call_id: String,
}

impl ExecContext {
    /// Builds an execution context for one tool call.
    pub fn new(workdir: impl Into<PathBuf>, call_id: impl Into<String>) -> Self {
        ExecContext {
            workdir: workdir.into(),
            call_id: call_id.into(),
        }
    }
}

/// A staged write: content is written to `host_path` on the host before the
/// rendered argv runs, and the argv then moves `sandbox_path` into place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedWrite {
    /// The host path content must be written to before execution.
    pub host_path: PathBuf,
    /// The sandbox path the staged file appears at.
    pub sandbox_path: String,
    /// The final target path inside the sandbox.
    pub target: String,
}

/// The result of rendering a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTool {
    /// The argv to execute inside the sandbox; `argv[0]` is absolute.
    pub argv: Vec<String>,
    /// A short human description shown in the UI and logs.
    pub summary: String,
    /// Present only for staged writes (`write_file`).
    pub staged_write: Option<StagedWrite>,
}

/// Language tool profiles known to the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProfile {
    /// No language assumptions; generic Linux tools only.
    Generic,
    /// The Rust toolchain (cargo, rustc).
    Rust,
    /// The Python toolchain (python3, pip).
    Python,
}

impl ToolProfile {
    /// The profile identifier accepted in configuration and on the CLI.
    pub const fn id(self) -> &'static str {
        match self {
            ToolProfile::Generic => "generic",
            ToolProfile::Rust => "rust",
            ToolProfile::Python => "python",
        }
    }

    /// A human description of the toolchain the profile surfaces.
    pub const fn toolkit(self) -> &'static str {
        match self {
            ToolProfile::Generic => "coreutils, git, and common Linux tools",
            ToolProfile::Rust => "cargo, rustc, and the Rust standard toolchain",
            ToolProfile::Python => "python3, pip, and common Python build tools",
        }
    }

    /// Parses a profile identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "generic" => Some(Self::Generic),
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            _ => None,
        }
    }
}

/// The registry of model-facing tools and their sandbox rendering.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    bash: PathBuf,
    policy: ToolPolicy,
    profiles: Vec<ToolProfile>,
}

impl ToolRegistry {
    /// Builds a registry with the built-in minimal (generic) profile set.
    pub fn new(policy: ToolPolicy) -> Self {
        ToolRegistry {
            bash: PathBuf::from("/usr/bin/bash"),
            policy,
            profiles: vec![ToolProfile::Generic],
        }
    }

    /// Overrides the resolved `bash` path (used by tests).
    pub fn with_bash(mut self, bash: impl Into<PathBuf>) -> Self {
        self.bash = bash.into();
        self
    }

    /// Adds language tool profiles to the registry.
    pub fn with_profiles(mut self, profiles: Vec<ToolProfile>) -> Self {
        if !profiles.is_empty() {
            self.profiles = profiles;
        }
        self
    }

    /// The tool authority policy backing this registry.
    pub fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    /// The enabled tool profile identifiers, sorted for determinism.
    pub fn profiles(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = self.profiles.iter().map(|p| p.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The tool definitions exposed to the model, in stable order.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let toolkit = self
            .profiles
            .iter()
            .map(|p| p.toolkit())
            .collect::<Vec<_>>()
            .join(", ");
        vec![
            ToolDefinition {
                name: "shell".to_owned(),
                description: format!(
                    "Run a shell command inside the sandbox. Use it to build, test, query, and \
                     manipulate the project. Available tooling: {toolkit}. Commands run under the \
                     working directory scope; writes outside it are refused."
                ),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to run."
                        }
                    },
                    "required": ["command"],
                }),
            },
            ToolDefinition {
                name: "read_file".to_owned(),
                description: "Read a text file inside the sandbox and return its contents."
                    .to_owned(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The absolute path of the file to read."
                        }
                    },
                    "required": ["path"],
                }),
            },
            ToolDefinition {
                name: "write_file".to_owned(),
                description:
                    "Write content to a file inside the working directory (create or overwrite)."
                        .to_owned(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The absolute path of the file to write, under the working directory."
                        },
                        "content": {
                            "type": "string",
                            "description": "The full content to write to the file."
                        }
                    },
                    "required": ["path", "content"],
                }),
            },
            ToolDefinition {
                name: "list_dir".to_owned(),
                description: "List the entries of a directory inside the sandbox.".to_owned(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The absolute path of the directory to list."
                        }
                    },
                    "required": ["path"],
                }),
            },
        ]
    }

    /// Resolves the canonical `bash` path and builds a registry.
    pub fn discover(policy: ToolPolicy) -> crate::error::Result<Self> {
        let bash = crate::sandbox::resolve_executable("bash")?;
        Ok(ToolRegistry::new(policy).with_bash(bash))
    }

    /// Renders a tool intent to a sandbox argv.
    pub fn render(
        &self,
        intent: &ToolIntent,
        context: &ExecContext,
    ) -> crate::error::Result<RenderedTool> {
        match intent.name.as_str() {
            "shell" => self.render_shell(intent),
            "read_file" => self.render_read(intent),
            "write_file" => self.render_write(intent, context),
            "list_dir" => self.render_list(intent),
            other => Err(crate::error::Error::ToolRender {
                tool: other.to_owned(),
                reason: "unknown or disabled tool".to_owned(),
            }),
        }
    }

    fn render_shell(&self, intent: &ToolIntent) -> crate::error::Result<RenderedTool> {
        let command = string_arg(intent, "command")?;
        if command.trim().is_empty() {
            return Err(crate::error::Error::ToolRender {
                tool: "shell".to_owned(),
                reason: "command must not be empty".to_owned(),
            });
        }
        if !self.policy.shell_permitted(&command) {
            Err(crate::error::Error::ToolPolicy {
                tool: "shell".to_owned(),
                reason: "command matches the forbidden-command policy".to_owned(),
            })
        } else {
            Ok(RenderedTool {
                argv: vec![
                    self.bash.to_string_lossy().into_owned(),
                    "-c".to_owned(),
                    command.clone(),
                    "agent-runner".to_owned(),
                ],
                summary: summarize(&command),
                staged_write: None,
            })
        }
    }

    fn render_read(&self, intent: &ToolIntent) -> crate::error::Result<RenderedTool> {
        let path = string_arg(intent, "path")?;
        Ok(RenderedTool {
            argv: vec![
                self.bash.to_string_lossy().into_owned(),
                "-c".to_owned(),
                "exec cat -- \"$1\"".to_owned(),
                "agent-runner".to_owned(),
                path.clone(),
            ],
            summary: format!("read {}", summarize_path(&path)),
            staged_write: None,
        })
    }

    fn render_list(&self, intent: &ToolIntent) -> crate::error::Result<RenderedTool> {
        let path = string_arg(intent, "path")?;
        Ok(RenderedTool {
            argv: vec![
                self.bash.to_string_lossy().into_owned(),
                "-c".to_owned(),
                "exec ls -la -- \"$1\"".to_owned(),
                "agent-runner".to_owned(),
                path.clone(),
            ],
            summary: format!("list {}", summarize_path(&path)),
            staged_write: None,
        })
    }

    fn render_write(
        &self,
        intent: &ToolIntent,
        context: &ExecContext,
    ) -> crate::error::Result<RenderedTool> {
        let path = string_arg(intent, "path")?;
        if !path.starts_with(&self.policy.write_root) {
            return Err(crate::error::Error::ToolPolicy {
                tool: "write_file".to_owned(),
                reason: format!(
                    "target `{path}` is outside the write root `{}`",
                    self.policy.write_root
                ),
            });
        }
        // The content is staged inside the working directory (already mounted
        // read-write as the sandbox write root) so arbitrarily large files can
        // be written without exceeding the sandbox argv byte limit, then moved
        // into place.
        let sandbox_staging = format!("{STAGING_DIR}/{}", context.call_id);
        let host_staging = context.workdir.join(STAGING_DIR).join(&context.call_id);
        Ok(RenderedTool {
            argv: vec![
                self.bash.to_string_lossy().into_owned(),
                "-c".to_owned(),
                "exec mv -f -- \"$1\" \"$2\"".to_owned(),
                "agent-runner".to_owned(),
                sandbox_staging.clone(),
                path.clone(),
            ],
            summary: format!("write {}", summarize_path(&path)),
            staged_write: Some(StagedWrite {
                host_path: host_staging,
                sandbox_path: sandbox_staging,
                target: path,
            }),
        })
    }
}

fn string_arg(intent: &ToolIntent, field: &str) -> crate::error::Result<String> {
    match intent.arguments.get(field) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Null) | None => Err(crate::error::Error::ToolRender {
            tool: intent.name.clone(),
            reason: format!("missing required string argument `{field}`"),
        }),
        Some(other) => Err(crate::error::Error::ToolRender {
            tool: intent.name.clone(),
            reason: format!(
                "argument `{field}` must be a string, found {}",
                value_kind(other)
            ),
        }),
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "null",
    }
}

fn summarize_path(path: &str) -> String {
    let kept: String = path.chars().take(MAX_PATH_SUMMARY_CHARS).collect();
    if kept.chars().count() == path.chars().count() {
        kept
    } else {
        format!("…{kept}")
    }
}

fn summarize(command: &str) -> String {
    let first: String = command
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_SUMMARY_BYTES)
        .collect();
    if first.is_empty() {
        "<shell command>".to_owned()
    } else {
        first
    }
}
