use super::*;
use std::io::{BufRead, Seek, SeekFrom};

struct Preimage {
    file: File,
    digest: String,
    metadata: std::fs::Metadata,
}

fn preimage(parent: &OwnedFd, name: &str, require_utf8: bool) -> Result<Option<Preimage>> {
    match openat(
        parent,
        name,
        OFlag::O_RDONLY | OFlag::O_NONBLOCK | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let mut file = File::from(fd);
            let metadata = file
                .metadata()
                .map_err(|e| io_error("inspect mutation preimage", None, e))?;
            if !metadata.is_file() {
                return Err(failure("target must be a regular file"));
            }
            let (digest, _) = if require_utf8 {
                stream::inspect(&mut file, true)?
            } else {
                stream::digest(&mut file)?
            };
            file.seek(SeekFrom::Start(0))
                .map_err(|e| io_error("rewind mutation preimage", None, e))?;
            Ok(Some(Preimage {
                file,
                digest,
                metadata,
            }))
        }
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(e) => Err(failure(format!(
            "cannot open mutation preimage (symlink targets are forbidden): {e}"
        ))),
    }
}

struct Replacement<'a> {
    parent: &'a OwnedFd,
    name: String,
    file: File,
}

impl<'a> Replacement<'a> {
    fn create(parent: &'a OwnedFd) -> Result<Self> {
        for _ in 0..64 {
            let serial = NEXT_REPLACEMENT.fetch_add(1, Ordering::Relaxed);
            let name = format!(".agent-file-{}-{serial}", std::process::id());
            match openat(
                parent,
                name.as_str(),
                OFlag::O_WRONLY
                    | OFlag::O_CREAT
                    | OFlag::O_EXCL
                    | OFlag::O_NOFOLLOW
                    | OFlag::O_CLOEXEC,
                Mode::from_bits_truncate(0o600),
            ) {
                Ok(fd) => {
                    return Ok(Self {
                        parent,
                        name,
                        file: File::from(fd),
                    });
                }
                Err(nix::errno::Errno::EEXIST) => continue,
                Err(e) => return Err(failure(format!("cannot create atomic replacement: {e}"))),
            }
        }
        Err(failure(
            "cannot allocate an exclusive atomic replacement after bounded retries",
        ))
    }
}

impl Drop for Replacement<'_> {
    fn drop(&mut self) {
        let _ = unlinkat(self.parent, self.name.as_str(), UnlinkatFlags::NoRemoveDir);
    }
}

fn same_preimage(first: &Option<Preimage>, fresh: &Option<Preimage>) -> bool {
    match (first, fresh) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.digest == b.digest
                && a.metadata.dev() == b.metadata.dev()
                && a.metadata.ino() == b.metadata.ino()
                && a.metadata.mode() == b.metadata.mode()
                && a.metadata.mtime() == b.metadata.mtime()
                && a.metadata.mtime_nsec() == b.metadata.mtime_nsec()
                && a.metadata.ctime() == b.metadata.ctime()
                && a.metadata.ctime_nsec() == b.metadata.ctime_nsec()
        }
        _ => false,
    }
}

pub(super) enum Mutation<'a> {
    Write(&'a str),
    Edit(&'a str, &'a str),
    Append(&'a str),
    Patch(&'a str),
}

fn unique_occurrence(source: &mut stream::Source, pattern: &[u8]) -> Result<u64> {
    // KMP preserves overlapping matches and requires only request-sized state.
    let mut prefixes = vec![0; pattern.len()];
    for index in 1..pattern.len() {
        let mut matched = prefixes[index - 1];
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = prefixes[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
        }
        prefixes[index] = matched;
    }
    let mut matched = 0;
    let mut position = 0_u64;
    let mut found = None;
    loop {
        let bytes = source
            .reader
            .fill_buf()
            .map_err(|e| io_error("scan exact edit preimage", None, e))?;
        if bytes.is_empty() {
            break;
        }
        let count = bytes.len();
        for byte in bytes {
            while matched > 0 && *byte != pattern[matched] {
                matched = prefixes[matched - 1];
            }
            if *byte == pattern[matched] {
                matched += 1;
            }
            position += 1;
            if matched == pattern.len() {
                if found.is_some() {
                    return Err(failure(
                        "old_text must have exactly one occurrence, including overlapping matches",
                    ));
                }
                found = Some(position - pattern.len() as u64);
                matched = prefixes[matched - 1];
            }
        }
        source.consume(count)?;
    }
    found.ok_or_else(|| {
        failure("old_text must have exactly one occurrence, including overlapping matches")
    })
}

fn edit(original: &Preimage, old: &str, new: &str, output: &mut stream::Output<'_>) -> Result<()> {
    let clone = || {
        original
            .file
            .try_clone()
            .map_err(|e| io_error("retain edit source", None, e))
    };
    let mut source = stream::Source::new(clone()?)?;
    let start = unique_occurrence(&mut source, old.as_bytes())?;
    source.finish(&original.digest)?;
    let mut source = stream::Source::new(clone()?)?;
    let end = start
        .checked_add(old.len() as u64)
        .ok_or_else(|| failure("edit offset overflow"))?;
    let mut position = 0_u64;
    let mut inserted = false;
    loop {
        let bytes = source
            .reader
            .fill_buf()
            .map_err(|e| io_error("stream exact edit", None, e))?;
        if bytes.is_empty() {
            break;
        }
        let count = bytes.len();
        let stop = position + count as u64;
        if position < start {
            output.push(&bytes[..(start.min(stop) - position) as usize])?;
        }
        if !inserted && stop >= start {
            output.push(new.as_bytes())?;
            inserted = true;
        }
        if stop > end {
            output.push(&bytes[(end.max(position) - position) as usize..])?;
        }
        source.consume(count)?;
        position = stop;
    }
    source.finish(&original.digest)
}

pub(super) fn replace(
    request: &FileRequest,
    path: &str,
    mutation: Mutation<'_>,
    expected: Option<&str>,
) -> Result<Value> {
    let root = directory(&request.write_root)?;
    let relative = path
        .strip_prefix(&request.write_root)
        .ok_or_else(|| failure("mutation escaped its root"))?
        .trim_start_matches('/');
    let components: Vec<&str> = relative.split('/').collect();
    let (name, parents) = components
        .split_last()
        .ok_or_else(|| failure("mutation requires a filename"))?;
    let mut parent = root;
    for component in parents {
        parent = openat(
            &parent,
            *component,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            failure(format!(
                "cannot traverse mutation parent (links are forbidden): {e}"
            ))
        })?;
    }
    let first = preimage(&parent, name, !matches!(mutation, Mutation::Write(_)))?;
    if let Some(expected) = expected {
        let original = first
            .as_ref()
            .ok_or_else(|| failure("expected preimage file does not exist"))?;
        if original.digest != expected {
            return Err(failure(
                "stale expected_sha256; inspect current content with read_file and explicit offset: 0 before retrying",
            ));
        }
    }
    let mode = first
        .as_ref()
        .map_or(0o644, |pre| pre.metadata.permissions().mode() & 0o7777);
    let mut replacement = Replacement::create(&parent)?;
    let mut output = stream::Output::new(&mut replacement.file);
    let mut is_patch = false;
    match mutation {
        Mutation::Write(content) => output.push(content.as_bytes())?,
        Mutation::Edit(old, new) => edit(
            first
                .as_ref()
                .ok_or_else(|| failure("edit target does not exist"))?,
            old,
            new,
            &mut output,
        )?,
        Mutation::Append(content) => {
            let original = first
                .as_ref()
                .ok_or_else(|| failure("append target does not exist"))?;
            let mut source = stream::Source::new(
                original
                    .file
                    .try_clone()
                    .map_err(|e| io_error("retain append source", None, e))?,
            )?;
            source.copy(&mut output)?;
            source.finish(&original.digest)?;
            output.push(content.as_bytes())?;
        }
        Mutation::Patch(patch) => {
            let original = first
                .as_ref()
                .ok_or_else(|| failure("patch target does not exist"))?;
            let mut source = stream::Source::new(
                original
                    .file
                    .try_clone()
                    .map_err(|e| io_error("retain patch source", None, e))?,
            )?;
            patch::apply(&mut source, &mut output, patch)?;
            source.finish(&original.digest)?;
            is_patch = true;
        }
    }
    let (digest, total) = output.revision();
    if is_patch && first.as_ref().is_some_and(|pre| pre.digest == digest) {
        return Err(failure("patch must change the file"));
    }
    let outcome = json!({"path":path,"sha256":digest,"total_bytes":total});
    ensure_output_bound(&outcome)?;
    fchmod(&replacement.file, Mode::from_bits_truncate(mode))
        .map_err(|e| failure(format!("cannot preserve file mode: {e}")))?;
    replacement
        .file
        .sync_all()
        .map_err(|e| io_error("synchronize atomic replacement", None, e))?;
    let fresh = preimage(&parent, name, false)?;
    if !same_preimage(&first, &fresh) {
        return Err(failure(
            "stale mutation preimage changed before replacement; inspect current content before retrying",
        ));
    }
    // Arbitrary host writers do not participate in a transactional CAS.
    renameat(&parent, replacement.name.as_str(), &parent, *name)
        .map_err(|e| failure(format!("cannot install atomic replacement: {e}")))?;
    File::from(
        parent
            .try_clone()
            .map_err(|e| io_error("clone mutation directory descriptor", None, e))?,
    )
    .sync_all()
    .map_err(|e| io_error("synchronize mutation directory", None, e))?;
    Ok(outcome)
}
