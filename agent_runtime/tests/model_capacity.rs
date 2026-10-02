use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use agent_runtime::{CancellationToken, DirectModelTransport, LocalModelProvider};
use serde_json::{Value, json};

fn capacity(
    provider: LocalModelProvider,
    model: &str,
    body: Value,
) -> agent_runtime::Result<Option<usize>> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let model_id = model.to_owned();
    let worker = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut header = Vec::new();
        let mut byte = [0];
        while !header.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
            assert!(header.len() <= 4096);
        }
        let header = String::from_utf8(header).unwrap();
        assert!(header.starts_with(match provider {
            LocalModelProvider::LlamaServer => "GET /props?model=",
            LocalModelProvider::Ollama => "GET /api/ps ",
        }));
        if model_id == "a/b +?" {
            assert!(header.starts_with("GET /props?model=a%2Fb%20%2B%3F HTTP"));
        }
        let payload = body.to_string();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        )
        .unwrap();
    });
    let transport = DirectModelTransport::new(
        provider,
        &format!("http://{address}"),
        Duration::from_secs(5),
        1024,
    )
    .unwrap();
    let result = transport.context_limit(model, &CancellationToken::new());
    worker.join().unwrap();
    result
}

#[test]
fn selected_llama_context_uses_model_qualified_runtime_properties() {
    assert_eq!(
        capacity(
            LocalModelProvider::LlamaServer,
            "a/b +?",
            json!({"default_generation_settings":{"n_ctx":262144},"n_ctx_train":8192})
        )
        .unwrap(),
        Some(262144)
    );
    assert_eq!(
        capacity(
            LocalModelProvider::LlamaServer,
            "test",
            json!({"n_ctx_train":262144})
        )
        .unwrap(),
        None
    );
    for bad in [json!(0), json!("262144"), json!(1048577), json!(-1)] {
        assert!(
            capacity(
                LocalModelProvider::LlamaServer,
                "test",
                json!({"default_generation_settings":{"n_ctx":bad}})
            )
            .is_err()
        );
    }
    assert!(matches!(
        capacity(LocalModelProvider::LlamaServer, "test", json!([])),
        Err(agent_runtime::Error::MalformedModelResponse { .. })
    ));
}

#[test]
fn ollama_capacity_is_for_the_matching_loaded_model_only() {
    assert_eq!(
        capacity(
            LocalModelProvider::Ollama,
            "chosen",
            json!({"models":[{"name":"chosen:latest","context_length":32768}]})
        )
        .unwrap(),
        Some(32768)
    );
    assert_eq!(
        capacity(
            LocalModelProvider::Ollama,
            "chosen",
            json!({"models":[{"name":"other","context_length":8192},
            {"model":"chosen","context_length":131072}]})
        )
        .unwrap(),
        Some(131072)
    );
    assert_eq!(
        capacity(
            LocalModelProvider::Ollama,
            "absent",
            json!({"models":[{"name":"other","context_length":8192}]})
        )
        .unwrap(),
        None
    );
    assert!(capacity(LocalModelProvider::Ollama, "chosen", json!({"models":{}})).is_err());
}

#[test]
fn capacity_discovery_rejects_cancelled_requests_and_unsafe_endpoints() {
    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        "http://127.0.0.1:1",
        Duration::from_secs(1),
        1024,
    )
    .unwrap();
    let token = CancellationToken::new();
    token.cancel();
    assert!(transport.context_limit("test", &token).is_err());
    for model in ["", "a\nb"] {
        assert!(
            transport
                .context_limit(model, &CancellationToken::new())
                .is_err()
        );
    }
    assert!(
        DirectModelTransport::new(
            LocalModelProvider::LlamaServer,
            "http://192.0.2.1:9931",
            Duration::from_secs(1),
            1024
        )
        .is_err()
    );
}
