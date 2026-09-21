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

/// The staging directory in the sandbox namespace, where a leading `/` is the
/// sandbox root (the mounted write root). Kept hidden so it does not collide
/// with user files and stays inside the writable scope.
const STAGING_DIR: &str = "/.agent-writes";
/// The staging directory on the host: the sandbox form [`STAGING_DIR`] with the
/// leading slash removed, so it joins under the working directory instead of the
/// filesystem root. `PathBuf::join` treats an absolute path as a full
/// replacement, so the host path must stay relative.
const HOST_STAGING_DIR: &str = ".agent-writes";
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
    /// Builds a registry advertising the built-in generic Linux tooling plus
    /// the Rust/Cargo toolchain. Building and testing the project are essential
    /// agent capabilities, so the Rust toolchain is advertised by default; the
    /// sandbox enforces exactly a `System` and a `Cargo` toolchain, so these are
    /// the honest defaults. Python is also advertised by default: it runs under
    /// the generic `System` path from the read-only host layout, and `python3`
    /// is present on the host, so advertising it is honest. Language profiles
    /// can still be narrowed with [`ToolRegistry::with_profiles`].
    pub fn new(policy: ToolPolicy) -> Self {
        ToolRegistry {
            bash: PathBuf::from("/usr/bin/bash"),
            policy,
            profiles: vec![ToolProfile::Generic, ToolProfile::Python, ToolProfile::Rust],
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
                summary: describe_tool_call(intent),
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
            summary: describe_tool_call(intent),
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
            summary: describe_tool_call(intent),
            staged_write: None,
        })
    }

    fn render_write(
        &self,
        intent: &ToolIntent,
        context: &ExecContext,
    ) -> crate::error::Result<RenderedTool> {
        let path = string_arg(intent, "path")?;
        if !under_write_root(&path, &self.policy.write_root) {
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
        let host_staging = context
            .workdir
            .join(HOST_STAGING_DIR)
            .join(&context.call_id);
        Ok(RenderedTool {
            argv: vec![
                self.bash.to_string_lossy().into_owned(),
                "-c".to_owned(),
                "exec mv -f -- \"$1\" \"$2\"".to_owned(),
                "agent-runner".to_owned(),
                sandbox_staging.clone(),
                path.clone(),
            ],
            summary: describe_tool_call(intent),
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

/// Whether `path` is inside `root` on a `/`-separated sandbox path namespace.
///
/// Requires the root boundary, not a bare `starts_with`, so that a sibling such
/// as `/workspace-evil` cannot be accepted when the write root is `/workspace`.
fn under_write_root(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
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

/// A short, human-readable description of a tool call, shown in the UI and logs.
///
/// This is the single source for the `RenderedTool.summary` each renderer
/// produces (`read {path}`, `list {path}`, `write {path}`, or the shell command
/// summary), so the live "tool proposed" line matches the executed one. It needs
/// only the intent name and arguments, so it also works in the streaming loop
/// before the call has been rendered and executed.
pub fn describe_tool_call(intent: &ToolIntent) -> String {
    match intent.name.as_str() {
        "shell" => match string_arg(intent, "command") {
            Ok(command) => summarize(&command),
            Err(_) => "<shell command>".to_owned(),
        },
        // Path-based tools: a literal verb prefixes the target path.
        "read_file" | "list_dir" | "write_file" => match string_arg(intent, "path") {
            Ok(path) => {
                let verb = match intent.name.as_str() {
                    "read_file" => "read",
                    "list_dir" => "list",
                    _ => "write",
                };
                format!("{verb} {}", summarize_path(&path))
            }
            Err(_) => intent.name.clone(),
        },
        // Any other path-accepting tool falls back to its own name as the verb.
        other => match string_arg(intent, "path") {
            Ok(path) => format!("{other} {}", summarize_path(&path)),
            Err(_) => other.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building and testing the project are essential, so the default registry
    /// must advertise the generic Linux tools, the Rust/Cargo toolchain, and
    /// the Python interpreter; otherwise an agent reasonably concludes it has
    /// no compiler and declines.
    #[test]
    fn default_registry_advertises_generic_and_rust_toolchains() {
        let registry = ToolRegistry::new(ToolPolicy::minimum());
        let advertised = registry.profiles();
        // `profiles()` sorts and dedupes, so the default set is stable order.
        assert_eq!(
            advertised,
            vec!["generic", "python", "rust"],
            "default registry must advertise generic, python, and rust, got {advertised:?}"
        );
    }

    #[test]
    fn shell_tool_description_names_the_rust_toolchain() {
        let registry = ToolRegistry::new(ToolPolicy::minimum());
        let shell = registry
            .tool_definitions()
            .into_iter()
            .find(|d| d.name == "shell")
            .expect("the shell tool is always available");
        let description = shell.description.to_string();
        assert!(
            description.to_lowercase().contains("cargo"),
            "the shell tool must advertise the Rust toolchain it can build with: {description}"
        );
    }

    #[test]
    fn explicit_profile_replaces_the_default_set() {
        let registry =
            ToolRegistry::new(ToolPolicy::minimum()).with_profiles(vec![ToolProfile::Python]);
        assert_eq!(registry.profiles(), vec!["python"]);
    }
}
