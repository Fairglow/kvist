//! Private, best-effort presentation feed, separate from protected evidence.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde_json::Value;

pub(crate) const ENV: &str = "KVIST_WORKSPACE_FEED";
const MAX_FRAME: usize = 64 * 1024;
const MAX_FEED: u64 = 64 * 1024 * 1024;

pub(crate) fn boundary(value: Value) {
    let result =
        Writer::open().and_then(|writer| writer.map_or(Ok(()), |mut writer| writer.send(&value)));
    if let Err(error) = result {
        eprintln!("Skott presentation boundary unavailable: {error}");
    }
}

pub(crate) struct Writer(File);

impl Writer {
    pub(crate) fn open() -> io::Result<Option<Self>> {
        let Some(path) = std::env::var_os(ENV) else {
            return Ok(None);
        };
        let file = File::options()
            .append(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
            return Err(io::Error::other(
                "workspace feed must be a private regular file",
            ));
        }
        Ok(Some(Self(file)))
    }

    pub(crate) fn send(&mut self, value: &Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_FRAME
            || self.0.metadata()?.len().saturating_add(bytes.len() as u64) > MAX_FEED
        {
            return Err(io::Error::other(
                "workspace presentation feed exceeded its limit",
            ));
        }
        self.0.write_all(&bytes)
    }
}

pub(crate) struct Reader {
    file: tempfile::NamedTempFile,
    stream: BufReader<File>,
}

impl Reader {
    pub(crate) fn new() -> io::Result<Self> {
        let file = tempfile::NamedTempFile::new()?;
        let stream = BufReader::new(file.reopen()?);
        Ok(Self { file, stream })
    }

    pub(crate) fn path(&self) -> &Path {
        self.file.path()
    }

    pub(crate) fn drain(&mut self) -> io::Result<(Vec<Value>, bool)> {
        let mut events = Vec::new();
        let mut line = Vec::new();
        for _ in 0..256 {
            let offset = self.stream.stream_position()?;
            line.clear();
            if self
                .stream
                .by_ref()
                .take(MAX_FRAME as u64)
                .read_until(b'\n', &mut line)?
                == 0
            {
                return Ok((events, false));
            }
            if !line.ends_with(b"\n") {
                if line.len() == MAX_FRAME {
                    return Err(io::Error::other(
                        "workspace presentation frame is too large",
                    ));
                }
                self.stream.seek(io::SeekFrom::Start(offset))?;
                return Ok((events, false));
            }
            events.push(serde_json::from_slice(&line)?);
        }
        Ok((events, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_frames_resume_on_the_next_drain_without_replaying() {
        let mut reader = Reader::new().unwrap();
        let mut writer = File::options().append(true).open(reader.path()).unwrap();
        writer.write_all(b"{\"first\":1}\n{\"second\":").unwrap();
        assert_eq!(reader.drain().unwrap().0, [serde_json::json!({"first": 1})]);
        writer.write_all(b"2}\n").unwrap();
        assert_eq!(
            reader.drain().unwrap().0,
            [serde_json::json!({"second": 2})]
        );
        assert!(reader.drain().unwrap().0.is_empty());
    }
}
