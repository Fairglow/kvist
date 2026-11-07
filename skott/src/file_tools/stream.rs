//! Fixed-buffer source processing; limits apply to requests, not file lengths.

use super::*;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

pub(super) const BUFFER_BYTES: usize = 8192;

#[derive(Default)]
pub(super) struct Utf8 {
    tail: Vec<u8>,
}

impl Utf8 {
    pub(super) fn pending_bytes(&self) -> usize {
        self.tail.len()
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) -> Result<()> {
        if self.tail.is_empty() {
            return match std::str::from_utf8(bytes) {
                Ok(_) => Ok(()),
                Err(error) if error.error_len().is_none() => {
                    self.tail
                        .try_reserve(3)
                        .map_err(|_| failure("cannot allocate UTF-8 continuation buffer"))?;
                    self.tail.extend_from_slice(&bytes[error.valid_up_to()..]);
                    Ok(())
                }
                Err(_) => Err(failure("operation requires UTF-8 text")),
            };
        }
        if bytes.len() > BUFFER_BYTES {
            return Err(failure(
                "UTF-8 processing buffer exceeds working-memory bound",
            ));
        }
        let mut joined = [0; BUFFER_BYTES + 3];
        let length = self.tail.len() + bytes.len();
        joined[..self.tail.len()].copy_from_slice(&self.tail);
        joined[self.tail.len()..length].copy_from_slice(bytes);
        self.tail.clear();
        match std::str::from_utf8(&joined[..length]) {
            Ok(_) => Ok(()),
            Err(error) if error.error_len().is_none() => {
                self.tail
                    .extend_from_slice(&joined[error.valid_up_to()..length]);
                Ok(())
            }
            Err(_) => Err(failure("operation requires UTF-8 text")),
        }
    }

    pub(super) fn finish(&self) -> Result<()> {
        if self.tail.is_empty() {
            Ok(())
        } else {
            Err(failure("operation requires complete UTF-8 text"))
        }
    }
}

pub(super) fn digest(reader: &mut impl Read) -> Result<(String, u64)> {
    inspect(reader, false)
}

pub(super) fn inspect(reader: &mut impl Read, require_utf8: bool) -> Result<(String, u64)> {
    let mut hash = Sha256::new();
    let mut utf8 = Utf8::default();
    let mut total = 0_u64;
    let mut bytes = [0; BUFFER_BYTES];
    loop {
        let count = read(reader, &mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
        if require_utf8 {
            utf8.feed(&bytes[..count])?;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| failure("file length overflow"))?;
    }
    if require_utf8 {
        utf8.finish()?;
    }
    Ok((format!("sha256:{}", hex::encode(hash.finalize())), total))
}

pub(super) fn read(reader: &mut impl Read, bytes: &mut [u8]) -> Result<usize> {
    loop {
        match reader.read(bytes) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error("stream regular file", None, error)),
        }
    }
}

pub(super) struct Output<'a> {
    writer: &'a mut File,
    hash: Sha256,
    pub(super) total: u64,
    pub(super) final_newline: bool,
}

impl<'a> Output<'a> {
    pub(super) fn new(writer: &'a mut File) -> Self {
        Self {
            writer,
            hash: Sha256::new(),
            total: 0,
            final_newline: true,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer
            .write_all(bytes)
            .map_err(|e| io_error("write atomic replacement", None, e))?;
        self.hash.update(bytes);
        self.total = self
            .total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| failure("replacement length overflow"))?;
        if let Some(last) = bytes.last() {
            self.final_newline = *last == b'\n';
        }
        Ok(())
    }

    pub(super) fn revision(self) -> (String, u64) {
        (
            format!("sha256:{}", hex::encode(self.hash.finalize())),
            self.total,
        )
    }
}

pub(super) struct Source {
    pub(super) reader: BufReader<File>,
    utf8: Utf8,
    hash: Sha256,
}

impl Source {
    pub(super) fn new(mut file: File) -> Result<Self> {
        file.seek(SeekFrom::Start(0))
            .map_err(|e| io_error("rewind mutation preimage", None, e))?;
        Ok(Self {
            reader: BufReader::with_capacity(BUFFER_BYTES, file),
            utf8: Utf8::default(),
            hash: Sha256::new(),
        })
    }

    pub(super) fn consume(&mut self, count: usize) -> Result<()> {
        let bytes = &self.reader.buffer()[..count];
        self.utf8.feed(bytes)?;
        self.hash.update(bytes);
        self.reader.consume(count);
        Ok(())
    }

    pub(super) fn copy(&mut self, output: &mut Output<'_>) -> Result<()> {
        loop {
            let bytes = self
                .reader
                .fill_buf()
                .map_err(|e| io_error("read mutation source", None, e))?;
            if bytes.is_empty() {
                break;
            }
            let count = bytes.len();
            output.push(bytes)?;
            self.consume(count)?;
        }
        Ok(())
    }

    pub(super) fn finish(self, expected: &str) -> Result<()> {
        self.utf8.finish()?;
        if format!("sha256:{}", hex::encode(self.hash.finalize())) != expected {
            return Err(failure(
                "stale mutation source changed while streaming; inspect current content before retrying",
            ));
        }
        Ok(())
    }
}

pub(super) fn read_page(args: &ReadArgs) -> Result<Value> {
    let mut file = open_read(&args.path, false)?;
    if !file
        .metadata()
        .map_err(|e| io_error("inspect read descriptor", None, e))?
        .is_file()
    {
        return Err(failure("target must be a regular file"));
    }
    let full_digest = if args.include_sha256 || args.expected_sha256.is_some() {
        let (digest, _) = digest(&mut file)?;
        if args
            .expected_sha256
            .as_ref()
            .is_some_and(|expected| expected != &digest)
        {
            return Err(failure(
                "stale read expected_sha256; inspect current content with explicit offset: 0 and no expected_sha256",
            ));
        }
        Some(digest)
    } else {
        None
    };
    let mut page = Vec::new();
    page.try_reserve_exact(args.limit + 10)
        .map_err(|_| failure("cannot allocate bounded read page"))?;
    let mut observed = None;
    for _ in 0..2 {
        let total = usize::try_from(
            file.metadata()
                .map_err(|e| io_error("inspect current read length", None, e))?
                .len(),
        )
        .map_err(|_| failure("file length exceeds addressable byte offsets"))?;
        let requested = args.offset.min(total);
        let start = requested.saturating_sub(3);
        file.seek(SeekFrom::Start(start as u64))
            .map_err(|e| io_error("seek requested read page", None, e))?;
        page.resize(args.limit + 10, 0);
        let mut count = 0;
        while count < page.len() {
            let read = read(&mut file, &mut page[count..])?;
            if read == 0 {
                break;
            }
            count += read;
        }
        page.truncate(count);
        if file
            .metadata()
            .map_err(|e| io_error("reinspect read length", None, e))?
            .len()
            == total as u64
            && requested - start <= page.len()
        {
            observed = Some((total, requested, start));
            break;
        }
    }
    let (total, requested, start) = observed.ok_or_else(|| {
        failure(
            "file length changed while reading this page; retry the read against current content",
        )
    })?;
    let mut index = requested - start;
    if page.get(index).is_some_and(|byte| byte & 0xc0 == 0x80) {
        let mut beginning = index;
        while beginning > 0 && page[beginning] & 0xc0 == 0x80 {
            beginning -= 1;
        }
        let width = match page[beginning] {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Err(failure("read page begins inside invalid UTF-8")),
        };
        let scalar = page
            .get(beginning..beginning + width)
            .ok_or_else(|| failure("read page begins inside incomplete UTF-8"))?;
        std::str::from_utf8(scalar)
            .map_err(|_| failure("read page begins inside invalid UTF-8"))?;
        if beginning + width <= index {
            return Err(failure("read page begins inside invalid UTF-8"));
        }
        index = beginning + width;
    }
    let offset = start + index;
    let skipped = offset - requested;
    let adjustment = if args.offset > total {
        Some("past_eof")
    } else if skipped > 0 {
        Some("utf8_boundary")
    } else {
        None
    };
    let bytes = &page[index..];
    let mut end = args.limit.min(bytes.len());
    if let Err(error) = std::str::from_utf8(&bytes[..end]) {
        if error.error_len().is_some() {
            return Err(failure("read_file page requires UTF-8 text"));
        }
        end = error.valid_up_to();
    }
    if end == 0 && offset < total {
        return Err(failure("limit is too small for the next UTF-8 character"));
    }
    let text =
        std::str::from_utf8(&bytes[..end]).map_err(|_| failure("read_file requires UTF-8 text"))?;
    let outcome = |length| {
        let mut value = json!({
            "content":&text[..length],"sha256":full_digest,
            "page_sha256":format!("sha256:{}", hex::encode(Sha256::digest(&text.as_bytes()[..length]))),
            "offset":offset,
            "next_offset":(offset + length < total).then_some(offset + length),"total_bytes":total
        });
        if let Some(reason) = adjustment {
            value["requested_offset"] = json!(args.offset);
            value["offset_adjustment"] = json!(reason);
        }
        value
    };
    if output_fits(&outcome(end))? {
        return Ok(outcome(end));
    }
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(end))
        .collect();
    let mut lower = 1;
    let mut upper = boundaries.len().saturating_sub(1);
    let mut fitted = None;
    while lower <= upper {
        let middle = lower + (upper - lower) / 2;
        let candidate = outcome(boundaries[middle]);
        if output_fits(&candidate)? {
            fitted = Some(candidate);
            lower = middle + 1;
        } else {
            upper = middle - 1;
        }
    }
    fitted.ok_or_else(|| {
        failure("a single UTF-8 character cannot fit the complete encoded output bound")
    })
}
