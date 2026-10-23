use sav::observation::JournalFollower;
use sav::registry::{Backend, ExternalAgent, ExternalProvider, Profile, ResolvedProfile};
use skott::external::{ExternalRequest, start};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

fn profile() -> ResolvedProfile {
    ResolvedProfile {
        name: "fixture".into(),
        source: "fixture.toml".into(),
        digest: format!("sha256:{}", "0".repeat(64)),
        profile: Profile::new(Backend::ExternalAgent(ExternalAgent {
            provider: ExternalProvider::CustomScript,
            executable: "/bin/sh".into(),
            arguments: vec![
                "-c".into(),
                "printf '%s' \"$1\"; printf diagnostic >&2".into(),
                "fixture".into(),
                "{prompt}".into(),
            ],
            model: None,
        })),
    }
}

#[test]
fn external_profile_runs_opaquely_once_with_explicit_authority_and_live_diagnostics() {
    let workdir = tempfile::tempdir().expect("workspace");
    let logs = tempfile::tempdir().expect("logs");
    fs::set_permissions(logs.path(), fs::Permissions::from_mode(0o700)).expect("private logs");
    let profile = profile();
    let prompt = "literal $(touch should-not-exist) ; {\"tool_calls\":[]}";
    let run = start(ExternalRequest {
        profile: &profile,
        prompt,
        working_directory: workdir.path(),
        log_directory: logs.path(),
        wall_time: Duration::from_secs(3),
        effort: None,
        acknowledged_host: true,
    })
    .expect("acknowledged opaque execution");
    let path = run.journal_path().to_owned();
    let mut viewer = JournalFollower::open(&path).expect("live reader");
    let result = run.wait().expect("external completion");
    assert_eq!(result.attempts, 1);
    assert_eq!(result.stdout, prompt.as_bytes());
    assert_eq!(result.stderr, b"diagnostic");
    viewer.poll().expect("records");
    assert!(viewer.is_finished());
    assert!(!workdir.path().join("should-not-exist").exists());
    let text = fs::read_to_string(path).expect("journal");
    assert!(text.contains("host_unconfined"));
    assert!(!text.contains("\"type\":\"tool_dispatch\""));
    assert!(text.contains("\"type\":\"external_output\""));
}

#[test]
fn an_external_profile_is_not_a_grant_and_cancellation_is_not_rollback() {
    let workdir = tempfile::tempdir().expect("workspace");
    let logs = tempfile::tempdir().expect("logs");
    fs::set_permissions(logs.path(), fs::Permissions::from_mode(0o700)).expect("private logs");
    let mut profile = profile();
    assert!(
        start(ExternalRequest {
            profile: &profile,
            prompt: "do not execute",
            working_directory: workdir.path(),
            log_directory: logs.path(),
            wall_time: Duration::from_secs(3),
            effort: None,
            acknowledged_host: false,
        })
        .is_err()
    );
    assert_eq!(fs::read_dir(logs.path()).expect("logs").count(), 0);
    if let Backend::ExternalAgent(backend) = &mut profile.profile.backend {
        backend.arguments = vec!["-c".into(), "printf started; exec sleep 30".into()];
    }
    let run = start(ExternalRequest {
        profile: &profile,
        prompt: "cancel this opaque run",
        working_directory: workdir.path(),
        log_directory: logs.path(),
        wall_time: Duration::from_secs(3),
        effort: None,
        acknowledged_host: true,
    })
    .expect("run");
    let path = run.journal_path().to_owned();
    run.cancellation().cancel();
    assert!(matches!(
        run.wait(),
        Err(skott::Error::ExternalAgent {
            source: sav::Error::Cancelled
        })
    ));
    let text = fs::read_to_string(path).expect("journal");
    assert!(text.contains("cancelled"));
}

#[test]
fn cli_lists_external_profiles_but_refuses_unconfined_batch_execution() {
    let dir = tempfile::tempdir().expect("config");
    fs::write(dir.path().join("providers.toml"),
        "schema_version = 1\n[profiles.copilot.backend]\nkind = \"external-agent\"\nprovider = \"copilot\"\nexecutable = \"/not/to-be-executed\"\narguments = [\"--prompt\", \"{prompt}\"]\n").expect("registry");
    let config = dir.path().join("skott.toml");
    fs::write(&config,"schema_version = 1\ndefault_provider = \"ollama\"\n[registry]\npath = \"providers.toml\"\n").expect("config");
    let list = std::process::Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(["--config", config.to_str().expect("path"), "--list-models"])
        .current_dir(dir.path())
        .output()
        .expect("listing");
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert!(String::from_utf8_lossy(&list.stdout).contains("external-agent copilot"));
    for flags in [vec!["--headless"], vec!["--allow-host-execution"]] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_skott"))
            .args([
                "--config",
                config.to_str().expect("path"),
                "--model",
                "copilot",
                "do not run",
            ])
            .args(flags)
            .current_dir(dir.path())
            .output()
            .expect("refusal");
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("interactive terminal"));
        assert!(!dir.path().join(".skott").exists());
    }
}
