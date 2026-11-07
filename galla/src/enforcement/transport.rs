//! Owned, bounded package-source proxy transport.

use crate::origin::{self, OriginPolicy};
use crate::protocol::AllowedSource;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HEADER_LIMIT: usize = 16 * 1024;
const BUFFER_BYTES: usize = 16 * 1024;
const ADDRESS_LIMIT: usize = 32;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const DIAGNOSTIC_LIMIT: u64 = 16;

/// CONNECT authorizes an opaque HTTPS authority; HTTP retains its visible path.
pub(super) enum ParsedRequestLine {
    HttpAbsolute {
        host: String,
        port: u16,
        path: String,
    },
    ConnectTunnel {
        host: String,
        port: u16,
    },
}

pub(super) fn parse_request_line(first_line: &str) -> Result<ParsedRequestLine, ()> {
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(());
    }
    let method = parts[0];
    let target = parts[1];
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = match target.split_once(':') {
            Some((host, port)) => {
                if host.is_empty() {
                    return Err(());
                }
                let Ok(port) = port.parse::<u16>() else {
                    return Err(());
                };
                validate_connect_host(host)?;
                (host.to_owned(), port)
            }
            None => {
                validate_connect_host(target)?;
                (target.to_owned(), 443)
            }
        };
        return Ok(ParsedRequestLine::ConnectTunnel { host, port });
    }
    if (method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
        && let Some(stripped) = target.strip_prefix("http://")
    {
        let (authority, path) = match stripped.split_once('/') {
            Some((auth, path)) => (auth, format!("/{}", path)),
            None => (stripped, "/".to_owned()),
        };
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let Ok(port) = port.parse::<u16>() else {
                    return Err(());
                };
                (host.to_owned(), port)
            }
            _ if !authority.is_empty() => (authority.to_owned(), 80),
            _ => return Err(()),
        };
        return Ok(ParsedRequestLine::HttpAbsolute { host, port, path });
    }
    Err(())
}

fn validate_connect_host(host: &str) -> Result<(), ()> {
    if host.is_empty() || host.contains(['/', '@', ' ', ':']) {
        return Err(());
    }
    if !host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(());
    }
    if host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return Err(());
    }
    Ok(())
}

impl ParsedRequestLine {
    pub(super) fn policy_target(&self) -> String {
        match self {
            Self::ConnectTunnel { host, port } => format!("https://{host}:{port}/"),
            Self::HttpAbsolute { host, port, path } => {
                let port_suffix = if *port == 80 {
                    String::new()
                } else {
                    format!(":{port}")
                };
                format!("http://{host}{port_suffix}{path}")
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub clients: usize,
    pub header: Duration,
    pub resolve: Duration,
    pub connect: Duration,
    pub idle: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            clients: 16,
            header: Duration::from_secs(5),
            resolve: Duration::from_secs(5),
            connect: Duration::from_secs(5),
            idle: Duration::from_secs(30),
        }
    }
}

pub(super) struct Guard {
    port: u16,
    cancelled: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

impl Guard {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(listener) = self.listener.take()
            && listener.join().is_err()
        {
            eprintln!("galla-runner: package proxy listener panicked during shutdown");
        }
    }
}

#[derive(Debug)]
enum Failure {
    BadRequest,
    Forbidden,
    HeaderTooLarge,
    Timeout(&'static str),
    Upstream(&'static str),
    Io(&'static str, io::Error),
    Cancelled,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest => f.write_str("malformed proxy request"),
            Self::Forbidden => f.write_str("source or resolved address is not authorized"),
            Self::HeaderTooLarge => f.write_str("proxy header exceeds 16 KiB"),
            Self::Timeout(stage) => write!(f, "{stage} deadline exceeded"),
            Self::Upstream(stage) => write!(f, "upstream {stage} failed"),
            Self::Io(stage, error) => write!(f, "{stage}: {error}"),
            Self::Cancelled => f.write_str("transport cancelled"),
        }
    }
}

impl Failure {
    fn status(&self) -> &'static str {
        match self {
            Self::BadRequest => "400 Bad Request",
            Self::Forbidden => "403 Forbidden",
            Self::HeaderTooLarge => "431 Request Header Fields Too Large",
            Self::Timeout("header") => "408 Request Timeout",
            Self::Timeout(_) => "504 Gateway Timeout",
            _ => "502 Bad Gateway",
        }
    }
}

type Addresses = Result<Vec<SocketAddr>, Failure>;
type AddressLookup = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;
struct Lookup {
    host: String,
    port: u16,
    deadline: Instant,
    response: SyncSender<Addresses>,
}

struct Resolver {
    queue: SyncSender<Lookup>,
    cancelled: Arc<AtomicBool>,
}

impl Resolver {
    fn start(
        capacity: usize,
        cancelled: Arc<AtomicBool>,
        address_lookup: AddressLookup,
    ) -> Result<Self, String> {
        let (queue, incoming) = mpsc::sync_channel::<Lookup>(capacity);
        let stop = Arc::clone(&cancelled);
        // getaddrinfo is not interruptible through std. Exactly one helper owns
        // no sockets; the bounded caller wait must not become an unbounded join.
        thread::Builder::new()
            .name("galla-proxy-dns".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let lookup = match incoming.recv_timeout(POLL_INTERVAL) {
                        Ok(lookup) => lookup,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if Instant::now() >= lookup.deadline {
                        continue;
                    }
                    let result = resolve_addresses(&lookup.host, lookup.port, address_lookup);
                    // A timed-out/cancelled caller has deliberately dropped its receiver.
                    if lookup.response.send(result).is_err() {
                        continue;
                    }
                }
            })
            .map_err(|error| format!("cannot start package-source resolver: {error}"))?;
        Ok(Self { queue, cancelled })
    }

    fn resolve(&self, host: &str, port: u16, timeout: Duration) -> Addresses {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }
        let deadline = Instant::now() + timeout;
        let (response, incoming) = mpsc::sync_channel(1);
        self.queue
            .try_send(Lookup {
                host: host.into(),
                port,
                deadline,
                response,
            })
            .map_err(|_| Failure::Upstream("resolver admission"))?;
        wait_for_resolution(&incoming, deadline, &self.cancelled)
    }
}

fn native_address_lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    Ok((host, port)
        .to_socket_addrs()?
        .take(ADDRESS_LIMIT + 1)
        .collect())
}

fn resolve_addresses(host: &str, port: u16, address_lookup: AddressLookup) -> Addresses {
    let mut addresses =
        address_lookup(host, port).map_err(|error| Failure::Io("DNS lookup", error))?;
    if addresses.is_empty() || addresses.len() > ADDRESS_LIMIT {
        return Err(Failure::Upstream("address bound"));
    }
    addresses.sort_unstable();
    addresses.dedup();
    Ok(addresses)
}

fn wait_for_resolution(
    incoming: &Receiver<Addresses>,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Addresses {
    loop {
        check_deadline(cancelled, deadline, "resolution")?;
        match incoming.recv_timeout(POLL_INTERVAL) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Failure::Upstream("resolver"));
            }
        }
    }
}

pub(super) fn start(sources: &[AllowedSource]) -> Result<Guard, String> {
    start_with_limits(sources, Limits::default())
}

pub(super) fn start_with_limits(
    sources: &[AllowedSource],
    limits: Limits,
) -> Result<Guard, String> {
    start_with_lookup(sources, limits, native_address_lookup)
}

pub(super) fn start_with_lookup(
    sources: &[AllowedSource],
    limits: Limits,
    address_lookup: AddressLookup,
) -> Result<Guard, String> {
    if limits.clients == 0
        || limits.clients > 16
        || [limits.header, limits.resolve, limits.connect, limits.idle]
            .iter()
            .any(Duration::is_zero)
    {
        return Err("invalid private package transport limits".into());
    }
    let loopback = !sources.is_empty()
        && sources.iter().all(|source| match source {
            AllowedSource::CargoRegistry {
                index_origin,
                download_origin,
                ..
            } => {
                origin::is_loopback_origin_url(index_origin)
                    && origin::is_loopback_origin_url(download_origin)
            }
            AllowedSource::CargoGit { repository, .. } => {
                origin::is_loopback_origin_url(repository)
            }
        });
    let mut policy = if loopback {
        OriginPolicy::test_loopback()
    } else {
        OriginPolicy::new()
    };
    for source in sources {
        let urls = match source {
            AllowedSource::CargoRegistry {
                index_origin,
                download_origin,
                ..
            } => vec![index_origin, download_origin],
            AllowedSource::CargoGit { repository, .. } => vec![repository],
        };
        for url in urls {
            policy
                .allow_url(url)
                .map_err(|error| format!("invalid package proxy source: {error}"))?;
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("cannot bind package-source proxy: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("cannot inspect package-source proxy: {error}"))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("cannot configure package-source proxy: {error}"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let resolver = Arc::new(Resolver::start(
        limits.clients,
        Arc::clone(&cancelled),
        address_lookup,
    )?);
    let policy = Arc::new(policy);
    let stop = Arc::clone(&cancelled);
    let handle = match thread::Builder::new()
        .name("galla-proxy".into())
        .spawn(move || serve(listener, policy, resolver, stop, limits, loopback))
    {
        Ok(handle) => handle,
        Err(error) => {
            cancelled.store(true, Ordering::Release);
            return Err(format!("cannot start package-source proxy: {error}"));
        }
    };
    Ok(Guard {
        port,
        cancelled,
        listener: Some(handle),
    })
}

fn serve(
    listener: TcpListener,
    policy: Arc<OriginPolicy>,
    resolver: Arc<Resolver>,
    cancelled: Arc<AtomicBool>,
    limits: Limits,
    loopback: bool,
) {
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let diagnostics = Arc::new(AtomicU64::new(0));
    while !cancelled.load(Ordering::Acquire) {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                if workers.swap_remove(index).join().is_err() {
                    report(&diagnostics, format_args!("worker panicked"));
                }
            } else {
                index += 1;
            }
        }
        let (mut client, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                report(&diagnostics, format_args!("accept failed: {error}"));
                break;
            }
        };
        let admitted = Instant::now();
        if let Err(error) = client.set_nonblocking(true) {
            report(&diagnostics, format_args!("socket setup failed: {error}"));
            continue;
        }
        if workers.len() >= limits.clients {
            // One nonblocking status attempt: overload never blocks admission.
            let response = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            match client.write(response) {
                Ok(count) if count == response.len() => {}
                Ok(_) => report(&diagnostics, format_args!("overload response truncated")),
                Err(error) => report(&diagnostics, format_args!("overload response: {error}")),
            }
            continue;
        }
        let policy = Arc::clone(&policy);
        let resolver = Arc::clone(&resolver);
        let stop = Arc::clone(&cancelled);
        let reports = Arc::clone(&diagnostics);
        match thread::Builder::new()
            .name("galla-proxy-client".into())
            .spawn(move || {
                let mut established = false;
                if let Err(failure) = handle_client(
                    &mut client,
                    &policy,
                    &resolver,
                    &stop,
                    limits,
                    loopback,
                    admitted,
                    &mut established,
                ) && !matches!(failure, Failure::Cancelled)
                {
                    report(&reports, format_args!("{failure}"));
                    if !established
                        && let Err(error) = send_status(&mut client, failure.status(), &stop)
                    {
                        report(&reports, format_args!("error response: {error}"));
                    }
                }
            }) {
            Ok(worker) => workers.push(worker),
            Err(error) => {
                report(&diagnostics, format_args!("cannot start worker: {error}"));
                break;
            }
        }
    }
    drop(listener);
    cancelled.store(true, Ordering::Release);
    for worker in workers {
        if worker.join().is_err() {
            report(
                &diagnostics,
                format_args!("worker panicked during shutdown"),
            );
        }
    }
    let total = diagnostics.load(Ordering::Relaxed);
    if total > DIAGNOSTIC_LIMIT {
        eprintln!(
            "galla-runner: package proxy: {} additional diagnostics suppressed; failed clients were rejected or closed",
            total - DIAGNOSTIC_LIMIT
        );
    }
}

fn report(count: &AtomicU64, detail: fmt::Arguments<'_>) {
    if count.fetch_add(1, Ordering::Relaxed) < DIAGNOSTIC_LIMIT {
        eprintln!("galla-runner: package proxy: {detail}");
    }
}

fn check_deadline(
    cancelled: &AtomicBool,
    deadline: Instant,
    stage: &'static str,
) -> Result<(), Failure> {
    if cancelled.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Failure::Timeout(stage));
    }
    Ok(())
}

fn write_until(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), Failure> {
    while !bytes.is_empty() {
        check_deadline(cancelled, deadline, "write")?;
        match stream.write(bytes) {
            Ok(0) => return Err(Failure::Upstream("zero-byte write")),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL_INTERVAL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(Failure::Io("socket write", error)),
        }
    }
    Ok(())
}

fn send_status(client: &mut TcpStream, status: &str, stop: &AtomicBool) -> Result<(), Failure> {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    write_until(
        client,
        response.as_bytes(),
        stop,
        Instant::now() + Duration::from_millis(100),
    )
}

fn read_header(
    client: &mut TcpStream,
    stop: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<u8>, usize), Failure> {
    let mut bytes = Vec::with_capacity(HEADER_LIMIT);
    let mut buffer = [0; 4096];
    loop {
        check_deadline(stop, deadline, "header")?;
        let available = (HEADER_LIMIT - bytes.len()).min(buffer.len());
        if available == 0 {
            return Err(Failure::HeaderTooLarge);
        }
        match client.read(&mut buffer[..available]) {
            Ok(0) => return Err(Failure::BadRequest),
            Ok(count) => {
                let scan_start = bytes.len().saturating_sub(3);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes[scan_start..]
                    .windows(4)
                    .position(|part| part == b"\r\n\r\n")
                {
                    return Ok((bytes, scan_start + end + 4));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL_INTERVAL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(Failure::Io("header read", error)),
        }
    }
}

fn connect_upstream(
    addresses: &[SocketAddr],
    stop: &AtomicBool,
    deadline: Instant,
) -> Result<TcpStream, Failure> {
    let mut last_error = None;
    for address in addresses {
        check_deadline(stop, deadline, "connect")?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Failure::Timeout("connect"));
        }
        match TcpStream::connect_timeout(address, remaining) {
            Ok(stream) => {
                check_deadline(stop, deadline, "connect")?;
                stream
                    .set_nonblocking(true)
                    .map_err(|error| Failure::Io("upstream socket setup", error))?;
                return Ok(stream);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(Failure::Timeout("connect"));
            }
            Err(error) => last_error = Some(error),
        }
    }
    check_deadline(stop, deadline, "connect")?;
    Err(match last_error {
        Some(error) => Failure::Io("upstream TCP connection", error),
        None => Failure::Upstream("empty address list"),
    })
}

fn authorized_address(address: IpAddr, loopback: bool) -> bool {
    if loopback {
        return address.is_loopback();
    }
    let host = match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    origin::parse_production(&format!("https://{host}/")).is_ok()
}

#[allow(clippy::too_many_arguments)]
fn handle_client(
    client: &mut TcpStream,
    policy: &OriginPolicy,
    resolver: &Resolver,
    stop: &AtomicBool,
    limits: Limits,
    loopback: bool,
    admitted: Instant,
    established: &mut bool,
) -> Result<(), Failure> {
    let (header, end) = read_header(client, stop, admitted + limits.header)?;
    let text = std::str::from_utf8(&header[..end]).map_err(|_| Failure::BadRequest)?;
    let first_line = text.lines().next().ok_or(Failure::BadRequest)?;
    let parsed = parse_request_line(first_line).map_err(|_| Failure::BadRequest)?;
    let target = parsed.policy_target();
    let (host, port, authorized) = match &parsed {
        ParsedRequestLine::ConnectTunnel { host, port } => {
            (host, *port, policy.authorizes_authority(&target))
        }
        ParsedRequestLine::HttpAbsolute { host, port, .. } => {
            (host, *port, policy.permits(&target))
        }
    };
    if !authorized {
        return Err(Failure::Forbidden);
    }
    let addresses = resolver.resolve(host, port, limits.resolve)?;
    if !addresses
        .iter()
        .all(|address| authorized_address(address.ip(), loopback))
    {
        return Err(Failure::Forbidden);
    }
    let mut upstream = connect_upstream(&addresses, stop, Instant::now() + limits.connect)?;
    match parsed {
        ParsedRequestLine::ConnectTunnel { .. } => {
            // Do not wait for the server before forwarding a client's TLS record.
            write_until(
                client,
                b"HTTP/1.1 200 Connection Established\r\n\r\n",
                stop,
                Instant::now() + limits.header,
            )?;
        }
        ParsedRequestLine::HttpAbsolute { path, .. } => {
            let method = first_line
                .split_whitespace()
                .next()
                .ok_or(Failure::BadRequest)?;
            let mut forward = format!("{method} {path} HTTP/1.1\r\n");
            for line in text.lines().skip(1).take_while(|line| !line.is_empty()) {
                if !line.to_ascii_lowercase().starts_with("proxy-") {
                    forward.push_str(line);
                    forward.push_str("\r\n");
                }
            }
            forward.push_str("\r\n");
            write_until(
                &mut upstream,
                forward.as_bytes(),
                stop,
                Instant::now() + limits.header,
            )?;
        }
    }
    *established = true;
    relay(client, &mut upstream, &header[end..], stop, limits.idle)
}

struct Direction {
    bytes: [u8; BUFFER_BYTES],
    start: usize,
    end: usize,
    eof: bool,
    closed: bool,
}

impl Direction {
    fn new(initial: &[u8]) -> Self {
        let mut bytes = [0; BUFFER_BYTES];
        bytes[..initial.len()].copy_from_slice(initial);
        Self {
            bytes,
            start: 0,
            end: initial.len(),
            eof: false,
            closed: false,
        }
    }

    fn pump(
        &mut self,
        source: &mut TcpStream,
        destination: &mut TcpStream,
    ) -> Result<bool, Failure> {
        let mut progress = false;
        if self.start < self.end {
            match destination.write(&self.bytes[self.start..self.end]) {
                Ok(0) => return Err(Failure::Upstream("zero-byte relay write")),
                Ok(count) => {
                    self.start += count;
                    progress = true;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(Failure::Io("relay write", error)),
            }
        }
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
            if self.eof && !self.closed {
                destination
                    .shutdown(Shutdown::Write)
                    .map_err(|error| Failure::Io("relay half-close", error))?;
                self.closed = true;
                progress = true;
            } else if !self.eof {
                match source.read(&mut self.bytes) {
                    Ok(0) => {
                        self.eof = true;
                        progress = true;
                    }
                    Ok(count) => {
                        self.end = count;
                        progress = true;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(Failure::Io("relay read", error)),
                }
            }
        }
        Ok(progress)
    }
}

fn relay(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    initial: &[u8],
    stop: &AtomicBool,
    idle: Duration,
) -> Result<(), Failure> {
    let mut outbound = Direction::new(initial);
    let mut inbound = Direction::new(&[]);
    let mut deadline = Instant::now() + idle;
    while !outbound.closed || !inbound.closed {
        check_deadline(stop, deadline, "relay inactivity")?;
        let outgoing = outbound.pump(client, upstream)?;
        let incoming = inbound.pump(upstream, client)?;
        if outgoing || incoming {
            deadline = Instant::now() + idle;
        } else {
            thread::sleep(POLL_INTERVAL);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_failure_preserves_native_cause() {
        let error = resolve_addresses("fixture.invalid", 443, |_, _| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "resolver refused the fixture hostname",
            ))
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("resolver refused the fixture hostname"),
            "{error}"
        );
    }

    #[test]
    fn resolver_wait_has_deadline_and_cancellation() {
        let (_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = AtomicBool::new(false);
        assert!(matches!(
            wait_for_resolution(
                &receiver,
                Instant::now() + Duration::from_millis(20),
                &cancelled
            ),
            Err(Failure::Timeout("resolution"))
        ));
        cancelled.store(true, Ordering::Release);
        assert!(matches!(
            wait_for_resolution(
                &receiver,
                Instant::now() + Duration::from_secs(1),
                &cancelled
            ),
            Err(Failure::Cancelled)
        ));
    }

    #[test]
    fn connect_deadline_and_cancel_prevent_socket_creation() {
        let stop = AtomicBool::new(false);
        let addresses = ["127.0.0.1:1".parse().unwrap()];
        assert!(matches!(
            connect_upstream(&addresses, &stop, Instant::now()),
            Err(Failure::Timeout("connect"))
        ));
        stop.store(true, Ordering::Release);
        assert!(matches!(
            connect_upstream(&addresses, &stop, Instant::now() + Duration::from_secs(1)),
            Err(Failure::Cancelled)
        ));
    }
}
