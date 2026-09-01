use std::{
    fs,
    io::{Cursor, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::mpsc,
    thread,
};

use agent_runtime::{
    ModelProfile, SetupOptions, collect_profile, collect_profile_with_options, render_command,
    verify_profile,
};
use tempfile::TempDir;

fn serve_llama_setup() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake llama-server");
    let address = listener.local_addr().expect("fake llama-server address");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for body in [
            r#"{"object":"list","data":[{"id":"small-model"},{"id":"Qwen3.8-9B-Q4_K_M"},{"id":"42"}]}"#,
            r#"{"choices":[{"message":{"content":"OK"}}]}"#,
        ] {
            let (mut stream, _) = listener.accept().expect("accept setup request");
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).expect("read setup request");
            let _ = sender.send(String::from_utf8_lossy(&request[..count]).into_owned());
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write setup response");
        }
    });
    (format!("http://{address}"), receiver)
}

#[test]
fn rejects_non_loopback_discovery_urls_without_starting_a_provider() {
    let mut reader = Cursor::new("2\nfile:///etc/passwd\n");
    let mut writer = Vec::new();

    let error = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect_err("reject local-file URL");

    assert!(error.to_string().contains("loopback"));
    assert!(
        String::from_utf8(writer)
            .expect("UTF-8 setup output")
            .contains("http://127.0.0.1:9931")
    );
}

#[test]
fn verification_refuses_before_spawning_without_host_acknowledgement() {
    let workspace = TempDir::new().expect("workspace");
    let marker = workspace.path().join("executed");
    let provider = workspace.path().join("provider.sh");
    fs::write(
        &provider,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");
    let profile = ModelProfile {
        name: "refused".to_owned(),
        provider: "custom-script".to_owned(),
        command: format!("{} '{{prompt}}'", provider.display()),
    };

    let error = verify_profile(&profile, "test", workspace.path(), false)
        .expect_err("host acknowledgement required");

    assert!(error.to_string().contains("--allow-host-execution"));
    assert!(!marker.exists());
}

#[test]
fn llama_server_default_json_encodes_the_rendered_prompt() {
    let mut reader = Cursor::new("2\nhttp://127.0.0.1:1\n2\nmodel\n\n\n");
    let mut writer = Vec::new();

    let profile = collect_profile_with_options(
        &mut reader,
        &mut writer,
        std::path::Path::new("."),
        SetupOptions { force: true },
    )
    .expect("collect llama-server profile");
    let (_, arguments) = render_command(
        &profile.command,
        "Say \"hello\"\nnext",
        &[],
        std::path::Path::new("."),
    )
    .expect("render llama-server command");

    let json_index = arguments
        .iter()
        .position(|argument| argument == "--json")
        .expect("curl JSON argument");
    assert!(arguments[json_index + 1].contains("\"content\":\"Say \\\"hello\\\"\\nnext\""));
}

#[test]
fn llama_server_lists_models_and_keeps_profile_name_separate() {
    let (endpoint, requests) = serve_llama_setup();
    let input = format!("2\n{endpoint}\n2\nlocal-qwen\n\n");
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    let profile = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect("collect discovered llama-server profile");
    let output = String::from_utf8(writer).expect("UTF-8 setup output");
    let (program, arguments) =
        render_command(&profile.command, "hello", &[], std::path::Path::new("."))
            .expect("render discovered llama-server command");

    assert_eq!(profile.name, "local-qwen");
    assert_eq!(profile.provider, "llama-server");
    assert_eq!(program, "curl");
    assert_eq!(arguments.first().map(String::as_str), Some("--disable"));
    assert!(
        arguments
            .iter()
            .any(|argument| argument.contains("\"model\":\"Qwen3.8-9B-Q4_K_M\""))
    );
    assert!(output.contains("Available llama-server models:"));
    assert!(output.contains("2) Qwen3.8-9B-Q4_K_M"));
    assert!(
        requests
            .recv()
            .expect("models request")
            .starts_with("GET /v1/models ")
    );
    let qualification = requests.recv().expect("qualification request");
    assert!(qualification.starts_with("POST /v1/chat/completions "));
    assert!(qualification.contains(r#""content":"Reply with exactly: OK""#));
}

#[test]
fn llama_server_keeps_explicit_default_when_discovery_is_unavailable() {
    let mut reader = Cursor::new("2\nhttp://127.0.0.1:1\n\nllama-default\n\n");
    let mut writer = Vec::new();

    let profile = collect_profile_with_options(
        &mut reader,
        &mut writer,
        std::path::Path::new("."),
        SetupOptions { force: true },
    )
    .expect("collect default llama-server profile");
    let (_, arguments) = render_command(&profile.command, "hello", &[], std::path::Path::new("."))
        .expect("render default llama-server command");

    assert_eq!(profile.name, "llama-default");
    assert!(
        arguments
            .iter()
            .any(|argument| argument.contains("\"model\":\"default\""))
    );
}

#[test]
fn llama_server_accepts_an_advertised_numeric_model_id() {
    let (endpoint, _) = serve_llama_setup();
    let input = format!("2\n{endpoint}\n3\nnumeric-model\n\n");
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    let profile = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect("collect numeric llama-server model");
    let (_, arguments) = render_command(&profile.command, "hello", &[], std::path::Path::new("."))
        .expect("render numeric llama-server command");

    assert!(
        arguments
            .iter()
            .any(|argument| argument.contains("\"model\":\"42\""))
    );
}

#[test]
fn llama_server_rejects_zero_for_forced_list_selection() {
    let (endpoint, _) = serve_llama_setup();
    let input = format!("2\n{endpoint}\n#0\n");
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    let error = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect_err("reject zero-based list selection");

    assert!(error.to_string().contains("number from 1 to 4"));
}

#[test]
fn llama_server_rejects_template_tokens_in_manual_model_ids() {
    let mut reader = Cursor::new("2\nhttp://127.0.0.1:1\n2\nx{prompt_json}y\n");
    let mut writer = Vec::new();

    let error = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect_err("reject template-bearing model ID");

    assert!(error.to_string().contains("without braces"));
}

#[test]
fn ollama_default_materializes_the_selected_endpoint() {
    let mut reader = Cursor::new("3\nhttp://127.0.0.1:1\n1\n\n\n");
    let mut writer = Vec::new();

    let profile = collect_profile_with_options(
        &mut reader,
        &mut writer,
        std::path::Path::new("."),
        SetupOptions { force: true },
    )
    .expect("collect Ollama profile");
    let (program, arguments) =
        render_command(&profile.command, "hello", &[], std::path::Path::new("."))
            .expect("render Ollama command");

    assert_eq!(program, "env");
    assert_eq!(arguments[0], "OLLAMA_HOST=http://127.0.0.1:1");
    assert_eq!(&arguments[1..], ["ollama", "run", "llama3.1:8b", "hello"]);
}
