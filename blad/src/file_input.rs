use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub(crate) fn read_regular_file(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    read_regular_descriptor(file, max_bytes)
}

pub(crate) fn read_regular_descriptor(
    file: std::fs::File,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input requires a regular file",
        ));
    }
    if metadata.len() > u64::try_from(max_bytes).map_err(io::Error::other)? {
        return Err(size_error(max_bytes));
    }
    read_bounded(file, max_bytes)
}

fn read_bounded(reader: impl Read, max_bytes: usize) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(max_bytes).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(size_error(max_bytes));
    }
    Ok(bytes)
}

fn size_error(max_bytes: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!("file exceeds the {max_bytes}-byte input limit"),
    )
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::{read_bounded, read_regular_file};

    #[test]
    fn byte_limit_is_inclusive_and_consumes_only_one_probe_byte() {
        for limit in 0..=5 {
            for size in 0..=7 {
                let bytes = vec![b'x'; size];
                let mut reader = io::Cursor::new(&bytes);
                let result = read_bounded(&mut reader, limit);
                if size <= limit {
                    assert_eq!(result.unwrap(), bytes);
                } else {
                    let error = result.unwrap_err();
                    assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
                    assert!(error.to_string().contains(&limit.to_string()));
                }
                assert!(reader.position() <= (limit + 1) as u64);
            }
        }
        assert_eq!(read_bounded("éé".as_bytes(), 4).unwrap(), "éé".as_bytes());
        assert_eq!(
            read_bounded("éé".as_bytes(), 3).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
    }

    #[test]
    fn endless_input_stops_after_one_probe_byte() {
        struct Endless {
            consumed: usize,
        }
        impl Read for Endless {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                bytes.fill(b'x');
                self.consumed += bytes.len();
                Ok(bytes.len())
            }
        }
        let mut reader = Endless { consumed: 0 };
        assert_eq!(
            read_bounded(&mut reader, 31).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
        assert_eq!(reader.consumed, 32);
    }

    #[test]
    fn read_errors_propagate_instead_of_returning_a_prefix() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "test read failure",
                ))
            }
        }
        let reader = io::Cursor::new(b"valid prefix").chain(Broken);
        let error = read_bounded(reader, 32).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "test read failure");
    }

    #[test]
    fn descriptor_size_type_and_missing_file_errors_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input");
        std::fs::write(&path, "12345").unwrap();
        assert_eq!(read_regular_file(&path, 5).unwrap(), b"12345");
        assert_eq!(
            read_regular_file(&path, 4).unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
        let error = read_regular_file(temp.path(), 5).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("requires a regular file"));
        assert_eq!(
            read_regular_file(&temp.path().join("missing"), 5)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn final_symlinks_are_rejected_without_reading_the_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        std::fs::write(&target, "target").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = read_regular_file(&link, 32).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ELOOP));
    }
}
