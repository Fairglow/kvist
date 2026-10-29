//! Bounded, no-follow snapshots and durable recovery originals.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::fd::OwnedFd,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

use nix::{
    fcntl::{OFlag, open, openat},
    sys::stat::Mode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Operation, failure};
use crate::{Result, file_io};

const MAX_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SNAPSHOT_ENTRIES: usize = 100_000;
pub(super) const MAX_TEXT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub kind: String,
    pub digest: String,
    pub mode: u32,
    #[serde(skip)]
    pub text: Option<String>,
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.digest == other.digest && self.mode == other.mode
    }
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub(super) fn relative(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            _ => {
                return Err(failure(format!(
                    "recovery path `{}` must be project-relative without traversal",
                    path.display()
                )));
            }
        }
    }
    if result.as_os_str().is_empty() || result.as_os_str().len() > 4096 {
        return Err(failure("recovery path is empty or exceeds 4096 bytes"));
    }
    Ok(result)
}

pub(super) fn root(path: &Path) -> Result<PathBuf> {
    // Validate before canonicalizing: canonicalization must not hide a link.
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| failure(format!("resolve recovery root: {error}")))?
            .join(path)
    };
    let mut current = PathBuf::new();
    for part in absolute.components() {
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| failure(format!("inspect root `{}`: {error}", current.display())))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(failure(format!(
                "root/parent `{}` must be a real directory, not a link",
                current.display()
            )));
        }
    }
    absolute
        .canonicalize()
        .map_err(|error| failure(format!("resolve recovery root: {error}")))
}

pub(super) struct Parent {
    _directory: OwnedFd,
    pub path: PathBuf,
}

pub(super) fn parent(root: &Path, relative_path: &Path) -> Result<Parent> {
    let path = root.join(relative(relative_path)?);
    let flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_DIRECTORY;
    let mut directory = open("/", flags, Mode::empty())
        .map_err(|error| failure(format!("open recovery filesystem root: {error}")))?;
    let filename = path
        .file_name()
        .ok_or_else(|| failure("recovery object has no filename"))?;
    for part in path
        .parent()
        .ok_or_else(|| failure("recovery object has no parent"))?
        .components()
    {
        match part {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                directory =
                    openat(&directory, Path::new(name), flags, Mode::empty()).map_err(|error| {
                        failure(format!(
                            "open real recovery parent `{}` without links: {error}",
                            path.display()
                        ))
                    })?;
            }
            _ => {
                return Err(failure(
                    "parent traversal is not supported in recovery paths",
                ));
            }
        }
    }
    use std::os::fd::AsRawFd;
    let path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(filename);
    Ok(Parent {
        _directory: directory,
        path,
    })
}

pub(super) fn snapshot(root: &Path, path: &Path) -> Result<Option<Snapshot>> {
    let relative = relative(path)?;
    let mut current = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(failure(format!(
                        "parent `{}` must be a real directory, not a link",
                        current.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(failure(format!("inspect parent: {error}"))),
            }
        }
    }
    let mut bytes = 0;
    let mut entries = 0;
    snapshot_inner(root, path, &mut bytes, &mut entries, 0)
}

pub(super) fn ensure_parents(root: &Path, path: &Path) -> Result<()> {
    let relative = relative(path)?;
    let mut current = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(failure(format!(
                        "artifact parent `{}` is not a real directory",
                        current.display()
                    )));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&current)
                        .map_err(|error| failure(format!("create artifact parent: {error}")))?;
                    file_io::sync_directory(
                        current
                            .parent()
                            .ok_or_else(|| failure("artifact parent has no parent"))?,
                    )?;
                }
                Err(error) => return Err(failure(format!("inspect artifact parent: {error}"))),
            }
        }
    }
    Ok(())
}
fn snapshot_inner(
    root: &Path,
    path: &Path,
    bytes: &mut u64,
    entries: &mut usize,
    depth: usize,
) -> Result<Option<Snapshot>> {
    *entries += 1;
    if *entries > MAX_SNAPSHOT_ENTRIES || depth > 64 {
        return Err(failure(format!(
            "snapshot of `{}` exceeds 100000 entries or 64 levels; originals remain untouched",
            path.display()
        )));
    }
    let pinned = parent(root, path)?;
    let before = match fs::symlink_metadata(&pinned.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(format!("inspect `{}`: {error}", path.display()))),
    };
    let mut text = None;
    let (kind, digest) = if before.is_file() {
        let mut options = OpenOptions::new();
        use std::os::unix::fs::OpenOptionsExt;
        options
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
        let mut file = options.open(&pinned.path).map_err(|error| {
            failure(format!("open `{}` without links: {error}", path.display()))
        })?;
        let opened = file
            .metadata()
            .map_err(|error| failure(error.to_string()))?;
        if !same_object(&before, &opened) {
            return Err(failure(format!(
                "`{}` changed during snapshot",
                path.display()
            )));
        }
        let mut hash = Sha256::new();
        let mut content = Vec::new();
        let mut buffer = [0_u8; 65536];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| failure(format!("read `{}`: {error}", path.display())))?;
            if count == 0 {
                break;
            }
            *bytes += count as u64;
            if *bytes > MAX_SNAPSHOT_BYTES {
                return Err(failure(
                    "recovery snapshot exceeds 512 MiB; provide an external archival/storage solution before retrying",
                ));
            }
            hash.update(&buffer[..count]);
            if before.len() <= MAX_TEXT_BYTES && content.len() + count <= MAX_TEXT_BYTES as usize {
                content.extend_from_slice(&buffer[..count]);
            }
        }
        if before.len() <= MAX_TEXT_BYTES {
            text = String::from_utf8(content).ok();
        }
        ("file", format!("sha256:{}", hex::encode(hash.finalize())))
    } else if before.file_type().is_symlink() {
        let target = fs::read_link(&pinned.path)
            .map_err(|error| failure(format!("read link `{}`: {error}", path.display())))?;
        ("link", digest(target.as_os_str().as_bytes()))
    } else if before.is_dir() {
        let mut children = Vec::new();
        for entry in fs::read_dir(&pinned.path)
            .map_err(|error| failure(format!("inspect directory `{}`: {error}", path.display())))?
        {
            let entry = entry.map_err(|error| failure(error.to_string()))?;
            children.push(entry.file_name());
            if children.len() > MAX_SNAPSHOT_ENTRIES {
                return Err(failure("recovery directory exceeds 100000 entries"));
            }
        }
        children.sort();
        let mut hash = Sha256::new();
        for name in children {
            let child = snapshot_inner(root, &path.join(&name), bytes, entries, depth + 1)?
                .ok_or_else(|| failure("recovery directory changed during snapshot"))?;
            hash.update((name.as_bytes().len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            hash.update(serde_json::to_vec(&child).map_err(|error| failure(error.to_string()))?);
        }
        (
            "directory",
            format!("sha256:{}", hex::encode(hash.finalize())),
        )
    } else {
        (
            "special",
            digest(
                format!(
                    "{}:{}:{}:{}",
                    before.dev(),
                    before.ino(),
                    before.mode(),
                    before.rdev()
                )
                .as_bytes(),
            ),
        )
    };
    let after = fs::symlink_metadata(&pinned.path).map_err(|error| failure(error.to_string()))?;
    if !same_object(&before, &after) {
        return Err(failure(format!(
            "`{}` changed during snapshot",
            path.display()
        )));
    }
    Ok(Some(Snapshot {
        kind: kind.to_owned(),
        digest,
        mode: before.mode(),
        text,
    }))
}

fn same_object(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mode() == after.mode()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

pub(super) fn create_backup(
    root: &Path,
    storage: &Path,
    operations: &[Operation],
    approval: &str,
) -> Result<PathBuf> {
    let storage_parent = storage
        .parent()
        .ok_or_else(|| failure("backup storage has no parent"))?;
    let canonical_parent = self::root(storage_parent)?;
    let filename = storage
        .file_name()
        .ok_or_else(|| failure("backup storage has no filename"))?;
    let storage = canonical_parent.join(filename);
    let pinned_parent = parent(&canonical_parent, Path::new(filename))?;
    match fs::symlink_metadata(&pinned_parent.path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(failure(
                "backup storage must be a real private directory; use --backup-dir PATH to select unused private storage",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&pinned_parent.path)
                .map_err(|error| failure(format!("create backup storage: {error}")))?;
        }
        Err(error) => return Err(failure(format!("inspect backup storage: {error}"))),
    }
    let pinned_storage = parent(&canonical_parent, &Path::new(filename).join("placeholder"))?;
    let directory = pinned_storage
        .path
        .parent()
        .ok_or_else(|| failure("backup storage has no parent"))?;
    let metadata = fs::metadata(directory).map_err(|error| failure(error.to_string()))?;
    if metadata.mode() & 0o077 != 0 {
        return Err(failure(
            "backup storage must be private (mode 0700); choose unused private storage with --backup-dir",
        ));
    }
    let bundle = tempfile::Builder::new()
        .prefix("recovery-")
        .tempdir_in(directory)
        .map_err(|error| failure(format!("create private backup: {error}")))?
        .keep()
        .canonicalize()
        .map_err(|error| failure(format!("resolve retained backup: {error}")))?;
    fs::set_permissions(&bundle, fs::Permissions::from_mode(0o700)).map_err(|error| {
        failure(format!(
            "protect retained backup `{}`: {error}; originals remain unchanged",
            bundle.display()
        ))
    })?;
    let prepare = || -> Result<()> {
        fs::create_dir(bundle.join("originals")).map_err(|error| failure(error.to_string()))?;
        file_io::write_new_file_atomically(&bundle.join(".gitignore"), "*\n")?;
        manifest(&bundle, approval, operations, &[], "prepared")?;
        for operation in operations {
            let Some(original) = &operation.before else {
                continue;
            };
            if original.kind != "file" || operation.contents.is_none() {
                continue;
            }
            let pinned = parent(root, Path::new(&operation.path))?;
            let destination = bundle.join("originals").join(&operation.path);
            fs::create_dir_all(
                destination
                    .parent()
                    .ok_or_else(|| failure("backup has no parent"))?,
            )
            .map_err(|error| failure(error.to_string()))?;
            use std::os::unix::fs::OpenOptionsExt;
            let source = OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
                .open(&pinned.path)
                .map_err(|error| failure(format!("open original for backup: {error}")))?;
            let mut saved = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&destination)
                .map_err(|error| failure(format!("create backup: {error}")))?;
            let copied = io::copy(&mut source.take(MAX_SNAPSHOT_BYTES + 1), &mut saved)
                .map_err(|error| failure(format!("copy backup: {error}")))?;
            if copied > MAX_SNAPSHOT_BYTES {
                return Err(failure(
                    "original grew beyond the 512 MiB backup bound; originals remain unchanged",
                ));
            }
            saved
                .set_permissions(fs::Permissions::from_mode(original.mode & 0o7777))
                .map_err(|error| failure(format!("retain original permissions: {error}")))?;
            saved
                .sync_all()
                .map_err(|error| failure(format!("sync backup: {error}")))?;
            file_io::sync_directory(
                destination
                    .parent()
                    .ok_or_else(|| failure("backup has no parent"))?,
            )?;
            let retained = snapshot(&bundle.join("originals"), Path::new(&operation.path))?;
            if retained.as_ref().map(|value| &value.digest) != Some(&original.digest) {
                return Err(failure(format!(
                    "backup verification failed for `{}`; no originals changed",
                    operation.path
                )));
            }
        }
        file_io::sync_directory(&bundle)?;
        file_io::sync_directory(&storage)?;
        file_io::sync_directory(root)
    };
    prepare().map_err(|error| {
        failure(format!(
            "{error}; retained backup preparation at `{}`; originals were not changed",
            bundle.display()
        ))
    })?;
    Ok(bundle)
}

pub(super) fn validate_backup_location(project: &Path, storage: &Path) -> Result<()> {
    if storage
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(failure("backup location cannot contain parent traversal"));
    }
    if let Ok(relative) = storage.strip_prefix(project) {
        let valid = relative.components().count() == 1
            && relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name == ".kvist-recovery" || name.starts_with(".kvist-recovery-")
                });
        if !valid {
            return Err(failure(
                "in-project backup storage must be `.kvist-recovery` or a root-level `.kvist-recovery-NAME` directory; choose an external location otherwise",
            ));
        }
    }
    let parent = storage
        .parent()
        .ok_or_else(|| failure("backup location has no parent"))?;
    self::root(parent)?;
    Ok(())
}
pub(super) fn manifest(
    bundle: &Path,
    approval: &str,
    operations: &[Operation],
    applied: &[String],
    state: &str,
) -> Result<()> {
    let contents = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": 1, "approval": approval, "state": state,
        "operations": operations, "applied": applied,
        "notice": "Original objects are under originals/. Execution effects are unknown. Drafts are not acceptance. Per-file application may be partial; never blindly replay tasks.",
    })).map_err(|error| failure(error.to_string()))?;
    file_io::replace_file_atomically(&bundle.join("manifest.json"), &contents)
}

pub(super) fn apply(root: &Path, bundle: &Path, operation: &Operation) -> Result<()> {
    if snapshot(root, Path::new(&operation.path))? != operation.before {
        return Err(failure(format!(
            "`{}` changed after approval/backup; refusing replacement",
            operation.path
        )));
    }
    let pinned = parent(root, Path::new(&operation.path))?;
    let should_move = operation.contents.is_none()
        || operation
            .before
            .as_ref()
            .is_some_and(|snapshot| snapshot.kind != "file");
    if should_move && operation.before.is_some() {
        let destination = bundle.join("originals").join(&operation.path);
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| failure("backup has no parent"))?,
        )
        .map_err(|error| failure(error.to_string()))?;
        if destination.symlink_metadata().is_ok() {
            return Err(failure(format!(
                "quarantine destination `{}` already exists",
                destination.display()
            )));
        }
        fs::rename(&pinned.path, &destination).map_err(|error| {
            if error.raw_os_error() == Some(nix::libc::EXDEV) {
                failure("intact object quarantine requires same-filesystem backup storage; use --backup-dir .kvist-recovery-alternative; original object was not moved")
            } else {
                failure(format!("retain original object: {error}"))
            }
        })?;
        file_io::sync_directory(
            destination
                .parent()
                .ok_or_else(|| failure("backup has no parent"))?,
        )?;
        file_io::sync_directory(
            pinned
                .path
                .parent()
                .ok_or_else(|| failure("original has no parent"))?,
        )?;
    }
    if let Some(contents) = &operation.contents {
        if operation.before.is_none() || should_move {
            file_io::write_new_file_atomically(&pinned.path, contents)?;
            file_io::sync_directory(
                pinned
                    .path
                    .parent()
                    .ok_or_else(|| failure("artifact has no parent"))?,
            )?;
        } else {
            file_io::replace_file_atomically(&pinned.path, contents)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replacement(root: &Path, name: &str) -> Operation {
        super::super::operation(
            name,
            "reset",
            snapshot(root, Path::new(name)).expect("snapshot").as_ref(),
            Some("replacement".to_owned()),
            vec!["test proposal".to_owned()],
            true,
        )
    }

    #[test]
    fn failed_backup_verification_leaves_original_unchanged() {
        let project = tempfile::tempdir().expect("project");
        fs::write(project.path().join("VISION.md"), "original").expect("original");
        let operation = replacement(project.path(), "VISION.md");
        fs::write(project.path().join("VISION.md"), "concurrent edit").expect("edit");
        let error = create_backup(
            project.path(),
            &project.path().join(".kvist-recovery"),
            &[operation],
            "test",
        )
        .expect_err("old snapshot must fail backup verification");
        assert!(error.to_string().contains("verification failed"));
        assert_eq!(
            fs::read_to_string(project.path().join("VISION.md")).expect("original"),
            "concurrent edit"
        );
    }

    #[test]
    fn partial_application_keeps_progress_originals_and_concurrent_edits() {
        let project = tempfile::tempdir().expect("project");
        for name in ["VISION.md", "ARCHITECTURE.md"] {
            fs::write(project.path().join(name), "original").expect("original");
        }
        let operations = vec![
            replacement(project.path(), "VISION.md"),
            replacement(project.path(), "ARCHITECTURE.md"),
        ];
        let bundle = create_backup(
            project.path(),
            &project.path().join(".kvist-recovery"),
            &operations,
            "test",
        )
        .expect("backup");
        apply(project.path(), &bundle, &operations[0]).expect("first replacement");
        manifest(
            &bundle,
            "test",
            &operations,
            &["VISION.md".to_owned()],
            "applying",
        )
        .expect("progress");
        fs::write(project.path().join("ARCHITECTURE.md"), "concurrent edit").expect("edit");
        assert!(apply(project.path(), &bundle, &operations[1]).is_err());
        assert_eq!(
            fs::read_to_string(project.path().join("ARCHITECTURE.md")).expect("concurrent edit"),
            "concurrent edit"
        );
        for name in ["VISION.md", "ARCHITECTURE.md"] {
            assert_eq!(
                fs::read_to_string(bundle.join("originals").join(name)).expect("backup"),
                "original"
            );
        }
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(bundle.join("manifest.json")).expect("manifest"))
                .expect("JSON");
        assert_eq!(record["state"], "applying");
        assert_eq!(record["applied"], serde_json::json!(["VISION.md"]));
    }
}
