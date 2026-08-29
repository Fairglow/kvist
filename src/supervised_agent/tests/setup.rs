use std::{fs, io::Cursor, os::unix::fs::PermissionsExt};

use supervised_agent::{ModelProfile, collect_profile, render_command, verify_profile};
use tempfile::TempDir;

#[test]
fn rejects_non_http_probe_urls_without_starting_curl() {
    let mut reader = Cursor::new("2\nfile:///etc/passwd\n");
    let mut writer = Vec::new();

    let error = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect_err("reject local-file URL");

    assert!(error.to_string().contains("HTTP or HTTPS"));
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
    let mut reader = Cursor::new("2\nhttp://127.0.0.1:1\nmodel\n\nn\n");
    let mut writer = Vec::new();

    let profile = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect("collect llama-server profile");
    let (_, arguments) = render_command(
        &profile.command,
        "Say \"hello\"\nnext",
        &[],
        std::path::Path::new("."),
    )
    .expect("render llama-server command");

    assert_eq!(arguments[4], "--json");
    assert!(arguments[5].contains("\"content\":\"Say \\\"hello\\\"\\nnext\""));
}

#[test]
fn ollama_default_materializes_the_selected_endpoint() {
    let mut reader = Cursor::new("3\nhttp://127.0.0.1:1\nllama3.1:8b\n\nn\n");
    let mut writer = Vec::new();

    let profile = collect_profile(&mut reader, &mut writer, std::path::Path::new("."))
        .expect("collect Ollama profile");
    let (program, arguments) =
        render_command(&profile.command, "hello", &[], std::path::Path::new("."))
            .expect("render Ollama command");

    assert_eq!(program, "env");
    assert_eq!(arguments[0], "OLLAMA_HOST=http://127.0.0.1:1");
    assert_eq!(&arguments[1..], ["ollama", "run", "llama3.1:8b", "hello"]);
}
