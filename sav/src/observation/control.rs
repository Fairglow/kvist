use std::fs::{self, Permissions};
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, poll},
    sys::socket::{
        AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket, sockopt,
    },
};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

use super::invalid;

const MAX_MESSAGE_BYTES: usize = 20 * 1024;
const MAX_GUIDANCE_BYTES: usize = 16 * 1024;
const SERVER_CLIENT_TIMEOUT: Duration = Duration::from_millis(250);

/// A host-interpreted control request; never a tool or grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub schema_version: u32,
    pub id: u64,
    pub action: ControlAction,
}

/// The only operations exposed by a local control capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlAction {
    Cancel,
    Guide { text: String },
}

impl ControlRequest {
    pub fn cancel(id: u64) -> Self {
        Self {
            schema_version: 1,
            id,
            action: ControlAction::Cancel,
        }
    }

    pub fn guide(id: u64, text: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            id,
            action: ControlAction::Guide { text: text.into() },
        }
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.id == 0 {
            return Err(invalid("control version or request identity is invalid"));
        }
        if let ControlAction::Guide { text } = &self.action
            && (text.trim().is_empty()
                || text.len() > MAX_GUIDANCE_BYTES
                || text
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t')))
        {
            return Err(invalid(
                "guidance must be nonblank text of at most 16384 bytes without terminal controls",
            ));
        }
        Ok(())
    }
}

/// An acknowledgement of enqueue/cancel request, not of model delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlReply {
    pub schema_version: u32,
    pub id: u64,
    pub status: String,
    pub message: Option<String>,
}

impl ControlReply {
    pub fn queued(id: u64) -> Self {
        Self {
            schema_version: 1,
            id,
            status: "queued".into(),
            message: None,
        }
    }

    pub fn cancellation_requested(id: u64) -> Self {
        Self {
            schema_version: 1,
            id,
            status: "cancellation_requested".into(),
            message: None,
        }
    }

    pub fn rejected(id: u64, message: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            id,
            status: "rejected".into(),
            message: Some(message.into()),
        }
    }

    fn validate(&self, id: u64) -> Result<()> {
        if self.schema_version != 1
            || self.id != id
            || !matches!(
                self.status.as_str(),
                "queued" | "cancellation_requested" | "rejected"
            )
            || self
                .message
                .as_ref()
                .is_some_and(|text| text.len() > 1024 || text.chars().any(char::is_control))
        {
            return Err(invalid(
                "control acknowledgement is invalid or uncorrelated",
            ));
        }
        Ok(())
    }
}

/// A process-owned listener whose unguessable private path is a control capability.
pub struct ControlServer {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    _directory: tempfile::TempDir,
}

impl ControlServer {
    /// Starts a bounded listener. The callback must only enqueue/acknowledge controls.
    pub fn bind(handler: impl Fn(ControlRequest) -> ControlReply + Send + 'static) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("sav-control-")
            .tempdir()
            .map_err(|source| {
                io_error(Path::new(""), "create private control directory", source)
            })?;
        fs::set_permissions(directory.path(), Permissions::from_mode(0o700))
            .map_err(|source| io_error(directory.path(), "protect control directory", source))?;
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path)
            .map_err(|source| io_error(&path, "bind local control", source))?;
        fs::set_permissions(&path, Permissions::from_mode(0o600))
            .map_err(|source| io_error(&path, "protect local control", source))?;
        listener
            .set_nonblocking(true)
            .map_err(|source| io_error(&path, "configure local control", source))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("sav-control".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let deadline = Instant::now() + SERVER_CLIENT_TIMEOUT;
                            let outcome = read_message(&mut stream, deadline).and_then(|bytes| {
                                let request: ControlRequest = serde_json::from_slice(&bytes)
                                    .map_err(|_| invalid("malformed control request"))?;
                                request.validate()?;
                                if worker_stop.load(Ordering::Acquire) {
                                    return Err(invalid("control owner is shutting down"));
                                }
                                let id = request.id;
                                let reply = handler(request);
                                reply.validate(id)?;
                                write_message(&mut stream, &reply, deadline)
                            });
                            if let Err(error) = outcome {
                                tracing::debug!(%error, "local control exchange rejected");
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            tracing::error!(%error, "local control listener failed");
                            break;
                        }
                    }
                }
            })
            .map_err(|source| io_error(&path, "start control listener", source))?;
        Ok(Self {
            path,
            stop,
            thread: Some(thread),
            _directory: directory,
        })
    }

    /// Returns an explicit capability path; it must not be exposed to the agent.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("local control listener panicked");
        }
    }
}

/// Sends one bounded command through an explicitly supplied private capability.
pub fn send_control(
    path: &Path,
    request: &ControlRequest,
    timeout: Duration,
) -> Result<ControlReply> {
    request.validate()?;
    if timeout.is_zero() || timeout > Duration::from_secs(5) {
        return Err(invalid(
            "control timeout must be positive and at most five seconds",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("control socket has no private parent"))?;
    let directory = fs::symlink_metadata(parent)
        .map_err(|source| io_error(parent, "inspect private control directory", source))?;
    let socket = fs::symlink_metadata(path)
        .map_err(|source| io_error(path, "inspect local control", source))?;
    if !directory.is_dir()
        || directory.permissions().mode() & 0o077 != 0
        || !socket.file_type().is_socket()
        || socket.permissions().mode() & 0o077 != 0
    {
        return Err(invalid(
            "control capability must name a socket inside a private non-link directory",
        ));
    }
    let deadline = Instant::now() + timeout;
    let mut stream = connect_bounded(path, deadline)?;
    write_message(&mut stream, request, deadline)?;
    let bytes = read_message(&mut stream, deadline)?;
    let reply: ControlReply =
        serde_json::from_slice(&bytes).map_err(|_| invalid("malformed control acknowledgement"))?;
    reply.validate(request.id)?;
    Ok(reply)
}

fn connect_bounded(path: &Path, deadline: Instant) -> Result<UnixStream> {
    let address = UnixAddr::new(path)
        .map_err(|error| io_error(path, "validate local control address", error.into()))?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(invalid("local control connection timed out"));
        }
        let descriptor = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
            None,
        )
        .map_err(|error| io_error(path, "create local control socket", error.into()))?;
        match connect(descriptor.as_raw_fd(), &address) {
            Ok(()) => {}
            Err(Errno::EAGAIN) => {
                thread::sleep(remaining.min(Duration::from_millis(10)));
                continue;
            }
            Err(Errno::EINPROGRESS) => {
                let mut descriptors = [PollFd::new(descriptor.as_fd(), PollFlags::POLLOUT)];
                let millis = remaining.as_millis().clamp(1, u16::MAX.into()) as u16;
                poll(&mut descriptors, millis)
                    .map_err(|error| io_error(path, "wait for local control", error.into()))?;
                let error = getsockopt(&descriptor, sockopt::SocketError).map_err(|error| {
                    io_error(path, "inspect local control connection", error.into())
                })?;
                if error != 0 {
                    return Err(io_error(
                        path,
                        "connect local control",
                        std::io::Error::from_raw_os_error(error),
                    ));
                }
                if !descriptors[0]
                    .revents()
                    .is_some_and(|flags| flags.contains(PollFlags::POLLOUT))
                {
                    return Err(invalid("local control connection timed out"));
                }
            }
            Err(error) => return Err(io_error(path, "connect local control", error.into())),
        }
        let stream = UnixStream::from(descriptor);
        stream
            .set_nonblocking(false)
            .map_err(|source| io_error(path, "configure control connection", source))?;
        return Ok(stream);
    }
}

fn read_message(stream: &mut UnixStream, deadline: Instant) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(invalid("local control exchange timed out"));
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|source| io_error(Path::new(""), "bound control read", source))?;
        let count = stream
            .read(&mut buffer)
            .map_err(|source| io_error(Path::new(""), "read local control", source))?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_MESSAGE_BYTES {
            return Err(invalid("local control message is incomplete or oversized"));
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            if end != bytes.len() - 1 {
                return Err(invalid(
                    "local control accepts exactly one request per connection",
                ));
            }
            return Ok(bytes);
        }
    }
}

fn write_message(stream: &mut UnixStream, value: &impl Serialize, deadline: Instant) -> Result<()> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|_| invalid("control cannot be serialized"))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(invalid("local control message is oversized"));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(invalid("local control exchange timed out"));
    }
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|source| io_error(Path::new(""), "bound control write", source))?;
    stream
        .write_all(&bytes)
        .map_err(|source| io_error(Path::new(""), "write local control", source))
}

fn io_error(path: &Path, operation: &'static str, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}
