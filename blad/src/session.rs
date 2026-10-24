//! Opt-in, read-only Kvist/Skott transcript integration.
//!
//! Session artifacts are bounded regular UTF-8 files opened relative to held
//! directory descriptors. An explicitly supplied root's ancestors are not
//! isolated. No live-following or aggregate-memory bound is provided.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, open, openat, statat};
use serde::Deserialize;
use serde_json::Value;

const MAX_ARTIFACT: usize = 16 * 1024 * 1024;
const READ_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

/// A validated session, retaining its opened directory across pathname changes.
#[derive(Clone)]
pub struct Session {
    /// Producer session identifier.
    pub id: String,
    /// Display path; message reads use the retained descriptor instead.
    pub path: PathBuf,
    /// Original producer metadata; required fields are revalidated on reading.
    pub manifest: Value,
    /// Optional producer model name.
    pub model: Option<String>,
    /// Whether the producer marked the snapshot completed.
    pub completed: bool,
    /// Optional creation time, sorted lexically as producer-authored ISO text.
    pub created_at: Option<String>,
    /// Number of manifest message entries.
    pub message_count: usize,
    directory: Arc<File>,
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    kind: String,
    session_id: String,
    status: String,
    model: Option<String>,
    created_at: Option<String>,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    sequence: usize,
    kind: String,
    path: String,
    disposition: Option<String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn context(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

fn directory(path: &Path) -> io::Result<File> {
    let normalized: PathBuf = path.components().collect();
    open(&normalized, READ_FLAGS | OFlags::DIRECTORY, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
}

fn child_directory(parent: &File, name: &Path) -> io::Result<File> {
    openat(parent, name, READ_FLAGS | OFlags::DIRECTORY, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
}

fn artifact(parent: &File, name: &Path) -> io::Result<String> {
    let descriptor = openat(parent, name, READ_FLAGS, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)?;
    let bytes = crate::file_input::read_regular_descriptor(descriptor, MAX_ARTIFACT)?;
    String::from_utf8(bytes).map_err(|error| invalid(error.to_string()))
}

fn entry_name(path: &str) -> io::Result<&Path> {
    let mut components = Path::new(path).components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::Normal(parent)), Some(Component::Normal(name)), None)
            if parent == "messages" =>
        {
            Ok(Path::new(name))
        }
        _ => Err(invalid("message path must be messages/<filename>")),
    }
}

fn manifest(value: &Value) -> io::Result<Manifest> {
    let parsed: Manifest =
        serde_json::from_value(value.clone()).map_err(|error| invalid(error.to_string()))?;
    if parsed.schema_version != 1 || parsed.kind != "skott-conversation" {
        return Err(invalid(
            "expected schema_version 1 and kind skott-conversation",
        ));
    }
    if parsed.session_id.trim().is_empty() || parsed.status.trim().is_empty() {
        return Err(invalid("session_id and status must not be empty"));
    }
    for entry in &parsed.entries {
        if entry.sequence == 0 || entry.kind.trim().is_empty() {
            return Err(invalid(
                "message sequence must be positive and kind nonempty",
            ));
        }
        entry_name(&entry.path)?;
    }
    Ok(parsed)
}

fn load_directory(path: &Path, directory: File) -> io::Result<Session> {
    let source = artifact(&directory, Path::new("session.json"))?;
    let value: Value = serde_json::from_str(&source).map_err(|error| invalid(error.to_string()))?;
    let parsed = manifest(&value)?;
    Ok(Session {
        id: parsed.session_id,
        path: path.to_path_buf(),
        manifest: value,
        model: parsed.model,
        completed: parsed.status == "completed",
        created_at: parsed.created_at,
        message_count: parsed.entries.len(),
        directory: Arc::new(directory),
    })
}

/// List newest sessions, breaking equal timestamps by descending session ID.
///
/// A missing runs root is empty. Directories without a manifest are ignored;
/// malformed artifacts, symlinks and other read failures are explicit errors.
pub fn list_sessions(log_dir: &Path, limit: usize) -> io::Result<Vec<Session>> {
    let root = match directory(log_dir) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(context(log_dir, error)),
    };
    let entries = Dir::read_from(&root).map_err(io::Error::from)?;
    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io::Error::from)?;
        let name = OsStr::from_bytes(entry.file_name().to_bytes());
        if name == "." || name == ".." {
            continue;
        }
        let path = log_dir.join(name);
        let metadata = statat(&root, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(io::Error::from)
            .map_err(|error| context(&path, error))?;
        match FileType::from_raw_mode(metadata.st_mode) {
            FileType::Directory => {}
            FileType::Symlink => {
                return Err(context(&path, invalid("session symlinks are not allowed")));
            }
            _ => continue,
        }
        let child =
            child_directory(&root, Path::new(name)).map_err(|error| context(&path, error))?;
        match load_directory(&path, child) {
            Ok(session) => sessions.push(session),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(context(&path.join("session.json"), error)),
        }
    }
    sessions.sort_by(|a, b| (&b.created_at, &b.id).cmp(&(&a.created_at, &a.id)));
    sessions.truncate(limit);
    Ok(sessions)
}

/// Load a session directory without following its final symlink.
pub fn load_session_by_path(path: &Path) -> io::Result<Session> {
    let root = directory(path).map_err(|error| context(path, error))?;
    load_directory(path, root).map_err(|error| context(&path.join("session.json"), error))
}

/// Load an exact ID or unique nonempty prefix without a listing cutoff.
pub fn load_session(log_dir: &Path, session_id: &str) -> io::Result<Session> {
    if session_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session ID must not be empty",
        ));
    }
    let sessions = list_sessions(log_dir, usize::MAX)?;
    if let Some(session) = sessions.iter().find(|session| session.id == session_id) {
        return Ok(session.clone());
    }
    let mut matches = sessions
        .into_iter()
        .filter(|session| session.id.starts_with(session_id));
    let first = matches.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("session not found: {session_id}"),
        )
    })?;
    if matches.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ambiguous session ID prefix; use a full ID",
        ));
    }
    Ok(first)
}

/// Load all message bodies, failing instead of substituting missing content.
pub fn load_messages(session: &Session) -> io::Result<Vec<Message>> {
    let parsed = manifest(&session.manifest)?;
    if parsed.entries.is_empty() {
        return Ok(Vec::new());
    }
    let messages = child_directory(&session.directory, Path::new("messages"))
        .map_err(|error| context(&session.path.join("messages"), error))?;
    parsed
        .entries
        .into_iter()
        .map(|entry| {
            let name = entry_name(&entry.path)?;
            let content = artifact(&messages, name)
                .map_err(|error| context(&session.path.join(&entry.path), error))?;
            Ok(Message {
                sequence: entry.sequence,
                kind: entry.kind,
                disposition: entry.disposition,
                content,
            })
        })
        .collect()
}

/// One producer-authored Markdown message.
pub struct Message {
    /// Producer sequence number, preserved in manifest order.
    pub sequence: usize,
    /// Producer message kind.
    pub kind: String,
    /// Optional assistant disposition.
    pub disposition: Option<String>,
    /// Original UTF-8 Markdown body.
    pub content: String,
}
