use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    thread,
};

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let header_end = loop {
        let mut buffer = [0_u8; 1024];
        let count = stream.read(&mut buffer).expect("read request");
        assert!(count > 0, "request ended before headers");
        request.extend_from_slice(&buffer[..count]);
        if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8(request[..header_end].to_vec()).expect("request headers");
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .expect("content length");
    while request.len() < header_end + content_length {
        let mut buffer = [0_u8; 1024];
        let count = stream.read(&mut buffer).expect("read request body");
        assert!(count > 0, "request body ended early");
        request.extend_from_slice(&buffer[..count]);
    }
    request
}

#[test]
fn model_command_prompts_a_local_ollama_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake Ollama");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake Ollama address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let request = read_request(&mut stream);
        let body = std::str::from_utf8(&request).expect("request");
        assert!(body.contains("\"content\":\"hello local model\""));
        assert!(body.contains("\"stream\":false"));
        assert!(body.contains("\"format\":{\"type\":\"object\"}"));

        let response = r#"{"model":"local-test","created_at":"2026-09-01T00:00:00Z","message":{"role":"assistant","content":"local response"},"done":true,"done_reason":"stop","prompt_eval_count":2,"eval_count":2}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .expect("write response");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "model",
            "--provider",
            "ollama",
            "--endpoint",
            &endpoint,
            "--model",
            "local-test",
            "--output-schema",
            r#"{"type":"object"}"#,
            "hello local model",
        ])
        .output()
        .expect("run model command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "local response"
    );
}

#[test]
fn streaming_model_command_preserves_provider_content_exactly() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake Ollama");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake Ollama address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let request = read_request(&mut stream);
        assert!(
            String::from_utf8_lossy(&request).contains("\"stream\":true"),
            "stream request was not enabled"
        );

        let response = concat!(
            "{\"model\":\"local-test\",\"created_at\":\"2026-09-01T00:00:00Z\",",
            "\"message\":{\"role\":\"assistant\",",
            "\"content\":\"streamed\"},\"done\":false}\n",
            "{\"model\":\"local-test\",\"created_at\":\"2026-09-01T00:00:01Z\",",
            "\"message\":{\"role\":\"assistant\",",
            "\"content\":\" response\"},\"done\":true,\"done_reason\":\"stop\"}\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .expect("write response");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "model",
            "--provider",
            "ollama",
            "--endpoint",
            &endpoint,
            "--model",
            "local-test",
            "--stream",
            "hello local model",
        ])
        .output()
        .expect("run streaming model command");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "streamed response"
    );
}
