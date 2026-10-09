//! Private local operational journal and diagnostic transcript.
//!
//! Arguments and outputs are represented by hashes in the versioned journal.
//! The bounded transcript may contain sensitive user/model/tool text. Neither
//! file is a maerg evidence or an executable checkpoint; an unanswered dispatch
//! is an unknown effect and must not be automatically replayed.

use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use nix::fcntl::{OFlag, openat, renameat};
use nix::sys::stat::{Mode, mkdirat};
use sav::{ModelRequest, ModelTurn, ModelUsage, ToolIntent};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result, io_error};
use crate::sandbox::ToolOutcome;
use crate::session::{Recorder, RunSummary};

/// Interactive history location; this agent-writable history is not evidence.
pub const DEFAULT_LOG_DIR: &str = ".skott/runs";
const MAX_TRANSCRIPT_TEXT: usize = 64 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// The actual boundary of a workspace session, not maerg task authority.
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
    pub context_source: String,
    pub response_reserve: u32,
    pub response_source: String,
    pub max_run_tokens: u64,
    pub max_run_secs: u64,
}

fn hash(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Formats epoch nanoseconds as a human-readable UTC stamp for session ids,
/// e.g. `2026-10-03T14-22-05Z`, so a transcript is findable by its date and
/// time. Second precision: the pid and per-process counter in the id already
/// disambiguate sessions started within the same second, and lexicographic
/// order of the stamp matches chronological order.
fn format_utc(epoch_nanos: u128) -> String {
    let secs = (epoch_nanos / 1_000_000_000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    // Proleptic Gregorian civil-from-days conversion.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}-{minute:02}-{second:02}Z")
}

/// Derives a bounded, filesystem-safe slug from the first few words of a
/// prompt, so a session transcript is recognisable by its content. Punctuation
/// becomes hyphens, case is lowered, and the result is capped so it can never
/// dominate the file name.
fn prompt_slug(prompt: &str) -> String {
    let mut slug = String::new();
    for (index, word) in prompt.split_whitespace().take(6).enumerate() {
        if index > 0 && !slug.ends_with('-') {
            slug.push('-');
        }
        for ch in word.chars() {
            if slug.len() >= 40 {
                break;
            }
            if ch.is_alphanumeric() {
                slug.push_str(&ch.to_lowercase().to_string());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
    }
    // Lowercasing can widen a single character, so re-apply the bound on a
    // char boundary.
    slug.truncate(40);
    slug.trim_end_matches('-').to_owned()
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
    /// True once the first prompt has been folded into the session id, so a
    /// later prompt in the same session never renames the record again.
    slug_applied: bool,
    task_id: String,
    started_at: Instant,
    sequence: u64,
    prompt: u64,
    turn: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
    usage_complete: bool,
    pending_usage: bool,
    estimated_tokens: u64,
    prompt_started_at: Instant,
    metadata: Option<SessionMetadata>,
}

impl SessionLog {
    /// Creates no-clobber mode-0600 files under a non-link mode-0700 directory.
    ///
    /// The session id is human-readable: a UTC date-time stamp plus a pid and
    /// per-process counter for uniqueness. The first prompt received later
    /// appends a slug of its first words (see [`Self::annotate_prompt`]), so a
    /// transcript reads as `session-2026-10-03T14-22-05Z-4242-1-fix-the-bug.log`.
    pub fn open(log_dir: &Path, task_id: impl Into<String>) -> io::Result<Self> {
        let (log_dir, directory) = private_directory(log_dir)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let session_id = format!(
            "{}-{}-{}",
            format_utc(stamp),
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
            slug_applied: false,
            task_id: task_id.into(),
            started_at: Instant::now(),
            sequence: 0,
            prompt: 0,
            turn: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            usage_complete: true,
            pending_usage: false,
            estimated_tokens: 0,
            prompt_started_at: Instant::now(),
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
    /// The session identifier.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    /// Caller-supplied descriptive label; not a maerg approval identity.
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Folds the first few words of a prompt into the session id and renames
    /// both record files to match. Only the first prompt renames; later prompts
    /// in the same session leave the name untouched. Renaming uses the held
    /// directory descriptor, so path substitution cannot redirect the record,
    /// and the open file handles follow their inodes across the rename.
    pub fn annotate_prompt(&mut self, prompt: &str) -> io::Result<()> {
        if self.slug_applied {
            return Ok(());
        }
        self.slug_applied = true;
        let slug = prompt_slug(prompt);
        if slug.is_empty() {
            return Ok(());
        }
        let dir = self
            .journal_path
            .parent()
            .ok_or_else(|| io::Error::other("session log directory is missing"))?
            .to_owned();
        let old_journal = self
            .journal_path
            .file_name()
            .ok_or_else(|| io::Error::other("session journal name is missing"))?;
        let old_transcript = self
            .transcript_path
            .file_name()
            .ok_or_else(|| io::Error::other("session transcript name is missing"))?;
        let journal_name = format!("session-{}-{slug}.jsonl", self.session_id);
        let transcript_name = format!("session-{}-{slug}.log", self.session_id);
        renameat(
            &self.directory,
            old_journal,
            &self.directory,
            journal_name.as_str(),
        )?;
        renameat(
            &self.directory,
            old_transcript,
            &self.directory,
            transcript_name.as_str(),
        )?;
        self.session_id = format!("{}-{slug}", self.session_id);
        self.journal_path = dir.join(journal_name);
        self.transcript_path = dir.join(transcript_name);
        self.directory.sync_all()?;
        Ok(())
    }
    /// Current-prompt provider input usage (possibly incomplete).
    pub fn total_input_tokens(&self) -> u64 {
        self.total_input_tokens
    }
    /// Current-prompt provider output usage (possibly incomplete).
    pub fn total_output_tokens(&self) -> u64 {
        self.total_output_tokens
    }
    /// Current-prompt provider total usage (possibly incomplete).
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
    fn notice(&mut self, message: &str) -> Result<()> {
        self.record(json!({"type":"notice", "prompt":self.prompt, "message":bounded(message)}))?;
        self.text("notice", message)
    }

    fn on_prompt(&mut self, prompt: &str) -> Result<()> {
        self.annotate_prompt(prompt)
            .map_err(|source| io_error("rename session record with prompt slug", None, source))
    }

    fn session_start(&mut self) -> Result<()> {
        self.prompt += 1;
        self.prompt_started_at = Instant::now();
        self.total_input_tokens = 0;
        self.total_output_tokens = 0;
        self.usage_complete = true;
        self.pending_usage = false;
        self.estimated_tokens = 0;
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
        if let Some(metadata) = &self.metadata {
            writeln!(
                self.transcript,
                "model budget: {} context tokens ({}), {} output tokens ({})",
                metadata.context_limit,
                metadata.context_source,
                metadata.response_reserve,
                metadata.response_source
            )?;
        }
        Ok(())
    }

    fn request(&mut self, request: &ModelRequest, attempt: u32) -> Result<()> {
        if self.pending_usage {
            self.usage_complete = false;
        }
        self.pending_usage = true;
        let bytes = serde_json::to_vec(request).map_err(|error| Error::Recording {
            reason: error.to_string(),
        })?;
        let input_estimate = crate::context::estimate_request(request);
        self.estimated_tokens = self
            .estimated_tokens
            .saturating_add(input_estimate as u64)
            .saturating_add(u64::from(request.max_output_tokens.unwrap_or(0)));
        self.record(
            json!({"type":"model_request", "attempt":attempt, "request_hash":hash(&bytes),
            "model":request.model, "message_count":request.messages.len(),
            "tools":request.tools.iter().map(|tool| &tool.name).collect::<Vec<_>>(),
            "max_output_tokens":request.max_output_tokens, "estimated_input_tokens":input_estimate}),
        )?;
        if attempt == 1
            && let Some(sav::ModelMessage::User(text)) = request.messages.last()
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
        self.pending_usage = false;
        if let Some(usage) = usage {
            self.total_input_tokens = self.total_input_tokens.saturating_add(usage.input_tokens);
            self.total_output_tokens = self.total_output_tokens.saturating_add(usage.output_tokens);
        } else {
            self.usage_complete = false;
        }
        self.record(json!({"type":"turn_finish", "turn":turn, "usage":usage, "finish_reason":finish_reason}))
    }

    fn tool_dispatch(&mut self, turn: usize, intent: &ToolIntent) -> Result<()> {
        self.record(
            json!({"type":"tool_dispatch", "turn":turn, "call_id":intent.id, "tool":intent.name,
            "argument_shape":argument_shape(&intent.arguments),
            "action_hash":sav::compute_action_hash(&intent.name, &intent.arguments)}),
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
        self.usage_complete &= !self.pending_usage;
        self.record(
            json!({"type":"session_finish", "session_id":self.session_id,
            "prompt":self.prompt, "disposition":summary.disposition(), "turns":summary.turns,
            "tools_executed":summary.tools_executed, "success":summary.success(),
            "failure":summary.failure, "total_tokens":self.usage_complete.then(||self.total_tokens()),
            "usage_complete":self.usage_complete, "estimated_budget_tokens":self.estimated_tokens}),
        )?;
        writeln!(
            self.transcript,
            "== session {} finished: {} turns, {} tokens, {}s, prompt {}, {} ==",
            self.task_id,
            summary.turns,
            if self.usage_complete {
                self.total_tokens().to_string()
            } else {
                "unknown provider usage".into()
            },
            self.prompt_started_at.elapsed().as_secs(),
            self.prompt,
            if summary.success() {
                "ok"
            } else {
                summary.disposition()
            }
        )?;
        if let Some(failure) = &summary.failure {
            self.text("failure", failure)?;
        }
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
                    context_source: "test".into(),
                    response_reserve: 1024,
                    response_source: "test".into(),
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
    fn reliability_notices_failure_and_unknown_usage_are_persisted_honestly() {
        let dir = private_tempdir();
        let mut log = SessionLog::open(dir.path(), "test").unwrap();
        log.session_start().unwrap();
        log.notice("context compacted; retry remains bounded")
            .unwrap();
        log.turn_finish(1, &None, "length").unwrap();
        log.session_finish(&RunSummary {
            turns: 1,
            failure: Some("actual failure".into()),
            ..RunSummary::default()
        })
        .unwrap();
        let journal: Vec<Value> = std::fs::read_to_string(log.journal_path())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            journal.last().unwrap()["event"]["total_tokens"],
            Value::Null
        );
        assert_eq!(journal.last().unwrap()["event"]["turns"], 1);
        let text = std::fs::read_to_string(log.transcript_path()).unwrap();
        assert!(text.contains("actual failure"));
        assert!(text.contains("unknown provider usage"));
        assert!(text.contains("context compacted"));
    }

    #[test]
    fn reliability_incomplete_request_usage_is_unknown_and_prompt_metrics_reset() {
        let dir = private_tempdir();
        let mut log = SessionLog::open(dir.path(), "test").unwrap();
        log.session_start().unwrap();
        let request = ModelRequest {
            model: "test".into(),
            messages: vec![sav::ModelMessage::User("work".into())],
            max_output_tokens: Some(8192),
            tools: vec![],
            tool_choice: sav::ToolChoice::Auto,
            reasoning_effort: None,
            output_schema: None,
        };
        log.request(&request, 1).unwrap();
        log.session_finish(&RunSummary {
            failure: Some("provider unavailable".into()),
            ..RunSummary::default()
        })
        .unwrap();
        log.session_start().unwrap();
        log.turn_finish(
            1,
            &Some(ModelUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
            }),
            "stop",
        )
        .unwrap();
        log.session_finish(&RunSummary {
            turns: 1,
            answer: Some("done".into()),
            ..RunSummary::default()
        })
        .unwrap();
        let journal: Vec<Value> = std::fs::read_to_string(log.journal_path())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let finishes: Vec<&Value> = journal
            .iter()
            .map(|record| &record["event"])
            .filter(|record| record["type"] == "session_finish")
            .collect();
        assert_eq!(finishes[0]["total_tokens"], Value::Null);
        assert!(finishes[0]["estimated_budget_tokens"].as_u64().unwrap() >= 8192);
        assert_eq!(finishes[1]["total_tokens"], 30);
        assert_eq!(finishes[1]["estimated_budget_tokens"], 0);
        assert_eq!(finishes[1]["turns"], 1);
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

    #[test]
    fn prompt_slug_is_bounded_lowercase_and_filesystem_safe() {
        assert_eq!(prompt_slug("Fix the bug, please!"), "fix-the-bug-please");
        assert_eq!(
            prompt_slug("Hello, world -- this is a long prompt"),
            "hello-world-this-is-a"
        );
        assert_eq!(prompt_slug("  spaced   out  "), "spaced-out");
        assert_eq!(prompt_slug("!!!"), "");
        assert_eq!(prompt_slug("   "), "");
        assert_eq!(prompt_slug(""), "");
        let long = "abcdefghijklm ".repeat(6);
        let slug = prompt_slug(long.trim_end());
        assert!(slug.len() <= 40, "slug is bounded: {slug:?}");
        assert!(!slug.ends_with('-'));
        assert!(
            slug.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'),
            "only alphanumerics and hyphens: {slug:?}"
        );
    }

    #[test]
    fn first_prompt_renames_journal_and_transcript_once() {
        let dir = private_tempdir();
        let mut log = SessionLog::open(dir.path(), "test").unwrap();
        let before = log.journal_path().to_owned();
        Recorder::on_prompt(&mut log, "Fix the bug, please!").unwrap();
        let after = log.journal_path().to_owned();
        assert_ne!(before, after);
        assert!(
            after
                .to_string_lossy()
                .ends_with("fix-the-bug-please.jsonl"),
            "{after:?}"
        );
        assert!(
            log.transcript_path()
                .to_string_lossy()
                .ends_with("fix-the-bug-please.log"),
            "{}",
            log.transcript_path().display()
        );
        assert!(!before.exists(), "the old journal name is gone");
        // A later prompt in the same session never renames the record again.
        Recorder::on_prompt(&mut log, "A completely different prompt").unwrap();
        assert_eq!(log.journal_path(), after);
        // Records keep working through the renamed files.
        log.notice("still writing").unwrap();
        let text = std::fs::read_to_string(log.transcript_path()).unwrap();
        assert!(text.contains("still writing"));
    }
}
