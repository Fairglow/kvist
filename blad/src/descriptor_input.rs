use std::fs::File;
use std::io::{self, Read};
use std::os::fd::FromRawFd;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;

pub(crate) fn number(path: &Path) -> Option<io::Result<i32>> {
    let path = path.to_str()?;
    let number = path
        .strip_prefix("/dev/fd/")
        .or_else(|| path.strip_prefix("/proc/self/fd/"))?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(number.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "descriptor number is out of range",
        )
    }))
}

pub(crate) fn snapshot(number: i32) -> io::Result<String> {
    // F_DUPFD_CLOEXEC takes integer arguments only. The kernel validates even
    // closed numbers; success returns a new descriptor owned exclusively here.
    let duplicate = unsafe { libc::fcntl(number, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // The successful fcntl result is live, uniquely owned and closed by File.
    let file = unsafe { File::from_raw_fd(duplicate) };
    let flags = rustix::fs::fcntl_getfl(&file)?;
    if flags.contains(rustix::fs::OFlags::WRONLY) || flags.contains(rustix::fs::OFlags::PATH) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "descriptor must be readable",
        ));
    }
    let metadata = file.metadata()?;
    if !metadata.is_file() && !metadata.file_type().is_fifo() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "descriptor requires a regular file or pipe",
        ));
    }
    let limit = crate::stream_input::SOURCE_LIMIT;
    let bytes = if metadata.is_file() {
        crate::file_input::read_regular_descriptor(file, limit)?
    } else {
        crate::file_input::read_bounded(ReadyReader(file), limit)?
    };
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

struct ReadyReader(File);

impl Read for ReadyReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.0.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let mut descriptors = [rustix::event::PollFd::new(
                        &self.0,
                        rustix::event::PollFlags::IN,
                    )];
                    match rustix::event::poll(&mut descriptors, None) {
                        Ok(_) | Err(rustix::io::Errno::INTR) => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                result => return result,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    #[test]
    fn descriptor_paths_are_exact_and_bounded() {
        for path in ["/dev/fd/63", "/proc/self/fd/063"] {
            assert_eq!(number(Path::new(path)).unwrap().unwrap(), 63);
        }
        for path in ["/dev/fd/-1", "/dev/fd/1/child", "dev/fd/1", "/proc/1/fd/0"] {
            assert!(number(Path::new(path)).is_none());
        }
        assert!(
            number(Path::new("/dev/fd/999999999999999"))
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn nonblocking_pipes_wait_without_changing_shared_flags() {
        let (reader, writer) = nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK).unwrap();
        let before = rustix::fs::fcntl_getfl(&reader).unwrap();
        let producer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            File::from(writer).write_all(b"# Delayed").unwrap();
        });
        assert_eq!(snapshot(reader.as_raw_fd()).unwrap(), "# Delayed");
        producer.join().unwrap();
        assert_eq!(rustix::fs::fcntl_getfl(&reader).unwrap(), before);
    }

    #[test]
    fn unsupported_and_closed_descriptors_fail_without_reading() {
        let device = File::open("/dev/null").unwrap();
        let directory = File::open(".").unwrap();
        let (socket, _) = std::os::unix::net::UnixStream::pair().unwrap();
        for fd in [
            device.as_raw_fd(),
            directory.as_raw_fd(),
            socket.as_raw_fd(),
            i32::MAX,
        ] {
            assert!(snapshot(fd).is_err());
        }
        let (_, writer) = nix::unistd::pipe().unwrap();
        assert!(
            snapshot(writer.as_raw_fd())
                .unwrap_err()
                .to_string()
                .contains("readable")
        );
    }

    #[test]
    fn regular_descriptor_snapshots_share_offsets_and_enforce_exact_limits() {
        use std::io::{Seek, SeekFrom};
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"prefix# Snapshot").unwrap();
        file.seek(SeekFrom::Start(6)).unwrap();
        assert_eq!(snapshot(file.as_raw_fd()).unwrap(), "# Snapshot");
        assert_eq!(file.stream_position().unwrap(), 16);
        for size in [
            crate::stream_input::SOURCE_LIMIT,
            crate::stream_input::SOURCE_LIMIT + 1,
        ] {
            file.set_len(size as u64).unwrap();
            file.rewind().unwrap();
            let result = snapshot(file.as_raw_fd());
            if size == crate::stream_input::SOURCE_LIMIT {
                assert_eq!(result.unwrap().len(), size);
            } else {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::FileTooLarge);
            }
        }
    }
}
