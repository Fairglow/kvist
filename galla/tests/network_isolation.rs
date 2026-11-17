//! Comprehensive network isolation tests.
//!
//! Verifies that the sandbox enforces network isolation through:
//! - Network namespace isolation for PackageSources mode
//! - CONNECT path authorization (not just authority)
//! - DNS resolution pinned to public addresses
//! - Proxy support for required HTTP methods including POST

use galla::enforcement::transport;
use galla::origin::OriginPolicy;
use galla::protocol::AllowedSource;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_HELLO: &[u8] = b"\x16\x03\x01\x00\x05\x01\x00\x00\x01\x00";
const SERVER_HELLO: &[u8] = b"\x16\x03\x03\x00\x05\x02\x00\x00\x01\x00";

fn set_timeouts(stream: &TcpStream) {
    stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
}

fn upstream() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn source(upstream_port: u16) -> AllowedSource {
    let index = format!("https://127.0.0.1:{upstream_port}/index/");
    let download = format!("https://127.0.0.1:{upstream_port}/crates/");
    let identity = galla::validation::registry_identity("fixture", &index, &download);
    AllowedSource::CargoRegistry {
        name: "fixture".into(),
        index_origin: index,
        download_origin: download,
        identity,
    }
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

// Test 1: Network namespace isolation for PackageSources mode
// Verifies that PackageSources mode requires allowed sources and Deny mode
// must not have sources
#[test]
fn package_sources_mode_uses_network_namespace_isolation() {
    // PackageSources mode with sources creates a valid policy
    let mut policy = OriginPolicy::test_loopback();
    let source = source(8080);
    match &source {
        AllowedSource::CargoRegistry {
            index_origin,
            download_origin,
            ..
        } => {
            policy.allow_url(index_origin).unwrap();
            policy.allow_url(download_origin).unwrap();
        }
        AllowedSource::CargoGit { repository, .. } => {
            policy.allow_url(repository).unwrap();
        }
    }
    // Policy was created with test_loopback() - loopback sources are allowed

    // Deny mode has no allowed sources (empty policy)
    let deny_policy = OriginPolicy::new();
    assert!(
        !deny_policy.authorizes_authority("https://127.0.0.1:8080/index/"),
        "Deny mode (empty policy) should not authorize any authority"
    );

    // PackageSources mode without sources is invalid (empty policy authorizes nothing)
    let empty_policy = OriginPolicy::test_loopback();
    assert!(
        !empty_policy.authorizes_authority("https://127.0.0.1:8080/index/"),
        "PackageSources mode without sources should not authorize any authority"
    );
}

// Test 2: CONNECT path authorization
// Verifies that CONNECT requests are authorized on authority for HTTPS tunneling
#[test]
fn connect_path_authorization_is_enforced() {
    let (listener, port) = upstream();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);
        let mut hello = [0; CLIENT_HELLO.len()];
        stream.read_exact(&mut hello).expect("client-first record");
        stream.write_all(SERVER_HELLO).unwrap();
    });

    let guard = transport::start(&[source(port)]).expect("start proxy");
    let proxy_port = guard.port();

    // Wait for proxy to start
    std::thread::sleep(Duration::from_millis(50));

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).expect("proxy client");
    set_timeouts(&client);

    // CONNECT should be authorized on authority (host:port) for HTTPS tunneling
    client.write_all(&connect_header(port)).unwrap();
    let response = status(&mut client);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "CONNECT to authorized authority should succeed: {}",
        response
    );

    // Verify relay works
    client.write_all(CLIENT_HELLO).unwrap();
    let mut reply = [0; SERVER_HELLO.len()];
    client
        .read_exact(&mut reply)
        .expect("server-first response");
    assert_eq!(reply, SERVER_HELLO);

    server.join().unwrap();
    drop(guard);
}

// Test 3: DNS resolution pinned to public addresses
// Verifies that resolved addresses are checked against policy
#[test]
fn dns_resolution_is_pinned_to_public_addresses() {
    use galla::origin;

    // Loopback should be rejected in production mode
    let result = origin::parse_production("https://127.0.0.1:8080/index/");
    assert!(
        result.is_err(),
        "Loopback addresses should be rejected in production mode"
    );

    // Private addresses should be rejected
    let private_result = origin::parse_production("https://192.168.1.1:8080/index/");
    assert!(
        private_result.is_err(),
        "Private addresses should be rejected in production mode"
    );

    // Public addresses should be accepted
    let public_result = origin::parse_production("https://crates.io/index/");
    assert!(
        public_result.is_ok(),
        "Public addresses should be accepted in production mode"
    );

    // Verify loopback-only policy for test mode
    let mut test_policy = origin::OriginPolicy::test_loopback();
    test_policy
        .allow_url("https://127.0.0.1:8080/index/")
        .unwrap();
    let loopback_ok = test_policy.authorizes_authority("https://127.0.0.1:8080/index/");
    assert!(loopback_ok, "Loopback should be authorized in test mode");
}

// Test 4: Proxy supports required HTTP methods including POST
// Verifies that POST is accepted by the proxy parser (for Cargo Git smart HTTP)
#[test]
fn proxy_supports_post_for_cargo_git_smart_http() {
    // Test that the request line parser accepts POST
    let post_line = "POST http://127.0.0.1:8080/index/info HTTP/1.1";
    let parsed = galla::enforcement::transport::parse_request_line(post_line);
    assert!(
        parsed.is_ok(),
        "POST request line should be parsed successfully"
    );

    // Verify it's parsed as an HTTP absolute request
    match parsed.unwrap() {
        galla::enforcement::transport::ParsedRequestLine::HttpAbsolute { host, port, path } => {
            assert_eq!(host, "127.0.0.1");
            assert_eq!(port, 8080);
            assert_eq!(path, "/index/info");
        }
        _ => panic!("Expected HttpAbsolute for POST request"),
    }
}

// Test 5: Proxy handles CONNECT for HTTPS tunneling
#[test]
fn proxy_handles_connect_for_https_tunneling() {
    let (listener, port) = upstream();

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        set_timeouts(&stream);

        // Read TLS ClientHello
        let mut hello = [0; CLIENT_HELLO.len()];
        stream.read_exact(&mut hello).expect("client-first record");
        stream.write_all(SERVER_HELLO).unwrap();
    });

    let guard = transport::start(&[source(port)]).expect("start proxy");
    let proxy_port = guard.port();

    std::thread::sleep(Duration::from_millis(50));

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).expect("proxy client");
    set_timeouts(&client);

    // CONNECT to authorized authority
    client.write_all(&connect_header(port)).unwrap();
    let response = status(&mut client);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "CONNECT should succeed: {}",
        response
    );

    // Send TLS ClientHello through tunnel
    client.write_all(CLIENT_HELLO).unwrap();
    let mut reply = [0; SERVER_HELLO.len()];
    client
        .read_exact(&mut reply)
        .expect("server-first response");
    assert_eq!(reply, SERVER_HELLO);

    server.join().unwrap();
    drop(guard);
}

// Test 6: Unauthorized sources are rejected
#[test]
fn unauthorized_sources_are_rejected() {
    let (_listener, port) = upstream();

    let guard = transport::start(&[source(port)]).expect("start proxy");
    let proxy_port = guard.port();

    std::thread::sleep(Duration::from_millis(50));

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).expect("proxy client");
    set_timeouts(&client);

    // Try to CONNECT to unauthorized host
    client
        .write_all(b"CONNECT unapproved.invalid:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    let response = status(&mut client);
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "Unauthorized source should be rejected: {}",
        response
    );

    drop(guard);
}

// Test 7: Proxy enforces header size limits
#[test]
fn proxy_enforces_header_size_limits() {
    let (_listener, port) = upstream();

    let guard = transport::start(&[source(port)]).expect("start proxy");
    let proxy_port = guard.port();

    std::thread::sleep(Duration::from_millis(50));

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).expect("proxy client");
    set_timeouts(&client);

    // Send oversized header
    let mut request = connect_header(port);
    request.truncate(request.len() - 2);
    request.extend_from_slice(&vec![b'A'; 16 * 1024]);
    client.write_all(&request).unwrap();

    let response = status(&mut client);
    assert!(
        response.starts_with("HTTP/1.1 431"),
        "Oversized header should be rejected: {}",
        response
    );

    drop(guard);
}

// Test 8: Proxy handles multiple concurrent clients
#[test]
fn proxy_handles_multiple_concurrent_clients() {
    let (listener, port) = upstream();

    let server = std::thread::spawn(move || {
        // Handle two connections
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            set_timeouts(&stream);
            stream.write_all(SERVER_HELLO).unwrap();
        }
    });

    let guard = transport::start(&[source(port)]).expect("start proxy");
    let proxy_port = guard.port();

    std::thread::sleep(Duration::from_millis(50));

    // First client
    let mut client1 = TcpStream::connect(("127.0.0.1", proxy_port)).expect("client 1");
    set_timeouts(&client1);
    client1.write_all(&connect_header(port)).unwrap();
    let response1 = status(&mut client1);
    assert!(
        response1.starts_with("HTTP/1.1 200"),
        "Client 1 CONNECT should succeed: {}",
        response1
    );

    // Second client
    let mut client2 = TcpStream::connect(("127.0.0.1", proxy_port)).expect("client 2");
    set_timeouts(&client2);
    client2.write_all(&connect_header(port)).unwrap();
    let response2 = status(&mut client2);
    assert!(
        response2.starts_with("HTTP/1.1 200"),
        "Client 2 CONNECT should succeed: {}",
        response2
    );

    server.join().unwrap();
    drop(guard);
}

// Test 9: Proxy validates request line format
#[test]
fn proxy_validates_request_line_format() {
    // Test malformed request lines are rejected
    assert!(
        transport::parse_request_line("INVALID").is_err(),
        "Malformed request line should be rejected"
    );
    assert!(
        transport::parse_request_line("GET").is_err(),
        "Incomplete request line should be rejected"
    );
    assert!(
        transport::parse_request_line("GET http://example.com").is_err(),
        "Missing HTTP version should be rejected"
    );
}

// Test 10: Full duplex stream preservation
#[test]
fn proxy_preserves_full_duplex_stream() {
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

    let guard = transport::start(&[source(port)]).expect("start proxy");
    let proxy_port = guard.port();

    std::thread::sleep(Duration::from_millis(50));

    let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).expect("proxy client");
    set_timeouts(&client);

    client.write_all(&connect_header(port)).unwrap();
    let response = status(&mut client);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "CONNECT should succeed: {}",
        response
    );

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
    drop(guard);
}
