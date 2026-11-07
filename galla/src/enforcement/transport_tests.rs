use super::start_network_guard_proxy;
use super::transport::{self, Limits};
use crate::protocol::AllowedSource;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_HELLO: &[u8] = b"\x16\x03\x01\x00\x05\x01\x00\x00\x01\x00";
const SERVER_HELLO: &[u8] = b"\x16\x03\x03\x00\x05\x02\x00\x00\x01\x00";

struct Proxy {
    port: u16,
    _guard: super::transport::Guard,
}

impl Proxy {
    fn source(upstream_port: u16, scheme: &str) -> AllowedSource {
        AllowedSource::CargoRegistry {
            name: "fixture".into(),
            index_origin: format!("{scheme}://127.0.0.1:{upstream_port}/index/"),
            download_origin: format!("{scheme}://127.0.0.1:{upstream_port}/crates/"),
            identity: format!("sha256:{}", "0".repeat(64)),
        }
    }

    fn new(upstream_port: u16) -> Self {
        let guard = start_network_guard_proxy(&[Self::source(upstream_port, "https")])
            .expect("start proxy");
        Self {
            port: guard.port(),
            _guard: guard,
        }
    }

    fn limited(upstream_port: u16, limits: Limits) -> Self {
        let guard = transport::start_with_limits(&[Self::source(upstream_port, "https")], limits)
            .expect("limited proxy");
        Self {
            port: guard.port(),
            _guard: guard,
        }
    }

    fn client(&self) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).expect("proxy client");
        set_timeouts(&stream);
        stream
    }
}

fn set_timeouts(stream: &TcpStream) {
    stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
}

fn upstream() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn connect_header(port: u16) -> Vec<u8> {
    format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").into_bytes()
}

fn status(client: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        client.read_exact(&mut byte).expect("proxy response header");
        bytes.push(byte[0]);
        assert!(bytes.len() <= 1024, "bounded response header");
    }
    String::from_utf8(bytes).unwrap()
}

fn handshake_server(listener: TcpListener) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        let mut hello = [0; CLIENT_HELLO.len()];
        stream.read_exact(&mut hello).expect("client-first record");
        assert_eq!(hello, CLIENT_HELLO);
        stream.write_all(SERVER_HELLO).unwrap();
    })
}

fn handshake(client: &mut TcpStream, port: u16) {
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(client).starts_with("HTTP/1.1 200"));
    client.write_all(CLIENT_HELLO).unwrap();
    let mut reply = [0; SERVER_HELLO.len()];
    client
        .read_exact(&mut reply)
        .expect("server-first response");
    assert_eq!(reply, SERVER_HELLO);
}

#[test]
fn connect_relays_client_first_handshake() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::new(port);
    handshake(&mut proxy.client(), port);
    server.join().unwrap();
}

#[test]
fn registered_loopback_name_resolves_then_relays_to_pinned_address() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let source = AllowedSource::CargoRegistry {
        name: "fixture".into(),
        index_origin: format!("https://localhost:{port}/index/"),
        download_origin: format!("https://localhost:{port}/crates/"),
        identity: format!("sha256:{}", "0".repeat(64)),
    };
    let guard = transport::start_with_lookup(&[source], Limits::default(), |host, port| {
        assert_eq!(host, "localhost");
        Ok(vec![std::net::SocketAddr::from(([127, 0, 0, 1], port))])
    })
    .unwrap();
    let proxy = Proxy {
        port: guard.port(),
        _guard: guard,
    };
    let mut client = proxy.client();
    client
        .write_all(format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n").as_bytes())
        .unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    client.write_all(CLIENT_HELLO).unwrap();
    let mut reply = [0; SERVER_HELLO.len()];
    client.read_exact(&mut reply).unwrap();
    assert_eq!(reply, SERVER_HELLO);
    server.join().unwrap();
}

#[test]
fn idle_client_does_not_block_another_connect() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::new(port);
    let _idle = proxy.client();
    std::thread::sleep(Duration::from_millis(30));
    handshake(&mut proxy.client(), port);
    server.join().unwrap();
}

#[test]
fn connect_preserves_coalesced_client_handshake() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::new(port);
    let mut client = proxy.client();
    let mut request = connect_header(port);
    request.extend_from_slice(CLIENT_HELLO);
    client.write_all(&request).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    let mut reply = [0; SERVER_HELLO.len()];
    client
        .read_exact(&mut reply)
        .expect("coalesced record reply");
    assert_eq!(reply, SERVER_HELLO);
    server.join().unwrap();
}

#[test]
fn connect_half_close_preserves_upstream_reply() {
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        let mut request = Vec::new();
        stream.read_to_end(&mut request).expect("client half-close");
        assert_eq!(request, b"complete request");
        stream.write_all(b"reply after client EOF").unwrap();
    });
    let proxy = Proxy::new(port);
    let mut client = proxy.client();
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    client.write_all(b"complete request").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut reply = Vec::new();
    client.read_to_end(&mut reply).expect("half-close reply");
    assert_eq!(reply, b"reply after client EOF");
    server.join().unwrap();
}

#[test]
fn denied_client_does_not_terminate_listener() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::new(port);
    let mut denied = proxy.client();
    denied
        .write_all(b"CONNECT unapproved.invalid:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    assert!(status(&mut denied).starts_with("HTTP/1.1 403"));
    handshake(&mut proxy.client(), port);
    server.join().unwrap();
}

#[test]
fn fragmented_connect_header_waits_for_complete_terminator() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::new(port);
    let mut client = proxy.client();
    let request = connect_header(port);
    for fragment in request.chunks(7) {
        client.write_all(fragment).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    client.write_all(CLIENT_HELLO).unwrap();
    let mut reply = [0; SERVER_HELLO.len()];
    client.read_exact(&mut reply).unwrap();
    assert_eq!(reply, SERVER_HELLO);
    server.join().unwrap();
}

#[test]
fn simultaneous_connect_clients_make_independent_progress() {
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let mut clients = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            set_timeouts(&stream);
            clients.push(std::thread::spawn(move || {
                let mut hello = [0; CLIENT_HELLO.len()];
                stream.read_exact(&mut hello).unwrap();
                assert_eq!(hello, CLIENT_HELLO);
                stream.write_all(SERVER_HELLO).unwrap();
            }));
        }
        for client in clients {
            client.join().unwrap();
        }
    });
    let proxy = Proxy::new(port);
    let mut first = proxy.client();
    first.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut first).starts_with("HTTP/1.1 200"));
    handshake(&mut proxy.client(), port);
    first.write_all(CLIENT_HELLO).unwrap();
    let mut reply = [0; SERVER_HELLO.len()];
    first.read_exact(&mut reply).unwrap();
    assert_eq!(reply, SERVER_HELLO);
    server.join().unwrap();
}

#[test]
fn full_duplex_stream_preserves_binary_bytes_across_buffer_boundaries() {
    const BYTES: usize = 512 * 1024;
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        let mut writer = stream.try_clone().unwrap();
        let outgoing = std::thread::spawn(move || {
            writer.write_all(&vec![0x83; BYTES]).unwrap();
            writer.shutdown(Shutdown::Write).unwrap();
        });
        let mut incoming = Vec::new();
        stream.read_to_end(&mut incoming).unwrap();
        assert_eq!(incoming, vec![0x17; BYTES]);
        outgoing.join().unwrap();
    });
    let proxy = Proxy::new(port);
    let mut client = proxy.client();
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    let mut writer = client.try_clone().unwrap();
    let outgoing = std::thread::spawn(move || {
        writer.write_all(&vec![0x17; BYTES]).unwrap();
        writer.shutdown(Shutdown::Write).unwrap();
    });
    let mut incoming = Vec::new();
    client.read_to_end(&mut incoming).unwrap();
    assert_eq!(incoming, vec![0x83; BYTES]);
    outgoing.join().unwrap();
    server.join().unwrap();
}

#[test]
fn partial_header_has_absolute_deadline_and_releases_admission() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::limited(
        port,
        Limits {
            clients: 1,
            header: Duration::from_millis(100),
            ..Limits::default()
        },
    );
    let mut slow = proxy.client();
    slow.write_all(b"CONNECT ").unwrap();
    std::thread::sleep(Duration::from_millis(70));
    slow.write_all(b"127").unwrap();
    let started = Instant::now();
    assert!(status(&mut slow).starts_with("HTTP/1.1 408"));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "deadline must not reset"
    );
    std::thread::sleep(Duration::from_millis(20));
    handshake(&mut proxy.client(), port);
    server.join().unwrap();
}

#[test]
fn oversized_and_malformed_headers_do_not_terminate_listener() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::new(port);
    let mut oversized = proxy.client();
    let mut bytes = connect_header(port);
    bytes.truncate(bytes.len() - 2);
    bytes.extend_from_slice(&vec![b'A'; 16 * 1024]);
    oversized.write_all(&bytes).unwrap();
    assert!(status(&mut oversized).starts_with("HTTP/1.1 431"));
    let mut malformed = proxy.client();
    malformed.write_all(b"CONNECT\r\n\r\n").unwrap();
    assert!(status(&mut malformed).starts_with("HTTP/1.1 400"));
    handshake(&mut proxy.client(), port);
    server.join().unwrap();
}

#[test]
fn bounded_admission_rejects_excess_connections_and_recovers() {
    let (listener, port) = upstream();
    let server = handshake_server(listener);
    let proxy = Proxy::limited(
        port,
        Limits {
            clients: 1,
            ..Limits::default()
        },
    );
    let idle = proxy.client();
    std::thread::sleep(Duration::from_millis(30));
    let mut excess = proxy.client();
    assert!(status(&mut excess).starts_with("HTTP/1.1 503"));
    drop(idle);
    std::thread::sleep(Duration::from_millis(30));
    handshake(&mut proxy.client(), port);
    server.join().unwrap();
}

#[test]
fn inactive_connect_closes_both_sockets() {
    let (listener, port) = upstream();
    let proxy = Proxy::limited(
        port,
        Limits {
            idle: Duration::from_millis(80),
            ..Limits::default()
        },
    );
    let mut client = proxy.client();
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    let (mut remote, _) = listener.accept().unwrap();
    set_timeouts(&remote);
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
    assert_eq!(remote.read(&mut [0]).unwrap(), 0);
}

#[test]
fn progress_keeps_a_tunnel_alive_beyond_one_inactivity_period() {
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        for expected in 0..6 {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [expected]);
            stream.write_all(&byte).unwrap();
        }
    });
    let proxy = Proxy::limited(
        port,
        Limits {
            idle: Duration::from_millis(150),
            ..Limits::default()
        },
    );
    let mut client = proxy.client();
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    for sent in 0..6 {
        client.write_all(&[sent]).unwrap();
        let mut reply = [0];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(reply, [sent]);
        std::thread::sleep(Duration::from_millis(50));
    }
    server.join().unwrap();
}

#[test]
fn upstream_half_close_preserves_later_client_bytes() {
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        stream.write_all(b"server done").unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).unwrap();
        assert_eq!(request, b"client after server EOF");
    });
    let proxy = Proxy::new(port);
    let mut client = proxy.client();
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    let mut reply = Vec::new();
    client.read_to_end(&mut reply).unwrap();
    assert_eq!(reply, b"server done");
    client.write_all(b"client after server EOF").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    server.join().unwrap();
}

#[test]
fn cancellation_interrupts_backpressured_relay() {
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        // The client deliberately does not read; this exceeds TCP's buffers.
        assert!(stream.write_all(&vec![0x80; 8 * 1024 * 1024]).is_err());
    });
    let proxy = Proxy::new(port);
    let mut client = proxy.client();
    client.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    drop(proxy);
    assert!(started.elapsed() < Duration::from_millis(300));
    server.join().unwrap();
}

#[test]
fn guard_cancellation_joins_active_and_header_workers() {
    let (listener, port) = upstream();
    let proxy = Proxy::new(port);
    let mut active = proxy.client();
    active.write_all(&connect_header(port)).unwrap();
    assert!(status(&mut active).starts_with("HTTP/1.1 200"));
    let (mut remote, _) = listener.accept().unwrap();
    set_timeouts(&remote);
    let mut idle = proxy.client();
    std::thread::sleep(Duration::from_millis(20));
    let started = Instant::now();
    drop(proxy);
    assert!(started.elapsed() < Duration::from_millis(300));
    assert_eq!(active.read(&mut [0]).unwrap(), 0);
    assert_eq!(idle.read(&mut [0]).unwrap(), 0);
    assert_eq!(remote.read(&mut [0]).unwrap(), 0);
}

#[test]
fn refused_upstream_reports_bad_gateway_without_ending_proxy() {
    let (listener, port) = upstream();
    drop(listener);
    let proxy = Proxy::new(port);
    for _ in 0..2 {
        let mut client = proxy.client();
        client.write_all(&connect_header(port)).unwrap();
        assert!(status(&mut client).starts_with("HTTP/1.1 502"));
    }
}

#[test]
fn plain_http_preserves_head_and_denies_outside_source_path() {
    let (listener, port) = upstream();
    let guard = transport::start(&[Proxy::source(port, "http")]).unwrap();
    let proxy = Proxy {
        port: guard.port(),
        _guard: guard,
    };
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        let header = status(&mut stream);
        assert!(header.starts_with("HEAD /index/item HTTP/1.1\r\n"));
        assert!(!header.contains("Proxy-"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });
    let mut denied = proxy.client();
    denied
        .write_all(format!("HEAD http://127.0.0.1:{port}/outside HTTP/1.1\r\n\r\n").as_bytes())
        .unwrap();
    assert!(status(&mut denied).starts_with("HTTP/1.1 403"));
    let mut client = proxy.client();
    client.write_all(format!("HEAD http://127.0.0.1:{port}/index/item HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nProxy-Connection: keep-alive\r\n\r\n").as_bytes()).unwrap();
    assert!(status(&mut client).starts_with("HTTP/1.1 200"));
    server.join().unwrap();
}
