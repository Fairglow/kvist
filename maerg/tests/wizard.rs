use std::{
    fs,
    io::{Cursor, Read, Write},
    net::{Shutdown, TcpListener},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use kvist::{config, config::Role, wizard};
use sav::registry::{Backend, ExternalAgent, ExternalProvider, Profile, Registry};

fn project() -> tempfile::TempDir {
    tempfile::tempdir_in(".").unwrap()
}

fn executable(project: &Path, body: &str) -> PathBuf {
    let path = project.canonicalize().unwrap().join("provider.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn command(project: &Path, arguments: &[&str]) -> Command {
    let project = project.canonicalize().unwrap();
    if !project.join("kvist.toml").exists() {
        fs::write(
            project.join("kvist.toml"),
            "schema_version = 1\ncomponent_root = \"src\"\n",
        )
        .unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_kvist"));
    command
        .current_dir(&project)
        .args(arguments)
        .env("HOME", &project)
        .env("XDG_CONFIG_HOME", project.join("user-config"))
        .env("KVIST_CONFIG_PATH", project.join("user-agent.toml"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(project: &Path, arguments: &[&str], input: &str) -> Output {
    let mut child = command(project, arguments).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn registry_path(project: &Path) -> PathBuf {
    project.join("user-config/sav/registry.toml")
}

fn shared(project: &Path) -> PathBuf {
    let path = project.canonicalize().unwrap().join("providers.toml");
    sav::registry::upsert(
        &path,
        "shared",
        &Profile::new(Backend::ExternalAgent(ExternalAgent {
            provider: ExternalProvider::CustomScript,
            executable: "/never/probed".into(),
            arguments: vec!["--prompt".into(), "{prompt}".into()],
            model: None,
        })),
    )
    .unwrap();
    path
}

fn bind(project: &Path) -> PathBuf {
    let registry = shared(project);
    wizard::run_wizard_with_profile_config(
        &mut Cursor::new("shared\n1\n\n"),
        &mut Vec::new(),
        project,
        &registry,
    )
    .unwrap();
    registry
}

#[test]
fn wizard_binds_shared_profiles_without_copying_or_probing() {
    let project = project();
    let registry = bind(project.path());
    let contents = fs::read_to_string(project.path().join("kvist.toml")).unwrap();
    assert!(contents.contains("[agent.registry]"));
    assert!(contents.contains("[agent.roles.developer]"));
    assert!(!contents.contains("command"));
    assert!(!contents.contains("executable"));
    assert!(!contents.contains("[agent.profiles"));
    let config = config::load(project.path()).unwrap();
    let selected = config.agent.resolve_role_profile(Role::Developer).unwrap();
    assert_eq!(selected.name, "shared");
    assert_eq!(selected.source, registry);
}

#[test]
fn wizard_qualifies_external_argv_with_fixed_prompt_and_discloses_authority() {
    let project = project();
    let marker = project.path().canonicalize().unwrap().join("prompt");
    let provider = executable(
        project.path(),
        &format!("printf '%s' \"$2\" > '{}'", marker.display()),
    );
    let output = run(
        project.path(),
        &["agent", "setup"],
        &format!("6\n{}\nqualified\n1\n\n", provider.display()),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("current host permissions"));
    assert!(stdout.contains("billing"));
    assert!(stdout.contains("Qualification succeeded"));
    assert_eq!(fs::read_to_string(marker).unwrap(), sav::SETUP_TEST_PROMPT);
    assert!(
        Registry::load(&registry_path(project.path()))
            .unwrap()
            .resolve("qualified")
            .is_ok()
    );
}

#[test]
fn failed_qualification_refuses_persistence_unless_forced() {
    let project = project();
    let provider = executable(project.path(), "exit 7");
    let input = format!("6\n{}\nfailing\n1\n\n", provider.display());
    let failed = run(project.path(), &["agent", "setup"], &input);
    assert!(!failed.status.success());
    assert!(!registry_path(project.path()).exists());
    assert!(
        !fs::read_to_string(project.path().join("kvist.toml"))
            .unwrap()
            .contains("[agent.registry]")
    );
    let forced = run(project.path(), &["agent", "setup", "--force"], &input);
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    let profile = Registry::load(&registry_path(project.path()))
        .unwrap()
        .resolve("failing")
        .unwrap();
    assert!(profile.profile.tested.is_empty());
    assert!(String::from_utf8_lossy(&forced.stdout).contains("--force saves this unqualified"));
}

#[test]
fn json_agent_setup_keeps_stdout_machine_readable() {
    let project = project();
    let provider = executable(
        project.path(),
        "printf 'private qualification stdout'; printf 'private stderr' >&2",
    );
    let output = run(
        project.path(),
        &["--json", "agent", "setup"],
        &format!("6\n{}\njson-model\n1\n\n", provider.display()),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"status\":\"success\",\"command\":\"agent-setup\",\"message\":\"agent setup wizard complete\"}\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Kvist Agent Setup Wizard"));
    assert!(!stderr.contains("private qualification stdout"));
    assert!(!stderr.contains("private stderr"));
}

#[test]
fn wizard_reuses_ollama_catalog_and_qualifies_native_transport() {
    let project = project();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for (route, body) in [
            (
                "GET /api/tags ",
                r#"{"models":[{"name":"small"},{"name":"selected:model"}]}"#,
            ),
            (
                "POST /api/chat ",
                r#"{"model":"selected:model","message":{"role":"assistant","content":"OK"},"done":true,"done_reason":"stop"}"#,
            ),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 8192];
            let count = stream.read(&mut bytes).unwrap();
            assert!(String::from_utf8_lossy(&bytes[..count]).starts_with(route));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            // Close the write half so the client sees EOF and reads the body to
            // completion instead of a connection reset. The mock drops the
            // socket here, which on some runs tears down the connection before
            // the client finishes reading, producing a spurious ECONNRESET.
            let _ = stream.shutdown(Shutdown::Both);
        }
    });
    let output = run(
        project.path(),
        &["agent", "setup"],
        &format!("3\n{endpoint}\n2\nnative\n1\n\n"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
    let loaded = config::load(project.path()).unwrap();
    let native = loaded
        .agent
        .resolve_role_profile(Role::Developer)
        .unwrap()
        .native()
        .unwrap();
    assert_eq!(native.model, "selected:model");
    assert_eq!(native.base_url, endpoint);
}

#[test]
fn force_cannot_persist_cancelled_qualification() {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    let project = project();
    let marker = project.path().canonicalize().unwrap().join("started");
    let provider = executable(
        project.path(),
        &format!("touch '{}'; exec sleep 30", marker.display()),
    );
    let mut child = command(project.path(), &["agent", "setup", "--force"])
        .spawn()
        .unwrap();
    write!(
        child.stdin.take().unwrap(),
        "6\n{}\ncancelled\n",
        provider.display()
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "qualification did not start");
    kill(
        Pid::from_raw(child.id().try_into().unwrap()),
        Signal::SIGINT,
    )
    .unwrap();
    assert!(!child.wait_with_output().unwrap().status.success());
    assert!(!registry_path(project.path()).exists());
    assert!(
        !fs::read_to_string(project.path().join("kvist.toml"))
            .unwrap()
            .contains("[agent.registry]")
    );
}

#[test]
fn binding_refuses_invalid_or_mixed_existing_configuration_without_modifying_it() {
    for original in [
        "schema_version = 77\ncomponent_root = \"src\"\n",
        "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.local]\nprovider = \"custom\"\n",
    ] {
        let project = project();
        let registry = shared(project.path());
        let config = project.path().join("kvist.toml");
        fs::write(&config, original).unwrap();
        assert!(
            wizard::run_wizard_with_profile_config(
                &mut Cursor::new("shared\n1\n\n"),
                &mut Vec::new(),
                project.path(),
                &registry,
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(config).unwrap(), original);
    }
}

#[test]
fn role_assignment_clear_and_removal_do_not_mutate_shared_definitions() {
    let project = project();
    let registry = bind(project.path());
    let original = fs::read_to_string(&registry).unwrap();
    let path = project.path().join("kvist.toml");
    wizard::set_role_model(&path, project.path(), true, "architect", "shared").unwrap();
    assert!(
        wizard::list_roles(project.path())
            .unwrap()
            .contains("shared")
    );
    assert!(
        wizard::list_models(project.path())
            .unwrap()
            .contains("external-agent custom-script")
    );
    wizard::clear_role(&path, project.path(), true, Some("architect"), false).unwrap();
    wizard::remove_model(&path, project.path(), true, "shared").unwrap();
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("profile = \"shared\"")
    );
    assert_eq!(fs::read_to_string(registry).unwrap(), original);
}

#[test]
fn remove_all_removes_bindings_not_the_shared_registry() {
    let project = project();
    let registry = bind(project.path());
    let original = fs::read_to_string(&registry).unwrap();
    wizard::remove_all_models(&project.path().join("kvist.toml"), project.path(), true).unwrap();
    assert_eq!(fs::read_to_string(registry).unwrap(), original);
    assert!(
        wizard::list_models(project.path())
            .unwrap()
            .contains("no models configured")
    );
}

#[test]
fn cancelled_binding_and_declined_check_have_no_effects() {
    let project = project();
    let registry = shared(project.path());
    assert!(matches!(
        wizard::run_wizard_with_profile_config(
            &mut Cursor::new("shared\nc\n"),
            &mut Vec::new(),
            project.path(),
            &registry,
        ),
        Err(kvist::KvistError::AgentSetupCancelled)
    ));
    assert!(!project.path().join("kvist.toml").exists());
    bind(project.path());
    let original = fs::read_to_string(project.path().join("kvist.toml")).unwrap();
    wizard::check_agents(
        &mut Cursor::new("n\n"),
        &mut Vec::new(),
        project.path(),
        false,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(project.path().join("kvist.toml")).unwrap(),
        original
    );
}

#[test]
fn global_setup_stores_only_a_shared_reference() {
    let project = project();
    let provider = executable(project.path(), "exit 0");
    let output = run(
        project.path(),
        &["agent", "setup"],
        &format!("6\n{}\nglobal\n2\narchitect\n", provider.display()),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(project.path().join("user-agent.toml")).unwrap();
    assert!(contents.contains("[agent.roles.architect]"));
    assert!(contents.contains("profile = \"global\""));
    assert!(!contents.contains("executable"));
    assert_eq!(
        fs::read_to_string(project.path().join("kvist.toml")).unwrap(),
        "schema_version = 1\ncomponent_root = \"src\"\n"
    );
}

#[test]
fn setup_can_reuse_a_shared_profile_without_repeating_host_probes() {
    let project = project();
    let provider = executable(project.path(), "exit 0");
    let created = run(
        project.path(),
        &["agent", "setup"],
        &format!("6\n{}\nreusable\n1\n\n", provider.display()),
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    executable(project.path(), "exit 7");
    let reused = run(
        project.path(),
        &["agent", "setup"],
        "reusable\n1\narchitect\n",
    );
    assert!(
        reused.status.success(),
        "{}",
        String::from_utf8_lossy(&reused.stderr)
    );
    assert!(!String::from_utf8_lossy(&reused.stdout).contains("Qualifying"));
    let config = config::load(project.path()).unwrap();
    assert_eq!(
        config
            .agent
            .architect
            .resolved_profile
            .as_ref()
            .unwrap()
            .name,
        "reusable"
    );
}

#[test]
fn discovery_configuration_is_retained_as_policy_not_an_endpoint_authorization() {
    let project = project();
    fs::write(project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n[agent.discovery]\nollama_url = \"http://192.168.1.100:11434\"\ntimeout_seconds = 10\nallow_host_discovery = true\n").unwrap();
    let config = config::load(project.path()).unwrap();
    assert_eq!(
        config.agent.discovery.ollama_url.as_deref(),
        Some("http://192.168.1.100:11434")
    );
    assert_eq!(config.agent.discovery.timeout_seconds, Some(10));
}
