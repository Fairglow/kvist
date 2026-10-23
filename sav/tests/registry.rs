use std::{fs, path::Path};

use sav::registry::{Backend, Registry, RegistryPaths};

fn native(model: &str) -> String {
    format!(
        "schema_version = 1\n[profiles.coder.backend]\nkind = \"native\"\n\
         provider = \"ollama\"\nmodel = \"{model}\"\n"
    )
}

#[test]
fn registry_resolution_is_ordered_pinned_and_does_not_probe() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let user = directory.path().join("user.toml");
    let project = directory.path().join("project.toml");
    let explicit = directory.path().join("explicit.toml");
    fs::write(&user, native("user")).unwrap();
    fs::write(&project, native("project")).unwrap();
    fs::write(&explicit, native("explicit")).unwrap();
    let paths = RegistryPaths {
        explicit: Some(explicit.clone()),
        project: Some(project),
        user: Some(user),
    };
    let registry = paths.load().unwrap();
    let resolved = registry.resolve("coder").unwrap();
    let Backend::Native(model) = &resolved.profile.backend else {
        panic!("expected native");
    };
    assert_eq!(model.model, "explicit");
    assert_eq!(resolved.source, explicit);
    assert!(resolved.digest.starts_with("sha256:"));
    fs::write(&explicit, native("changed")).unwrap();
    assert_eq!(registry.resolve("coder").unwrap(), resolved);
}

#[test]
fn rejects_unknown_fields_credentials_bounds_and_retired_command_schema() {
    for contents in [
        native("model") + "typo = true\n",
        native("") ,
        native("model").replace("provider = \"ollama\"", "provider = \"copilot\""),
        native("model") + "base_url = \"http://token@127.0.0.1:11434\"\n",
        native("model") + "base_url = \"http://localhost:11434\"\n",
        native("model") + "deadline_secs = 0\n",
        native("model") + "context_limit = 1\n",
        "schema_version = 1\n[[profiles]]\nname = \"old\"\nprovider = \"ollama\"\ncommand = \"false\"\n".to_owned(),
    ] {
        assert!(Registry::parse(Path::new("fixture"), &contents).is_err(), "{contents}");
    }
}

#[test]
fn external_identity_is_structured_and_resolution_never_executes() {
    let contents = r#"schema_version = 1
[profiles.external]
advertised = ["tools"]
tested = []
policy_enabled = []
[profiles.external.backend]
kind = "external-agent"
provider = "copilot"
executable = "/does/not/exist/copilot"
arguments = ["--prompt", "{prompt}"]
model = "dynamic-model"
"#;
    let registry = Registry::parse(Path::new("fixture"), contents).unwrap();
    let profile = registry.resolve("external").unwrap();
    assert!(matches!(
        sav::qualify_registry_profile(&profile.profile, Path::new("."), false),
        Err(sav::Error::HostExecutionNotAcknowledged)
    ));
    assert_eq!(profile.profile.advertised, ["tools"]);
    assert!(profile.profile.tested.is_empty());
    assert!(profile.native().is_err());
    let Backend::ExternalAgent(agent) = profile.profile.backend else {
        panic!()
    };
    assert_eq!(agent.executable, "/does/not/exist/copilot");
}

#[test]
fn disabled_profiles_and_invalid_present_sources_fail_closed() {
    let disabled = native("model").replace(
        "[profiles.coder.backend]",
        "[profiles.coder]\nenabled = false\n[profiles.coder.backend]",
    );
    assert!(
        Registry::parse(Path::new("fixture"), &disabled)
            .unwrap()
            .resolve("coder")
            .is_err()
    );
    let directory = tempfile::tempdir_in(".").unwrap();
    let bad = directory.path().join("bad.toml");
    fs::write(&bad, "not valid TOML").unwrap();
    assert!(
        RegistryPaths {
            explicit: None,
            project: Some(bad),
            user: None
        }
        .load()
        .is_err()
    );
}

#[test]
fn registry_upsert_roundtrips_and_refuses_symlinks() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let path = directory.path().join("registry.toml");
    let source = Registry::parse(Path::new("fixture"), &native("model")).unwrap();
    let profile = source.resolve("coder").unwrap().profile;
    sav::registry::upsert(&path, "coder", &profile).unwrap();
    assert_eq!(
        Registry::load(&path)
            .unwrap()
            .resolve("coder")
            .unwrap()
            .profile,
        profile
    );
    let link = directory.path().join("link.toml");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(sav::registry::upsert(&link, "coder", &profile).is_err());
}

#[test]
fn structured_argv_never_reinterprets_prompt_placeholders_or_shell_syntax() {
    use sav::registry::{ExternalAgent, ExternalProvider};
    let agent = ExternalAgent {
        provider: ExternalProvider::CustomScript,
        executable: "/not/executed".into(),
        arguments: vec![
            "{prompt}".into(),
            "{model}".into(),
            "{context_files}".into(),
        ],
        model: Some("model".into()),
    };
    let command = agent
        .command_spec(
            "{model}; $(not-executed)",
            &["file with spaces".into()],
            Path::new("."),
            None,
        )
        .unwrap();
    assert_eq!(
        command.arguments,
        ["{model}; $(not-executed)", "model", "file with spaces"]
    );
    assert!(
        agent
            .command_spec("NUL\0", &[], Path::new("."), None)
            .is_err()
    );
}

#[test]
fn rejects_inline_credentials_and_independent_registry_bounds() {
    for credential in [
        "--token=private",
        "Authorization: Bearer private",
        "--api-key",
    ] {
        let contents = format!(
            "schema_version = 1\n[profiles.agent.backend]\nkind = \"external-agent\"\nprovider = \"copilot\"\nexecutable = \"copilot\"\narguments = [\"{credential}\"]\n"
        );
        assert!(Registry::parse(Path::new("fixture"), &contents).is_err());
    }
    assert!(Registry::parse(Path::new("fixture"), &" ".repeat(65_537)).is_err());
    let mut contents = "schema_version = 1\n".to_owned();
    for index in 0..129 {
        contents.push_str(&format!(
            "[profiles.model{index}.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"dynamic\"\n"
        ));
    }
    assert!(Registry::parse(Path::new("fixture"), &contents).is_err());
}

#[test]
fn explicit_source_is_required_and_whole_profiles_override_not_fields() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let user = directory.path().join("user.toml");
    let project = directory.path().join("project.toml");
    fs::write(&user, native("user") + "context_limit = 8192\n").unwrap();
    fs::write(&project, native("project")).unwrap();
    let mut paths = RegistryPaths {
        explicit: None,
        project: Some(project),
        user: Some(user),
    };
    let selected = paths.load().unwrap().resolve("coder").unwrap();
    assert_eq!(selected.native().unwrap().context_limit, None);
    paths.explicit = Some(directory.path().join("missing.toml"));
    assert!(paths.load().is_err());
}

#[test]
fn native_cli_executes_the_selected_structured_profile_and_inspection_is_read_only() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        thread,
    };
    let directory = tempfile::tempdir_in(".").unwrap();
    let registry = directory.path().join("registry.toml");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    fs::write(
        &registry,
        native("pinned-model").replace("provider = \"ollama\"", "provider = \"llama-server\"")
            + &format!("base_url = \"{endpoint}\"\n"),
    )
    .unwrap();
    let original = fs::read_to_string(&registry).unwrap();
    let inspected = Command::new(env!("CARGO_BIN_EXE_sav-run"))
        .args(["registry", "--registry"])
        .arg(&registry)
        .output()
        .unwrap();
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    assert!(String::from_utf8_lossy(&inspected.stdout).contains("pinned-model"));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            headers.push(byte[0]);
            assert!(headers.len() < 8192);
        }
        let headers = String::from_utf8(headers).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        let mut request = vec![0; length];
        stream.read_exact(&mut request).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(request["model"], "pinned-model");
        let body = r#"{"choices":[{"message":{"content":"OK"},"finish_reason":"stop"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_sav-run"))
        .args(["model", "--profile", "coder", "--registry"])
        .arg(&registry)
        .arg("hello")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"OK");
    server.join().unwrap();
    assert_eq!(fs::read_to_string(registry).unwrap(), original);
}
