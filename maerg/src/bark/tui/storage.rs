//! Private, bounded transcript segments with no total-history retention limit.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub(super) const SEGMENT_BYTES: usize = 1_048_576;
pub(super) const DOCUMENT_BYTES: usize = 16_777_216;

pub(super) fn private_directory(path: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(io::Error::other(
                "parent traversal is not permitted in private state",
            ));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{} must be a non-link directory",
                    current.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&current)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn open_private(path: &Path, create: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(create)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::other(format!(
            "{} must be a private regular file with one link",
            path.display()
        )));
    }
    Ok(file)
}

pub(super) fn read_private(path: &Path, limit: usize) -> io::Result<String> {
    let file = open_private(path, false)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other(format!(
            "{} exceeds the {limit}-byte limit",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn block_boundary(source: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut before = None;
    for (event, range) in
        pulldown_cmark::Parser::new_ext(source, pulldown_cmark::Options::all()).into_offset_iter()
    {
        match event {
            pulldown_cmark::Event::Start(_) => depth += 1,
            pulldown_cmark::Event::End(_) => {
                depth = depth.saturating_sub(1);
                let newlines = usize::from(source[..range.end].ends_with('\n'))
                    + source[range.end..]
                        .chars()
                        .take_while(|ch| ch.is_whitespace())
                        .filter(|ch| *ch == '\n')
                        .count();
                // A trailing partial line can still become part of this block.
                // Rotate only after a completed blank line, not a provisional EOF.
                if depth == 0 && newlines >= 2 && range.end < source.len() {
                    if range.end > SEGMENT_BYTES {
                        return before.or((range.end <= DOCUMENT_BYTES).then_some(range.end));
                    }
                    before = Some(range.end);
                }
            }
            _ => {}
        }
    }
    before
}

pub(super) struct Transcript {
    pub(super) directory: PathBuf,
    latest: u64,
    bytes: usize,
    _lock: File,
}

impl Drop for Transcript {
    fn drop(&mut self) {
        // Release immediately even if a concurrent fork briefly inherited the descriptor.
        if let Err(error) = self._lock.unlock() {
            tracing::warn!(%error, "could not explicitly release transcript writer lock");
        }
    }
}

impl Transcript {
    pub(super) fn open(scope: &Path) -> io::Result<Self> {
        let directory = scope.join(".kvist/transcript");
        private_directory(&directory)?;
        let lock = open_private(&directory.join(".writer"), true)?;
        lock.try_lock().map_err(|error| {
            io::Error::other(format!(
                "transcript is already open by another workspace, or cannot be locked: {error}"
            ))
        })?;
        let mut latest = 0;
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && let Some(number) = name.strip_suffix(".md")
                && number.len() == 20
                && number.bytes().all(|byte| byte.is_ascii_digit())
            {
                let number = number.parse::<u64>().map_err(io::Error::other)?;
                latest = latest.max(number);
            }
        }
        let path = directory.join(format!("{latest:020}.md"));
        let file = open_private(&path, true)?;
        let bytes = usize::try_from(file.metadata()?.len()).map_err(io::Error::other)?;
        if bytes > DOCUMENT_BYTES {
            return Err(io::Error::other(
                "transcript segment exceeds its byte bound",
            ));
        }
        read_private(&path, DOCUMENT_BYTES)?;
        file.sync_all()?;
        crate::file_io::sync_directory(&directory).map_err(io::Error::other)?;
        Ok(Self {
            directory,
            latest,
            bytes,
            _lock: lock,
        })
    }

    pub(super) fn latest(&self) -> u64 {
        self.latest
    }

    pub(super) fn read(&self, sequence: u64) -> io::Result<String> {
        private_directory(&self.directory)?;
        let path = self.directory.join(format!("{sequence:020}.md"));
        read_private(&path, DOCUMENT_BYTES)
    }

    pub(super) fn append(&mut self, markdown: &str) -> io::Result<()> {
        private_directory(&self.directory)?;
        let path = self.directory.join(format!("{:020}.md", self.latest));
        let mut file = open_private(&path, false)?;
        if file.metadata()?.len() != self.bytes as u64 {
            return Err(io::Error::other(
                "transcript changed outside this workspace",
            ));
        }
        if self.bytes.saturating_add(markdown.len()) <= SEGMENT_BYTES {
            file.write_all(markdown.as_bytes())?;
            file.sync_data()?;
            self.bytes += markdown.len();
            return Ok(());
        }
        let mut source = read_private(&path, DOCUMENT_BYTES)?;
        source.push_str(markdown);
        let mut remaining = source.as_str();
        let mut segments = Vec::new();
        while remaining.len() > SEGMENT_BYTES {
            let Some(boundary) = block_boundary(remaining) else {
                break;
            };
            segments.push(&remaining[..boundary]);
            remaining = &remaining[boundary..];
        }
        segments.push(remaining);
        if segments
            .iter()
            .any(|segment| segment.len() > DOCUMENT_BYTES)
        {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "one Markdown block exceeds the 16-MiB transcript limit; inspect the retained task log",
            ));
        }
        if segments.len() == 1 {
            file.write_all(markdown.as_bytes())?;
            file.sync_data()?;
            self.bytes += markdown.len();
            return Ok(());
        }
        // Persist new pages before replacing the old page: interrupted rotation
        // can duplicate diagnostics, but never silently loses previously saved text.
        let mut latest = self.latest;
        for segment in segments.iter().skip(1) {
            latest = latest
                .checked_add(1)
                .ok_or_else(|| io::Error::other("transcript sequence exhausted"))?;
            let next = self.directory.join(format!("{latest:020}.md"));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(next)?;
            file.write_all(segment.as_bytes())?;
            file.sync_all()?;
        }
        crate::file_io::sync_directory(&self.directory).map_err(io::Error::other)?;
        crate::file_io::replace_file_atomically(&path, segments[0]).map_err(io::Error::other)?;
        self.latest = latest;
        self.bytes = remaining.len();
        Ok(())
    }

    pub(super) fn new_document(&mut self) -> io::Result<()> {
        let next = self
            .latest
            .checked_add(1)
            .ok_or_else(|| io::Error::other("transcript sequence exhausted"))?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.directory.join(format!("{next:020}.md")))?;
        file.sync_all()?;
        crate::file_io::sync_directory(&self.directory).map_err(io::Error::other)?;
        self.latest = next;
        self.bytes = 0;
        Ok(())
    }

    pub(super) fn history(&self) -> io::Result<Vec<String>> {
        let path = self.directory.join("commands.json");
        match read_private(&path, SEGMENT_BYTES) {
            Ok(text) => {
                let history: Vec<String> = serde_json::from_str(&text).map_err(io::Error::other)?;
                if history.len() > 500 || history.iter().any(|line| line.len() > 8192) {
                    return Err(io::Error::other("command history exceeds its bounds"));
                }
                Ok(history)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn save_history(&self, history: &[String]) -> io::Result<()> {
        private_directory(&self.directory)?;
        let text = serde_json::to_string(history).map_err(io::Error::other)?;
        if text.len() > SEGMENT_BYTES {
            return Err(io::Error::other("command history exceeds its byte bound"));
        }
        crate::file_io::replace_file_atomically(&self.directory.join("commands.json"), &text)
            .map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_rotates_restores_and_reads_old_segments() {
        let dir = tempfile::tempdir().unwrap();
        let mut transcript = Transcript::open(dir.path()).unwrap();
        transcript.append("# First\n\nhello\n").unwrap();
        transcript.append(&"x".repeat(SEGMENT_BYTES)).unwrap();
        assert!(transcript.latest() > 0);
        assert!(transcript.read(0).unwrap().starts_with("# First"));
        let latest = transcript.latest();
        drop(transcript);
        let transcript = Transcript::open(dir.path()).unwrap();
        assert_eq!(transcript.latest(), latest);
        assert!(transcript.read(latest).unwrap().len() <= DOCUMENT_BYTES);
    }

    #[test]
    fn transcript_rejects_links_and_competing_writers() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = Transcript::open(dir.path()).unwrap();
        assert!(Transcript::open(dir.path()).is_err());
        drop(transcript);
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "do not touch").unwrap();
        std::fs::remove_file(dir.path().join(".kvist/transcript/00000000000000000000.md")).unwrap();
        std::os::unix::fs::symlink(
            &target,
            dir.path().join(".kvist/transcript/00000000000000000000.md"),
        )
        .unwrap();
        assert!(Transcript::open(dir.path()).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "do not touch");
    }

    #[test]
    fn dropping_the_writer_releases_a_briefly_inherited_descriptor_lock() {
        let directory = tempfile::tempdir().unwrap();
        let transcript = Transcript::open(directory.path()).unwrap();
        let inherited = transcript._lock.try_clone().unwrap();
        drop(transcript);
        assert!(Transcript::open(directory.path()).is_ok());
        drop(inherited);
    }

    #[test]
    fn private_state_is_bounded_and_component_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let mut root = Transcript::open(dir.path()).unwrap();
        let mut child = Transcript::open(&child).unwrap();
        root.append("root").unwrap();
        child.append("child").unwrap();
        assert_eq!(root.read(0).unwrap(), "root");
        assert_eq!(child.read(0).unwrap(), "child");
        assert!(read_private(&root.directory.join("00000000000000000000.md"), 2).is_err());
    }

    #[test]
    fn rotation_never_bisects_fences_tables_or_lists_even_across_appends() {
        let dir = tempfile::tempdir().unwrap();
        let mut transcript = Transcript::open(dir.path()).unwrap();
        let fence = format!(
            "```rust\n{}{}\n```\n\n",
            "let x = 1;\n".repeat(90_000),
            "last"
        );
        let table = format!("| A | B |\n|---|---|\n{}\n", "| x | y |\n".repeat(110_000));
        let list = format!("{}\n", "- first\n  - nested\n".repeat(60_000));
        let source = format!("# Start\n\n{fence}{table}{list}# End\n\n");
        for chunk in source.as_bytes().chunks(4096) {
            transcript
                .append(std::str::from_utf8(chunk).unwrap())
                .unwrap();
        }
        let segments: Vec<_> = (0..=transcript.latest())
            .map(|n| transcript.read(n).unwrap())
            .collect();
        assert_eq!(segments.concat(), source);
        assert!(
            segments
                .iter()
                .any(|segment| segment.contains(fence.trim_end()))
        );
        assert!(
            segments
                .iter()
                .any(|segment| segment.contains(table.trim_end())),
            "{:?}",
            segments
                .iter()
                .map(|s| (
                    s.len(),
                    &s[..s.len().min(40)],
                    &s[s.len().saturating_sub(40)..]
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            segments
                .iter()
                .any(|segment| segment.contains(list.trim_end()))
        );
        assert!(
            segments
                .iter()
                .all(|segment| segment.len() <= DOCUMENT_BYTES)
        );
    }

    #[test]
    fn oversized_indivisible_blocks_fail_without_changing_persisted_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut transcript = Transcript::open(dir.path()).unwrap();
        transcript.append("```text\nretained\n").unwrap();
        assert!(transcript.append(&"x".repeat(DOCUMENT_BYTES)).is_err());
        assert_eq!(transcript.read(0).unwrap(), "```text\nretained\n");
    }
}
