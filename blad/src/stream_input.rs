use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};

pub(crate) const SOURCE_LIMIT: usize = 16 * 1024 * 1024;
const READ_SIZE: usize = 64 * 1024;
const PUMP_READS: usize = 4;
pub(crate) const REFRESH_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Default)]
enum State {
    #[default]
    Live,
    Eof,
    Stopped,
    Failed(io::Error),
}

pub(crate) struct StdinStream {
    input: Option<OwnedFd>,
    buffer: InputBuffer,
    state: State,
    pub revision: usize,
}

impl StdinStream {
    fn new() -> Self {
        Self {
            input: None,
            buffer: InputBuffer::new(SOURCE_LIMIT),
            state: State::Live,
            revision: 0,
        }
    }

    /// This interactive entry point exclusively takes over process stdin.
    /// Keep fd 0 valid as the keyboard terminal, with no extra pipe reader.
    pub fn take_stdin(keyboard: &impl AsFd) -> io::Result<Self> {
        let input = rustix::io::fcntl_dupfd_cloexec(io::stdin(), 3)?;
        rustix::stdio::dup2_stdin(keyboard)?;
        let mut stream = Self::new();
        stream.input = Some(input);
        Ok(stream)
    }

    pub fn source(&self) -> &str {
        &self.buffer.source
    }

    pub fn live(&self) -> bool {
        matches!(self.state, State::Live)
    }

    pub fn failed(&self) -> bool {
        matches!(self.state, State::Failed(_))
    }

    pub fn stop(&mut self) {
        self.input = None;
        if self.live() {
            self.state = State::Stopped;
        }
    }

    pub fn finish(self) -> io::Result<()> {
        match self.state {
            State::Failed(error) => Err(io::Error::new(error.kind(), format!("stdin: {error}"))),
            _ => Ok(()),
        }
    }

    pub fn footer(&self, following: bool) -> String {
        match &self.state {
            State::Live if following => "[LIVE FOLLOW]".into(),
            State::Live => "[LIVE PAUSED]".into(),
            State::Eof => "[EOF]".into(),
            State::Stopped => "[stdin closed]".into(),
            State::Failed(error) => format!("stdin error: {error}"),
        }
    }

    /// The viewer exclusively reads this descriptor. Poll before each bounded
    /// read without changing flags on the caller's shared open-file description.
    /// A full batch requests an immediate keyboard check and another pump.
    pub fn pump(&mut self) -> bool {
        let Some(input) = self.input.take() else {
            return false;
        };
        let full = self.pump_from(&input);
        if self.live() {
            self.input = Some(input);
        }
        full
    }

    pub fn wait(&self, keyboard: &impl AsFd, timeout: Duration) -> io::Result<()> {
        let Some(input) = &self.input else {
            return Ok(());
        };
        let mut descriptors = [
            PollFd::new(keyboard, PollFlags::IN),
            PollFd::new(input, PollFlags::IN),
        ];
        let timeout = Timespec::try_from(timeout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn pump_from(&mut self, input: &impl AsFd) -> bool {
        if !self.live() {
            return false;
        }
        let mut bytes = [0; READ_SIZE];
        for _ in 0..PUMP_READS {
            let mut descriptors = [PollFd::new(input, PollFlags::IN)];
            match poll(&mut descriptors, Some(&Timespec::default())) {
                Ok(0) => return false,
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => return false,
                Err(error) => {
                    self.state = State::Failed(error.into());
                    return false;
                }
            }
            let ready = descriptors[0].revents();
            if ready.contains(PollFlags::NVAL) {
                self.state = State::Failed(rustix::io::Errno::BADF.into());
                return false;
            }
            let allowance = (self.buffer.limit - self.buffer.received + 1).min(READ_SIZE);
            match rustix::io::read(input, &mut bytes[..allowance]) {
                Ok(0) => {
                    self.state = match self.buffer.finish() {
                        Ok(()) => State::Eof,
                        Err(error) => State::Failed(error),
                    };
                    return false;
                }
                Ok(count) => {
                    let before = self.buffer.source.len();
                    let result = self.buffer.push(&bytes[..count]);
                    if self.buffer.source.len() != before {
                        self.revision += 1;
                    }
                    if let Err(error) = result {
                        self.state = State::Failed(error);
                        return false;
                    }
                }
                Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => return false,
                Err(error) => {
                    self.state = State::Failed(error.into());
                    return false;
                }
            }
        }
        true
    }
}

struct InputBuffer {
    source: String,
    pending: Vec<u8>,
    received: usize,
    limit: usize,
}

impl InputBuffer {
    fn new(limit: usize) -> Self {
        Self {
            source: String::new(),
            pending: Vec::new(),
            received: 0,
            limit,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > self.limit - self.received {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!("file exceeds the {}-byte input limit", self.limit),
            ));
        }
        self.received += bytes.len();
        self.pending.extend_from_slice(bytes);
        let (valid, invalid) = match std::str::from_utf8(&self.pending) {
            Ok(text) => (text.len(), None),
            Err(error) => (error.valid_up_to(), error.error_len().map(|_| error)),
        };
        let text = std::str::from_utf8(&self.pending[..valid])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.source.push_str(text);
        self.pending.drain(..valid);
        if let Some(error) = invalid {
            self.pending.clear();
            return Err(io::Error::new(io::ErrorKind::InvalidData, error));
        }
        Ok(())
    }

    fn finish(&self) -> io::Result<()> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete UTF-8 sequence at end of stdin",
            ))
        }
    }
}

pub(crate) struct Refresh {
    revision: usize,
    next: Instant,
}

impl Refresh {
    pub fn new(revision: usize) -> Self {
        Self {
            revision,
            next: Instant::now() + REFRESH_INTERVAL,
        }
    }

    pub fn due(&self, revision: usize, now: Instant) -> bool {
        self.pending(revision) && now >= self.next
    }

    pub fn pending(&self, revision: usize) -> bool {
        self.revision != revision
    }

    pub fn completed(&mut self, revision: usize, elapsed: Duration) {
        self.revision = revision;
        self.next = Instant::now() + REFRESH_INTERVAL.max(elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    #[test]
    fn all_utf8_boundaries_and_newlines_are_lossless_with_at_most_three_carried_bytes() {
        let source = "a界👩\u{200d}💻e\u{301}\r\n\rfinal\n";
        for width in 1..=source.len() {
            let mut buffer = InputBuffer::new(source.len());
            for chunk in source.as_bytes().chunks(width) {
                buffer.push(chunk).unwrap();
                assert!(buffer.pending.len() <= 3);
                assert!(buffer.source.len() + buffer.pending.len() <= source.len());
            }
            buffer.finish().unwrap();
            assert_eq!(buffer.source, source);
            assert_eq!(buffer.received, source.len());
        }
    }

    #[test]
    fn exact_limits_empty_input_and_decoder_failures_are_explicit() {
        for limit in 0..=8 {
            let mut buffer = InputBuffer::new(limit);
            buffer.push(&vec![b'x'; limit]).unwrap();
            buffer.finish().unwrap();
            assert_eq!(buffer.source.len(), limit);
            assert_eq!(
                buffer.push(b"x").unwrap_err().kind(),
                io::ErrorKind::FileTooLarge
            );
            assert_eq!(buffer.source.len(), limit);
        }
        for bytes in [b"ok\xff".as_slice(), b"ok\xe7x".as_slice()] {
            let mut buffer = InputBuffer::new(10);
            assert_eq!(
                buffer.push(bytes).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(buffer.source, "ok");
        }
        let mut buffer = InputBuffer::new(10);
        buffer.push(b"ok\xe7\x95").unwrap();
        assert_eq!(buffer.source, "ok");
        assert_eq!(
            buffer.finish().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn pump_is_readiness_driven_bounded_and_does_not_change_descriptor_flags() {
        let (input, mut producer) = UnixStream::pair().unwrap();
        let flags = rustix::fs::fcntl_getfl(&input).unwrap();
        let mut stream = StdinStream::new();
        assert!(!stream.pump_from(&input));
        assert!(stream.live());
        producer.write_all(b"first").unwrap();
        stream.pump_from(&input);
        assert_eq!(stream.source(), "first");
        assert_eq!(stream.revision, 1);
        assert_eq!(rustix::fs::fcntl_getfl(&input).unwrap(), flags);
        drop(producer);
        stream.pump_from(&input);
        assert!(!stream.live());
        assert_eq!(stream.footer(false), "[EOF]");
        stream.finish().unwrap();
    }

    #[test]
    fn owned_input_waits_for_either_descriptor_and_releases_on_eof_error_or_stop() {
        for terminal in ["eof", "error", "stop"] {
            let (input, mut producer) = UnixStream::pair().unwrap();
            let (keyboard, mut keys) = UnixStream::pair().unwrap();
            let mut stream = StdinStream::new();
            stream.input = Some(input.into());
            keys.write_all(b"q").unwrap();
            stream.wait(&keyboard, Duration::from_secs(5)).unwrap();
            let mut key = [0];
            assert_eq!(rustix::io::read(&keyboard, &mut key).unwrap(), 1);
            assert_eq!(key, *b"q");
            producer.write_all(b"data").unwrap();
            stream.wait(&keyboard, Duration::from_secs(5)).unwrap();
            stream.pump();
            assert_eq!(stream.source(), "data");
            match terminal {
                "eof" => {
                    producer.shutdown(std::net::Shutdown::Write).unwrap();
                    stream.pump();
                }
                "error" => {
                    producer.write_all(b"\xff").unwrap();
                    stream.pump();
                }
                _ => stream.stop(),
            }
            assert!(stream.input.is_none());
            assert!(!stream.live());
            assert_eq!(stream.source(), "data");
        }
    }

    #[test]
    fn pump_obeys_byte_budget_and_one_overflow_probe_then_stops() {
        let mut input = tempfile::tempfile().unwrap();
        input.set_len((SOURCE_LIMIT + READ_SIZE) as u64).unwrap();
        let mut stream = StdinStream::new();
        assert!(stream.pump_from(&input));
        assert_eq!(stream.buffer.received, PUMP_READS * READ_SIZE);
        assert_eq!(stream.source().len(), PUMP_READS * READ_SIZE);
        while stream.live() {
            stream.pump_from(&input);
        }
        assert_eq!(stream.buffer.received, SOURCE_LIMIT);
        use std::io::Seek;
        assert_eq!(input.stream_position().unwrap(), (SOURCE_LIMIT + 1) as u64);
        assert!(stream.failed());
        assert!(stream.footer(false).contains("16777216"));
        assert!(!stream.pump_from(&input));
        assert_eq!(input.stream_position().unwrap(), (SOURCE_LIMIT + 1) as u64);
        assert_eq!(
            stream.finish().unwrap_err().kind(),
            io::ErrorKind::FileTooLarge
        );
    }

    #[test]
    fn read_errors_and_stop_keep_the_source_and_failures_sticky() {
        let input = std::fs::File::open(std::env::temp_dir()).unwrap();
        let mut stream = StdinStream::new();
        stream.pump_from(&input);
        assert!(stream.failed());
        stream.stop();
        assert!(stream.failed());
        assert!(stream.finish().is_err());
        let mut stream = StdinStream::new();
        stream.buffer.push(b"retained").unwrap();
        stream.stop();
        assert!(!stream.pump_from(&input));
        assert_eq!(stream.source(), "retained");
        stream.finish().unwrap();
    }

    #[test]
    fn refresh_coalesces_without_idle_debounce_and_accounts_for_render_cost() {
        let mut refresh = Refresh::new(0);
        let deadline = refresh.next;
        assert!(!refresh.due(1, deadline - Duration::from_nanos(1)));
        assert!(refresh.due(100, deadline));
        assert!(!refresh.due(0, deadline));
        refresh.completed(100, Duration::from_secs(1));
        assert!(!refresh.due(101, Instant::now() + Duration::from_millis(500)));
        assert!(refresh.due(101, refresh.next));
    }

    #[test]
    fn streamed_corpora_match_one_shot_bytes_links_and_anchors_at_eof() {
        let corpora = [
            "# Same\n\n# Same\n\n[label][ref]\n\nnote[^late]\n\n[ref]: #same-1\n\n[^late]: footnote 界",
            "1. first\n\n   continued **bold**\n\n2. next\n\n> quoted\n>\n> continuation\n\n```rust\nlet value = 1;\n\nstill_code",
            "| A | B |\n| - | - |\n| [link](#title) | $x^2$ |\n\n# Title\n\nUnicode 👩\u{200d}💻 e\u{301}\r\nlast",
        ];
        for source in corpora {
            for width in [1, 12, 40, 90] {
                for options in [
                    crate::RenderOptions::default(),
                    crate::RenderOptions::plain(),
                ] {
                    let mut renderer = crate::MarkdownRenderer::with_options(options);
                    renderer.set_width(Some(width));
                    let mut expected = Vec::new();
                    let navigation = renderer.render_navigation(source, &mut expected).unwrap();
                    for chunk in [1, 2, 3, 7, 64] {
                        let mut buffer = InputBuffer::new(source.len());
                        for part in source.as_bytes().chunks(chunk) {
                            buffer.push(part).unwrap();
                        }
                        buffer.finish().unwrap();
                        let mut actual = Vec::new();
                        let observed = renderer
                            .render_navigation(&buffer.source, &mut actual)
                            .unwrap();
                        assert_eq!(actual, expected);
                        assert_eq!(observed.links, navigation.links);
                        assert_eq!(observed.anchors, navigation.anchors);
                    }
                }
            }
        }
    }
}
