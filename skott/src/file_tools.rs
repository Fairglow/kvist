//! Streaming native file primitives. This module is not an isolation boundary:
//! the sandbox executor supplies the namespace and authority of the helper.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, openat, renameat};
use nix::sys::stat::{Mode, SFlag, fchmod, fstatat};
use nix::unistd::{UnlinkatFlags, unlinkat};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result, io_error};

mod mutation;
mod patch;
mod revision;
mod search;
mod stream;
use mutation::{Mutation, replace};
use search::search;
use stream::read_page;

/// Maximum encoded helper request, checked before parsing.
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Maximum success output including the helper's line terminator. Complete
/// JSON pages fit below the loop's combined 8 KiB result preview.
pub const MAX_OUTPUT_BYTES: usize = 7000;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const MAX_ENTRIES: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_SCAN_BYTES: usize = 8 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 1024;
const MAX_QUERY_BYTES: usize = 1024;
static NEXT_REPLACEMENT: AtomicU64 = AtomicU64::new(0);

fn default_read_limit() -> usize {
    4096
}
fn default_page_limit() -> usize {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_read_limit")]
    limit: usize,
    #[serde(default)]
    include_sha256: bool,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageArgs {
    path: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_page_limit")]
    limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindArgs {
    path: String,
    pattern: String,
    #[serde(default)]
    include_generated: bool,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_page_limit")]
    limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    path: String,
    query: String,
    #[serde(default)]
    include_generated: bool,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    file_pattern: Option<String>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_page_limit")]
    limit: usize,
    #[serde(
        default,
        deserialize_with = "optional_offset",
        skip_serializing_if = "Option::is_none"
    )]
    scan_offset: Option<u64>,
}

fn optional_offset<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error> {
    u64::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    expected_sha256: Option<String>,
}

fn optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppendArgs {
    path: String,
    content: String,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchArgs {
    path: String,
    patch: String,
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "tool",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Operation {
    ReadFile(ReadArgs),
    ListDir(PageArgs),
    FindFiles(FindArgs),
    SearchFiles(SearchArgs),
    WriteFile(WriteArgs),
    EditFile(EditArgs),
    AppendFile(AppendArgs),
    ApplyPatch(PatchArgs),
}

impl Operation {
    fn path(&self) -> &str {
        match self {
            Self::ReadFile(a) => &a.path,
            Self::ListDir(a) => &a.path,
            Self::FindFiles(a) => &a.path,
            Self::SearchFiles(a) => &a.path,
            Self::WriteFile(a) => &a.path,
            Self::EditFile(a) => &a.path,
            Self::AppendFile(a) => &a.path,
            Self::ApplyPatch(a) => &a.path,
        }
    }

    fn path_mut(&mut self) -> &mut String {
        match self {
            Self::ReadFile(a) => &mut a.path,
            Self::ListDir(a) => &mut a.path,
            Self::FindFiles(a) => &mut a.path,
            Self::SearchFiles(a) => &mut a.path,
            Self::WriteFile(a) => &mut a.path,
            Self::EditFile(a) => &mut a.path,
            Self::AppendFile(a) => &mut a.path,
            Self::ApplyPatch(a) => &mut a.path,
        }
    }
}

/// Closed helper payload. Construction and execution both validate bounds;
/// callers must not mistake the write root for confinement of host shell tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRequest {
    write_root: String,
    operation: Operation,
}

impl FileRequest {
    /// Validates one model-facing native tool and its closed arguments.
    pub fn new(write_root: &str, tool: &str, arguments: Value) -> Result<Self> {
        let request = Self::from_arguments(write_root, tool, arguments)?;
        request.validate()?;
        Ok(request)
    }

    fn from_arguments(write_root: &str, tool: &str, arguments: Value) -> Result<Self> {
        let operation = serde_json::from_value(json!({"tool":tool,"arguments":arguments}))
            .map_err(|_| failure("invalid or unknown native file tool arguments"))?;
        Ok(Self {
            write_root: write_root.to_owned(),
            operation,
        })
    }

    pub(crate) fn for_registry(write_root: &str, tool: &str, arguments: Value) -> Result<Self> {
        // Closed typed parsing precedes policy checks so unknown keys or wrong
        // argument types are never misclassified as authority denials.
        let request = Self::from_arguments(write_root, tool, arguments)?;
        let denial = |reason: &str| Error::ToolPolicy {
            tool: tool.to_owned(),
            reason: reason.to_owned(),
        };
        canonical_path(write_root)
            .map_err(|_| denial("configured write root must be canonical"))?;
        canonical_path(request.operation.path()).map_err(|_| {
            denial("path must be canonical, absolute and bounded without . or .. or NUL")
        })?;
        if matches!(
            request.operation,
            Operation::WriteFile(_)
                | Operation::EditFile(_)
                | Operation::AppendFile(_)
                | Operation::ApplyPatch(_)
        ) {
            request.validate_mutation(request.operation.path())
                .map_err(|_| denial("mutation target must be strictly inside the write root; outside and sibling paths are forbidden"))?;
        }
        request.validate()?;
        Ok(request)
    }

    /// Maps the sandbox workspace namespace to an explicitly opted-in host
    /// workspace. This does not constrain any host shell command.
    pub fn host_mapped(&self, workdir: &Path) -> Result<Self> {
        let root = workdir
            .canonicalize()
            .map_err(|e| io_error("resolve host file-tool workspace", None, e))?;
        self.mapped_to_scope(&root)
    }

    /// Maps to the exact host source of the prepared sandbox grant, without
    /// resolving the original workspace selection again.
    pub(crate) fn sandbox_mapped(&self, write_root: &str, source: &Path) -> Result<Self> {
        if write_root != self.write_root {
            return Err(failure(
                "prepared sandbox write root differs from the native helper namespace",
            ));
        }
        self.mapped_to_scope(source)
    }

    fn mapped_to_scope(&self, root: &Path) -> Result<Self> {
        let host_root = root
            .to_str()
            .ok_or_else(|| failure("workspace must be UTF-8"))?;
        canonical_path(host_root)?;
        let mut mapped = self.clone();
        if within(self.operation.path(), &self.write_root) {
            let relative = self
                .operation
                .path()
                .strip_prefix(&self.write_root)
                .ok_or_else(|| failure("cannot map workspace path"))?
                .trim_start_matches('/');
            let path = if relative.is_empty() {
                root.to_owned()
            } else {
                root.join(relative)
            };
            *mapped.operation.path_mut() = path
                .to_str()
                .ok_or_else(|| failure("mapped path must be UTF-8"))?
                .to_owned();
        }
        mapped.write_root = host_root.to_owned();
        mapped.validate()?;
        Ok(mapped)
    }

    /// Returns the bounded JSON bytes that the executor stages read-only.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| failure("cannot encode file request"))?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(failure("encoded file request exceeds the byte bound"));
        }
        Ok(bytes)
    }

    fn validate(&self) -> Result<()> {
        canonical_path(&self.write_root)?;
        canonical_path(self.operation.path())?;
        match &self.operation {
            Operation::ReadFile(a) => {
                page_bounds(a.offset, a.limit, 16384, usize::MAX)?;
                if let Some(expected) = &a.expected_sha256 {
                    validate_digest(expected)?;
                }
            }
            Operation::ListDir(a) => page_bounds(a.offset, a.limit, 256, MAX_ENTRIES)?,
            Operation::FindFiles(a) => {
                page_bounds(a.offset, a.limit, 256, MAX_ENTRIES)?;
                literal(&a.pattern, "pattern", true)?;
            }
            Operation::SearchFiles(a) => {
                page_bounds(a.offset, a.limit, 256, MAX_ENTRIES)?;
                literal(&a.query, "query", false)?;
                if let Some(pattern) = &a.file_pattern {
                    literal(pattern, "file_pattern", true)?;
                }
            }
            Operation::WriteFile(a) => {
                self.validate_mutation(&a.path)?;
                text_bound(&a.content)?;
                if let Some(expected) = &a.expected_sha256 {
                    validate_digest(expected)?;
                }
            }
            Operation::EditFile(a) => {
                self.validate_mutation(&a.path)?;
                text_bound(&a.old_text)?;
                text_bound(&a.new_text)?;
                if a.old_text.is_empty() {
                    return Err(failure("old_text must be nonempty"));
                }
                if let Some(expected) = &a.expected_sha256 {
                    validate_digest(expected)?;
                }
            }
            Operation::AppendFile(a) => {
                self.validate_mutation(&a.path)?;
                text_bound(&a.content)?;
                if let Some(expected) = &a.expected_sha256 {
                    validate_digest(expected)?;
                }
            }
            Operation::ApplyPatch(a) => {
                self.validate_mutation(&a.path)?;
                text_bound(&a.patch)?;
                if let Some(expected) = &a.expected_sha256 {
                    validate_digest(expected)?;
                }
            }
        }
        Ok(())
    }

    fn validate_mutation(&self, path: &str) -> Result<()> {
        if path == self.write_root || !within(path, &self.write_root) {
            return Err(failure(
                "mutation path must be strictly inside the write root",
            ));
        }
        Ok(())
    }
}

fn failure(reason: impl Into<String>) -> Error {
    Error::ToolRender {
        tool: "file_tool".to_owned(),
        reason: reason.into(),
    }
}

/// Validates strict lexical absolute paths without normalization or effects.
pub fn canonical_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || !path.starts_with('/')
        || path.contains('\0')
        || (path != "/" && path.ends_with('/'))
        || (path != "/"
            && path[1..]
                .split('/')
                .any(|c| c.is_empty() || c == "." || c == ".."))
    {
        return Err(failure(
            "path must be a bounded canonical absolute path without . or ..",
        ));
    }
    Ok(())
}

fn within(path: &str, root: &str) -> bool {
    root == "/"
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn text_bound(text: &str) -> Result<()> {
    if text.len() > MAX_TEXT_BYTES {
        Err(failure("text argument exceeds the byte bound"))
    } else {
        Ok(())
    }
}

fn literal(text: &str, label: &str, empty_allowed: bool) -> Result<()> {
    if text.len() > MAX_QUERY_BYTES || text.contains('\0') || (!empty_allowed && text.is_empty()) {
        Err(failure(format!(
            "{label} must be a bounded {}literal substring",
            if empty_allowed { "" } else { "nonempty " }
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_digest(value: &str) -> Result<()> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    });
    if valid {
        Ok(())
    } else {
        Err(failure(
            "expected_sha256 must be sha256: followed by 64 lowercase hexadecimal digits",
        ))
    }
}

fn page_bounds(offset: usize, limit: usize, max_limit: usize, max_offset: usize) -> Result<()> {
    if limit == 0 || limit > max_limit || offset > max_offset {
        Err(failure(format!(
            "pagination exceeds bounds (limit 1..={max_limit}, offset 0..={max_offset})"
        )))
    } else {
        Ok(())
    }
}

/// Parses one bounded, strictly typed payload without filesystem effects.
pub fn parse_file_request(bytes: &[u8]) -> Result<FileRequest> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(failure("file request exceeds the byte bound"));
    }
    let request: FileRequest = serde_json::from_slice(bytes)
        .map_err(|_| failure("malformed file request or unknown fields"))?;
    request.validate()?;
    Ok(request)
}

/// Executes an independently revalidated native primitive under the caller's
/// existing filesystem authority. It is not a sandbox or an authorization API.
pub fn execute_file_request(request: &FileRequest) -> Result<Value> {
    request.validate()?;
    let output = match &request.operation {
        Operation::ReadFile(a) => read_page(a)?,
        Operation::ListDir(a) => list_page(a)?,
        Operation::FindFiles(a) => discover(a)?,
        Operation::SearchFiles(a) => search(a)?,
        Operation::WriteFile(a) => replace(
            request,
            &a.path,
            Mutation::Write(&a.content),
            a.expected_sha256.as_deref(),
        )?,
        Operation::EditFile(a) => replace(
            request,
            &a.path,
            Mutation::Edit(&a.old_text, &a.new_text),
            a.expected_sha256.as_deref(),
        )?,
        Operation::AppendFile(a) => replace(
            request,
            &a.path,
            Mutation::Append(&a.content),
            a.expected_sha256.as_deref(),
        )?,
        Operation::ApplyPatch(a) => replace(
            request,
            &a.path,
            Mutation::Patch(&a.patch),
            a.expected_sha256.as_deref(),
        )?,
    };
    ensure_output_bound(&output)?;
    Ok(output)
}

fn output_fits(output: &Value) -> Result<bool> {
    let encoded =
        serde_json::to_string(output).map_err(|_| failure("cannot encode file outcome"))?;
    if encoded.len() >= MAX_OUTPUT_BYTES {
        return Ok(false);
    }
    Ok(serde_json::to_vec(&encoded)
        .map_err(|_| failure("cannot encode model-facing file outcome"))?
        .len()
        < MAX_OUTPUT_BYTES)
}

fn ensure_output_bound(output: &Value) -> Result<()> {
    if output_fits(output)? {
        Ok(())
    } else {
        Err(failure(
            "file outcome exceeds the complete encoded output bound",
        ))
    }
}

fn open_read(path: &str, nofollow: bool) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK | if nofollow { nix::libc::O_NOFOLLOW } else { 0 })
        .open(path)
        .map_err(|e| io_error("open file for bounded read", Some(path), e))
}

fn directory(path: &str) -> Result<OwnedFd> {
    let mut fd = openat(
        AT_FDCWD,
        "/",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| failure(format!("cannot open filesystem root: {e}")))?;
    for component in path[1..].split('/').filter(|c| !c.is_empty()) {
        let child = openat(
            &fd,
            component,
            OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| failure(format!("cannot traverse directory component: {e}")))?;
        let child = File::from(child);
        let metadata = child
            .metadata()
            .map_err(|e| io_error("inspect directory component", None, e))?;
        if metadata.file_type().is_symlink() {
            return Err(failure(
                "cannot traverse directory: symlink components are forbidden",
            ));
        }
        if !metadata.is_dir() {
            return Err(failure("directory component is not a directory"));
        }
        // Reopen the pinned, classified inode, not the pathname an external
        // writer could replace between classification and readable access.
        fd = openat(
            &child,
            ".",
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| failure(format!("cannot open directory component: {e}")))?;
    }
    Ok(fd)
}

struct Entry {
    name: String,
    kind: std::fs::FileType,
}

fn entries(fd: &OwnedFd) -> Result<Vec<Entry>> {
    // The descriptor pins the directory even if an external writer renames it.
    let path = format!("/proc/self/fd/{}", fd.as_raw_fd());
    let iterator =
        std::fs::read_dir(&path).map_err(|e| io_error("list directory descriptor", None, e))?;
    let mut result = Vec::new();
    for entry in iterator {
        if result.len() >= MAX_ENTRIES {
            return Err(failure(format!(
                "directory entry count exceeds the {MAX_ENTRIES}-entry traversal bound; select a smaller directory scope"
            )));
        }
        let entry = entry.map_err(|e| io_error("read ordinary directory entry", None, e))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| failure("directory entry name must be UTF-8"))?;
        if name.len() > MAX_PATH_BYTES {
            return Err(failure("entry name exceeds the path byte bound"));
        }
        let kind = entry
            .file_type()
            .map_err(|e| io_error("inspect ordinary directory entry", None, e))?;
        result.push(Entry { name, kind });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

fn fitted_page<T: Serialize>(
    items: &[T],
    offset: usize,
    limit: usize,
    outcome: impl Fn(Value, Option<usize>) -> Value,
) -> Result<Value> {
    if offset > items.len() {
        return Err(failure("offset exceeds the result count"));
    }
    let count = limit.min(items.len() - offset);
    let candidate = |count| {
        let end = offset + count;
        let page = serde_json::to_value(&items[offset..end])
            .map_err(|_| failure("cannot encode result page"))?;
        Ok::<_, Error>(outcome(page, (end < items.len()).then_some(end)))
    };
    let requested = candidate(count)?;
    if output_fits(&requested)? {
        return Ok(requested);
    }
    let mut lower = 1;
    let mut upper = count;
    let mut fitted = None;
    while lower <= upper {
        let middle = lower + (upper - lower) / 2;
        let page = candidate(middle)?;
        if output_fits(&page)? {
            fitted = Some(page);
            lower = middle + 1;
        } else {
            upper = middle - 1;
        }
    }
    fitted.ok_or_else(|| failure("a single entry cannot fit the complete encoded output bound"))
}

fn list_page(args: &PageArgs) -> Result<Value> {
    let fd = directory(&args.path)?;
    let values: Vec<Value> = entries(&fd)?.into_iter().map(|entry| json!({
        "name":entry.name,
        "kind":if entry.kind.is_symlink() {"symlink"} else if entry.kind.is_dir() {"directory"}
            else if entry.kind.is_file() {"file"} else {"other"}
    })).collect();
    fitted_page(
        &values,
        args.offset,
        args.limit,
        |entries, next| json!({"entries":entries,"offset":args.offset,"next_offset":next,"total":values.len()}),
    )
}

#[derive(Default)]
struct WalkState {
    visited: usize,
    scanned_bytes: usize,
    skipped_symlinks: usize,
    skipped_binary: usize,
    skipped_generated: usize,
    skipped_special: usize,
    excluded_files: usize,
    scan_limit_reached: bool,
}

impl WalkState {
    fn complete(&self) -> bool {
        self.skipped_symlinks == 0
            && self.skipped_binary == 0
            && self.skipped_generated == 0
            && self.skipped_special == 0
            && self.excluded_files == 0
            && !self.scan_limit_reached
    }
}

fn generated_directory(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | "vendor" | "vendored" | ".skott"
    )
}

fn walk(
    fd: &OwnedFd,
    path: &str,
    depth: usize,
    include_generated: bool,
    state: &mut WalkState,
    visit: &mut impl FnMut(&OwnedFd, &str, &str, &mut WalkState) -> Result<()>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(failure(format!(
            "recursive traversal exceeds the depth bound ({MAX_DEPTH}); select a deeper, smaller directory scope"
        )));
    }
    for entry in entries(fd)? {
        if state.scan_limit_reached {
            break;
        }
        state.visited += 1;
        if state.visited > MAX_ENTRIES {
            return Err(failure(format!(
                "recursive traversal exceeds the entry count bound ({MAX_ENTRIES}); narrow the directory path; a file filter does not prune directories"
            )));
        }
        let full = if path == "/" {
            format!("/{}", entry.name)
        } else {
            format!("{path}/{}", entry.name)
        };
        canonical_path(&full)?;
        if entry.kind.is_symlink() {
            state.skipped_symlinks += 1;
        } else if entry.kind.is_dir() {
            if !include_generated && generated_directory(&entry.name) {
                state.skipped_generated += 1;
                continue;
            }
            let child = openat(
                fd,
                entry.name.as_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| failure(format!("cannot open ordinary traversal directory: {e}")))?;
            walk(&child, &full, depth + 1, include_generated, state, visit)?;
        } else if entry.kind.is_file() {
            visit(fd, &entry.name, &full, state)?;
        } else {
            state.skipped_special += 1;
        }
    }
    Ok(())
}

fn discover(args: &FindArgs) -> Result<Value> {
    let root = directory(&args.path)?;
    let mut state = WalkState::default();
    let mut files = Vec::<String>::new();
    walk(
        &root,
        &args.path,
        0,
        args.include_generated,
        &mut state,
        &mut |fd, name, path, _state| {
            // Reject substitutions or unreadable ordinary files, even on discovery.
            let file = openat(
                fd,
                name,
                OFlag::O_RDONLY | OFlag::O_NONBLOCK | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| failure(format!("cannot inspect ordinary discovered file: {e}")))?;
            if !File::from(file)
                .metadata()
                .map_err(|e| io_error("inspect discovered file", None, e))?
                .is_file()
            {
                return Err(failure("discovered file changed to an unsupported entry"));
            }
            if path
                .strip_prefix(&args.path)
                .unwrap_or(path)
                .contains(&args.pattern)
            {
                files.push(path.to_owned());
            }
            Ok(())
        },
    )?;
    files.sort();
    fitted_page(&files, args.offset, args.limit, |page, next| {
        json!({"files":page,"offset":args.offset,"next_offset":next,"total":files.len(),
        "skipped_symlinks":state.skipped_symlinks,"skipped_generated":state.skipped_generated,
        "skipped_special":state.skipped_special,
        "visited_entries":state.visited,"complete":state.complete()})
    })
}

enum SearchScope {
    Directory(OwnedFd),
    File(File),
}

fn search_scope(path: &str) -> Result<SearchScope> {
    if path == "/" {
        return directory(path).map(SearchScope::Directory);
    }
    let (parent_path, name) = path
        .rsplit_once('/')
        .ok_or_else(|| failure("search scope requires an absolute path"))?;
    let parent = directory(if parent_path.is_empty() {
        "/"
    } else {
        parent_path
    })?;
    let metadata = fstatat(&parent, name, AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|e| failure(format!("cannot inspect search scope: {e}")))?;
    let kind = SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT;
    if kind == SFlag::S_IFLNK {
        return Err(failure("search scope must not be a symlink"));
    }
    if kind != SFlag::S_IFREG && kind != SFlag::S_IFDIR {
        return Err(failure(
            "search scope must be a regular file or directory; choose a source file or directory, not a socket, FIFO or device",
        ));
    }
    let fd = openat(
        &parent,
        name,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| {
        if e == nix::errno::Errno::ELOOP {
            failure("search scope must not be a symlink")
        } else {
            failure(format!("cannot open search scope: {e}"))
        }
    })?;
    let file = File::from(fd);
    let metadata = file
        .metadata()
        .map_err(|e| io_error("inspect search scope", None, e))?;
    if metadata.is_dir() {
        Ok(SearchScope::Directory(file.into()))
    } else if metadata.is_file() {
        Ok(SearchScope::File(file))
    } else {
        Err(failure("search scope must be a regular file or directory"))
    }
}
