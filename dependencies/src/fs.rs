use crate::{Error, Result, unavailable};
use nix::{
    fcntl::{OFlag, open, openat},
    sys::stat::Mode,
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};

pub(crate) const LOCK_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const METADATA_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const O_NOFOLLOW: i32 = nix::libc::O_NOFOLLOW;

pub(crate) fn io_error(operation: &'static str, path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}

pub(crate) fn segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 255
        && !value
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf> {
    let result = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| io_error("locate current directory", path, e))?
            .join(path)
    };
    if result
        .components()
        .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || result.as_os_str().len() > 32 * 1024
    {
        return Err(unavailable(
            path,
            "path must be bounded and contain no traversal",
        ));
    }
    for c in result.components() {
        if let Component::Normal(value) = c
            && !value.to_str().is_some_and(segment)
        {
            return Err(unavailable(
                path,
                "path contains an unsafe or non-UTF-8 component",
            ));
        }
    }
    Ok(result)
}

/// Descriptor-anchored directories keep ancestor replacement from redirecting I/O.
pub(crate) struct Dir {
    file: File,
    pub(crate) display: PathBuf,
}

impl Dir {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let path = absolute(path)?;
        let fd = open(
            "/",
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| io_error("open filesystem root", &path, e.into()))?;
        let mut dir = Self {
            file: fd.into(),
            display: PathBuf::from("/"),
        };
        for c in path.components() {
            if let Component::Normal(value) = c {
                let name = value
                    .to_str()
                    .ok_or_else(|| unavailable(&path, "non-UTF-8 path"))?;
                dir = dir.child(name)?;
            }
        }
        Ok(dir)
    }

    pub(crate) fn child(&self, name: &str) -> Result<Self> {
        if !segment(name) {
            return Err(unavailable(&self.display, "unsafe directory name"));
        }
        let display = self.display.join(name);
        let fd = openat(
            &self.file,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| io_error("open non-link directory", &display, e.into()))?;
        Ok(Self {
            file: fd.into(),
            display,
        })
    }

    pub(crate) fn ensure_child(&self, name: &str) -> Result<Self> {
        let path = self.path().join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => self.sync()?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(io_error("create directory", &self.display.join(name), e)),
        }
        self.child(name)
    }

    pub(crate) fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }

    pub(crate) fn sync(&self) -> Result<()> {
        self.file
            .sync_all()
            .map_err(|e| io_error("synchronize directory", &self.display, e))
    }

    pub(crate) fn still_at_path(&self) -> Result<()> {
        let current = Self::open(&self.display)?;
        let original = self
            .file
            .metadata()
            .map_err(|e| io_error("identify anchored directory", &self.display, e))?;
        let now = current
            .file
            .metadata()
            .map_err(|e| io_error("identify current directory", &self.display, e))?;
        if original.dev() != now.dev() || original.ino() != now.ino() {
            return Err(unavailable(
                &self.display,
                "directory identity changed during the operation",
            ));
        }
        Ok(())
    }

    pub(crate) fn exists(&self, name: &str) -> Result<bool> {
        match std::fs::symlink_metadata(self.path().join(name)) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error(
                "inspect directory entry",
                &self.display.join(name),
                e,
            )),
        }
    }

    pub(crate) fn entries(&self) -> Result<Vec<String>> {
        let mut result = Vec::new();
        for entry in std::fs::read_dir(self.path())
            .map_err(|e| io_error("enumerate directory", &self.display, e))?
        {
            let entry = entry.map_err(|e| io_error("read directory entry", &self.display, e))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| unavailable(&self.display, "non-UTF-8 tree entry"))?;
            if !segment(&name) || result.len() >= 100_000 {
                return Err(unavailable(
                    &self.display,
                    "unsafe or over-limit tree entries",
                ));
            }
            result.push(name);
        }
        result.sort();
        Ok(result)
    }

    pub(crate) fn regular(&self, name: &str, limit: u64) -> Result<File> {
        if !segment(name) {
            return Err(unavailable(&self.display, "unsafe file name"));
        }
        let display = self.display.join(name);
        // O_NONBLOCK prevents a malicious FIFO from blocking before its type is checked.
        let fd = openat(
            &self.file,
            name,
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| io_error("open non-link regular file", &display, e.into()))?;
        let file: File = fd.into();
        let metadata = file
            .metadata()
            .map_err(|e| io_error("inspect regular file", &display, e))?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > limit {
            return Err(unavailable(
                &display,
                "expected bounded regular file with one link",
            ));
        }
        Ok(file)
    }

    pub(crate) fn read(&self, name: &str, limit: u64) -> Result<(Vec<u8>, Identity)> {
        let mut file = self.regular(name, limit)?;
        let display = self.display.join(name);
        let before = Identity::of(&file, &display)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_error("read bounded regular file", &display, e))?;
        if bytes.len() as u64 > limit || before != Identity::of(&file, &display)? {
            return Err(unavailable(
                &display,
                "file changed or exceeded bounds while reading",
            ));
        }
        Ok((bytes, before))
    }

    pub(crate) fn atomic(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if !segment(name) {
            return Err(unavailable(&self.display, "unsafe publication name"));
        }
        if self.exists(name)? {
            self.regular(name, METADATA_LIMIT)?;
        }
        let mut temp = tempfile::Builder::new()
            .prefix(".vendor-write-")
            .tempfile_in(self.path())
            .map_err(|e| io_error("stage atomic file", &self.display, e))?;
        temp.write_all(bytes)
            .map_err(|e| io_error("write staged file", &self.display, e))?;
        temp.as_file()
            .sync_all()
            .map_err(|e| io_error("synchronize staged file", &self.display, e))?;
        temp.persist(self.path().join(name))
            .map_err(|e| io_error("publish atomic file", &self.display.join(name), e.error))?;
        self.sync()
    }

    pub(crate) fn exclusive(&self, name: &str) -> Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(self.path().join(name))
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::OperationLocked {
                        path: self.display.join(name),
                    }
                } else {
                    io_error("create operation lock", &self.display.join(name), e)
                }
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    device: u64,
    inode: u64,
    size: u64,
    modified: i64,
    modified_ns: i64,
    changed: i64,
    changed_ns: i64,
}
impl Identity {
    pub(crate) fn of(file: &File, path: &Path) -> Result<Self> {
        let m = file
            .metadata()
            .map_err(|e| io_error("identify regular file", path, e))?;
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
            size: m.len(),
            modified: m.mtime(),
            modified_ns: m.mtime_nsec(),
            changed: m.ctime(),
            changed_ns: m.ctime_nsec(),
        })
    }
}
