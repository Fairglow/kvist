//! Exact, local checkpoints of unaccepted continuous-task work.
#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

use nix::fcntl::{OFlag, openat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use super::{
    MAX_SCOPE_DEPTH, MAX_SCOPE_ENTRIES, MAX_SCOPE_FILE_BYTES, MAX_SCOPE_TOTAL_BYTES,
    ScopeHashBudget, TaskContext,
};
use crate::{KvistError, Result};

mod process;
use process::{advance, git};

const COAUTHOR: &str = "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>";
const ROOTS: [&str; 5] = [
    "src",
    "tests",
    "TODOS.yaml",
    "IMPL.md",
    ".kvist/continuous-reviews",
];
const MAX_PATH: usize = 4096;
const MAX_GIT_OUTPUT: u64 = 16 * 1024 * 1024;
const MAX_INDEX: u64 = 64 * 1024 * 1024;
type Entries = BTreeMap<String, GitEntry>;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    component: String,
    entries: BTreeMap<String, FileState>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileState {
    digest: String,
    mode: u32,
    size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct GitEntry {
    mode: u32,
    oid: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Change {
    before: Option<GitEntry>,
    after: Option<GitEntry>,
}

/// Persist this value durably before calling `finish`; it is not acceptance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreparedCommit {
    pub(super) commit_id: String,
    expected_head: String,
    branch: String,
    after: Snapshot,
    changes: BTreeMap<String, Change>,
}

fn fail(reason: impl Into<String>) -> KvistError {
    KvistError::VcsCommitFailed {
        reason: format!("continuous checkpoint: {}", reason.into()),
    }
}

fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|error| fail(format!("filesystem/process operation failed: {error}; inspect permissions and retry the pending checkpoint")))
}

fn reject(invalid: bool, reason: &str) -> Result<()> {
    if invalid { Err(fail(reason)) } else { Ok(()) }
}

fn parent_dir(path: &Path) -> Result<&Path> {
    path.parent()
        .ok_or_else(|| fail("filesystem path has no parent"))
}

fn stamp(metadata: &fs::Metadata) -> (u64, i64, i64, i64, i64) {
    (
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

fn regular(file: &File, max_bytes: u64) -> Result<fs::Metadata> {
    let metadata = io(file.metadata())?;
    reject(
        !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > max_bytes,
        &format!(
            "expected a single-link regular file of at most {max_bytes} bytes; inspect path type/size"
        ),
    )?;
    Ok(metadata)
}

fn name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| fail("invalid filename"))
}

fn valid_relative(path: &str, empty: bool) -> bool {
    (empty && path.is_empty())
        || (!path.is_empty()
            && path.len() <= MAX_PATH
            && !path.contains('\0')
            && path
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".." | ".git")))
}

fn owned(path: &str) -> bool {
    valid_relative(path, false)
        && (matches!(path, "TODOS.yaml" | "IMPL.md")
            || ["src/", "tests/", ".kvist/continuous-reviews/"]
                .iter()
                .any(|prefix| path.starts_with(prefix)))
}

fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && lower_hex(oid) && oid.bytes().any(|b| b != b'0')
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn valid_mode(mode: u32) -> bool {
    matches!(mode, 0o100644 | 0o100755)
}

fn component_name(context: &TaskContext) -> Result<String> {
    let relative = context
        .component_dir
        .strip_prefix(&context.project_dir)
        .map_err(|_| fail("component is outside the project root"))?;
    let name = relative
        .to_str()
        .ok_or_else(|| fail("component path is not UTF-8"))?;
    reject(
        !valid_relative(name, true),
        "component path must be normal and relative",
    )?;
    Ok(name.to_owned())
}

impl Snapshot {
    fn validate(&self, context: &TaskContext) -> Result<()> {
        reject(
            self.component != component_name(context)? || self.entries.len() > MAX_SCOPE_ENTRIES,
            "snapshot component or entry count is invalid",
        )?;
        let mut bytes = 0u64;
        for (path, state) in &self.entries {
            reject(
                !owned(path)
                    || path.split('/').count() > MAX_SCOPE_DEPTH
                    || !valid_mode(state.mode)
                    || state.size > MAX_SCOPE_FILE_BYTES
                    || state.digest.len() != 64
                    || !lower_hex(&state.digest),
                "invalid snapshot path, mode, size or digest",
            )?;
            reject(
                path.match_indices('/')
                    .any(|(i, _)| self.entries.contains_key(&path[..i])),
                "snapshot has conflicting file and directory paths",
            )?;
            bytes = bytes
                .saturating_add(path.len() as u64)
                .saturating_add(state.size);
        }
        reject(
            bytes > MAX_SCOPE_TOTAL_BYTES,
            "snapshot exceeds the 8-MiB inventory bound",
        )
    }

    fn repo_path(&self, local: &str) -> String {
        if self.component.is_empty() {
            local.to_owned()
        } else {
            format!("{}/{local}", self.component)
        }
    }

    fn local_path<'a>(&self, repo: &'a str) -> Result<&'a str> {
        let path = if self.component.is_empty() {
            repo
        } else {
            repo.strip_prefix(&self.component)
                .and_then(|p| p.strip_prefix('/'))
                .ok_or_else(|| fail("Git returned a path outside the component"))?
        };
        reject(
            !owned(path),
            "Git returned a path outside exact checkpoint ownership",
        )?;
        Ok(path)
    }
}

// Open every untrusted path component relative to an already-open directory.
// The proc-fd enumeration below refers only to these owned directory handles.
fn open_dir(path: &Path) -> Result<File> {
    reject(!path.is_absolute(), "expected an absolute filesystem path")?;
    let mut directory = io(File::open("/"))?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = File::from(
                    openat(
                        &directory,
                        name,
                        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                        nix::sys::stat::Mode::empty(),
                    )
                    .map_err(|e| fail(format!("directory is missing or link-like: {e}")))?,
                );
            }
            _ => return Err(fail("filesystem path contains non-normal components")),
        }
    }
    Ok(directory)
}

fn open_child(parent: &File, name: &str) -> Result<Option<File>> {
    match openat(
        parent,
        name,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(fd) => Ok(Some(File::from(fd))),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(error) => Err(fail(format!(
            "cannot read checkpoint path without following links: {error}"
        ))),
    }
}

fn state(file: &mut File) -> Result<(FileState, Vec<u8>)> {
    let before = regular(file, MAX_SCOPE_FILE_BYTES)?;
    let mut bytes = Vec::new();
    io(file.take(MAX_SCOPE_FILE_BYTES + 1).read_to_end(&mut bytes))?;
    let after = io(file.metadata())?;
    reject(
        before.len() != bytes.len() as u64 || stamp(&before) != stamp(&after),
        "file changed while being read; stop concurrent edits and retry",
    )?;
    Ok((
        FileState {
            digest: hex::encode(Sha256::digest(&bytes)),
            mode: if before.mode() & 0o100 != 0 {
                0o100755
            } else {
                0o100644
            },
            size: bytes.len() as u64,
        },
        bytes,
    ))
}

pub(super) fn capture(context: &TaskContext) -> Result<Snapshot> {
    let mut snapshot = Snapshot {
        component: component_name(context)?,
        entries: BTreeMap::new(),
    };
    let directory = open_dir(&context.component_dir)?;
    let mut budget = ScopeHashBudget::default();
    for root in ROOTS {
        if let Some(file) = open_relative(&directory, root)? {
            walk(file, root, &mut snapshot, &mut budget)?;
        }
    }
    snapshot.validate(context)?;
    Ok(snapshot)
}

fn walk(
    mut file: File,
    path: &str,
    snapshot: &mut Snapshot,
    budget: &mut ScopeHashBudget,
) -> Result<()> {
    budget.charge_entry(Path::new(path), path.len())?;
    reject(
        path.split('/').count() > MAX_SCOPE_DEPTH || !valid_relative(path, false),
        "checkpoint inventory exceeds path or depth bounds",
    )?;
    let metadata = io(file.metadata())?;
    if metadata.is_dir() {
        reject(
            matches!(path, "TODOS.yaml" | "IMPL.md"),
            "workflow artifact must be a regular file",
        )?;
        let mut children = Vec::new();
        for entry in io(fs::read_dir(format!("/proc/self/fd/{}", file.as_raw_fd())))? {
            let name = io(entry)?
                .file_name()
                .into_string()
                .map_err(|_| fail("checkpoint paths must be UTF-8"))?;
            reject(
                children.len() + budget.entries >= MAX_SCOPE_ENTRIES,
                "inventory exceeds entry bound",
            )?;
            children.push(name);
        }
        children.sort();
        for child in children {
            if let Some(child_file) = open_child(&file, &child)? {
                walk(child_file, &format!("{path}/{child}"), snapshot, budget)?;
            }
        }
    } else {
        reject(!owned(path), "checkpoint scope root must be a directory")?;
        let (entry, _) = state(&mut file)?;
        budget.charge(Path::new(path), entry.size as usize)?;
        snapshot.entries.insert(path.to_owned(), entry);
    }
    Ok(())
}

fn open_relative(root: &File, path: &str) -> Result<Option<File>> {
    let mut file = io(root.try_clone())?;
    for part in path.split('/') {
        let Some(child) = open_child(&file, part)? else {
            return Ok(None);
        };
        file = child;
    }
    Ok(Some(file))
}

fn read_local(context: &TaskContext, path: &str) -> Result<(FileState, Vec<u8>)> {
    reject(!owned(path), "invalid owned checkpoint path")?;
    let mut file = open_relative(&open_dir(&context.component_dir)?, path)?
        .ok_or_else(|| fail("checkpoint path disappeared"))?;
    state(&mut file)
}

fn text(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes)
        .map(|s| s.trim_end_matches('\n').to_owned())
        .map_err(|_| fail("Git output is not UTF-8"))
}

fn git_text(context: &TaskContext, index: Option<&Path>, args: &[&str]) -> Result<String> {
    text(git(context, index, args, &[], MAX_GIT_OUTPUT)?)
}

fn head_branch(context: &TaskContext) -> Result<(String, String)> {
    let project = open_dir(&context.project_dir)?;
    let marker = open_child(&project, ".git")?.ok_or_else(|| fail("Git metadata is missing"))?;
    let metadata = io(marker.metadata())?;
    reject(
        !metadata.is_dir() && (!metadata.is_file() || metadata.nlink() != 1),
        "Git metadata must be a non-link directory or worktree pointer file",
    )?;
    let top = git_text(context, None, &["rev-parse", "--show-toplevel"])?;
    reject(
        Path::new(&top) != context.project_dir,
        "project root must be the Git worktree root",
    )?;
    let head =
        git_text(context, None, &["rev-parse", "--verify", "HEAD^{commit}"]).map_err(|error| {
            fail(format!(
                "cannot resolve HEAD; create an initial commit or repair Git if missing: {error}"
            ))
        })?;
    reject(
        !valid_oid(&head),
        "invalid HEAD; create an initial commit first",
    )?;
    let branch = git_text(context, None, &["symbolic-ref", "-q", "HEAD"]).map_err(|error| {
        fail(format!(
            "attached branch required; switch to a branch if detached: {error}"
        ))
    })?;
    validate_branch(&branch)?;
    Ok((head, branch))
}

fn validate_branch(branch: &str) -> Result<()> {
    reject(
        !branch.starts_with("refs/heads/")
            || branch.len() > MAX_PATH
            || branch.bytes().any(|b| b <= b' ' || b == 127)
            || !valid_relative(branch, false)
            || branch.contains(['~', '^', ':', '?', '*', '[', '\\'])
            || branch.contains("..")
            || branch.contains("@{")
            || branch.ends_with('.')
            || branch
                .split('/')
                .any(|p| p.starts_with('.') || p.ends_with(".lock")),
        "prepared checkpoint branch is invalid",
    )
}

fn entries(
    context: &TaskContext,
    snapshot: &Snapshot,
    revision: Option<&str>,
    index: Option<&Path>,
) -> Result<Entries> {
    let mut args = if let Some(revision) = revision {
        vec!["ls-tree", "-r", "-z", revision, "--"]
    } else {
        vec!["ls-files", "--stage", "-z", "--"]
    };
    let paths: Vec<_> = ROOTS
        .iter()
        .map(|root| format!(":(top,literal){}", snapshot.repo_path(root)))
        .collect();
    args.extend(paths.iter().map(String::as_str));
    let output = git(context, index, &args, &[], MAX_GIT_OUTPUT)?;
    let mut result = BTreeMap::new();
    let mut names = 0u64;
    for record in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let record =
            std::str::from_utf8(record).map_err(|_| fail("Git scope contains a non-UTF-8 path"))?;
        let (header, path) = record
            .split_once('\t')
            .ok_or_else(|| fail("malformed Git entry"))?;
        let fields: Vec<_> = header.split(' ').collect();
        let (mode, oid) = match (fields.as_slice(), revision.is_some()) {
            ([mode, "blob", oid], true) | ([mode, oid, "0"], false) => (*mode, *oid),
            _ => {
                return Err(fail(
                    "scope contains an unmerged, link-like or non-file Git entry",
                ));
            }
        };
        let mode = u32::from_str_radix(mode, 8).map_err(|_| fail("invalid Git file mode"))?;
        let local = snapshot.local_path(path)?;
        names = names.saturating_add(local.len() as u64);
        let entry = GitEntry {
            mode,
            oid: oid.to_owned(),
        };
        let duplicate = result.insert(local.to_owned(), entry).is_some();
        reject(
            !valid_mode(mode)
                || !valid_oid(oid)
                || result.len() > MAX_SCOPE_ENTRIES
                || names > MAX_SCOPE_TOTAL_BYTES
                || duplicate,
            "Git scope has links, invalid entries or exceeds bounds",
        )?;
    }
    Ok(result)
}

fn verify_tree(context: &TaskContext, snapshot: &Snapshot, tree: &Entries) -> Result<()> {
    reject(
        snapshot.entries.len() != tree.len(),
        "dirty inventory: missing tracked or new untracked files; commit/stash explicitly",
    )?;
    let mut total = 0u64;
    for (path, expected) in &snapshot.entries {
        let entry = tree.get(path).ok_or_else(|| {
            fail("dirty overlapping checkpoint path; commit or stash it explicitly")
        })?;
        let blob = git(
            context,
            None,
            &["cat-file", "blob", &entry.oid],
            &[],
            MAX_SCOPE_FILE_BYTES,
        )?;
        total = total.saturating_add(blob.len() as u64);
        reject(
            total > MAX_SCOPE_TOTAL_BYTES
                || expected.mode != entry.mode
                || expected.size != blob.len() as u64
                || expected.digest != hex::encode(Sha256::digest(&blob)),
            "dirty overlapping content or mode; commit/stash explicitly",
        )?;
    }
    Ok(())
}

pub(super) fn ensure_clean(context: &TaskContext, snapshot: &Snapshot) -> Result<String> {
    snapshot.validate(context)?;
    let (head, branch) = head_branch(context)?;
    index_path(context)?;
    for identity in ["GIT_COMMITTER_IDENT", "GIT_AUTHOR_IDENT"] {
        git_text(
            context,
            None,
            &["-c", "user.useConfigOnly=true", "var", identity],
        )?;
    }
    signing_policy(context)?;
    let tree = entries(context, snapshot, Some(&head), None)?;
    verify_tree(context, snapshot, &tree)?;
    reject(
        capture(context)? != *snapshot || entries(context, snapshot, None, None)? != tree,
        "dirty overlapping worktree or staging; commit/stash explicitly",
    )?;
    reject(
        head_branch(context)? != (head.clone(), branch),
        "HEAD or branch changed during preflight",
    )?;
    Ok(head)
}

fn signing_policy(context: &TaskContext) -> Result<bool> {
    let config = |key, default| {
        git_text(
            context,
            None,
            &["config", "--default", default, "--get", key],
        )
    };
    let enabled = git_text(
        context,
        None,
        &["config", "--bool", "--default=false", "commit.gpgsign"],
    )?;
    match enabled.as_str() {
        "false" => Ok(false),
        "true" => {
            let format = config("gpg.format", "openpgp")?;
            reject(
                !matches!(format.as_str(), "openpgp" | "x509" | "ssh"),
                "unsupported Git signing format",
            )?;
            if format == "ssh"
                && config("user.signingkey", "")?.is_empty()
                && config("gpg.ssh.defaultKeyCommand", "")?.is_empty()
            {
                return Err(fail(
                    "SSH commit signing requires user.signingkey or gpg.ssh.defaultKeyCommand; configure signing before running tasks",
                ));
            }
            Ok(true)
        }
        _ => Err(fail("commit.gpgsign is not a valid boolean")),
    }
}

fn isolated_index(parent: &Path) -> Result<tempfile::TempPath> {
    let path = io(NamedTempFile::new_in(parent))?.into_temp_path();
    io(fs::remove_file(&path))?;
    Ok(path)
}

fn write_entries(
    context: &TaskContext,
    index: &Path,
    snapshot: &Snapshot,
    changes: &BTreeMap<String, Change>,
) -> Result<()> {
    let mut input = Vec::new();
    for (path, change) in changes {
        let header = match &change.after {
            Some(entry) => format!("{:o} {}\t", entry.mode, entry.oid),
            None => format!(
                "0 {}\t",
                "0".repeat(change.before.as_ref().map_or(40, |e| e.oid.len()))
            ),
        };
        input.extend(header.as_bytes());
        input.extend(snapshot.repo_path(path).as_bytes());
        input.push(0);
    }
    git(
        context,
        Some(index),
        &["update-index", "-z", "--index-info"],
        &input,
        MAX_GIT_OUTPUT,
    )?;
    Ok(())
}

fn index_path(context: &TaskContext) -> Result<PathBuf> {
    let path = PathBuf::from(git_text(
        context,
        None,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?);
    let parent = open_dir(parent_dir(&path)?)?;
    let file = open_child(&parent, "index")?
        .ok_or_else(|| fail("Git index is missing; restore it explicitly before retrying"))?;
    regular(&file, MAX_INDEX)?;
    Ok(path)
}

pub(super) fn prepare(
    context: &TaskContext,
    expected_head: &str,
    before: &Snapshot,
    after: &Snapshot,
    message: &str,
) -> Result<PreparedCommit> {
    before.validate(context)?;
    after.validate(context)?;
    reject(
        !valid_oid(expected_head)
            || message.trim().is_empty()
            || message.len() > 16384
            || message.contains('\0'),
        "invalid expected HEAD or checkpoint message",
    )?;
    let (head, branch) = head_branch(context)?;
    let real_index = index_path(context)?;
    let expected_baseline = entries(context, before, Some(expected_head), None)?;
    verify_tree(context, before, &expected_baseline)?;
    let baseline = if head == expected_head {
        expected_baseline.clone()
    } else {
        entries(context, before, Some(&head), None)?
    };
    reject(
        entries(context, before, None, None)? != baseline || capture(context)? != *after,
        "worktree or staged overlap changed before preparation",
    )?;
    let mut changes = BTreeMap::new();
    for path in before
        .entries
        .keys()
        .chain(baseline.keys())
        .chain(after.entries.keys())
    {
        if changes.contains_key(path) {
            continue;
        }
        // Fast path: unchanged since the run started and HEAD is consistent.
        if before.entries.get(path) == after.entries.get(path)
            && baseline.get(path) == expected_baseline.get(path)
        {
            continue;
        }
        let post = match after.entries.get(path) {
            Some(expected) => {
                let (actual, bytes) = read_local(context, path)?;
                reject(actual != *expected, "content changed during preparation")?;
                let oid = text(git(
                    context,
                    None,
                    &["hash-object", "-w", "--no-filters", "--stdin"],
                    &bytes,
                    128,
                )?)?;
                reject(!valid_oid(&oid), "Git produced an invalid blob ID")?;
                Some(GitEntry {
                    mode: actual.mode,
                    oid,
                })
            }
            None => None,
        };
        // The worktree can match HEAD even though it differed at the start of
        // the run (a reverted edit, or a rewrite that lands on identical
        // content). Such a path carries no delta, so it must not enter the
        // prepared commit; validation would otherwise reject it as an invalid
        // changed path.
        if baseline.get(path) == post.as_ref() {
            continue;
        }
        changes.insert(
            path.clone(),
            Change {
                before: baseline.get(path).cloned(),
                after: post,
            },
        );
    }
    reject(changes.is_empty(), "checkpoint has no inventory changes")?;
    reject(
        changes.len() > MAX_SCOPE_ENTRIES
            || changes.keys().map(|p| p.len() as u64).sum::<u64>() > MAX_SCOPE_TOTAL_BYTES,
        "changed paths exceed checkpoint entry or byte bounds; split the work explicitly",
    )?;
    let index = isolated_index(parent_dir(&real_index)?)?;
    git(
        context,
        Some(&index),
        &["read-tree", &head],
        &[],
        MAX_GIT_OUTPUT,
    )?;
    write_entries(context, &index, after, &changes)?;
    let tree = git_text(context, Some(&index), &["write-tree"])?;
    reject(!valid_oid(&tree), "Git produced an invalid tree ID")?;
    let mut args = vec!["commit-tree", &tree, "-p", &head];
    if signing_policy(context)? {
        args.push("-S");
    }
    let message = format!("{}\n\n{COAUTHOR}\n", message.trim_end());
    let commit_id = text(git(
        context,
        Some(&index),
        &args,
        message.as_bytes(),
        MAX_GIT_OUTPUT,
    )?)?;
    reject(!valid_oid(&commit_id), "Git produced an invalid commit ID")?;
    reject(
        head_branch(context)? != (head.clone(), branch.clone())
            || capture(context)? != *after
            || entries(context, before, None, None)? != baseline,
        "HEAD, branch, worktree or staged overlap changed during preparation",
    )?;
    Ok(PreparedCommit {
        commit_id,
        expected_head: head,
        branch,
        after: after.clone(),
        changes,
    })
}

impl PreparedCommit {
    fn validate(&self, context: &TaskContext) -> Result<()> {
        self.after.validate(context)?;
        validate_branch(&self.branch)?;
        reject(
            !valid_oid(&self.commit_id)
                || !valid_oid(&self.expected_head)
                || self.commit_id.len() != self.expected_head.len()
                || self.commit_id == self.expected_head
                || self.changes.is_empty()
                || self.changes.len() > MAX_SCOPE_ENTRIES,
            "prepared checkpoint has invalid commit IDs or change count",
        )?;
        let mut material = 0u64;
        for (path, change) in &self.changes {
            material = material.saturating_add(path.len() as u64);
            if !owned(path) {
                return Err(fail(format!(
                    "changed path `{path}` is outside checkpoint ownership (src/, tests/, TODOS.yaml, IMPL.md, .kvist/continuous-reviews/); the run wrote a file the component does not own"
                )));
            }
            if path.split('/').count() > MAX_SCOPE_DEPTH {
                return Err(fail(format!(
                    "changed path `{path}` is deeper than {MAX_SCOPE_DEPTH} path components; flatten the layout or split the work"
                )));
            }
            // A no-op change (byte-identical before and after) is harmless: the
            // commit tree is built from the after snapshot, so publishing it
            // writes content that already equals the parent commit. This occurs
            // when the ledger baseline predates commits that already absorbed
            // the worktree change; tolerate it so a retained checkpoint can
            // still be finished rather than wedging the run.
            if change.before == change.after {
                continue;
            }
            if change.after.as_ref().map(|e| e.mode) != self.after.entries.get(path).map(|s| s.mode)
            {
                return Err(fail(format!(
                    "changed path `{path}` has a file mode that disagrees with the captured worktree snapshot"
                )));
            }
            if material > MAX_SCOPE_TOTAL_BYTES {
                return Err(fail(format!(
                    "changed paths exceed the {MAX_SCOPE_TOTAL_BYTES}-byte inventory bound; split the work explicitly"
                )));
            }
            for entry in [&change.before, &change.after].into_iter().flatten() {
                if !valid_mode(entry.mode)
                    || !valid_oid(&entry.oid)
                    || entry.oid.len() != self.commit_id.len()
                {
                    return Err(fail(format!(
                        "changed path `{path}` has an invalid object ID or mode"
                    )));
                }
            }
        }
        Ok(())
    }
}

struct IndexLock {
    path: PathBuf,
    file: File,
    installed: bool,
    marker: Option<(PathBuf, File)>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockOwner {
    commit: String,
    pid: u32,
    device: u64,
    inode: u64,
}

fn same_file(file: &File, path: &Path) -> bool {
    matches!((file.metadata(), fs::symlink_metadata(path)),
        (Ok(a), Ok(b)) if b.is_file() && a.dev() == b.dev() && a.ino() == b.ino())
}

impl IndexLock {
    fn acquire(index: &Path, commit: &str) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let parent = parent_dir(index)?;
        let owner_path = parent.join(format!("kvist-checkpoint-{commit}.owner"));
        let path = PathBuf::from(format!("{}.lock", index.display()));
        Self::recover(&path, &owner_path, commit)?;
        let file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW).open(&path)
            .map_err(|e| fail(format!("cannot acquire Git index lock: {e}; stop concurrent Git operations; inspect a stale index.lock explicitly")))?;
        let mut lock = Self {
            path,
            file,
            installed: false,
            marker: None,
        };
        let owner_file = io(OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&owner_path))?;
        lock.marker = Some((owner_path, owner_file));
        let metadata = io(lock.file.metadata())?;
        let owner = LockOwner {
            commit: commit.to_owned(),
            pid: std::process::id(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        if let Some((_, file)) = &mut lock.marker {
            io(file.write_all(
                &serde_json::to_vec(&owner).map_err(|_| fail("cannot encode index lock owner"))?,
            ))?;
            io(file.sync_all())?;
        }
        crate::file_io::sync_directory(parent)?;
        Ok(lock)
    }

    fn owned(&self) -> bool {
        same_file(&self.file, &self.path)
    }

    fn recover(path: &Path, marker: &Path, commit: &str) -> Result<()> {
        let parent = open_dir(parent_dir(marker)?)?;
        let Some(mut file) = open_child(&parent, name(marker)?)? else {
            return Ok(());
        };
        regular(&file, 4096)?;
        let mut bytes = Vec::new();
        io((&mut file).take(4097).read_to_end(&mut bytes))?;
        let owner: LockOwner = serde_json::from_slice(&bytes)
            .map_err(|_| fail("invalid checkpoint index owner record"))?;
        reject(
            owner.commit != commit || owner.pid == 0 || owner.pid > i32::MAX as u32,
            "index owner does not match pending commit",
        )?;
        reject(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(owner.pid as i32), None)
                != Err(nix::errno::Errno::ESRCH),
            "index lock owner is live or cannot be inspected",
        )?;
        match fs::symlink_metadata(path) {
            Ok(metadata)
                if metadata.is_file()
                    && metadata.dev() == owner.device
                    && metadata.ino() == owner.inode =>
            {
                io(fs::remove_file(path))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                return Err(fail(
                    "index lock no longer belongs to the interrupted checkpoint; inspect it explicitly",
                ));
            }
        }
        reject(
            !same_file(&file, marker),
            "index lock owner changed during recovery",
        )?;
        io(fs::remove_file(marker))?;
        crate::file_io::sync_directory(parent_dir(marker)?)
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.installed && self.owned() {
            let _ = fs::remove_file(&self.path);
        }
        if let Some((path, file)) = &self.marker
            && same_file(file, path)
        {
            let _ = fs::remove_file(path);
        }
    }
}

/// Drop change records whose before and after entries are identical. Such a
/// record carries no delta; it can appear in a checkpoint prepared by an
/// older build (before preparation itself filtered them out), and it would
/// otherwise fail validation and block recovery of an otherwise valid commit.
fn without_noops(prepared: &PreparedCommit) -> PreparedCommit {
    PreparedCommit {
        changes: prepared
            .changes
            .iter()
            .filter(|(_, change)| change.before != change.after)
            .map(|(path, change)| (path.clone(), change.clone()))
            .collect(),
        ..prepared.clone()
    }
}

pub(super) fn finish(context: &TaskContext, prepared: &PreparedCommit) -> Result<String> {
    let prepared = without_noops(prepared);
    prepared.validate(context)?;
    let (head, branch) = head_branch(context)?;
    reject(
        branch != prepared.branch || (head != prepared.expected_head && head != prepared.commit_id),
        "HEAD or branch changed; inspect retained checkpoint; do not replay tasks",
    )?;
    let parents = git_text(
        context,
        None,
        &["rev-list", "--parents", "-n", "1", &prepared.commit_id],
    )?;
    reject(
        parents != format!("{} {}", prepared.commit_id, prepared.expected_head),
        "prepared commit must have exactly the expected HEAD as its parent",
    )?;
    let baseline = entries(
        context,
        &prepared.after,
        Some(&prepared.expected_head),
        None,
    )?;
    let post = entries(context, &prepared.after, Some(&prepared.commit_id), None)?;
    verify_tree(context, &prepared.after, &post)?;
    let changed = git(
        context,
        None,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            "-z",
            &prepared.expected_head,
            &prepared.commit_id,
        ],
        &[],
        MAX_GIT_OUTPUT,
    )?;
    let mut actual = BTreeMap::new();
    for path in changed.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path =
            std::str::from_utf8(path).map_err(|_| fail("commit contains a non-UTF-8 path"))?;
        let local = prepared.after.local_path(path)?;
        actual.insert(
            local.to_owned(),
            Change {
                before: baseline.get(local).cloned(),
                after: post.get(local).cloned(),
            },
        );
    }
    // `diff-tree` lists only paths whose content actually differs from the
    // parent. `prepared` already had its no-op changes dropped by
    // `without_noops`, so it matches the commit's real delta exactly.
    reject(
        actual != prepared.changes,
        "prepared changes do not match their exact commit; the retained checkpoint lists changes the commit does not contain",
    )?;
    let index = index_path(context)?;
    let mut lock = IndexLock::acquire(&index, &prepared.commit_id)?;
    let parent = parent_dir(&index)?;
    let held_parent = open_dir(parent)?;
    let mut source = open_child(&held_parent, name(&index)?)?
        .ok_or_else(|| fail("Git index is missing; restore it explicitly before retrying"))?;
    let metadata = regular(&source, MAX_INDEX)?;
    let mut isolated = io(NamedTempFile::new_in(parent))?;
    let copied = io(std::io::copy(
        &mut (&mut source).take(MAX_INDEX + 1),
        &mut isolated,
    ))?;
    reject(
        copied != metadata.len(),
        "Git index changed while it was copied",
    )?;
    let staged = entries(context, &prepared.after, None, Some(isolated.path()))?;
    reject(
        staged != baseline && !(head == prepared.commit_id && staged == post),
        "staged overlap changed; preserve or unstage it explicitly before retrying",
    )?;
    write_entries(context, isolated.path(), &prepared.after, &prepared.changes)?;
    let mut staged_index = io(File::open(isolated.path()))?;
    io(std::io::copy(&mut staged_index, &mut lock.file))?;
    io(lock.file.set_permissions(metadata.permissions()))?;
    io(lock.file.sync_all())?;
    let current_index = io(source.metadata())?;
    reject(
        capture(context)? != prepared.after
            || head_branch(context)? != (head.clone(), branch.clone())
            || !lock.owned()
            || !same_file(&source, &index)
            || stamp(&metadata) != stamp(&current_index),
        "worktree, branch or index lock changed before publication",
    )?;
    if head == prepared.expected_head {
        advance(context, &prepared)?;
    }
    // HEAD may already have advanced on a prior attempt. Replacing only this
    // copied index's changed entries makes that recovery idempotent.
    reject(
        !lock.owned() || head_branch(context)? != (prepared.commit_id.clone(), branch),
        "index lock, HEAD or branch changed after ref advance; inspect/retry pending checkpoint",
    )?;
    io(fs::rename(&lock.path, &index))?;
    lock.installed = true;
    crate::file_io::sync_directory(parent)?;
    Ok(prepared.commit_id.clone())
}

#[cfg(test)]
mod tests;
