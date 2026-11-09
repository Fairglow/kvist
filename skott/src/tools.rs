//! Model-facing tools and their mapping to sandbox-executed commands.
//!
//! Tools never execute directly. Each [`ToolRegistry::render`] call produces an
//! argv and, for native file tools, a bounded typed payload. Rendering never
//! performs filesystem effects; the executor privately stages that payload.

use std::collections::BTreeMap;
use std::path::PathBuf;

use galla::protocol::MAX_VALUE_BYTES;
use sav::{ToolDefinition, ToolIntent};
use serde_json::Value;

use crate::file_tools::FileRequest;

// Re-export the policy type so consumers can refer to it as `skott::tools::ToolPolicy`.
pub use crate::config::ToolPolicy;

// The language tool profiles, their on/auto/off setting, and the sandbox-aware
// availability probe live in `toolchain`; re-export them here so consumers can
// refer to them as `skott::tools::ToolProfile`, etc.
pub use crate::toolchain::{ProfileSetting, ToolProfile, ToolchainProbe};

/// The number of leading characters kept in a tool summary.
const MAX_SUMMARY_BYTES: usize = 120;
/// The number of leading characters kept in a path summary.
const MAX_PATH_SUMMARY_CHARS: usize = 60;
/// The largest shell command that fits one sandbox argv entry. Commands above
/// this bound travel as a private read-only `/context/0` script file instead, so
/// every legal command (up to the 16384-byte tool bound) produces a valid,
/// protocol-bounded sandbox request.
const MAX_INLINE_SHELL_COMMAND_BYTES: usize = MAX_VALUE_BYTES;
/// The sandbox script that stages a long shell command: the exec'd bash first
/// performs the quoted command substitution (results inside double quotes are
/// not re-expanded, split, or globbed), then `exec`s itself into the real
/// interpreter so the process tree, exit status, and `$0` are identical to the
/// inline form. A bare `$(cat /context/0)` as the `-c` script would be wrong:
/// the exec'd bash would run `cat` and word-split its output into a command.
const STAGED_SCRIPT_WRAPPER: &str = "exec bash -c \"$(cat /context/0)\" skott";
const RUST_SHELL_WRAPPER: &str = "if test -r /rust/runtime/initialize; then source /rust/runtime/initialize; fi; exec /usr/bin/bash -c \"$1\" skott";

pub(crate) fn default_file_helper_path() -> crate::error::Result<PathBuf> {
    std::env::current_exe()
        .map_err(|e| crate::error::io_error("locate executable for native helper", None, e))?
        .parent()
        .map(|parent| parent.join("skott-toerne"))
        .ok_or_else(|| crate::error::Error::SandboxBuild {
            reason: "cannot find executable directory for native helper".to_owned(),
        })
}

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

/// The result of rendering a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTool {
    /// The argv to execute inside the sandbox; `argv[0]` is absolute.
    pub argv: Vec<String>,
    /// A short human description shown in the UI and logs.
    pub summary: String,
    /// A bounded, validated native file operation for read-only payload staging.
    pub file_request: Option<FileRequest>,
    /// An explicit configured helper override; absence selects the executable
    /// next to the running skott binary at execution time.
    pub file_helper: Option<PathBuf>,
    /// The complete shell command text that must be staged as the `/context/0`
    /// read-only script file before execution; `argv` references it. `None`
    /// for every non-shell tool and for shell commands that fit one argv entry.
    /// Never set together with `file_request`.
    pub shell_script: Option<String>,
}

/// The registry of model-facing tools and their sandbox rendering.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    bash: PathBuf,
    policy: ToolPolicy,
    profiles: Vec<ToolProfile>,
    file_helper: Option<PathBuf>,
    rust_environment: Option<crate::toolchain::rust_environment::RustEnvironment>,
    rust_requested: bool,
    diagnostics: Vec<String>,
}

impl ToolRegistry {
    /// Builds a pure registry with Generic only; production language
    /// availability is established by [`ToolRegistry::resolve_for_workspace`].
    pub fn new(policy: ToolPolicy) -> Self {
        ToolRegistry {
            bash: PathBuf::from("/usr/bin/bash"),
            policy,
            profiles: vec![ToolProfile::Generic],
            file_helper: None,
            rust_environment: None,
            rust_requested: false,
            diagnostics: Vec::new(),
        }
    }

    /// Resolves the canonical `bash` path and the honest, gated profile set,
    /// advertising only tool-chains that actually reach the sandbox.
    ///
    /// `Generic` is always advertised. Every configurable profile follows its
    /// configured [`ProfileSetting`]: `On` (including a `forced` profile from the
    /// CLI) advertises the profile and fails with [`crate::error::Error::ToolchainUnavailable`]
    /// if its interpreter does not reach the sandbox, `Auto` advertises it only
    /// when available, and `Off` never does. An explicit request therefore never
    /// resolves to a dishonest tool list. The `probe` is judged against what
    /// reaches the sandbox, never against the host `PATH`.
    /// The installed helper location is supplied next to the current executable;
    /// its regular-file identity and writable-scope separation are checked by
    /// the executor before staging or spawning.
    pub fn resolve(
        policy: ToolPolicy,
        settings: &BTreeMap<ToolProfile, ProfileSetting>,
        probe: &dyn ToolchainProbe,
        forced: Option<ToolProfile>,
    ) -> crate::error::Result<Self> {
        let bash = crate::sandbox::resolve_executable("bash")?;
        let mut profiles = vec![ToolProfile::Generic];
        profiles.extend(crate::toolchain::resolve_profiles(settings, probe, forced)?);
        Ok(ToolRegistry {
            bash,
            policy,
            profiles,
            file_helper: Some(default_file_helper_path()?),
            rust_environment: None,
            rust_requested: false,
            diagnostics: Vec::new(),
        })
    }

    /// Resolves sandbox profiles for this workspace, including a concrete,
    /// installed Rust toolchain and private offline resources. This never
    /// provisions a toolchain or dependencies. Log [`Self::diagnostics`] at
    /// startup, including the explicit absence note for unavailable Auto Rust.
    /// Interactive host opt-out must use [`Self::resolve`] instead: these paths
    /// and authority descriptions belong only to the sandbox namespace.
    pub fn resolve_for_workspace(
        policy: ToolPolicy,
        settings: &BTreeMap<ToolProfile, ProfileSetting>,
        probe: &dyn ToolchainProbe,
        forced: Option<ToolProfile>,
        workdir: &std::path::Path,
    ) -> crate::error::Result<Self> {
        Self::resolve_for_workspace_cancellable(
            policy,
            settings,
            probe,
            forced,
            workdir,
            &sav::CancellationToken::new(),
        )
    }

    pub(crate) fn resolve_for_workspace_cancellable(
        policy: ToolPolicy,
        settings: &BTreeMap<ToolProfile, ProfileSetting>,
        probe: &dyn ToolchainProbe,
        forced: Option<ToolProfile>,
        workdir: &std::path::Path,
        cancellation: &sav::CancellationToken,
    ) -> crate::error::Result<Self> {
        let setting = if forced == Some(ToolProfile::Rust) {
            ProfileSetting::On
        } else {
            settings
                .get(&ToolProfile::Rust)
                .copied()
                .unwrap_or(ProfileSetting::Auto)
        };
        let mut settings_without_rust = settings.clone();
        settings_without_rust.insert(ToolProfile::Rust, ProfileSetting::Off);
        let mut registry = Self::resolve(
            policy,
            &settings_without_rust,
            probe,
            forced.filter(|p| *p != ToolProfile::Rust),
        )?;
        if setting != ProfileSetting::Off {
            registry.rust_requested = true;
            match crate::toolchain::rust_environment::RustEnvironment::resolve_cancellable(
                workdir,
                cancellation,
            ) {
                Ok(environment) => {
                    registry.diagnostics.push(environment.diagnostic());
                    registry.profiles.push(ToolProfile::Rust);
                    registry.rust_environment = Some(environment);
                }
                Err(error) if !cancellation.is_cancelled() => {
                    registry
                        .diagnostics
                        .push(format!("Rust requested but unavailable (not advertised): {error}. Independent workspace tools remain usable; provision or repair the installed resource on the host and retry only the affected Rust command without restart"));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(registry)
    }

    /// Startup diagnostics for exact Rust resource identities and absence.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub(crate) fn rust_environment(
        &self,
    ) -> Option<&crate::toolchain::rust_environment::RustEnvironment> {
        self.rust_environment.as_ref()
    }

    pub(crate) fn rust_requested(&self) -> bool {
        self.rust_requested
    }

    /// Overrides the resolved `bash` path (used by tests).
    pub fn with_bash(mut self, bash: impl Into<PathBuf>) -> Self {
        self.bash = bash.into();
        self
    }

    /// Overrides the installed native helper location. The executor rejects
    /// absent, link-like, nonregular or workspace-writable helper paths.
    pub fn with_file_helper(mut self, helper: impl Into<PathBuf>) -> Self {
        self.file_helper = Some(helper.into());
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
        let rust_scope = if self.rust_environment.is_some() {
            " Rust exposes the usable selected toolchain and bounded optional registrations read-only. Discover \
              them with rustup toolchain list and rustup target list --installed --toolchain <name>; \
              select with cargo +<name>, rustc +<name>, rustup run <name>, or invocation-local \
              rustup default <installed-name> (a project pin takes precedence). Cargo is explicitly \
              offline with the current read-only vendor view, HOME/cache are scratch and \
              build output persists at /workspace/target. \
              Missing dependencies require host provisioning; sandbox builds never download."
        } else {
            ""
        };
        let mut definitions = vec![ToolDefinition {
            name: "shell".to_owned(),
            description: format!(
                "Run a shell command inside the sandbox. Use it to build, test, query, and \
                     manipulate the project. Available tooling: {toolkit}. Commands run under the \
                     working directory scope; sandbox mounts enforce write authority. \
                     The command denylist is an advisory filter, not isolation.{rust_scope}"
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
                "additionalProperties": false,
            }),
        }];
        for (name, description, extra, required, read) in [
            (
                "read_file",
                "Read a page-local UTF-8 byte page (default 4096, maximum 16384) using bounded I/O, with page_sha256, total_bytes and next_offset. sha256 is null by default: no whole-file scan or UTF-8 preflight. Explicit include_sha256:true or expected_sha256 streams the whole raw file for a full revision and may be expensive. Never use page_sha256 as a full-file guard. Within this prompt, omitted offset continues the last actual page; explicit offset: 0 restarts. Offsets are bytes, NOT lines. Changed offsets move forward to a UTF-8 boundary or clamp to EOF, reporting requested_offset/offset_adjustment. Pages shrink to fit 7000 encoded bytes.",
                None,
                vec!["path"],
                true,
            ),
            (
                "write_file",
                "Atomically create or replace a file strictly inside the write root. Request content is bounded to 64 KiB; existing files have no size cap. Streaming revision checks prevent detected lost updates; optional expected_sha256 rejects stale requests.",
                Some(("content", "The complete replacement UTF-8 content.")),
                vec!["path", "content"],
                false,
            ),
            (
                "list_dir",
                "List deterministically sorted directory entries in bounded pages (default 100, maximum 256). Pages shrink to fit 7000 encoded bytes; resume at actual next_offset until null.",
                None,
                vec!["path"],
                false,
            ),
            (
                "find_files",
                "Find regular files under this directory using a literal substring of the scoped path. Generated directories are pruned by default; include_generated opts in. Symlinks and special entries (sockets/FIFOs) are skipped and counted, never opened. Coverage reports complete. Sorted pages shrink to 7000 encoded bytes; use actual next_offset. Recursion/count/depth bounds fail explicitly with narrowing guidance.",
                Some((
                    "pattern",
                    "Literal substring, not a glob or regular expression; empty matches all.",
                )),
                vec!["path", "pattern"],
                false,
            ),
            (
                "search_files",
                "Stream a directory or regular UTF-8 file for a nonempty literal substring, including multiline queries. No file-size cap. file_pattern is a literal path filter. Generated, binary, linked and special entries are counted as skipped. At 8 MiB scan work, use next_scan.path with scan_offset=next_scan.offset (regular-file scope only); offset/next_offset separately paginate collected matches. Continuations overlap enough bytes to retain split queries, but do not cover omitted prefixes or later directory siblings. Global line is null beyond an 8 MiB prefix-count budget; line_in_scan/preview_offset then identify local context. Pages shrink to 7000 encoded bytes.",
                Some((
                    "query",
                    "Nonempty literal substring, not a regular expression.",
                )),
                vec!["path", "query"],
                false,
            ),
            (
                "edit_file",
                "Stream and atomically replace exactly one occurrence of old_text, including overlapping matches. No source/result size cap. Optional expected_sha256 guards a requested revision; detected concurrent changes reject only this mutation. Preserves unrelated bytes, CRLF, missing final newline and mode.",
                None,
                vec!["path", "old_text", "new_text"],
                false,
            ),
            (
                "append_file",
                "Stream and atomically append content verbatim to an existing UTF-8 file inside the write root. Optional expected_sha256 guards a revision. No newline is inserted. Preserves prior bytes and mode. Request content maximum 64 KiB; no source/result size cap.",
                Some((
                    "content",
                    "The UTF-8 content to append, without an implicit newline.",
                )),
                vec!["path", "content"],
                false,
            ),
            (
                "apply_patch",
                "Stream and atomically update one existing UTF-8 file using strict unified-diff hunks. Optional expected_sha256 guards a revision. Supply @@ -old_start[,count] +new_start[,count] @@ and space/-/+ lines ending in LF, with optional \\ No newline at end of file markers. No file headers, paths, fuzz, renames or multi-file operations. Exact coordinates, context and counts required, without a file-size/line-coordinate cap. CR before LF remains content. Request patch maximum 64 KiB.",
                Some((
                    "patch",
                    "Single-file unified hunks only, without ---/+++ file headers.",
                )),
                vec!["path", "patch"],
                false,
            ),
        ] {
            let mut properties = serde_json::json!({
                "path":{"type":"string","description":"Canonical absolute path; no . or .. components.","maxLength":4096}
            });
            if matches!(
                name,
                "read_file" | "list_dir" | "find_files" | "search_files"
            ) {
                properties["offset"] = serde_json::json!({"type":"integer","minimum":0});
                if !read {
                    properties["offset"]["maximum"] = serde_json::json!(4096);
                    properties["offset"]["default"] = serde_json::json!(0);
                }
                properties["limit"] = serde_json::json!({"type":"integer","minimum":1,"maximum":if read {16384} else {256},"default":if read {4096} else {100}});
                if read {
                    properties["include_sha256"] = serde_json::json!({
                        "type":"boolean","default":false,
                        "description":"Explicitly stream the full raw file for sha256. Expensive for huge files; ordinary reads hash/validate only the returned page."
                    });
                }
            }
            if let Some((field, desc)) = extra {
                properties[field] = serde_json::json!({"type":"string","description":desc,"maxLength":if matches!(field, "content" | "patch") {65536} else {1024}});
            }
            if matches!(name, "find_files" | "search_files") {
                properties["include_generated"] = serde_json::json!({
                    "type":"boolean","default":false,
                    "description":"Include generated directory trees otherwise excluded from discovery/search."
                });
            }
            if name == "search_files" {
                properties["scan_offset"] = serde_json::json!({
                    "type":"integer","minimum":0,
                    "description":"Byte-work continuation for a regular-file path, from next_scan.offset. Distinct from collected-result offset."
                });
                properties["file_pattern"] = serde_json::json!({
                    "type":"string","maxLength":1024,
                    "description":"Optional literal substring of the scope-relative path (basename for a file scope); not a glob."
                });
            }
            if name == "edit_file" {
                properties["old_text"] =
                    serde_json::json!({"type":"string","minLength":1,"maxLength":65536});
                properties["new_text"] = serde_json::json!({"type":"string","maxLength":65536});
            }
            if matches!(
                name,
                "read_file" | "write_file" | "edit_file" | "append_file" | "apply_patch"
            ) {
                properties["expected_sha256"] = serde_json::json!({"type":"string","description":"sha256: followed by 64 lowercase hexadecimal digits."});
            }
            definitions.push(ToolDefinition {
                name: name.to_owned(), description: description.to_owned(),
                parameters: serde_json::json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
            });
        }
        definitions
    }

    /// Renders a tool intent to a sandbox argv.
    pub fn render(
        &self,
        intent: &ToolIntent,
        _context: &ExecContext,
    ) -> crate::error::Result<RenderedTool> {
        match intent.name.as_str() {
            "shell" => self.render_shell(intent),
            "read_file" | "write_file" | "list_dir" | "find_files" | "search_files"
            | "edit_file" | "append_file" | "apply_patch" => {
                let request = FileRequest::for_registry(
                    &self.policy.write_root,
                    &intent.name,
                    intent.arguments.clone(),
                )?;
                Ok(RenderedTool {
                    argv: vec!["/context/1".to_owned(), "/context/0".to_owned()],
                    summary: describe_tool_call(intent),
                    file_request: Some(request),
                    file_helper: self.file_helper.clone(),
                    shell_script: None,
                })
            }
            other => Err(crate::error::Error::ToolRender {
                tool: other.to_owned(),
                reason: "unknown or disabled tool".to_owned(),
            }),
        }
    }

    fn render_shell(&self, intent: &ToolIntent) -> crate::error::Result<RenderedTool> {
        if !intent
            .arguments
            .as_object()
            .is_some_and(|args| args.len() == 1 && args.contains_key("command"))
        {
            return Err(crate::error::Error::ToolRender {
                tool: "shell".to_owned(),
                reason: "expected exactly the command argument".to_owned(),
            });
        }
        let command = string_arg(intent, "command")?;
        if command.trim().is_empty() || command.len() > 16 * 1024 || command.contains('\0') {
            return Err(crate::error::Error::ToolRender {
                tool: "shell".to_owned(),
                reason: "command must be nonempty, NUL-free and within 16384 bytes".to_owned(),
            });
        }
        if !self.policy.shell_permitted(&command) {
            Err(crate::error::Error::ToolPolicy {
                tool: "shell".to_owned(),
                reason: "command matches the forbidden-command policy".to_owned(),
            })
        } else if command.len() <= MAX_INLINE_SHELL_COMMAND_BYTES {
            let argv = if self.rust_requested {
                vec![
                    self.bash.to_string_lossy().into_owned(),
                    "-c".to_owned(),
                    RUST_SHELL_WRAPPER.to_owned(),
                    "skott".to_owned(),
                    command.clone(),
                ]
            } else {
                vec![
                    self.bash.to_string_lossy().into_owned(),
                    "-c".to_owned(),
                    command.clone(),
                    "skott".to_owned(),
                ]
            };
            Ok(RenderedTool {
                argv,
                summary: describe_tool_call(intent),
                file_request: None,
                file_helper: None,
                shell_script: None,
            })
        } else {
            // The command no longer fits one 4096-byte argv entry. The executor
            // stages it as a private regular file outside the workspace, which
            // the request mounts read-only at /context/0; the wrapper reads the
            // file contents verbatim (quoted command substitution is not
            // re-expanded) and `exec`s the real interpreter, so semantics,
            // exit status and `$0` match the inline form exactly.
            Ok(RenderedTool {
                argv: vec![
                    self.bash.to_string_lossy().into_owned(),
                    "-c".to_owned(),
                    if self.rust_requested {
                        format!(
                            "if test -r /rust/runtime/initialize; then source /rust/runtime/initialize; fi; {STAGED_SCRIPT_WRAPPER}"
                        )
                    } else {
                        STAGED_SCRIPT_WRAPPER.to_owned()
                    },
                ],
                summary: describe_tool_call(intent),
                file_request: None,
                file_helper: None,
                shell_script: Some(command.clone()),
            })
        }
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

    #[test]
    fn default_registry_advertises_only_generic_without_probing() {
        let registry = ToolRegistry::new(ToolPolicy::minimum());
        let advertised = registry.profiles();
        // `profiles()` sorts and dedupes, so the default set is stable order.
        assert_eq!(
            advertised,
            vec!["generic"],
            "pure registry must not advertise unprobed languages, got {advertised:?}"
        );
    }

    #[test]
    fn shell_tool_description_does_not_claim_unprobed_languages() {
        let registry = ToolRegistry::new(ToolPolicy::minimum());
        let shell = registry
            .tool_definitions()
            .into_iter()
            .find(|d| d.name == "shell")
            .expect("the shell tool is always available");
        let description = shell.description.to_lowercase();
        assert!(
            !description.contains("python"),
            "the pure registry must not claim an unprobed Python interpreter: {description}"
        );
        assert!(
            !description.contains("cargo"),
            "the pure registry must not claim an unprobed Rust/Cargo toolchain: {description}"
        );
    }

    #[test]
    fn explicit_profile_replaces_the_default_set() {
        let registry =
            ToolRegistry::new(ToolPolicy::minimum()).with_profiles(vec![ToolProfile::Python]);
        assert_eq!(registry.profiles(), vec!["python"]);
    }
}
