use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use sav::{CancellationToken, DirectModelTransport, LocalModelProvider};
use serde_json::{Value, json};

fn loaded(props: Value, models: Option<Value>) -> (sav::Result<Option<String>>, Vec<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut requests = Vec::new();
        for body in std::iter::once(props).chain(models) {
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return requests;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() <= 4096);
            }
            requests.push(String::from_utf8(request).unwrap());
            let body = body.to_string();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        requests
    });
    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &format!("http://{address}"),
        Duration::from_secs(1),
        1024,
    )
    .unwrap();
    let result = transport.loaded_model(&CancellationToken::new());
    (result, server.join().unwrap())
}

#[test]
fn router_reads_loaded_status_not_router_alias_or_unloaded_default() {
    let (result, requests) = loaded(
        json!({"role":"router", "model_alias":"llama-server", "model_path":"none",
            "default_generation_settings":{"n_ctx":0}}),
        Some(json!({"data":[
            {"id":"default", "status":{"value":"unloaded"}},
            {"id":"loading", "status":{"value":"loading"}},
            {"id":"already-active", "status":{"value":"loaded"}}
        ]})),
    );
    assert_eq!(result.unwrap().as_deref(), Some("already-active"));
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /props HTTP"));
    assert!(requests[1].starts_with("GET /models HTTP"));
}

#[test]
fn standalone_server_accepts_model_alias_and_path() {
    for props in [
        json!({"model":"chosen"}),
        json!({"model_alias":"chosen", "model_path":"/models/other.gguf"}),
        json!({"model_path":"chosen"}),
        json!({"model":"", "model_alias":"chosen"}),
    ] {
        assert_eq!(loaded(props, None).0.unwrap().as_deref(), Some("chosen"));
    }
}

#[test]
fn router_with_no_loaded_models_returns_none() {
    let (result, _) = loaded(
        json!({"role":"router"}),
        Some(json!({"data":[
            {"id":"default", "status":{"value":"unloaded"}}
        ]})),
    );
    assert_eq!(result.unwrap(), None);
}

#[test]
fn router_rejects_invalid_catalogs_and_identifiers() {
    for body in [
        json!({"data":{}}),
        json!({"data":vec![json!({"id":"a","status":{"value":"loaded"}});129]}),
        json!({"data":[{"id":"bad\nid","status":{"value":"loaded"}}]}),
        json!({"data":[{"id":"x".repeat(257),"status":{"value":"loaded"}}]}),
    ] {
        assert!(loaded(json!({"role":"router"}), Some(body)).0.is_err());
    }
}

#[test]
fn loaded_model_cancellation_interrupts_a_stalled_probe() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &format!("http://{}", listener.local_addr().unwrap()),
        Duration::from_secs(60),
        1024,
    )
    .unwrap();
    let token = CancellationToken::new();
    let child_token = token.clone();
    let started = Instant::now();
    let worker = std::thread::spawn(move || transport.loaded_model(&child_token));
    let (_socket, _) = listener.accept().unwrap();
    token.cancel();
    assert!(worker.join().unwrap().is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
}
