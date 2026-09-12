#![cfg(target_os = "linux")]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use agent_runtime::{
    CancellationToken, CatalogProvider, ModelCatalog, ModelDiscoveryOptions, ProviderModel,
    discover_models,
};
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
use tempfile::TempDir;

static ACP_CLEANUP_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serve_json(body: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let address = listener.local_addr().expect("provider address");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept catalog request");
        let mut request = [0_u8; 4096];
        let count = stream.read(&mut request).expect("read catalog request");
        sender
            .send(String::from_utf8_lossy(&request[..count]).into_owned())
            .expect("record request");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write catalog response");
    });
    (format!("http://{address}"), receiver)
}

fn options(workspace: &TempDir) -> ModelDiscoveryOptions {
    ModelDiscoveryOptions {
        working_directory: workspace.path().to_path_buf(),
        timeout: Duration::from_secs(10),
        max_response_bytes: 64 * 1024,
        ..ModelDiscoveryOptions::default()
    }
}

fn executable(workspace: &TempDir, body: &str) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = workspace.path().join(format!("provider-{id}"));
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake ACP provider");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .expect("make fake ACP provider executable");
    path.to_string_lossy().into_owned()
}

#[test]
fn canonical_catalog_is_bounded_deduplicated_and_serializes_as_version_one() {
    let catalog = ModelCatalog::new(
        CatalogProvider::Copilot,
        Some("missing".to_owned()),
        vec![
            ProviderModel::new("gpt-5", "GPT 5", Some("first".to_owned())).expect("first model"),
            ProviderModel::new("gpt-5", "duplicate", None).expect("duplicate model"),
            ProviderModel::new("claude", "Claude", None).expect("second model"),
        ],
    )
    .expect("valid catalog");

    assert_eq!(catalog.current_model_id(), None);
    assert_eq!(catalog.default_model_id(), "gpt-5");
    assert_eq!(
        catalog
            .models()
            .iter()
            .map(ProviderModel::id)
            .collect::<Vec<_>>(),
        ["gpt-5", "claude"]
    );
    assert_eq!(
        serde_json::to_value(&catalog).expect("serialize catalog"),
        serde_json::json!({
            "format_version": 1,
            "provider": "copilot",
            "current_model_id": null,
            "models": [
                {"id": "gpt-5", "name": "GPT 5", "description": "first"},
                {"id": "claude", "name": "Claude"}
            ]
        })
    );
}

#[test]
fn canonical_catalog_rejects_invalid_or_empty_models() {
    assert!(ProviderModel::new("bad{id}", "name", None).is_err());
    assert!(ProviderModel::new("model id", "name", None).is_ok());
    assert!(ProviderModel::new("ok", "bad\nname", None).is_err());
    assert!(ProviderModel::new("ok", "name", Some("bad\u{7f}".to_owned())).is_err());
    assert!(ModelCatalog::new(CatalogProvider::Ollama, None, Vec::new()).is_err());

    let models = (0..129)
        .map(|index| ProviderModel::new(format!("m{index}"), format!("Model {index}"), None))
        .collect::<Result<Vec<_>, _>>()
        .expect("individually valid models");
    assert!(ModelCatalog::new(CatalogProvider::Ollama, None, models).is_err());
}

#[test]
fn acp_catalog_rejects_empty_and_oversized_state_and_drops_inconsistent_current() {
    let workspace = TempDir::new().expect("workspace");
    for session_response in [
        r#"{"jsonrpc":"2.0","id":1,"result":{"models":{"availableModels":[],"currentModelId":null}}}"#
            .to_owned(),
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"models":{{"availableModels":[{{"modelId":"ok","name":"{}"}}]}}}}}}"#,
            "x".repeat(70_000)
        ),
    ] {
        let provider = executable(
            &workspace,
            &format!(
                r#"IFS= read -r initialize
printf '%s\n' '{{"jsonrpc":"2.0","id":0,"result":{{"protocolVersion":1}}}}'
IFS= read -r session
printf '%s\n' '{}'"#,
                session_response.replace('\'', "'\\''")
            ),
        );
        let mut discovery = options(&workspace);
        discovery.executable = Some(provider);
        discovery.allow_host_discovery = true;
        let error = discover_models(
            CatalogProvider::Copilot,
            &discovery,
            &CancellationToken::new(),
        )
        .expect_err("reject invalid ACP catalog");
        assert!(
            error.to_string().contains("at least one model")
                || error.to_string().contains("record exceeded")
                || error.to_string().contains("output exceeded"),
            "unexpected error message: {error}"
        );
    }

    let provider = executable(
        &workspace,
        r#"IFS= read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
IFS= read -r session
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"models":{"availableModels":[{"modelId":"first","name":"First"}],"currentModelId":"missing"}}}'"#,
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;
    let catalog = discover_models(
        CatalogProvider::Copilot,
        &discovery,
        &CancellationToken::new(),
    )
    .expect("inconsistent current is discarded");
    assert_eq!(catalog.current_model_id(), None);
    assert_eq!(catalog.default_model_id(), "first");
}

#[test]
fn discovers_ollama_details_and_llama_server_ids_over_bounded_loopback_http() {
    let workspace = TempDir::new().expect("workspace");
    let (ollama_endpoint, ollama_request) = serve_json(
        r#"{"models":[
          {"name":"qwen3:8b","details":{"family":"qwen3","parameter_size":"8.2B","quantization_level":"Q4_K_M"}},
          {"name":"qwen3:8b","details":{"family":"duplicate"}}
        ]}"#,
    );
    let mut ollama_options = options(&workspace);
    ollama_options.endpoint = Some(ollama_endpoint);
    let catalog = discover_models(
        CatalogProvider::Ollama,
        &ollama_options,
        &CancellationToken::new(),
    )
    .expect("discover Ollama");

    assert_eq!(catalog.default_model_id(), "qwen3:8b");
    assert_eq!(catalog.models().len(), 1);
    assert_eq!(catalog.models()[0].name(), "qwen3:8b");
    assert!(
        catalog.models()[0]
            .description()
            .expect("Ollama details")
            .contains("8.2B")
    );
    assert!(
        ollama_request
            .recv()
            .expect("Ollama request")
            .starts_with("GET /api/tags ")
    );

    let (llama_endpoint, llama_request) =
        serve_json(r#"{"object":"list","data":[{"id":"local-a"},{"id":"local-b"}]}"#);
    let mut llama_options = options(&workspace);
    llama_options.endpoint = Some(llama_endpoint);
    let catalog = discover_models(
        CatalogProvider::LlamaServer,
        &llama_options,
        &CancellationToken::new(),
    )
    .expect("discover llama-server");
    assert_eq!(catalog.default_model_id(), "local-a");
    assert_eq!(
        catalog
            .models()
            .iter()
            .map(ProviderModel::id)
            .collect::<Vec<_>>(),
        ["local-a", "local-b"]
    );
    assert!(
        llama_request
            .recv()
            .expect("llama-server request")
            .starts_with("GET /v1/models ")
    );
}

#[test]
fn acp_discovery_correlates_responses_and_uses_absolute_no_bridge_session() {
    let workspace = TempDir::new().expect("workspace");
    let transcript = workspace.path().join("transcript");
    let provider = executable(
        &workspace,
        &format!(
            r#"test "$1" = --acp || exit 20
IFS= read -r initialize || exit 21
printf '%s\n' "$initialize" >> "{}"
printf '%s\n' '{{"jsonrpc":"2.0","id":0,"result":{{"protocolVersion":1}}}}'
IFS= read -r session || exit 22
printf '%s\n' "$session" >> "{}"
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"sessionId":"s","models":{{"availableModels":[{{"modelId":"gpt-5","name":"GPT 5","description":"account model"}},{{"modelId":"mini","name":"Mini"}}],"currentModelId":"mini"}}}}}}'"#,
            transcript.display(),
            transcript.display()
        ),
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;

    let catalog = discover_models(
        CatalogProvider::Copilot,
        &discovery,
        &CancellationToken::new(),
    )
    .expect("discover ACP models");

    assert_eq!(catalog.current_model_id(), Some("mini"));
    assert_eq!(catalog.default_model_id(), "mini");
    let records = fs::read_to_string(transcript).expect("read ACP transcript");
    let mut records = records.lines().map(|line| {
        serde_json::from_str::<serde_json::Value>(line).expect("valid client ACP record")
    });
    let initialize = records.next().expect("initialize");
    assert_eq!(initialize["id"], 0);
    assert_eq!(initialize["method"], "initialize");
    assert_eq!(initialize["params"]["protocolVersion"], 1);
    let session = records.next().expect("session/new");
    assert_eq!(session["id"], 1);
    assert_eq!(session["method"], "session/new");
    assert_eq!(
        session["params"]["cwd"],
        workspace.path().to_string_lossy().as_ref()
    );
    assert_eq!(session["params"]["mcpServers"], serde_json::json!([]));
}

#[test]
fn acp_discovery_ignores_bounded_notifications_between_correlated_responses() {
    let workspace = TempDir::new().expect("workspace");
    let provider = executable(
        &workspace,
        r#"IFS= read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
IFS= read -r session
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"models":{"availableModels":[{"modelId":"current","name":"Current"}],"currentModelId":"current"}}}'"#,
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;

    let catalog = discover_models(
        CatalogProvider::Copilot,
        &discovery,
        &CancellationToken::new(),
    )
    .expect("ignore notification and read correlated response");

    assert_eq!(catalog.current_model_id(), Some("current"));
}

#[test]
fn acp_discovery_requires_acknowledgement_and_rejects_wrong_ids_and_requests() {
    let workspace = TempDir::new().expect("workspace");
    let marker = workspace.path().join("started");
    let never_started = executable(&workspace, &format!("touch '{}'\nexit 0", marker.display()));
    let mut discovery = options(&workspace);
    discovery.executable = Some(never_started);
    let error = discover_models(
        CatalogProvider::Gemini,
        &discovery,
        &CancellationToken::new(),
    )
    .expect_err("host acknowledgement required");
    assert!(error.to_string().contains("--allow-host-discovery"));
    assert!(!marker.exists());

    for response in [
        r#"{"jsonrpc":"2.0","id":9,"result":{"protocolVersion":1}}"#,
        r#"{"jsonrpc":"2.0","id":77,"method":"fs/read_text_file","params":{}}"#,
    ] {
        let provider = executable(
            &workspace,
            &format!(
                "IFS= read -r line || exit 1\nprintf '%s\\n' '{}'",
                response.replace('\'', "'\\''")
            ),
        );
        discovery.executable = Some(provider);
        discovery.allow_host_discovery = true;
        let error = discover_models(
            CatalogProvider::Gemini,
            &discovery,
            &CancellationToken::new(),
        )
        .expect_err("reject invalid ACP record");
        assert!(
            error.to_string().contains("response id")
                || error.to_string().contains("provider-to-client request")
        );
    }
}

#[test]
fn acp_discovery_reports_a_descendant_that_retains_stdout_after_group_cleanup() {
    let _guard = ACP_CLEANUP_TEST_LOCK.lock().expect("ACP cleanup test lock");
    let workspace = TempDir::new().expect("workspace");
    let escaped_pid_path = workspace.path().join("escaped.pid");
    let provider = executable(
        &workspace,
        &format!(
            r#"IFS= read -r initialize
printf '%s\n' '{{"jsonrpc":"2.0","id":0,"result":{{"protocolVersion":1}}}}'
IFS= read -r session
setsid sh -c 'printf "%s" "$$" > "{}"; exec /bin/sleep 2' &
while [ ! -s "{}" ]; do /bin/sleep 0.01; done
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"models":{{"availableModels":[{{"modelId":"model","name":"Model"}}],"currentModelId":"model"}}}}}}'
exec /bin/sleep 30"#,
            escaped_pid_path.display(),
            escaped_pid_path.display()
        ),
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;

    let error = discover_models(
        CatalogProvider::Copilot,
        &discovery,
        &CancellationToken::new(),
    )
    .expect_err("retained stdout must not look like successful cleanup");

    assert!(error.to_string().contains("output remained open"));
    let escaped_pid = fs::read_to_string(escaped_pid_path)
        .expect("escaped process PID")
        .parse::<i32>()
        .expect("numeric escaped process PID");
    terminate_test_process(escaped_pid);
}

#[test]
fn acp_cleanup_deadline_applies_while_a_descendant_keeps_writing() {
    let _guard = ACP_CLEANUP_TEST_LOCK.lock().expect("ACP cleanup test lock");
    let workspace = TempDir::new().expect("workspace");
    let escaped_pid_path = workspace.path().join("escaped-writer.pid");
    let provider = executable(
        &workspace,
        &format!(
            r#"IFS= read -r initialize
printf '%s\n' '{{"jsonrpc":"2.0","id":0,"result":{{"protocolVersion":1}}}}'
IFS= read -r session
            parent_pid=$$
            setsid sh -c 'printf "%s" "$$" > "{}"; while kill -0 "$1" 2>/dev/null; do /bin/sleep 0.01; done; while :; do printf x; done' sh "$parent_pid" &
            while [ ! -s "{}" ]; do /bin/sleep 0.01; done
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"models":{{"availableModels":[{{"modelId":"model","name":"Model"}}],"currentModelId":"model"}}}}}}'
exec /bin/sleep 30"#,
            escaped_pid_path.display(),
            escaped_pid_path.display()
        ),
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;
    let started = Instant::now();

    let error = discover_models(
        CatalogProvider::Copilot,
        &discovery,
        &CancellationToken::new(),
    )
    .expect_err("continuous retained output must remain bounded");

    assert!(error.to_string().contains("output remained open"));
    assert!(started.elapsed() < Duration::from_secs(3));
    let escaped_pid = fs::read_to_string(escaped_pid_path)
        .expect("escaped writer PID")
        .parse::<i32>()
        .expect("numeric escaped writer PID");
    terminate_test_process(escaped_pid);
}

#[test]
fn acp_timeout_terminates_and_reaps_the_process_group() {
    let _guard = ACP_CLEANUP_TEST_LOCK.lock().expect("ACP cleanup test lock");
    let workspace = TempDir::new().expect("workspace");
    let pid_path = workspace.path().join("provider.pid");
    let provider = executable(
        &workspace,
        &format!(
            "printf '%s' \"$$\" > '{}'\nIFS= read -r line\nexec /bin/sleep 30",
            pid_path.display()
        ),
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;
    discovery.timeout = Duration::from_millis(150);

    let error = discover_models(
        CatalogProvider::Copilot,
        &discovery,
        &CancellationToken::new(),
    )
    .expect_err("ACP timeout");
    assert!(error.to_string().contains("timed out"));

    let pid = fs::read_to_string(pid_path)
        .expect("provider PID")
        .parse::<i32>()
        .expect("numeric provider PID");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match kill(Pid::from_raw(pid), None) {
            Err(Errno::ESRCH) => break,
            _ if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => panic!("ACP provider remained after timeout: {result:?}"),
        }
    }
}

#[test]
fn acp_cancellation_terminates_and_reaps_the_process_group() {
    let _guard = ACP_CLEANUP_TEST_LOCK.lock().expect("ACP cleanup test lock");
    let workspace = TempDir::new().expect("workspace");
    let pid_path = workspace.path().join("provider.pid");
    let provider = executable(
        &workspace,
        &format!(
            "printf '%s' \"$$\" > '{}'\nIFS= read -r line\nexec /bin/sleep 30",
            pid_path.display()
        ),
    );
    let mut discovery = options(&workspace);
    discovery.executable = Some(provider);
    discovery.allow_host_discovery = true;
    let cancellation = CancellationToken::new();
    let cancellation_request = cancellation.clone();
    let marker = pid_path.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        cancellation_request.cancel();
    });

    let error = discover_models(CatalogProvider::Copilot, &discovery, &cancellation)
        .expect_err("ACP cancellation");
    assert!(error.to_string().contains("cancelled"));

    let pid = fs::read_to_string(pid_path)
        .expect("provider PID")
        .parse::<i32>()
        .expect("numeric provider PID");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match kill(Pid::from_raw(pid), None) {
            Err(Errno::ESRCH) => break,
            _ if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => panic!("ACP provider remained after cancellation: {result:?}"),
        }
    }
}

fn terminate_test_process(pid: i32) {
    let pid = Pid::from_raw(pid);
    let _ = kill(pid, nix::sys::signal::Signal::SIGKILL);
    thread::sleep(Duration::from_millis(20));
}
