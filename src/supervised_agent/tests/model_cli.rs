use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
};

#[test]
fn model_command_prompts_a_local_ollama_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake Ollama");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake Ollama address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
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
                line.strip_prefix("Content-Length: ")
                    .and_then(|value| value.parse::<usize>().ok())
            })
            .expect("content length");
        while request.len() < header_end + content_length {
            let mut buffer = [0_u8; 1024];
            let count = stream.read(&mut buffer).expect("read request body");
            assert!(count > 0, "request body ended early");
            request.extend_from_slice(&buffer[..count]);
        }
        let body = std::str::from_utf8(&request[header_end..]).expect("request body");
        assert!(body.contains("\"content\":\"hello local model\""));
        assert!(body.contains("\"stream\":false"));

        let response = r#"{"model":"local-test","message":{"role":"assistant","content":"local response"},"done":true,"done_reason":"stop","prompt_eval_count":2,"eval_count":2}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .expect("write response");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
        .args([
            "model",
            "--provider",
            "ollama",
            "--endpoint",
            &endpoint,
            "--model",
            "local-test",
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
        "local response\n"
    );
}
