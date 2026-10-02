//! Private local operational journal and diagnostic transcript.
//!
//! Arguments and outputs are represented by hashes in the versioned journal.
//! The bounded transcript may contain sensitive user/model/tool text. Neither
//! file is engine evidence or an executable checkpoint; an unanswered dispatch
//! is an unknown effect and must not be automatically replayed.

use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use agent_runtime::{ModelRequest, ModelTurn, ModelUsage, ToolIntent};
use nix::fcntl::{OFlag, openat};
use nix::sys::stat::{Mode, mkdirat};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result, io_error};
use crate::sandbox::ToolOutcome;
use crate::session::{Recorder, RunSummary};

/// Interactive history location; this agent-writable history is not evidence.
pub const DEFAULT_LOG_DIR: &str = ".agent-runner/runs";
const MAX_TRANSCRIPT_TEXT: usize = 64 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// The actual boundary of a workspace session, not engine task authority.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    SandboxedWorkspace,
    HostUnconfined,
    Unspecified,
}

impl ExecutionScope {
    /// Persistent operator-facing scope label.
    pub fn label(self) -> &'static str {
        match self {
            Self::SandboxedWorkspace => "SANDBOX",
            Self::HostUnconfined => "HOST UNCONFINED",
            Self::Unspecified => "SCOPE UNKNOWN",
        }
    }
}

/// Descriptive provenance for an operational record, never approval evidence.
#[derive(Debug, Clone, Serialize)]
pub struct SessionMetadata {
    pub execution_scope: ExecutionScope,
    pub working_directory: PathBuf,
    pub write_root: String,
    pub policy_identity: String,
    pub context_limit: usize,
    pub response_reserve: u32,
    pub max_run_tokens: u64,
    pub max_run_secs: u64,
}

fn hash(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn bounded(text: &str) -> &str {
    let mut end = text.len().min(MAX_TRANSCRIPT_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn argument_shape(args: &Value) -> Value {
    let Some(fields) = args.as_object() else {
        return json!("invalid");
    };
    let shape: serde_json::Map<String, Value> = fields
        .iter()
        .map(|(key, value)| {
            let kind = match value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            (key.clone(), json!(kind))
        })
        .collect();
    Value::Object(shape)
}

fn private_directory(path: &Path) -> io::Result<(PathBuf, File)> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut dir = File::open("/")?;
    for component in absolute.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "log directory must not contain . or ..",
                ));
            }
        };
        let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let fd = match openat(&dir, name, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(nix::errno::Errno::ENOENT) => {
                match mkdirat(&dir, name, Mode::S_IRWXU) {
                    Ok(()) => dir.sync_all()?,
                    Err(nix::errno::Errno::EEXIST) => {}
                    Err(error) => return Err(error.into()),
                }
                openat(&dir, name, flags, Mode::empty())?
            }
            Err(error) => return Err(error.into()),
        };
        dir = File::from(fd);
    }
    if dir.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "log directory must be private (chmod 700), and all directory components must be non-links",
        ));
    }
    Ok((absolute, dir))
}

#[derive(Serialize)]
struct Envelope<'a> {
    schema_version: u32,
    sequence: u64,
    event: &'a Value,
}

/// A fallible, held-descriptor journal; path substitution cannot redirect writes.
#[derive(Debug)]
pub struct SessionLog {
    journal: File,
    directory: File,
    transcript: File,
    journal_path: PathBuf,
    transcript_path: PathBuf,
    session_id: String,
    task_id: String,
    started_at: Instant,
    sequence: u64,
    prompt: u64,
    turn: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
    metadata: Option<SessionMetadata>,
}

impl SessionLog {
    /// Creates no-clobber mode-0600 files under a non-link mode-0700 directory.
    pub fn open(log_dir: &Path, task_id: impl Into<String>) -> io::Result<Self> {
        let (log_dir, directory) = private_directory(log_dir)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let session_id = format!(
            "{stamp:032}-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        );
        let journal_name = format!("session-{session_id}.jsonl");
        let transcript_name = format!("session-{session_id}.log");
        let flags =
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let mode = Mode::S_IRUSR | Mode::S_IWUSR;
        let journal = File::from(openat(&directory, journal_name.as_str(), flags, mode)?);
        let transcript = File::from(openat(&directory, transcript_name.as_str(), flags, mode)?);
        directory.sync_all()?;
        Ok(Self {
            journal,
            directory,
            transcript,
            journal_path: log_dir.join(journal_name),
            transcript_path: log_dir.join(transcript_name),
            session_id,
            task_id: task_id.into(),
            started_at: Instant::now(),
            sequence: 0,
            prompt: 0,
            turn: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            metadata: None,
        })
    }

    /// Attaches caller-supplied operational scope; absence is recorded explicitly.
    pub fn with_metadata(mut self, metadata: SessionMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Location of the version-one journal.
    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }
    /// Location of the bounded, potentially sensitive transcript.
    pub fn transcript_path(&self) -> &Path {
        &self.transcript_path
    }
    /// Caller-supplied descriptive label; not an engine approval identity.
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    /// Cumulative provider input usage.
    pub fn total_input_tokens(&self) -> u64 {
        self.total_input_tokens
    }
    /// Cumulative provider output usage.
    pub fn total_output_tokens(&self) -> u64 {
        self.total_output_tokens
    }
    /// Cumulative provider total usage.
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens
            .saturating_add(self.total_output_tokens)
    }
    /// Wall time since this file was opened.
    pub fn elapsed_secs(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }

    fn record(&mut self, event: Value) -> Result<()> {
        self.sequence += 1;
        serde_json::to_writer(
            &mut self.journal,
            &Envelope {
                schema_version: 1,
                sequence: self.sequence,
                event: &event,
            },
        )
        .map_err(|error| Error::Recording {
            reason: format!("encode/write journal: {error}"),
        })?;
        self.journal
            .write_all(b"\n")
            .map_err(|source| io_error("write journal", None, source))
    }

    fn text(&mut self, label: &str, text: &str) -> Result<()> {
        let preview = bounded(text);
        writeln!(
            self.transcript,
            "{label}: {preview}{}",
            if preview.len() < text.len() {
                "\n[transcript truncated]"
            } else {
                ""
            }
        )
        .map_err(|source| io_error("write transcript", None, source))
    }

    fn synchronize(&self) -> Result<()> {
        self.journal
            .sync_data()
            .and_then(|()| self.transcript.sync_data())
            .and_then(|()| self.directory.sync_all())
            .map_err(|source| io_error("synchronize session record", None, source))
    }
}

impl Recorder for SessionLog {
    fn session_start(&mut self) -> Result<()> {
        self.prompt += 1;
        self.record(json!({"type":"session_start", "session_id":self.session_id,
            "prompt":self.prompt, "task_id":self.task_id, "canonical_evidence":false,
            "execution_scope":self.metadata.as_ref().map_or(ExecutionScope::Unspecified, |meta| meta.execution_scope),
            "metadata":self.metadata}))?;
        writeln!(
            self.transcript,
            "== session {} started {}s ago ==",
            self.task_id,
            self.elapsed_secs().round()
        )?;
        Ok(())
    }

    fn request(&mut self, request: &ModelRequest, attempt: u32) -> Result<()> {
        let bytes = serde_json::to_vec(request).map_err(|error| Error::Recording {
            reason: error.to_string(),
        })?;
        self.record(
            json!({"type":"model_request", "attempt":attempt, "request_hash":hash(&bytes),
            "model":request.model, "message_count":request.messages.len(),
            "tools":request.tools.iter().map(|tool| &tool.name).collect::<Vec<_>>(),
            "max_output_tokens":request.max_output_tokens}),
        )?;
        if attempt == 1
            && let Some(agent_runtime::ModelMessage::User(text)) = request.messages.last()
        {
            self.text("user", text)?;
        }
        Ok(())
    }

    fn turn_start(&mut self, turn: &ModelTurn) -> Result<usize> {
        self.turn += 1;
        self.record(
            json!({"type":"turn_start", "turn":self.turn, "text_hash":hash(turn.text.as_bytes()),
            "response_id":turn.response_id, "model":turn.model, "provider":turn.provider,
            "reasoning_hash":turn.reasoning.as_ref().map(|text| hash(text.as_bytes()))}),
        )?;
        self.text("assistant", &turn.text)?;
        if let Some(reasoning) = &turn.reasoning {
            self.text("provider reasoning", reasoning)?;
        }
        Ok(self.turn)
    }

    fn turn_finish(
        &mut self,
        turn: usize,
        usage: &Option<ModelUsage>,
        finish_reason: &str,
    ) -> Result<()> {
        if let Some(usage) = usage {
            self.total_input_tokens = self.total_input_tokens.saturating_add(usage.input_tokens);
            self.total_output_tokens = self.total_output_tokens.saturating_add(usage.output_tokens);
        }
        self.record(json!({"type":"turn_finish", "turn":turn, "usage":usage, "finish_reason":finish_reason}))
    }

    fn tool_dispatch(&mut self, turn: usize, intent: &ToolIntent) -> Result<()> {
        self.record(
            json!({"type":"tool_dispatch", "turn":turn, "call_id":intent.id, "tool":intent.name,
            "argument_shape":argument_shape(&intent.arguments),
            "action_hash":agent_runtime::compute_action_hash(&intent.name, &intent.arguments)}),
        )?;
        self.synchronize()
    }

    fn tool_result(
        &mut self,
        turn: usize,
        call_id: &str,
        tool: &str,
        _args: &Value,
        outcome: &ToolOutcome,
    ) -> Result<()> {
        self.record(json!({"type":"tool_result", "turn":turn, "call_id":call_id, "tool":tool,
            "stdout_hash":hash(&outcome.stdout), "stderr_hash":hash(&outcome.stderr),
            "exited":outcome.exited, "status":outcome.status, "timed_out":outcome.timed_out,
            "cancelled":outcome.cancelled, "output_limit_exceeded":outcome.output_limit_exceeded,
            "bytes":outcome.stdout.len().saturating_add(outcome.stderr.len()), "state_mutated":null}))?;
        self.text("tool stdout", &outcome.output_text(MAX_TRANSCRIPT_TEXT))?;
        self.text("tool stderr", &outcome.error_text(MAX_TRANSCRIPT_TEXT))
    }

    fn session_finish(&mut self, summary: &RunSummary) -> Result<()> {
        self.record(
            json!({"type":"session_finish", "session_id":self.session_id,
            "prompt":self.prompt, "disposition":summary.disposition(), "turns":summary.turns,
            "tools_executed":summary.tools_executed, "success":summary.success(),
            "failure":summary.failure, "total_tokens":self.total_tokens()}),
        )?;
        writeln!(
            self.transcript,
            "== session {} finished: {} turns, {} tokens, {}s, {} ==",
            self.task_id,
            self.turn,
            self.total_tokens(),
            self.elapsed_secs().round(),
            if summary.success() {
                "ok"
            } else {
                summary.disposition()
            }
        )?;
        self.synchronize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_tempdir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn private_no_clobber_files_and_versioned_records() {
        let dir = private_tempdir();
        let mut first = SessionLog::open(dir.path(), "test").unwrap();
        let second = SessionLog::open(dir.path(), "test").unwrap();
        assert_ne!(first.journal_path(), second.journal_path());
        assert_eq!(
            first.journal.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        first.session_start().unwrap();
        first
            .session_finish(&RunSummary {
                answer: Some("done".into()),
                ..RunSummary::default()
            })
            .unwrap();
        for (index, line) in std::fs::read_to_string(first.journal_path())
            .unwrap()
            .lines()
            .enumerate()
        {
            let event: Value = serde_json::from_str(line).unwrap();
            assert_eq!(event["schema_version"], 1);
            assert_eq!(event["sequence"], index + 1);
        }
    }

    #[test]
    fn argument_values_and_output_are_not_in_journal_and_mutation_is_unknown() {
        let dir = private_tempdir();
        let mut log = SessionLog::open(dir.path(), "test").unwrap();
        let intent = ToolIntent {
            id: "call".into(),
            provider_id: None,
            name: "shell".into(),
            arguments: json!({"command":"secret-test-string"}),
        };
        log.tool_dispatch(1, &intent).unwrap();
        log.tool_result(
            1,
            "call",
            "shell",
            &intent.arguments,
            &ToolOutcome::rejected_with("sensitive-output"),
        )
        .unwrap();
        let journal = std::fs::read_to_string(log.journal_path()).unwrap();
        assert!(!journal.contains("secret-test-string"));
        assert!(!journal.contains("sensitive-output"));
        assert!(journal.contains("\"state_mutated\":null"));
        assert!(journal.contains("\"action_hash\":\"sha256:"));
    }

    #[test]
    fn scope_metadata_is_explicit_and_not_engine_evidence() {
        let dir = private_tempdir();
        let mut log =
            SessionLog::open(dir.path(), "test")
                .unwrap()
                .with_metadata(SessionMetadata {
                    execution_scope: ExecutionScope::HostUnconfined,
                    working_directory: PathBuf::from("/tmp/workspace"),
                    write_root: "/workspace".into(),
                    policy_identity: "sha256:test".into(),
                    context_limit: 8192,
                    response_reserve: 1024,
                    max_run_tokens: 10000,
                    max_run_secs: 30,
                });
        log.session_start().unwrap();
        let value: Value =
            serde_json::from_str(std::fs::read_to_string(log.journal_path()).unwrap().trim())
                .unwrap();
        assert_eq!(value["event"]["execution_scope"], "host_unconfined");
        assert_eq!(value["event"]["canonical_evidence"], false);
        assert_eq!(
            value["event"]["metadata"]["working_directory"],
            "/tmp/workspace"
        );
        assert_eq!(value["event"]["metadata"]["response_reserve"], 1024);
    }

    #[test]
    fn write_failure_is_returned() {
        let dir = private_tempdir();
        let mut log = SessionLog::open(dir.path(), "test").unwrap();
        log.journal = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        assert!(log.session_start().is_err());
    }

    #[test]
    fn rejects_symlink_and_nonprivate_log_directory() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert!(SessionLog::open(&link, "test").is_err());
        let public = dir.path().join("public");
        std::fs::create_dir(&public).unwrap();
        std::fs::set_permissions(&public, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(SessionLog::open(&public, "test").is_err());
    }
}
