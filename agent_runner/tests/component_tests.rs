// Component tests for the agent-runner building blocks: configuration loading
// and validation, the tool registry and command policy, sandbox request
// construction, the error surface, and the CLI helpers. These exercise the
// public library API directly, so they document and guard the exact contract
// the terminal UI and the loop depend on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_runtime::{CancellationToken, ToolIntent};
use kvist_sandbox_runner::protocol::{NetworkMode, Phase};
use serde_json::json;
use tempfile::tempdir;

use agent_runner::sandbox;
use agent_runner::{
    Config, DEFAULT_WRITE_ROOT, Error, ExecContext, Model, ModelProvider, SandboxExecutor,
    SandboxPaths, ToolExecutor, ToolPolicy, ToolProfile, ToolRegistry, describe_tool_call,
    parse_effort, resolve_config_path,
};

const VALID_CONFIG: &str = r#"
schema_version = 1
working_directory = "%WD%"
default_model = "local"
default_thinking_effort = "medium"
[[models]]
id = "local"
provider = "llama-server"
base_url = "http://127.0.0.1:9931"
model = "qwen2.5-14b"
deadline_secs = 120
[[models]]
id = "ollama"
provider = "ollama"
base_url = "http://127.0.0.1:11434"
model = "qwen2.5"
deadline_secs = 120
[sandbox]
runner = "/usr/local/bin/kvist-sandbox-runner"
backend = "/usr/bin/bwrap"
[tool_policy]
shell_deny_substrings = []
shell_deny_prefixes = []
"#;

fn write_config(dir: &tempfile::TempDir, contents: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, contents).expect("write config");
    path
}

fn valid_dir(workdir: Option<&Path>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempdir().expect("temp dir");
    let default_wd = match workdir {
        Some(wd) => wd.to_path_buf(),
        None => dir.path().to_path_buf(),
    };
    let contents = VALID_CONFIG.replace("%WD%", default_wd.to_str().unwrap());
    let path = write_config(&dir, &contents);
    (dir, path)
}

#[test]
fn loads_a_valid_configuration() {
    let (_dir, path) = valid_dir(None);
    let cfg = Config::load(&path).expect("config loads");
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.models.len(), 2);
    assert_eq!(cfg.default_model, "local");
    assert!(cfg.model("local").is_some());
    assert!(cfg.model("nope").is_none());
}

#[test]
fn rejects_wrong_schema_version() {
    let (dir, path) = valid_dir(None);
    let contents = std::fs::read_to_string(&path).unwrap();
    let contents = contents.replace("schema_version = 1", "schema_version = 2");
    write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("wrong schema version");
    assert!(matches!(err, Error::Config { .. }));
    assert!(err.describe().to_lowercase().contains("schema_version"));
}

#[test]
fn rejects_unknown_default_model() {
    let (dir, path) = valid_dir(None);
    let contents = std::fs::read_to_string(&path).unwrap();
    let contents = contents.replace("default_model = \"local\"", "default_model = \"missing\"");
    write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("unknown default model");
    assert!(matches!(err, Error::ModelNotFound { .. }));
}

#[test]
fn rejects_invalid_default_effort() {
    let (dir, path) = valid_dir(None);
    let contents = std::fs::read_to_string(&path).unwrap();
    let contents = contents.replace(
        "default_thinking_effort = \"medium\"",
        "default_thinking_effort = \"gigantic\"",
    );
    write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("invalid effort");
    assert!(matches!(err, Error::Config { .. }));
}

#[test]
fn rejects_missing_working_directory() {
    let dir = tempdir().expect("temp dir");
    let path = write_config(
        &dir,
        &VALID_CONFIG.replace("%WD%", dir.path().join("does-not-exist").to_str().unwrap()),
    );
    let err = Config::load(&path).expect_err("missing working directory");
    assert!(matches!(err, Error::Config { .. }));
}

#[test]
fn rejects_duplicate_model_ids() {
    let (dir, path) = valid_dir(None);
    let contents = std::fs::read_to_string(&path).unwrap();
    let contents = contents.replace("id = \"ollama\"", "id = \"local\"");
    write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("duplicate model id");
    assert!(matches!(err, Error::Config { .. }));
}

#[test]
fn rejects_out_of_bound_deadline() {
    let (dir, path) = valid_dir(None);
    let contents = std::fs::read_to_string(&path).unwrap();
    let contents = contents.replace("deadline_secs = 120", "deadline_secs = 99999");
    write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("deadline too large");
    assert!(matches!(err, Error::Config { .. }));
}

#[test]
fn cadence_timeout_secs_defaults_when_omitted() {
    let (_dir, path) = valid_dir(None);
    let cfg = Config::load(&path).expect("config loads");
    // `cadence_timeout_secs` is optional and defaults to 30, keeping the
    // inter-token watchdog enabled without an explicit value.
    assert_eq!(cfg.model("local").expect("local").cadence_timeout_secs, 30);
}

#[test]
fn rejects_out_of_bound_cadence() {
    let (dir, path) = valid_dir(None);
    let contents = std::fs::read_to_string(&path).unwrap();
    // A cadence value above the deadline bound is meaningless and rejected.
    let contents = contents.replace("deadline_secs = 120", "cadence_timeout_secs = 99999");
    write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("cadence too large");
    assert!(matches!(err, Error::Config { .. }));
}

#[test]
fn rejects_unknown_top_level_fields() {
    let dir = tempdir().expect("temp dir");
    let contents = VALID_CONFIG.replace("schema_version = 1", "schema_version = 1\nnope = 1");
    let path = write_config(&dir, &contents);
    let err = Config::load(&path).expect_err("unknown field");
    assert!(matches!(err, Error::Config { .. }));
}

#[test]
fn from_parts_rejects_unknown_model() {
    let model = Model {
        id: "local".to_owned(),
        provider: ModelProvider::LlamaServer,
        base_url: "http://127.0.0.1:9931".to_owned(),
        model: "m".to_owned(),
        deadline_secs: 120,
        max_attempts: 3,
        retry_base_delay_secs: 2,
        retry_max_delay_secs: 30,
        cadence_timeout_secs: 30,
    };
    let err = Config::from_parts(
        tempdir().unwrap().path().to_path_buf(),
        "missing",
        vec![model],
        ToolPolicy::minimum(),
    )
    .expect_err("unknown default model");
    assert!(matches!(err, Error::ModelNotFound { .. }));
}

fn registry() -> ToolRegistry {
    ToolRegistry::new(ToolPolicy::minimum())
}

fn tool_intent(name: &str, arguments: serde_json::Value) -> ToolIntent {
    ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: name.to_owned(),
        arguments,
    }
}

#[test]
fn tool_definitions_have_stable_order_and_names() {
    let defs = registry().tool_definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["shell", "read_file", "write_file", "list_dir"]);
}

#[test]
fn shell_render_uses_absolute_bash_argv() {
    let reg = registry();
    let rendered = reg
        .render(
            &tool_intent("shell", json!({ "command": "echo hi" })),
            &ExecContext::new("/tmp/work", "call-1"),
        )
        .expect("shell renders");
    assert_eq!(rendered.argv[0], "/usr/bin/bash");
    assert!(rendered.argv.contains(&"-c".to_owned()));
    assert!(rendered.argv.contains(&"echo hi".to_owned()));
    assert!(!rendered.summary.is_empty());
}

#[test]
fn render_rejects_unknown_tool() {
    let reg = registry();
    let err = reg
        .render(
            &tool_intent("exec_file", json!({})),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect_err("unknown tool");
    assert!(matches!(err, Error::ToolRender { .. }));
}

#[test]
fn write_file_outside_write_root_is_rejected() {
    let reg = registry();
    let err = reg
        .render(
            &tool_intent(
                "write_file",
                json!({ "path": "/etc/passwd", "content": "x" }),
            ),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect_err("write outside root");
    assert!(matches!(err, Error::ToolPolicy { .. }));
}

#[test]
fn write_file_inside_write_root_is_allowed() {
    let reg = registry();
    let rendered = reg
        .render(
            &tool_intent(
                "write_file",
                json!({ "path": "/workspace/main.txt", "content": "x" }),
            ),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect("write inside root renders");
    assert!(rendered.staged_write.is_some());
}

#[test]
fn write_file_stages_under_the_working_directory_not_the_root() {
    // Regression: the host staging path reused the sandbox form (`/.agent-writes`,
    // leading slash) joined onto the workdir, which `PathBuf::join` treats as
    // absolute and replaces with the filesystem root, so staging failed with
    // permission denied at `/`. The host path must stay relative so it joins
    // under the working directory.
    let reg = registry();
    let rendered = reg
        .render(
            &tool_intent(
                "write_file",
                json!({ "path": "/workspace/main.txt", "content": "x" }),
            ),
            &ExecContext::new("/tmp/work", "call-1"),
        )
        .expect("write inside root renders");
    let staged = rendered.staged_write.expect("write stages a host file");
    assert_eq!(
        staged.host_path,
        PathBuf::from("/tmp/work/.agent-writes/call-1"),
        "host staging stays under the working directory"
    );
    assert!(
        staged.host_path.starts_with("/tmp/work"),
        "host path is scoped to the workdir, not the filesystem root"
    );
    // The sandbox form keeps its leading slash; inside the sandbox root it is
    // still under the writable scope.
    assert_eq!(staged.sandbox_path, "/.agent-writes/call-1");
}

#[test]
fn describe_tool_call_names_the_target_and_matches_the_rendered_summary() {
    // `describe_tool_call` is the single source for `RenderedTool.summary`, so
    // the live "tool proposed" line shows the same target as execution and names
    // the file, directory, or command rather than just the tool.
    let reg = registry();

    let read = reg
        .render(
            &tool_intent("read_file", json!({ "path": "/workspace/main.txt" })),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect("read renders");
    let list = reg
        .render(
            &tool_intent("list_dir", json!({ "path": "/workspace/src" })),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect("list renders");
    let write = reg
        .render(
            &tool_intent(
                "write_file",
                json!({ "path": "/workspace/new.txt", "content": "x" }),
            ),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect("write renders");
    let shell = reg
        .render(
            &tool_intent("shell", json!({ "command": "cargo build" })),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect("shell renders");

    assert_eq!(
        describe_tool_call(&tool_intent(
            "read_file",
            json!({ "path": "/workspace/main.txt" })
        )),
        read.summary
    );
    assert_eq!(
        describe_tool_call(&tool_intent(
            "list_dir",
            json!({ "path": "/workspace/src" })
        )),
        list.summary
    );
    assert_eq!(
        describe_tool_call(&tool_intent(
            "write_file",
            json!({ "path": "/workspace/new.txt", "content": "x" })
        )),
        write.summary
    );
    assert_eq!(
        describe_tool_call(&tool_intent("shell", json!({ "command": "cargo build" }))),
        shell.summary
    );

    // The descriptions name the target, not just the tool name.
    assert_eq!(
        describe_tool_call(&tool_intent(
            "read_file",
            json!({ "path": "/workspace/a.rs" })
        )),
        "read /workspace/a.rs"
    );
    assert_eq!(
        describe_tool_call(&tool_intent("shell", json!({ "command": "ls -la" }))),
        "ls -la"
    );
}

#[test]
fn write_file_to_a_sibling_prefix_outside_write_root_is_rejected() {
    // Regression: a bare `starts_with` on the write root accepted `/workspace-evil`,
    // a sibling of `/workspace`. The render gate must reject it before any sandbox
    // request is built.
    let reg = registry();
    let err = reg
        .render(
            &tool_intent(
                "write_file",
                json!({ "path": "/workspace-evil/passwd", "content": "x" }),
            ),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect_err("write to a sibling prefix must be rejected");
    assert!(matches!(err, Error::ToolPolicy { .. }));
}

#[test]
fn write_root_is_enforced_on_a_slash_boundary_for_custom_roots() {
    // With a configured root `/work`, `/works/x` must not be accepted.
    let policy = ToolPolicy {
        write_root: "/work".to_owned(),
        ..ToolPolicy::minimum()
    };
    let reg = ToolRegistry::new(policy);
    let rejected = reg
        .render(
            &tool_intent("write_file", json!({ "path": "/works/x", "content": "x" })),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect_err("/works must be outside root /work");
    assert!(matches!(rejected, Error::ToolPolicy { .. }));
    let accepted = reg
        .render(
            &tool_intent("write_file", json!({ "path": "/work/x", "content": "x" })),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect("/work/x is inside root /work");
    assert!(accepted.staged_write.is_some());
}

#[test]
fn denylisted_shell_command_is_rejected() {
    let reg = registry();
    let err = reg
        .render(
            &tool_intent("shell", json!({ "command": "rm -rf /" })),
            &ExecContext::new("/tmp/work", "c"),
        )
        .expect_err("denylisted command");
    assert!(matches!(err, Error::ToolPolicy { .. }));
}

#[test]
fn profile_ids_are_sorted_and_unique() {
    let reg = registry().with_profiles(vec![
        ToolProfile::Python,
        ToolProfile::Rust,
        ToolProfile::Rust,
    ]);
    assert_eq!(reg.profiles(), ["python", "rust"]);
}

fn fake_sandbox(dir: &tempfile::TempDir) -> SandboxPaths {
    let runner = dir.path().join("runner");
    let backend = dir.path().join("backend");
    std::fs::write(&runner, b"runner-bytes").expect("write runner");
    std::fs::write(&backend, b"bwrap-bytes").expect("write backend");
    SandboxPaths { runner, backend }
}

fn build<'a>(
    argv: &'a [String],
    workdir: &'a Path,
    read_roots: &'a [PathBuf],
    environment: BTreeMap<String, String>,
    policy: &'a ToolPolicy,
) -> sandbox::BuildRequest<'a> {
    sandbox::BuildRequest {
        argv,
        working_directory: workdir,
        read_roots,
        environment,
        policy,
        resources: sandbox::default_resources(),
    }
}

/// A minimal, valid shell argv that passes the runner's own validator.
fn shell_argv() -> Vec<String> {
    vec![
        "/usr/bin/bash".to_owned(),
        "-c".to_owned(),
        "true".to_owned(),
        "agent-runner".to_owned(),
    ]
}

/// Asserts the error is a sandbox-build failure whose reason mentions `needle`,
/// so a regression cannot substitute an unrelated construction failure.
fn assert_sandbox_build_reason(err: &Error, needle: &str) {
    match err {
        Error::SandboxBuild { reason } => {
            assert!(
                reason.contains(needle),
                "expected a SandboxBuild mentioning `{needle}`, got: {reason}"
            );
        }
        other => panic!("expected SandboxBuild, got {other:?}"),
    }
}

#[test]
fn build_request_produces_a_closed_authoring_request() {
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let argv: Vec<String> = vec![
        "/usr/bin/bash".to_owned(),
        "-c".to_owned(),
        "echo hi".to_owned(),
        "agent-runner".to_owned(),
    ];
    let read_roots: Vec<PathBuf> = Vec::new();
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let policy = ToolPolicy::minimum();
    let req = sandbox::build_request(
        &sb,
        &build(&argv, workdir.path(), &read_roots, environment, &policy),
    )
    .expect("request builds");
    assert_eq!(req.phase, Phase::Authoring);
    assert_eq!(req.network.mode, NetworkMode::Deny);
    assert_eq!(req.grants.len(), 1);
    assert!(req.identities.runner.starts_with("sha256:"));
    assert!(req.identities.toolchain.starts_with("sha256:"));
    assert_eq!(req.working_directory, DEFAULT_WRITE_ROOT);
    // HOME must point at the sandbox's guaranteed `/tmp` tmpfs, never at a
    // scratch path the request does not declare.
    assert_eq!(
        req.environment.get("HOME").map(String::as_str),
        Some("/tmp")
    );
}

#[test]
fn build_request_rejects_symlink_that_escapes_writable_scope() {
    // The scope is symlink-free only in the sense that no link may escape it: a
    // link whose resolved target lies outside the working directory could write
    // through and leave the sandbox, so the build fails closed up front and names
    // both the offending link and the target it points at. This holds at any
    // depth, including the depth-two links real projects ship (Node's `.bin`).
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let nested = workdir.path().join("sub");
    std::fs::create_dir_all(&nested).expect("nested dir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(std::path::Path::new("/etc"), nested.join("escape"))
        .expect("create out-of-scope symlink");
    let argv = shell_argv();
    let read_roots: Vec<PathBuf> = Vec::new();
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let policy = ToolPolicy::minimum();
    let err = sandbox::build_request(
        &sb,
        &build(&argv, workdir.path(), &read_roots, environment, &policy),
    )
    .expect_err("a scope-escaping symlink must fail the build");
    assert_sandbox_build_reason(&err, "escape");
    assert_sandbox_build_reason(&err, "escapes the writable scope");
}

#[test]
fn build_request_allows_symlinks_that_stay_in_writable_scope() {
    // In-project links (Node's `.bin`, aliases) are common and safe: a link that
    // resolves inside the scope cannot write through and leave it, so the build
    // succeeds whether the link is immediate or nested.
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let target = workdir.path().join("real.txt");
    std::fs::write(&target, b"data").expect("write target");
    let nested = workdir.path().join("sub");
    std::fs::create_dir_all(&nested).expect("nested dir");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, workdir.path().join("link.txt"))
            .expect("create immediate in-scope symlink");
        std::os::unix::fs::symlink(&target, nested.join("nested-link"))
            .expect("create nested in-scope symlink");
    }
    let argv = shell_argv();
    let read_roots: Vec<PathBuf> = Vec::new();
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let policy = ToolPolicy::minimum();
    sandbox::build_request(
        &sb,
        &build(&argv, workdir.path(), &read_roots, environment, &policy),
    )
    .expect("in-scope symlinks must be allowed");
}

#[test]
fn default_resources_are_bounded() {
    let r = sandbox::default_resources();
    assert!(r.wall_time_ms > 0);
    assert!(r.max_output_bytes > 0);
    assert!(r.max_processes > 0);
    assert!(r.max_scratch_bytes > 0);
}

#[test]
fn exit_codes_reflect_severity() {
    assert_eq!(
        Error::NotInteractive {
            reason: "no".to_owned()
        }
        .exit_code(),
        2
    );
    assert_eq!(
        Error::ToolPolicy {
            tool: "shell".to_owned(),
            reason: "no".to_owned()
        }
        .exit_code(),
        3
    );
    assert_eq!(
        Error::Config {
            path: None,
            reason: "no".to_owned()
        }
        .exit_code(),
        1
    );
}

#[test]
fn describe_messages_are_non_empty_and_no_panics() {
    let message = Error::ModelNotFound {
        requested: "x".to_owned(),
        available: vec!["a".to_owned(), "b".to_owned()],
    }
    .describe();
    assert!(message.contains("x"));
    assert!(message.contains("a"));
}

#[test]
fn parse_effort_accepts_known_levels_and_rejects_others() {
    assert!(parse_effort("medium").is_ok());
    assert!(parse_effort("max").is_ok());
    assert!(parse_effort("h").is_err());
}

#[test]
fn build_request_rejects_read_root_inside_write_root() {
    // A read root declared within the working directory overlaps the write
    // root, so the request must fail closed instead of mounting that path both
    // read-write and read-only.
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let read_root = workdir.path().join("context");
    std::fs::create_dir_all(&read_root).expect("read root dir");
    let argv: Vec<String> = shell_argv();
    let read_roots: Vec<PathBuf> = vec![read_root];
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let policy = ToolPolicy::minimum();
    let err = sandbox::build_request(
        &sb,
        &build(&argv, workdir.path(), &read_roots, environment, &policy),
    )
    .expect_err("overlapping read root must be rejected");
    assert_sandbox_build_reason(&err, "read root");
}

#[test]
fn build_request_rejects_read_root_equal_to_write_root() {
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let argv: Vec<String> = shell_argv();
    let read_roots: Vec<PathBuf> = vec![workdir.path().to_path_buf()];
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let policy = ToolPolicy::minimum();
    let err = sandbox::build_request(
        &sb,
        &build(&argv, workdir.path(), &read_roots, environment, &policy),
    )
    .expect_err("a read root equal to the write root must be rejected");
    assert_sandbox_build_reason(&err, "read root");
}

#[test]
fn build_request_allows_read_root_outside_write_root() {
    // A read root that is a sibling of the working directory is allowed and
    // produces one authoring grant plus one context grant.
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let external = tempdir().expect("external read root");
    let external_file = external.path().join("notes.txt");
    std::fs::write(&external_file, b"context").expect("external file");
    let argv: Vec<String> = shell_argv();
    let read_roots: Vec<PathBuf> = vec![external_file];
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let policy = ToolPolicy::minimum();
    let req = sandbox::build_request(
        &sb,
        &build(&argv, workdir.path(), &read_roots, environment, &policy),
    )
    .expect("request builds with an external read root");
    assert_eq!(req.grants.len(), 2);
}

#[test]
fn execute_fails_closed_when_runner_is_missing() {
    // A request that builds cleanly against a real runner still fails closed
    // when the runner cannot be spawned: execution must never fall back to the
    // host. This exercises the spawn-failure path without bwrap.
    let scope = tempdir().expect("temp scope");
    let sb = fake_sandbox(&scope);
    let workdir = tempdir().expect("temp workdir");
    let argv: Vec<String> = vec![
        "/usr/bin/bash".to_owned(),
        "-c".to_owned(),
        "echo hi".to_owned(),
        "agent-runner".to_owned(),
    ];
    let req = sandbox::build_request(
        &sb,
        &build(
            &argv,
            workdir.path(),
            &[],
            BTreeMap::new(),
            &ToolPolicy::minimum(),
        ),
    )
    .expect("request builds against a valid runner");
    let missing = SandboxPaths {
        runner: scope.path().join("does-not-exist-runner"),
        backend: sb.backend.clone(),
    };
    let outcome = sandbox::execute(&missing, &req, &CancellationToken::new());
    assert!(
        matches!(outcome, Err(Error::SandboxUnavailable { .. })),
        "expected fail-closed, got {outcome:?}"
    );
}

#[test]
fn sandbox_executor_fails_closed_without_a_valid_runner() {
    // The real executor renders the intent, then must fail closed when the
    // sandbox runner is not a usable binary rather than executing on the host.
    let dir = tempdir().expect("temp dir");
    let runner = dir.path().join("runner");
    std::fs::create_dir_all(&runner).expect("runner as a directory");
    let sb = SandboxPaths {
        runner,
        backend: dir.path().join("backend"),
    };
    let exec = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum()),
        sb,
        dir.path().to_path_buf(),
    );
    let outcome = exec
        .execute(
            &tool_intent("shell", json!({ "command": "echo hi" })),
            &CancellationToken::new(),
        )
        .expect_err("executor must fail closed");
    assert!(matches!(outcome, Error::SandboxUnavailable { .. }));
}

#[test]
fn resolve_config_path_honours_explicit_first() {
    let path = PathBuf::from("/tmp/explicit.toml");
    assert_eq!(resolve_config_path(Some(path.clone())), Ok(path));
}
