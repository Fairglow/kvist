use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    File {
        device: u64,
        inode: u64,
        size: u64,
        mode: u32,
        modified: (i64, i64),
        changed: (i64, i64),
    },
    Error(io::ErrorKind, String),
}

impl Observation {
    fn read(path: &Path) -> Self {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => Self::File {
                device: metadata.dev(),
                inode: metadata.ino(),
                size: metadata.len(),
                mode: metadata.mode(),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
                changed: (metadata.ctime(), metadata.ctime_nsec()),
            },
            Err(error) => Self::Error(error.kind(), error.to_string()),
        }
    }
}

pub(crate) struct FileWatch {
    next_poll: Instant,
    candidate: Option<Observation>,
    stable_since: Instant,
    attempted: Option<Observation>,
}

impl FileWatch {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            next_poll: now,
            candidate: None,
            stable_since: now,
            attempted: None,
        }
    }

    pub(crate) fn wait(&self) -> Duration {
        self.next_poll.saturating_duration_since(Instant::now())
    }

    pub(crate) fn changed(&mut self, path: &Path) -> bool {
        let now = Instant::now();
        if now < self.next_poll {
            return false;
        }
        self.next_poll = now + INTERVAL;
        self.observe(Observation::read(path), now)
    }

    fn observe(&mut self, observed: Observation, now: Instant) -> bool {
        if self.candidate.as_ref() != Some(&observed) {
            self.candidate = Some(observed);
            self.stable_since = now;
            return false;
        }
        if now.duration_since(self.stable_since) < INTERVAL
            || self.attempted.as_ref() == Some(&observed)
        {
            return false;
        }
        self.attempted = Some(observed);
        true
    }
}

pub(crate) fn read_document_source(path: &Path) -> io::Result<String> {
    let bytes = crate::file_input::read_regular_file(path, 16 * 1024 * 1024)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn read_reload_source(path: Option<&Path>) -> io::Result<String> {
    let path = path.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "reload unavailable: this document has no file path",
        )
    })?;
    read_document_source(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_debounce_and_attempt_each_state_once_including_failures() {
        let mut watch = FileWatch::new();
        let start = Instant::now();
        let missing = Observation::Error(io::ErrorKind::NotFound, "missing".into());
        let denied = Observation::Error(io::ErrorKind::PermissionDenied, "denied".into());
        assert!(!watch.observe(missing.clone(), start));
        assert!(!watch.observe(missing.clone(), start + INTERVAL / 2));
        assert!(!watch.observe(denied.clone(), start + INTERVAL));
        assert!(watch.observe(denied.clone(), start + INTERVAL * 2));
        assert!(!watch.observe(denied, start + INTERVAL * 3));
        assert!(!watch.observe(missing.clone(), start + INTERVAL * 4));
        assert!(watch.observe(missing, start + INTERVAL * 5));
    }

    #[test]
    fn same_length_writes_and_atomic_replacements_change_observations() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("document");
        let other = temp.path().join("other");
        std::fs::write(&path, "before").unwrap();
        let before = Observation::read(&path);
        std::fs::write(&path, "after!").unwrap();
        assert_ne!(before, Observation::read(&path));
        let before = Observation::read(&path);
        std::fs::write(&other, "after!").unwrap();
        std::fs::rename(&other, &path).unwrap();
        assert_ne!(before, Observation::read(&path));
    }

    #[test]
    fn reload_source_has_an_inclusive_16_mib_byte_limit() {
        const LIMIT: u64 = 16 * 1024 * 1024;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("boundary.md");
        let file = std::fs::File::create(&path).unwrap();
        for size in [LIMIT - 1, LIMIT, LIMIT + 1] {
            file.set_len(size).unwrap();
            let result = read_reload_source(Some(&path));
            if size <= LIMIT {
                assert_eq!(result.unwrap().len() as u64, size);
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
                assert!(error.to_string().contains("16777216"));
            }
        }
    }

    #[test]
    fn reload_invalid_utf8_does_not_return_a_valid_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("invalid.md");
        std::fs::write(&path, [b'x', 0xff]).unwrap();
        assert_eq!(
            read_reload_source(Some(&path)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn reload_without_a_file_path_is_an_explicit_error() {
        let error = read_reload_source(None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("reload unavailable"));
    }
}
