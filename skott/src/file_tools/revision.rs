use super::*;
use sav::CancellationToken;

fn observing(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(Error::SandboxExec {
            tool: Some("file_tool".into()),
            reason: "cancelled while observing native mutation revision".into(),
        });
    }
    Ok(())
}

fn revision_bytes(reader: impl Read, cancellation: &CancellationToken) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut reader = reader.take((MAX_FILE_BYTES + 1) as u64);
    let mut chunk = [0_u8; 8192];
    loop {
        observing(cancellation)?;
        let read = reader.read(&mut chunk);
        observing(cancellation)?;
        let count = match read {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error("read bounded mutation revision", None, error)),
        };
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_FILE_BYTES {
            return Err(failure("file exceeds the complete-file byte bound"));
        }
    }
    observing(cancellation)?;
    Ok(bytes)
}

fn observed_revision(
    parent: &OwnedFd,
    name: &str,
    cancellation: &CancellationToken,
) -> Result<MutationRevision> {
    observing(cancellation)?;
    let opened = openat(
        parent,
        name,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    );
    observing(cancellation)?;
    let file = match opened {
        Ok(fd) => File::from(fd),
        Err(nix::errno::Errno::ENOENT) => return Ok(MutationRevision::Missing),
        Err(error) => {
            return Err(failure(format!(
                "cannot open mutation revision (symlink targets are forbidden): {error}"
            )));
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect mutation revision", None, error))?;
    observing(cancellation)?;
    if !metadata.is_file() {
        return Err(failure("target must be a regular file"));
    }
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(failure("file exceeds the complete-file byte bound"));
    }
    let bytes = revision_bytes(file, cancellation)?;
    observing(cancellation)?;
    let digest = Sha256::digest(&bytes).into();
    observing(cancellation)?;
    Ok(MutationRevision::Present(digest))
}

#[derive(PartialEq, Eq)]
pub(crate) enum MutationRevision {
    Missing,
    Present([u8; 32]),
}

/// Pins the approved workspace inode across execution. Rebinding its host
/// pathname cannot redirect post-effect observation into another directory.
pub(crate) struct MutationObserver {
    root: OwnedFd,
    root_path: String,
    parents: Vec<String>,
    name: String,
}

impl MutationObserver {
    pub(crate) fn check_binding(&self, cancellation: &CancellationToken) -> Result<()> {
        observing(cancellation)?;
        let expected = nix::sys::stat::fstat(&self.root)
            .map_err(|error| failure(format!("cannot inspect pinned mutation scope: {error}")))?;
        let current = directory(&self.root_path)?;
        observing(cancellation)?;
        let actual = nix::sys::stat::fstat(&current)
            .map_err(|error| failure(format!("cannot inspect current mutation scope: {error}")))?;
        observing(cancellation)?;
        if expected.st_dev != actual.st_dev || expected.st_ino != actual.st_ino {
            return Err(failure(
                "mutation execution scope was rebound; effects may exist; reconcile before \
                 further work; no automatic replay",
            ));
        }
        Ok(())
    }

    pub(crate) fn revision(&self, cancellation: &CancellationToken) -> Result<MutationRevision> {
        self.check_binding(cancellation)?;
        let mut parent = self
            .root
            .try_clone()
            .map_err(|error| io_error("retain mutation observation scope", None, error))?;
        for component in &self.parents {
            observing(cancellation)?;
            parent = openat(
                &parent,
                component.as_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| {
                failure(format!(
                    "cannot traverse mutation observation parent (links are forbidden): {error}"
                ))
            })?;
            observing(cancellation)?;
        }
        let revision = observed_revision(&parent, &self.name, cancellation)?;
        self.check_binding(cancellation)?;
        Ok(revision)
    }
}

impl FileRequest {
    /// Observes only the authorized target with the helper's bounded,
    /// descriptor-relative no-follow traversal, retaining its scope for rereads.
    pub(crate) fn observe_mutation(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Option<(MutationObserver, MutationRevision)>> {
        observing(cancellation)?;
        if !matches!(
            self.operation,
            Operation::WriteFile(_)
                | Operation::EditFile(_)
                | Operation::AppendFile(_)
                | Operation::ApplyPatch(_)
        ) {
            return Ok(None);
        }
        self.validate()?;
        let mapped = self;
        observing(cancellation)?;
        let path = mapped.operation.path();
        mapped.validate_mutation(path)?;
        let relative = path
            .strip_prefix(&mapped.write_root)
            .ok_or_else(|| failure("mutation observation escaped its root"))?
            .trim_start_matches('/');
        let mut parts: Vec<String> = relative.split('/').map(str::to_owned).collect();
        let name = parts
            .pop()
            .ok_or_else(|| failure("mutation revision requires a filename"))?;
        let observer = MutationObserver {
            root: directory(&mapped.write_root)?,
            root_path: mapped.write_root.clone(),
            parents: parts,
            name,
        };
        let before = observer.revision(cancellation)?;
        Ok(Some((observer, before)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct CancellingReader<'a> {
        cancellation: &'a CancellationToken,
        reads: &'a Cell<usize>,
        remaining: usize,
    }
    impl Read for CancellingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            self.cancellation.cancel();
            let count = buffer.len().min(self.remaining).min(8192);
            buffer[..count].fill(b'x');
            self.remaining -= count;
            Ok(count)
        }
    }

    #[test]
    fn followup_cancelled_revision_reader_never_reads() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let reads = Cell::new(0);
        let result = revision_bytes(
            CancellingReader {
                cancellation: &cancellation,
                reads: &reads,
                remaining: 32_768,
            },
            &cancellation,
        );
        assert!(
            result.is_err(),
            "cancelled revision read must fail before I/O"
        );
        let error = result.unwrap_err();
        assert!(error.describe().contains("cancel"), "{error}");
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn followup_revision_reader_observes_mid_chunk_cancellation() {
        let cancellation = CancellationToken::new();
        let reads = Cell::new(0);
        let result = revision_bytes(
            CancellingReader {
                cancellation: &cancellation,
                reads: &reads,
                remaining: 32_768,
            },
            &cancellation,
        );
        assert!(
            result.is_err(),
            "revision read must stop at the first cancelled chunk"
        );
        let error = result.unwrap_err();
        assert!(error.describe().contains("cancel"), "{error}");
        assert_eq!(reads.get(), 1);
    }
}
