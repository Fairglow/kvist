use super::*;
use std::fs::{File, Metadata};

struct Parent {
    file: File,
    original: PathBuf,
    destination: PathBuf,
}

impl Parent {
    fn open(path: &Path) -> Result<Self> {
        let path = absolute(path)?;
        safe_path(&path, true)?;
        let original = path
            .parent()
            .ok_or_else(|| fail(&path, "missing parent"))?
            .to_path_buf();
        let name = path
            .file_name()
            .ok_or_else(|| fail(&path, "missing filename"))?;
        #[cfg(target_os = "linux")]
        {
            use nix::{
                fcntl::{OFlag, openat},
                sys::stat::Mode,
            };
            use std::os::fd::AsRawFd;
            let mut file = File::open("/").map_err(|error| fail(&path, error.to_string()))?;
            for part in original.components() {
                if let Component::Normal(name) = part {
                    file = File::from(
                        openat(
                            &file,
                            name,
                            OFlag::O_RDONLY
                                | OFlag::O_DIRECTORY
                                | OFlag::O_NOFOLLOW
                                | OFlag::O_CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(|error| {
                            fail(&path, format!("open safe directory ancestor: {error}"))
                        })?,
                    );
                }
            }
            let destination =
                PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())).join(name);
            let parent = Self {
                file,
                original,
                destination,
            };
            parent.verify()?;
            Ok(parent)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let file = File::open(&original).map_err(|error| fail(&path, error.to_string()))?;
            Ok(Self {
                file,
                original,
                destination: path,
            })
        }
    }

    fn verify(&self) -> Result<()> {
        safe_path(&self.original, false)?;
        let observed = fs::symlink_metadata(&self.original)
            .map_err(|error| fail(&self.original, error.to_string()))?;
        let opened = self
            .file
            .metadata()
            .map_err(|error| fail(&self.original, error.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if observed.dev() != opened.dev() || observed.ino() != opened.ino() {
                return Err(fail(
                    &self.original,
                    "directory identity changed during decision I/O",
                ));
            }
        }
        if !observed.is_dir() || !opened.is_dir() {
            return Err(fail(
                &self.original,
                "expected pinned real parent directory",
            ));
        }
        Ok(())
    }
}

pub(super) fn fail(path: &Path, reason: impl Into<String>) -> KvistError {
    KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: format!("decision: {}", reason.into()),
    }
}

pub(super) fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| fail(path, error.to_string()))?
            .join(path)
    };
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(fail(&path, "parent traversal is not permitted"));
    }
    Ok(path
        .components()
        .filter(|part| *part != Component::CurDir)
        .collect())
}

pub(super) fn safe_path(path: &Path, missing: bool) -> Result<()> {
    let path = absolute(path)?;
    let mut ancestor = PathBuf::new();
    for part in path.components() {
        ancestor.push(part);
        match fs::symlink_metadata(&ancestor) {
            Ok(metadata) => {
                if is_link_like(&metadata)
                    || !(metadata.is_dir() || metadata.is_file())
                    || hardlinked(&metadata)
                {
                    return Err(fail(
                        &ancestor,
                        "every path ancestor must be a non-link directory or single-link regular file",
                    ));
                }
                if ancestor != path && !metadata.is_dir() {
                    return Err(fail(&ancestor, "path ancestor is not a directory"));
                }
            }
            Err(error) if missing && error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(fail(&ancestor, error.to_string())),
        }
    }
    Ok(())
}

fn hardlinked(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.is_file() && metadata.nlink() != 1
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn identical(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
}

pub(super) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    safe_path(path, false)?;
    let before = fs::symlink_metadata(path).map_err(|error| fail(path, error.to_string()))?;
    if !before.is_file() || before.len() > MAX_FILE {
        return Err(fail(
            path,
            "input must be a regular file no larger than 1 MiB",
        ));
    }
    let parent = Parent::open(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits());
    }
    let file = options
        .open(&parent.destination)
        .map_err(|error| fail(path, error.to_string()))?;
    let opened = file
        .metadata()
        .map_err(|error| fail(path, error.to_string()))?;
    if !identical(&before, &opened) || hardlinked(&opened) {
        return Err(fail(path, "input changed while opening"));
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| fail(path, error.to_string()))?;
    safe_path(path, false)?;
    parent.verify()?;
    let after = fs::symlink_metadata(path).map_err(|error| fail(path, error.to_string()))?;
    let finished = file
        .metadata()
        .map_err(|error| fail(path, error.to_string()))?;
    if bytes.len() as u64 > MAX_FILE
        || !identical(&before, &after)
        || !identical(&opened, &finished)
        || hardlinked(&after)
    {
        return Err(fail(path, "input changed while reading or exceeds 1 MiB"));
    }
    Ok(bytes)
}

pub(super) fn read_text(path: &Path) -> Result<String> {
    String::from_utf8(read_bytes(path)?)
        .map_err(|error| fail(path, format!("input is not UTF-8: {error}")))
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| fail(Path::new(DIRECTORY), error.to_string()))
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_bytes(path)?)
        .map_err(|error| fail(path, format!("invalid JSON: {error}")))
}

pub(super) fn bounded(text: &str, path: &Path) -> Result<()> {
    if text.len() as u64 > MAX_FILE {
        return Err(fail(path, "artifact exceeds 1 MiB; split the decision"));
    }
    Ok(())
}

pub(super) fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

pub(super) fn directory(path: &Path) -> Result<()> {
    safe_path(path, true)?;
    if !path.exists() {
        let parent = path
            .parent()
            .ok_or_else(|| fail(path, "directory has no parent"))?;
        directory(parent)?;
        let pinned = Parent::open(path)?;
        match fs::create_dir(&pinned.destination) {
            Ok(()) => pinned
                .file
                .sync_all()
                .map_err(|error| fail(path, error.to_string()))?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(fail(path, error.to_string())),
        }
        pinned.verify()?;
    }
    safe_path(path, false)?;
    if !fs::symlink_metadata(path)
        .map_err(|error| fail(path, error.to_string()))?
        .is_dir()
    {
        return Err(fail(path, "expected a real directory"));
    }
    Ok(())
}

pub(super) fn write_new(path: &Path, text: &str) -> Result<()> {
    bounded(text, path)?;
    safe_path(path, true)?;
    let parent = Parent::open(path)?;
    write_new_file_atomically(&parent.destination, text)?;
    parent.verify()?;
    parent
        .file
        .sync_all()
        .map_err(|error| fail(path, error.to_string()))
}

pub(super) fn replace(path: &Path, text: &str) -> Result<()> {
    bounded(text, path)?;
    safe_path(path, true)?;
    let parent = Parent::open(path)?;
    replace_file_atomically(&parent.destination, text)?;
    parent.verify()?;
    safe_path(path, false)?;
    parent
        .file
        .sync_all()
        .map_err(|error| fail(path, error.to_string()))
}

pub(super) fn secret(context: &TaskContext) -> Result<Vec<u8>> {
    let base = user_state_base()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            fail(
                &context.project_dir,
                "cannot determine absolute user-owned approval state",
            )
        })?;
    let state = base.join("kvist").join(APPROVAL_STATE_DIRECTORY);
    if state.starts_with(&context.project_dir) {
        return Err(fail(
            &state,
            "decision authentication state must be outside the project",
        ));
    }
    safe_path(&state, true)?;
    let path = state.join(APPROVAL_SECRET_FILE);
    safe_path(&path, true)?;
    let secret = load_or_create_approval_secret(&state)?;
    if read_bytes(&path)? != secret {
        return Err(fail(&path, "approval secret changed while reading"));
    }
    Ok(secret)
}

pub(super) fn existing_secret() -> Result<Vec<u8>> {
    let base = user_state_base()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            fail(
                Path::new(DIRECTORY),
                "cannot determine absolute user-owned approval state",
            )
        })?;
    let path = base
        .join("kvist")
        .join(APPROVAL_STATE_DIRECTORY)
        .join(APPROVAL_SECRET_FILE);
    safe_path(&path, false)?;
    let secret = load_existing_recovery_secret()?;
    if read_bytes(&path)? != secret {
        return Err(fail(&path, "approval secret changed while reading"));
    }
    Ok(secret)
}

pub(super) fn replace_controlled(
    path: &Path,
    before: &str,
    after: &str,
    lock: &TaskLock,
) -> Result<()> {
    bounded(after, path)?;
    let parent = Parent::open(path)?;
    let directory = parent
        .destination
        .parent()
        .ok_or_else(|| fail(path, "missing parent"))?;
    let mut staged =
        NamedTempFile::new_in(directory).map_err(|error| fail(path, error.to_string()))?;
    staged
        .write_all(after.as_bytes())
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|error| fail(path, error.to_string()))?;
    parent.verify()?;
    lock.revalidate()?;
    if read_text(path)? != before {
        return Err(fail(
            path,
            "controlled input changed while staging its replacement",
        ));
    }
    lock.revalidate()?;
    staged
        .persist(&parent.destination)
        .map_err(|error| fail(path, error.error.to_string()))?;
    parent
        .file
        .sync_all()
        .map_err(|error| fail(path, error.to_string()))?;
    parent.verify()?;
    if read_text(path)? != after {
        return Err(fail(
            path,
            "controlled output changed immediately after replacement",
        ));
    }
    Ok(())
}

pub(super) fn authenticated<T: Serialize>(value: &T, secret: &[u8]) -> Result<String> {
    let mut value = serde_json::to_value(value)
        .map_err(|error| fail(Path::new(DIRECTORY), error.to_string()))?;
    value["authentication_tag"] = Value::String(String::new());
    Ok(format!(
        "hmac-sha256:{}",
        hmac_sha256(secret, encode(&value)?.as_bytes())
    ))
}

pub(super) fn authenticated_struct_order<T: Serialize>(
    value: &T,
    current_tag: &str,
    secret: &[u8],
) -> Result<String> {
    let encoded = encode(value)?;
    let current_field = format!(
        "\"authentication_tag\":{}",
        serde_json::to_string(current_tag)
            .map_err(|error| fail(Path::new(DIRECTORY), error.to_string()))?
    );
    let cleared = match encoded.rsplit_once(&current_field) {
        Some((before, after)) => format!("{before}\"authentication_tag\":\"\"{after}"),
        None => encoded,
    };
    Ok(format!(
        "hmac-sha256:{}",
        hmac_sha256(secret, cleared.as_bytes())
    ))
}

pub(super) fn now() -> Result<Timestamp> {
    Timestamp::now().map_err(|source| KvistError::TaskClock { source })
}

pub(super) fn exists(path: &Path) -> Result<bool> {
    safe_path(path, true)?;
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(fail(path, error.to_string())),
    }
}

pub(super) fn remove(path: &Path) -> Result<()> {
    safe_path(path, false)?;
    let parent = Parent::open(path)?;
    fs::remove_file(&parent.destination).map_err(|error| fail(path, error.to_string()))?;
    parent.verify()?;
    parent
        .file
        .sync_all()
        .map_err(|error| fail(path, error.to_string()))
}
