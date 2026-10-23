use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};

use nix::fcntl::{OFlag, openat};
use nix::sys::stat::Mode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, Result};

use super::{identifier, invalid};

const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_POLL_RECORDS: usize = 256;

/// One validated version-one diagnostic event, not an executable checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalRecord {
    pub schema_version: u32,
    pub run_id: String,
    pub sequence: u64,
    pub event: Value,
}

/// A no-clobber, private, held-descriptor diagnostic writer.
pub struct RunJournal {
    file: File,
    path: PathBuf,
    run_id: String,
    sequence: u64,
    bytes: u64,
    finished: bool,
    poisoned: bool,
}

impl RunJournal {
    /// Creates a private journal and synchronizes its initial metadata.
    pub fn create(path: &Path, run_id: &str, metadata: Value) -> Result<Self> {
        if !identifier(run_id) || !metadata.is_object() {
            return Err(invalid(
                "run ID must be a bounded identifier and metadata an object",
            ));
        }
        let file = open_regular(path, true)?;
        let mut writer = Self {
            file,
            path: path.to_owned(),
            run_id: run_id.into(),
            sequence: 0,
            bytes: 0,
            finished: false,
            poisoned: false,
        };
        writer.append(json!({"type":"run_start", "data":metadata}))?;
        writer.checkpoint()?;
        Ok(writer)
    }

    /// Returns the diagnostic pathname; writes retain the original descriptor.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends a bounded application event; reserved lifecycle events are rejected.
    pub fn record(&mut self, event: &impl Serialize) -> Result<()> {
        let event =
            serde_json::to_value(event).map_err(|_| invalid("event cannot be serialized"))?;
        validate_event(&event)?;
        if matches!(event["type"].as_str(), Some("run_start" | "run_end")) {
            return Err(invalid(
                "run lifecycle event is reserved for the journal owner",
            ));
        }
        self.append(event)
    }

    /// Synchronizes prior observations before the host dispatches effects.
    pub fn checkpoint(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(invalid(
                "journal failed previously; no further effects are safe",
            ));
        }
        if let Err(source) = self.file.sync_data() {
            self.poisoned = true;
            return Err(io_error(&self.path, "synchronize live journal", source));
        }
        Ok(())
    }

    /// Appends and synchronizes a terminal disposition, without implying rollback.
    pub fn finish(&mut self, disposition: &str) -> Result<()> {
        if !identifier(disposition) || disposition.len() > 64 {
            return Err(invalid("terminal disposition must be a bounded identifier"));
        }
        self.append(json!({"type":"run_end", "data":{"disposition":disposition}}))?;
        self.checkpoint()?;
        self.finished = true;
        Ok(())
    }

    fn append(&mut self, event: Value) -> Result<()> {
        if self.finished || self.poisoned {
            return Err(invalid(
                "journal is terminal or failed; no further records are allowed",
            ));
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("journal sequence exhausted"))?;
        let record = JournalRecord {
            schema_version: 1,
            run_id: self.run_id.clone(),
            sequence,
            event,
        };
        let mut bytes =
            serde_json::to_vec(&record).map_err(|_| invalid("event cannot be serialized"))?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD_BYTES
            || self.bytes.saturating_add(bytes.len() as u64) > MAX_JOURNAL_BYTES
        {
            return Err(invalid(
                "live journal exceeds its record or total byte limit",
            ));
        }
        if let Err(source) = self.file.write_all(&bytes) {
            self.poisoned = true;
            return Err(io_error(&self.path, "append live journal", source));
        }
        self.sequence = sequence;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
}

/// A bounded follower of the original regular file, including partial live tails.
pub struct JournalFollower {
    reader: BufReader<File>,
    path: PathBuf,
    pending: Vec<u8>,
    run_id: Option<String>,
    sequence: u64,
    bytes: u64,
    finished: bool,
    failed: bool,
}

impl JournalFollower {
    /// Opens a non-link regular file without following any path component links.
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            reader: BufReader::new(open_regular(path, false)?),
            path: path.to_owned(),
            pending: Vec::new(),
            run_id: None,
            sequence: 0,
            bytes: 0,
            finished: false,
            failed: false,
        })
    }

    /// Reads at most 256 complete events; EOF without `run_end` remains incomplete.
    pub fn poll(&mut self) -> Result<Vec<JournalRecord>> {
        if self.failed {
            return Err(invalid("journal follower failed previously"));
        }
        let result = self.read_available();
        self.failed = result.is_err();
        result
    }

    /// Whether an explicit terminal record has been read.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Reports a truncated final record when no more input is expected.
    pub fn end_of_input(&self) -> Result<()> {
        if !self.pending.is_empty() {
            return Err(invalid("journal ends with an incomplete record"));
        }
        if !self.finished {
            return Err(invalid(
                "journal has no terminal disposition; effects may be unknown",
            ));
        }
        Ok(())
    }

    fn read_available(&mut self) -> Result<Vec<JournalRecord>> {
        let mut records = Vec::new();
        while records.len() < MAX_POLL_RECORDS {
            let available = self
                .reader
                .fill_buf()
                .map_err(|source| io_error(&self.path, "read live journal", source))?;
            if available.is_empty() {
                break;
            }
            if self.finished {
                return Err(invalid("journal contains data after its terminal event"));
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let count = newline.map_or(available.len(), |index| index + 1);
            if self.pending.len().saturating_add(count) > MAX_RECORD_BYTES
                || self.bytes.saturating_add(count as u64) > MAX_JOURNAL_BYTES
            {
                return Err(invalid(
                    "live journal exceeds its record or total byte limit",
                ));
            }
            self.pending.extend_from_slice(&available[..count]);
            self.reader.consume(count);
            self.bytes += count as u64;
            if newline.is_none() {
                continue;
            }
            let record: JournalRecord = serde_json::from_slice(&self.pending)
                .map_err(|_| invalid("journal record is malformed"))?;
            self.pending.clear();
            validate_event(&record.event)?;
            if record.schema_version != 1
                || !identifier(&record.run_id)
                || record.sequence != self.sequence.saturating_add(1)
                || self.run_id.as_ref().is_some_and(|id| id != &record.run_id)
            {
                return Err(invalid(
                    "journal version, run identity or sequence is invalid",
                ));
            }
            if self.sequence == 0 && record.event["type"] != "run_start"
                || self.sequence > 0 && record.event["type"] == "run_start"
            {
                return Err(invalid("journal must begin with exactly one run_start"));
            }
            self.run_id.get_or_insert_with(|| record.run_id.clone());
            self.sequence = record.sequence;
            self.finished = record.event["type"] == "run_end";
            if self.finished
                && !record.event["data"]["disposition"]
                    .as_str()
                    .is_some_and(|value| identifier(value) && value.len() <= 64)
            {
                return Err(invalid("journal terminal disposition is invalid"));
            }
            records.push(record);
        }
        Ok(records)
    }
}

fn validate_event(event: &Value) -> Result<()> {
    if !event.is_object()
        || !event["type"]
            .as_str()
            .is_some_and(|kind| identifier(kind) && kind.len() <= 64)
    {
        return Err(invalid(
            "journal event must be an object with a bounded type",
        ));
    }
    Ok(())
}

fn io_error(path: &Path, operation: &'static str, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}

fn open_regular(path: &Path, create: bool) -> Result<File> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| io_error(path, "resolve journal directory", source))?
            .join(path)
    };
    let parent = absolute
        .parent()
        .ok_or_else(|| invalid("journal requires a parent directory"))?;
    let name = absolute
        .file_name()
        .ok_or_else(|| invalid("journal requires a filename"))?;
    let mut directory =
        File::open("/").map_err(|source| io_error(path, "open journal root", source))?;
    for component in parent.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = File::from(
                    openat(
                        &directory,
                        name,
                        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|error| io_error(path, "open journal directory", error.into()))?,
                );
            }
            _ => return Err(invalid("journal path must not contain . or ..")),
        }
    }
    let access = if create {
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL
    } else {
        OFlag::O_RDONLY
    };
    let file = File::from(
        openat(
            &directory,
            name,
            access | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )
        .map_err(|error| io_error(path, "open live journal", error.into()))?,
    );
    if !file
        .metadata()
        .map_err(|source| io_error(path, "inspect live journal", source))?
        .is_file()
    {
        return Err(invalid("live journal must be a regular file"));
    }
    if create {
        directory
            .sync_all()
            .map_err(|source| io_error(path, "synchronize journal directory", source))?;
    }
    Ok(file)
}
