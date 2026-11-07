//! Per-call mount origins. Contents are live; pathname pointers cannot add grants.

use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use nix::fcntl::{OFlag, open, openat};
use nix::sys::stat::{Mode, SFlag, fstat};

fn directory() -> Result<OwnedFd, String> {
    open(
        Path::new("/"),
        OFlag::O_PATH | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open mount origin root: {error}"))
}

fn walk(mut parent: OwnedFd, path: &Path) -> Result<OwnedFd, nix::errno::Errno> {
    let parts = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let mut flags = OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        if index + 1 != parts.len() {
            flags |= OFlag::O_DIRECTORY;
        }
        parent = openat(&parent, *part, flags, Mode::empty())?;
    }
    Ok(parent)
}

fn ordinary(fd: &OwnedFd) -> Result<bool, String> {
    let metadata = fstat(fd).map_err(|error| format!("cannot inspect mount origin: {error}"))?;
    let kind = SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT;
    Ok(kind == SFlag::S_IFDIR || kind == SFlag::S_IFREG)
}

/// Opens an ordinary mount origin without following any pathname pointer.
/// The `O_PATH` descriptor is close-on-exec; its contents are not frozen.
pub fn pin(path: &Path) -> Result<OwnedFd, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(format!(
            "mount source `{}` must be a direct absolute path",
            path.display()
        ));
    }
    let fd = walk(directory()?, path).map_err(|error| format!(
        "cannot authorize mount source `{}` without following source/ancestor links: {error}; use a direct ordinary source path, not a writable pointer to another host origin",
        path.display()
    ))?;
    if !ordinary(&fd)? {
        return Err(format!(
            "mount source `{}` must be an ordinary file or directory, not a link or special node",
            path.display()
        ));
    }
    Ok(fd)
}

pub(super) fn child(root: &OwnedFd, path: &Path) -> Result<Option<OwnedFd>, String> {
    let parent = root
        .try_clone()
        .map_err(|error| format!("cannot retain workspace mount origin: {error}"))?;
    let fd = match walk(parent, path) {
        Ok(fd) => fd,
        Err(nix::errno::Errno::ENOENT | nix::errno::Errno::ENOTDIR | nix::errno::Errno::EACCES) => {
            return Ok(None);
        }
        Err(error) => return Err(format!("cannot inspect workspace vendor origin: {error}")),
    };
    Ok(ordinary(&fd)?.then_some(fd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn pinned_origins_survive_replacement_without_following_new_pointers() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let outside = directory.path().join("outside");
        std::fs::write(&original, "approved").unwrap();
        std::fs::write(&outside, "unapproved").unwrap();
        let fd = pin(&original).unwrap();
        let before = fstat(&fd).unwrap();
        std::fs::rename(&original, directory.path().join("retired")).unwrap();
        symlink(&outside, &original).unwrap();
        assert_eq!(fstat(&fd).unwrap().st_ino, before.st_ino);
        assert!(pin(&original).is_err());
        std::fs::remove_file(&original).unwrap();
        std::fs::write(&original, "new host revision").unwrap();
        assert_ne!(
            fstat(pin(&original).unwrap()).unwrap().st_ino,
            before.st_ino
        );
    }
}
