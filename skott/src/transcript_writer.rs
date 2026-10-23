//! Skott session transcript writer.
//!
//! Writes sessions in the proposed structured format:
//! - JSON manifest (session.json) with message index
//! - Individual .md files for each message body
//!
//! This enables source-derived re-theming and reliable message role identification.
//! The operational NDJSON journal remains unchanged; this is an additional
//! human-readable structured record.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nix::fcntl::{OFlag, openat, renameat};
use nix::sys::stat::{Mode, mkdirat};
use nix::unistd::{UnlinkatFlags, unlinkat};
use serde::{Deserialize, Serialize};

static NEXT_MANIFEST: AtomicU64 = AtomicU64::new(0);

/// Session manifest in the proposed format.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionManifest {
    pub schema_version: u32,
    pub kind: String,
    pub session_id: String,
    pub canonical_evidence: bool,
    pub status: String,
    pub markdown_profile: String,
    pub model: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ManifestEntry {
    pub id: String,
    pub sequence: u64,
    pub kind: String,
    pub disposition: Option<String>,
    pub media_type: String,
    pub path: String,
    pub bytes: u64,
    pub complete: bool,
    pub truncated: bool,
}

/// TranscriptWriter accumulates session events and writes them to the structured format.
pub struct TranscriptWriter {
    base_dir: PathBuf,
    directory: File,
    messages: File,
    manifest: SessionManifest,
    sequence: u64,
}

impl TranscriptWriter {
    /// Creates a private no-clobber session using held, non-following directories.
    pub fn new(session_id: &str, base_dir: &Path, model: Option<&str>) -> io::Result<Self> {
        if session_id.is_empty()
            || session_id.len() > 256
            || session_id == "."
            || session_id == ".."
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "transcript session identity must be one bounded filesystem-safe component",
            ));
        }
        let (_, parent) = crate::session_log::private_directory(base_dir)?;
        mkdirat(&parent, session_id, Mode::S_IRWXU)?;
        let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let directory = File::from(openat(&parent, session_id, flags, Mode::empty())?);
        mkdirat(&directory, "messages", Mode::S_IRWXU)?;
        let messages = File::from(openat(&directory, "messages", flags, Mode::empty())?);
        directory.sync_all()?;
        parent.sync_all()?;
        let base = base_dir.join(session_id);

        let manifest = SessionManifest {
            schema_version: 1,
            kind: "skott-conversation".to_string(),
            session_id: session_id.to_string(),
            canonical_evidence: false,
            status: "streaming".to_string(),
            markdown_profile: "commonmark-gfm-subset-v1".to_string(),
            model: model.map(|s| s.to_string()),
            created_at: Self::now_iso(),
            completed_at: None,
            entries: Vec::new(),
        };

        Ok(Self {
            base_dir: base,
            directory,
            messages,
            manifest,
            sequence: 0,
        })
    }

    fn now_iso() -> String {
        // A clock set before the Unix epoch is not a real failure mode for a
        // local CLI; degrade to a zero timestamp instead of panicking so the
        // session manifest still gets a well-formed (if ancient) value.
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = elapsed.as_secs();
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
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
        format!(
            "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z",
            year, month, day, hour, minute, second
        )
    }

    /// Write the manifest to disk.
    pub fn write_manifest(&mut self) -> io::Result<()> {
        let temporary = format!(
            ".session-{}-{}.tmp",
            std::process::id(),
            NEXT_MANIFEST.fetch_add(1, Ordering::Relaxed)
        );
        let flags =
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let mut file = File::from(openat(
            &self.directory,
            temporary.as_str(),
            flags,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )?);
        let result = (|| -> io::Result<()> {
            serde_json::to_writer_pretty(&mut file, &self.manifest)?;
            file.sync_all()?;
            renameat(
                &self.directory,
                temporary.as_str(),
                &self.directory,
                "session.json",
            )?;
            self.directory.sync_all()
        })();
        if result.is_err()
            && let Err(error) = unlinkat(
                &self.directory,
                temporary.as_str(),
                UnlinkatFlags::NoRemoveDir,
            )
            && error != nix::errno::Errno::ENOENT
        {
            tracing::warn!(%error, "failed to remove an unpublished transcript manifest");
        }
        result
    }

    /// Record a user message.
    pub fn record_user(&mut self, content: &str) -> io::Result<u64> {
        self.manifest.status = "streaming".into();
        self.manifest.completed_at = None;
        self.sequence += 1;
        self.write_message_file(self.sequence, "user", None, content, true, false)
    }

    /// Record an assistant message.
    pub fn record_assistant(&mut self, content: &str, is_final: bool) -> io::Result<u64> {
        self.sequence += 1;
        let disposition = if is_final {
            Some("final")
        } else {
            Some("interim")
        };
        self.write_message_file(
            self.sequence,
            "assistant",
            Some(disposition),
            content,
            true,
            false,
        )
    }

    /// Record a reasoning block.
    pub fn record_reasoning(&mut self, content: &str) -> io::Result<u64> {
        self.sequence += 1;
        self.write_message_file(self.sequence, "reasoning", None, content, true, false)
    }

    /// Record a tool dispatch.
    pub fn record_tool_dispatch(&mut self, tool: &str, args: &str) -> io::Result<u64> {
        self.sequence += 1;
        let content = format!("```tool-call\n{}({})\n```", tool, args);
        self.write_message_file(self.sequence, "tool_dispatch", None, &content, true, false)
    }

    /// Record a tool result.
    pub fn record_tool_result(&mut self, tool: &str, output: &str) -> io::Result<u64> {
        self.sequence += 1;
        let content = format!("```tool-result\n[{}]: {}\n```", tool, output);
        self.write_message_file(self.sequence, "tool_result", None, &content, true, false)
    }

    /// Record a notice (system message).
    pub fn record_notice(&mut self, message: &str) -> io::Result<u64> {
        self.sequence += 1;
        self.write_message_file(self.sequence, "notice", None, message, true, false)
    }

    fn write_message_file(
        &mut self,
        seq: u64,
        kind: &str,
        disposition: Option<Option<&str>>,
        content: &str,
        complete: bool,
        truncated: bool,
    ) -> io::Result<u64> {
        let filename = format!("{:06}.md", seq);
        let flags =
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        let mut f = File::from(openat(
            &self.messages,
            filename.as_str(),
            flags,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )?);
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        self.messages.sync_all()?;

        let entry = ManifestEntry {
            id: format!("message-{:06}", seq),
            sequence: seq,
            kind: kind.to_string(),
            disposition: disposition.flatten().map(|s| s.to_string()),
            media_type: "text/markdown".to_string(),
            path: format!("messages/{}", filename),
            bytes: content.len() as u64,
            complete,
            truncated,
        };

        self.manifest.entries.push(entry);
        self.write_manifest()?;

        Ok(seq)
    }

    /// Complete the session transcript.
    pub fn complete(&mut self) -> io::Result<()> {
        self.manifest.status = "completed".to_string();
        self.manifest.completed_at = Some(Self::now_iso());
        self.write_manifest()?;
        Ok(())
    }

    /// Get the session directory path.
    pub fn session_dir(&self) -> &Path {
        &self.base_dir
    }
}
