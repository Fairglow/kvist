//! `agent role list` presentation and `agent check` live verification.

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Output, Stdio},
};

use tempfile::TempDir;

use kvist::init::initialize;

fn run_kvist(project: &TempDir, arguments: &[&str]) -> Output {
    run_kvist_with_stdin(project, arguments, "")
}

fn run_kvist_with_stdin(project: &TempDir, arguments: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kvist");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin.as_bytes())
        .expect("write child stdin");
    child.wait_with_output().expect("collect kvist output")
}

fn initialize_project(project: &TempDir) {
    initialize(project.path()).expect("initialize project");
}

fn set_config(project: &TempDir, contents: &str) {
    fs::write(project.path().join("kvist.toml"), contents).expect("write kvist.toml");
}

fn config_contents(project: &TempDir) -> String {
    fs::read_to_string(project.path().join("kvist.toml")).expect("read kvist.toml")
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn agent_role_list_presents_only_predefined_roles() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    set_config(
        &project,
        r#"schema_version = 1
component_root = "src"

[agent.profiles.qwen-coder]
provider = "localhost"
model = "qwen3:8b"
command = "/usr/bin/echo '{prompt}'"

[agent.profiles.llama-local]
provider = "localhost"
model = "llama-3-8b"
command = "/usr/bin/echo '{prompt}'"

[agent.roles.developer]
profile = "qwen-coder"
"#,
    );

    let output = run_kvist(&project, &["agent", "role", "list"]);
    let text = combined(&output);
    assert!(output.status.success(), "{text}");

    let lines: Vec<&str> = text.lines().collect();
    let header = lines
        .iter()
        .position(|line| line.contains("Assigned Model Profile"))
        .expect("role table header");
    // The line after the header is the table separator; the role rows follow it.
    let role_rows = &lines[header + 2..header + 5];
    for row in role_rows {
        let name = row.trim_start_matches(['│', ' ']);
        let role = name.get(..20).unwrap_or(name).trim_end();
        assert!(
            matches!(role, "architect" | "developer" | "security-reviewer"),
            "role column presents `{role}`: {text}"
        );
    }
    // The assigned profile and the available profiles remain visible.
    assert!(role_rows[1].contains("qwen-coder"), "{text}");
    assert!(text.contains("llama-local"), "{text}");
}

#[test]
fn agent_check_reports_healthy_profiles_without_changes() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    let before = r#"schema_version = 1
component_root = "src"

[agent.providers.localhost]
type = "llama-server"
base_url = "http://127.0.0.1:9931"

[agent.profiles.echo-one]
provider = "localhost"
command = "/usr/bin/echo '{prompt}'"

[agent.profiles.echo-two]
provider = "localhost"
command = "/usr/bin/echo '{prompt}'"
"#;
    set_config(&project, before);

    let output = run_kvist_with_stdin(&project, &["agent", "check"], "y\n");
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("echo-one"), "{text}");
    assert!(text.contains("echo-two"), "{text}");
    assert!(text.contains("ok"), "{text}");
    assert_eq!(config_contents(&project), before);
}

#[test]
fn agent_check_removes_a_failed_profile_on_choice() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    set_config(
        &project,
        r#"schema_version = 1
component_root = "src"

[agent.profiles.broken]
provider = "localhost"
command = "/usr/bin/false '{prompt}'"

[agent.roles.developer]
profile = "broken"
"#,
    );

    let output = run_kvist_with_stdin(&project, &["agent", "check"], "y\n1\n");
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("broken"), "{text}");
    let after = config_contents(&project);
    assert!(!after.contains("broken"), "{after}");
}

#[test]
fn agent_check_ignores_a_failed_profile_on_choice() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    let before = r#"schema_version = 1
component_root = "src"

[agent.profiles.broken]
provider = "localhost"
command = "/usr/bin/false '{prompt}'"
"#;
    set_config(&project, before);

    let output = run_kvist_with_stdin(&project, &["agent", "check"], "y\n2\n");
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.to_lowercase().contains("ignored"), "{text}");
    assert_eq!(config_contents(&project), before);
}

#[test]
fn agent_check_refusal_executes_nothing_and_changes_nothing() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    let script = project.path().join("marker.sh");
    fs::write(&script, "#!/bin/sh\ntouch marker-output\n").expect("write marker script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("make executable");
    let before = format!(
        r#"schema_version = 1
component_root = "src"

[agent.profiles.marker]
provider = "localhost"
command = "{script} '{{prompt}}'"
"#,
        script = script.display()
    );
    set_config(&project, &before);

    let output = run_kvist_with_stdin(&project, &["agent", "check"], "n\n");
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("cancelled"), "{text}");
    assert!(!project.path().join("marker-output").exists());
    assert_eq!(config_contents(&project), before);
}

#[test]
fn agent_check_cancels_at_the_failure_prompt_without_changes() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    let before = r#"schema_version = 1
component_root = "src"

[agent.profiles.broken]
provider = "localhost"
command = "/usr/bin/false '{prompt}'"
"#;
    set_config(&project, before);

    let output = run_kvist_with_stdin(&project, &["agent", "check"], "y\nq\n");
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("cancelled"), "{text}");
    assert_eq!(config_contents(&project), before);
}

#[test]
fn agent_check_without_profiles_reports_nothing_to_check() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);

    let output = run_kvist(&project, &["agent", "check"]);
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("nothing to check"), "{text}");
}

#[test]
fn agent_check_global_scope_uses_the_user_configuration() {
    let project = TempDir::new().expect("project");
    initialize_project(&project);
    let global = TempDir::new().expect("global");
    let global_config = global.path().join("config.toml");
    let before = r#"[agent.providers.localhost]
type = "llama-server"
base_url = "http://127.0.0.1:9931"

[agent.profiles.echo-global]
provider = "localhost"
command = "/usr/bin/echo '{prompt}'"
"#;
    fs::write(&global_config, before).expect("write global config");

    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["agent", "check", "--global"])
        .current_dir(project.path())
        .env("KVIST_CONFIG_PATH", &global_config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kvist");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(b"y\n")
        .expect("write child stdin");
    let output = child.wait_with_output().expect("collect kvist output");
    let text = combined(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("echo-global"), "{text}");
    assert!(text.contains("ok"), "{text}");
    assert_eq!(
        fs::read_to_string(&global_config).expect("read global config"),
        before
    );
}
