//! Native helper behavior tests; these do not establish sandbox isolation.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use agent_runner::file_tools::{FileRequest, MAX_FILE_BYTES, execute_file_request};
use agent_runner::session::ToolExecutor;
use agent_runner::{
    ExecContext, HostExecutor, SandboxExecutor, SandboxPaths, ToolPolicy, ToolRegistry,
};
use agent_runtime::{CancellationToken, ToolIntent};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::{Builder, TempDir};

fn fixture() -> TempDir {
    Builder::new()
        .prefix(".native-file-test-")
        .tempdir_in(".")
        .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn request(root: &Path, tool: &str, args: Value) -> FileRequest {
    FileRequest::new(root.to_str().unwrap(), tool, args).unwrap()
}

fn run(root: &Path, tool: &str, args: Value) -> agent_runner::Result<Value> {
    execute_file_request(&request(root, tool, args))
}

#[test]
fn paginated_read_is_byte_bounded_and_hashes_complete_file() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    fs::write(&path, "aé\r\nlast").unwrap();
    let first = run(&root, "read_file", json!({"path":path,"limit":3})).unwrap();
    assert_eq!(first["content"], "aé");
    assert_eq!(first["sha256"], digest("aé\r\nlast".as_bytes()));
    assert_eq!(first["next_offset"], 3);
    assert_eq!(first["total_bytes"], 9);
    let rest = run(&root, "read_file", json!({"path":path,"offset":3})).unwrap();
    assert_eq!(rest["content"], "\r\nlast");
    assert!(rest["next_offset"].is_null());
    assert!(run(&root, "read_file", json!({"path":path,"offset":2})).is_err());
}

#[test]
fn read_and_write_sizes_are_explicitly_bounded() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("oversized");
    fs::write(&path, vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
    assert!(
        run(&root, "read_file", json!({"path":path}))
            .unwrap_err()
            .to_string()
            .contains("bound")
    );
    assert!(
        FileRequest::new(
            root.to_str().unwrap(),
            "write_file",
            json!({"path":root.join("out"),"content":"x".repeat(256 * 1024)})
        )
        .is_err()
    );
}

#[test]
fn edits_preserve_unrelated_bytes_crlf_missing_newline_and_mode() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let original = b"first\r\nold\r\nlast";
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let out = run(
        &root,
        "edit_file",
        json!({
            "path":path,"old_text":"old","new_text":"new","expected_sha256":digest(original)
        }),
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"first\r\nnew\r\nlast");
    assert_eq!(out["sha256"], digest(b"first\r\nnew\r\nlast"));
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn stale_zero_multiple_and_overlapping_matches_never_mutate() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    for (text, old, expected) in [
        ("abc", "z", digest(b"abc")),
        ("abc abc", "abc", digest(b"abc abc")),
        ("aaa", "aa", digest(b"aaa")),
        ("abc", "abc", digest(b"stale")),
    ] {
        fs::write(&path, text).unwrap();
        assert!(
            run(
                &root,
                "edit_file",
                json!({
                    "path":path,"old_text":old,"new_text":"changed","expected_sha256":expected
                })
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
    assert!(
        FileRequest::new(
            root.to_str().unwrap(),
            "edit_file",
            json!({
                "path":path,"old_text":"","new_text":"x","expected_sha256":digest(b"abc")
            })
        )
        .is_err()
    );
}

#[test]
fn mutations_reject_traversal_sibling_prefix_and_symlink_parents_or_targets() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("real")).unwrap();
    fs::write(root.join("real/text"), "original").unwrap();
    symlink(root.join("real"), root.join("link")).unwrap();
    symlink(root.join("real/text"), root.join("file-link")).unwrap();
    for path in [
        format!("{}/../escape", root.display()),
        format!("{}-evil/escape", root.display()),
        format!("{}/./escape", root.display()),
        format!("{}/link/text", root.display()),
        format!("{}/file-link", root.display()),
    ] {
        assert!(
            FileRequest::new(
                root.to_str().unwrap(),
                "write_file",
                json!({"path":path,"content":"replacement"})
            )
            .and_then(|req| execute_file_request(&req))
            .is_err()
        );
    }
    assert_eq!(
        fs::read_to_string(root.join("real/text")).unwrap(),
        "original"
    );
}

#[test]
fn deterministic_list_find_and_literal_search_are_paginated_and_scoped() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/z.txt"), "a.b\nplain\n").unwrap();
    fs::write(root.join("sub/a.txt"), "a.b\n").unwrap();
    fs::write(root.join("outside.txt"), "a.b\n").unwrap();
    symlink(root.join("outside.txt"), root.join("sub/link")).unwrap();
    let scope = root.join("sub");
    let listing = run(&root, "list_dir", json!({"path":scope,"limit":1})).unwrap();
    assert_eq!(listing["entries"][0]["name"], "a.txt");
    assert_eq!(listing["next_offset"], 1);
    let files = run(
        &root,
        "find_files",
        json!({"path":scope,"pattern":".txt","offset":1,"limit":1}),
    )
    .unwrap();
    assert_eq!(files["files"], json!([scope.join("z.txt")]));
    assert!(files["next_offset"].is_null());
    let matches = run(
        &root,
        "search_files",
        json!({"path":scope,"query":"a.b","limit":1}),
    )
    .unwrap();
    assert_eq!(
        matches["matches"][0]["path"],
        scope.join("a.txt").to_str().unwrap()
    );
    assert_eq!(matches["matches"][0]["line"], 1);
    assert_eq!(matches["total"], 2);
    assert_eq!(matches["next_offset"], 1);
    assert_eq!(matches["skipped_symlinks"], 1);
    let multiline = run(
        &root,
        "search_files",
        json!({"path":scope,"query":"a.b\nplain"}),
    )
    .unwrap();
    assert_eq!(multiline["total"], 1);
    assert_eq!(
        multiline["matches"][0]["path"],
        scope.join("z.txt").to_str().unwrap()
    );
    assert_eq!(multiline["matches"][0]["line"], 1);
}

#[test]
fn unknown_arguments_types_bounds_and_noncanonical_paths_fail_before_effects() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    for (tool, args) in [
        ("read_file", json!({"path":root,"extra":true})),
        ("read_file", json!({"path":root,"limit":16385})),
        ("read_file", json!({"path":root,"offset":-1})),
        ("list_dir", json!({"path":root,"limit":257})),
        ("search_files", json!({"path":root,"query":""})),
        (
            "find_files",
            json!({"path":root,"pattern":"x","command":"rm"}),
        ),
        (
            "write_file",
            json!({"path":format!("{}//file", root.display()),"content":"x"}),
        ),
        ("write_file", json!({"path":"bad\0path","content":"x"})),
        (
            "write_file",
            json!({"path":root.join("text"),"content":"x","expected_sha256":null}),
        ),
    ] {
        assert!(
            FileRequest::new(root.to_str().unwrap(), tool, args).is_err(),
            "{tool}"
        );
    }
}

#[test]
fn render_is_effect_free_even_with_hostile_ids_and_workspace_staging_links() {
    let dir = fixture();
    let outside = fixture();
    let root = dir.path().canonicalize().unwrap();
    symlink(
        outside.path().canonicalize().unwrap(),
        root.join(".agent-writes"),
    )
    .unwrap();
    let registry = ToolRegistry::new(ToolPolicy::minimum());
    let intent = ToolIntent {
        id: "../../escaped".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: json!({"path":"/workspace/file","content":"safe"}),
    };
    let rendered = registry
        .render(&intent, &ExecContext::new(&root, &intent.id))
        .unwrap();
    assert!(rendered.file_request.is_some());
    assert_eq!(rendered.argv, ["/context/1", "/context/0"]);
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(!root.join("file").exists());
}

#[test]
fn executor_rejects_invalid_sandbox_without_writes_and_cancels_before_staging() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let helper = Path::new(env!("CARGO_BIN_EXE_agent-runner-file-tool"));
    let executor = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum()).with_file_helper(helper),
        SandboxPaths {
            runner: root.join("missing-runner"),
            backend: root.join("missing-backend"),
        },
        root.clone(),
    );
    let intent = ToolIntent {
        id: "/hostile".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: json!({"path":"/workspace/file","content":"safe"}),
    };
    assert!(
        executor
            .execute(&intent, &CancellationToken::new())
            .is_err()
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let token = CancellationToken::new();
    token.cancel();
    assert!(
        executor
            .execute(&intent, &token)
            .unwrap_err()
            .to_string()
            .contains("cancel")
    );
}

#[test]
fn helper_binary_rejects_unknown_fields_and_oversized_payloads() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let payload = root.join("payload.json");
    fs::write(&payload, r#"{"write_root":"/workspace","operation":{"tool":"read_file","arguments":{"path":"/workspace/a","extra":true}}}"#).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-runner-file-tool"))
        .arg(&payload)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    fs::write(&payload, vec![b' '; 256 * 1024 + 1]).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-runner-file-tool"))
        .arg(&payload)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn traversal_depth_oversized_scan_and_long_lines_are_bounded_explicitly() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let mut deep = root.join("deep");
    fs::create_dir(&deep).unwrap();
    for _ in 0..33 {
        deep = deep.join("d");
        fs::create_dir(&deep).unwrap();
    }
    let error = run(
        &root,
        "find_files",
        json!({"path":root.join("deep"),"pattern":""}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("depth bound"));
    fs::create_dir(root.join("scan")).unwrap();
    fs::write(root.join("scan/large"), vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
    assert!(
        run(
            &root,
            "search_files",
            json!({"path":root.join("scan"),"query":"x"})
        )
        .unwrap_err()
        .to_string()
        .contains("byte bound")
    );
    fs::write(
        root.join("scan/large"),
        format!("{}needle", "x".repeat(10_000)),
    )
    .unwrap();
    let out = run(
        &root,
        "search_files",
        json!({"path":root.join("scan"),"query":"needle"}),
    )
    .unwrap();
    assert_eq!(out["matches"][0]["content"].as_str().unwrap().len(), 1024);
    assert_eq!(out["matches"][0]["truncated"], true);
}

#[test]
fn whole_file_write_is_atomic_mode_preserving_and_optionally_preimage_bound() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    run(&root, "write_file", json!({"path":path,"content":"first"})).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o751)).unwrap();
    run(
        &root,
        "write_file",
        json!({"path":path,"content":"second","expected_sha256":digest(b"first")}),
    )
    .unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o751
    );
    assert!(
        run(
            &root,
            "write_file",
            json!({"path":path,"content":"stale","expected_sha256":digest(b"first")})
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn explicit_host_executor_uses_same_helper_without_workspace_staging() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let executor = HostExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum())
            .with_file_helper(env!("CARGO_BIN_EXE_agent-runner-file-tool")),
        root.clone(),
    );
    let intent = ToolIntent {
        id: "../evil".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: json!({"path":"/workspace/text","content":"hello"}),
    };
    let out = executor
        .execute(&intent, &CancellationToken::new())
        .unwrap();
    assert_eq!(out.status, Some(0), "{}", out.error_text(4096));
    assert_eq!(fs::read(root.join("text")).unwrap(), b"hello");
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn helper_must_be_regular_nonlink_and_outside_write_scope() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("helper"), b"fake").unwrap();
    symlink(
        env!("CARGO_BIN_EXE_agent-runner-file-tool"),
        root.join("link"),
    )
    .unwrap();
    let intent = ToolIntent {
        id: "test".to_owned(),
        provider_id: None,
        name: "read_file".to_owned(),
        arguments: json!({"path":"/workspace/file"}),
    };
    for helper in [
        root.join("helper"),
        root.join("link"),
        root.clone(),
        root.join("missing"),
    ] {
        let executor = SandboxExecutor::new(
            ToolRegistry::new(ToolPolicy::minimum()).with_file_helper(helper),
            SandboxPaths {
                runner: root.join("missing-runner"),
                backend: root.join("missing-backend"),
            },
            root.clone(),
        );
        assert!(
            executor
                .execute(&intent, &CancellationToken::new())
                .is_err()
        );
    }
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn sandbox_build_failure_drops_private_payload_and_rejects_unsupported_read_roots() {
    use agent_runner::sandbox::{BuildRequest, build_request, default_resources};
    let scope = fixture();
    let infrastructure = fixture();
    fs::create_dir(scope.path().join("workspace")).unwrap();
    let root = scope.path().join("workspace").canonicalize().unwrap();
    let infra = infrastructure.path().canonicalize().unwrap();
    // Parent-owned installations are outside the authoring scope.
    let runner = infra.join("runner");
    let backend = infra.join("backend");
    fs::write(&runner, b"fixture runner").unwrap();
    fs::write(&backend, b"fixture backend").unwrap();
    let sandbox = SandboxPaths { runner, backend };
    let policy = ToolPolicy::minimum();
    let argv = vec!["/usr/bin/true".to_owned()];
    for read_root in [infra.clone(), infra.join("missing")] {
        let roots = [read_root];
        assert!(
            build_request(
                &sandbox,
                &BuildRequest {
                    argv: &argv,
                    working_directory: &root,
                    read_roots: &roots,
                    environment: Default::default(),
                    policy: &policy,
                    resources: default_resources(),
                }
            )
            .is_err()
        );
    }
    let context = infra.join("context");
    fs::write(&context, b"readonly").unwrap();
    let roots = [context.clone()];
    let request = build_request(
        &sandbox,
        &BuildRequest {
            argv: &argv,
            working_directory: &root,
            read_roots: &roots,
            environment: Default::default(),
            policy: &policy,
            resources: default_resources(),
        },
    )
    .unwrap();
    let grant = request
        .grants
        .iter()
        .find(|g| g.destination == "/context/0")
        .unwrap();
    assert_eq!(grant.identity, digest(b"readonly"));
    // A workspace escaping symlink makes construction fail after private
    // staging. RAII must clean the payload even along this error path.
    symlink(&infra, root.join(".agent-writes")).unwrap();
    let executor = SandboxExecutor::new(
        ToolRegistry::new(policy).with_file_helper(env!("CARGO_BIN_EXE_agent-runner-file-tool")),
        sandbox,
        root.clone(),
    );
    let intent = ToolIntent {
        id: "../../escaped".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: json!({"path":"/workspace/file","content":"safe"}),
    };
    assert!(
        executor
            .execute(&intent, &CancellationToken::new())
            .is_err()
    );
    assert!(!root.join("file").exists());
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
    assert_eq!(fs::read_dir(scope.path()).unwrap().count(), 1);
}

#[test]
fn private_payload_and_helper_are_readonly_hashed_context_files_and_cleaned_after_failure() {
    let container = fixture();
    let infrastructure = fixture();
    fs::create_dir(container.path().join("workspace")).unwrap();
    let root = container.path().join("workspace").canonicalize().unwrap();
    let infra = infrastructure.path().canonicalize().unwrap();
    let runner = infra.join("runner");
    let backend = infra.join("backend");
    // Request echo is only an executor construction fixture, not isolation.
    fs::write(&runner, "#!/usr/bin/bash\ncat\nexit 7\n").unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(&backend, b"backend").unwrap();
    let helper = Path::new(env!("CARGO_BIN_EXE_agent-runner-file-tool"));
    let executor = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum()).with_file_helper(helper),
        SandboxPaths { runner, backend },
        root.clone(),
    );
    let intent = ToolIntent {
        id: "/../../evil".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: json!({"path":"/workspace/file","content":"safe"}),
    };
    let out = executor
        .execute(&intent, &CancellationToken::new())
        .unwrap();
    assert_eq!(out.status, Some(7));
    let request: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(request["argv"], json!(["/context/1", "/context/0"]));
    let grants = request["grants"].as_array().unwrap();
    let payload = grants
        .iter()
        .find(|g| g["destination"] == "/context/0")
        .unwrap();
    let binary = grants
        .iter()
        .find(|g| g["destination"] == "/context/1")
        .unwrap();
    assert_eq!(payload["access"], "read-only");
    assert_eq!(binary["access"], "read-only");
    assert_eq!(binary["identity"], digest(&fs::read(helper).unwrap()));
    let source = Path::new(payload["source"].as_str().unwrap());
    assert!(!source.starts_with(&root));
    assert!(
        !source.exists(),
        "RAII payload must disappear after failed effects"
    );
    assert_eq!(fs::read_dir(container.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn failed_spawn_also_removes_private_payload() {
    let container = fixture();
    let infrastructure = fixture();
    fs::create_dir(container.path().join("workspace")).unwrap();
    let root = container.path().join("workspace").canonicalize().unwrap();
    let infra = infrastructure.path().canonicalize().unwrap();
    fs::write(infra.join("runner"), b"not executable").unwrap();
    fs::write(infra.join("backend"), b"backend").unwrap();
    let executor = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum())
            .with_file_helper(env!("CARGO_BIN_EXE_agent-runner-file-tool")),
        SandboxPaths {
            runner: infra.join("runner"),
            backend: infra.join("backend"),
        },
        root.clone(),
    );
    let intent = ToolIntent {
        id: "hostile/id".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: json!({"path":"/workspace/file","content":"safe"}),
    };
    assert!(
        executor
            .execute(&intent, &CancellationToken::new())
            .is_err()
    );
    assert_eq!(fs::read_dir(container.path()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn binary_skips_are_reported_and_ordinary_unsupported_entries_fail() {
    use nix::sys::stat::Mode;
    use nix::unistd::mkfifo;
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("binary"), [0xff, 0xfe]).unwrap();
    fs::write(root.join("nul"), b"text\0needle").unwrap();
    fs::write(root.join("text"), "needle").unwrap();
    let result = run(&root, "search_files", json!({"path":root,"query":"needle"})).unwrap();
    assert_eq!(result["skipped_binary"], 2);
    assert_eq!(result["total"], 1);
    mkfifo(&root.join("pipe"), Mode::from_bits_truncate(0o600)).unwrap();
    assert!(
        run(&root, "search_files", json!({"path":root,"query":"needle"}))
            .unwrap_err()
            .to_string()
            .contains("unsupported ordinary")
    );
    assert!(run(&root, "read_file", json!({"path":root.join("pipe")})).is_err());
}

#[test]
fn scan_bytes_entry_count_and_encoded_pages_fail_at_explicit_bounds() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let scan = root.join("scan");
    fs::create_dir(&scan).unwrap();
    let bytes = vec![b'x'; MAX_FILE_BYTES];
    for index in 0..9 {
        fs::write(scan.join(index.to_string()), &bytes).unwrap();
    }

    assert!(
        run(&root, "search_files", json!({"path":scan,"query":"absent"}))
            .unwrap_err()
            .to_string()
            .contains("total scanned byte bound")
    );
    let count = root.join("count");
    fs::create_dir(&count).unwrap();
    for index in 0..4097 {
        fs::write(count.join(index.to_string()), b"").unwrap();
    }
    assert!(
        run(&root, "list_dir", json!({"path":count}))
            .unwrap_err()
            .to_string()
            .contains("entry count")
    );
    let mut long = root.join("long");
    fs::create_dir(&long).unwrap();
    for _ in 0..10 {
        long = long.join("d".repeat(200));
        fs::create_dir(&long).unwrap();
    }
    for index in 0..100 {
        fs::write(long.join(index.to_string()), b"").unwrap();
    }
    let page = run(&root, "find_files", json!({"path":long,"pattern":""})).unwrap();
    assert!(serde_json::to_vec(&page).unwrap().len() <= 7000);
    assert!(page["files"].as_array().unwrap().len() < 100);
    assert!(page["next_offset"].as_u64().is_some());
    assert_eq!(
        run(
            &root,
            "find_files",
            json!({"path":long,"pattern":"","limit":1})
        )
        .unwrap()["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn tool_outcome_requires_observed_clean_exit_to_succeed() {
    use agent_runner::ToolOutcome;
    let clean = ToolOutcome {
        exited: true,
        status: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    };
    assert!(!clean.failed());
    for outcome in [
        ToolOutcome {
            exited: false,
            ..clean.clone()
        },
        ToolOutcome {
            status: None,
            ..clean.clone()
        },
        ToolOutcome {
            status: Some(1),
            ..clean.clone()
        },
        ToolOutcome {
            timed_out: true,
            ..clean.clone()
        },
        ToolOutcome {
            output_limit_exceeded: true,
            ..clean.clone()
        },
        ToolOutcome {
            cancelled: true,
            ..clean.clone()
        },
        ToolOutcome::rejected(),
    ] {
        assert!(outcome.failed(), "{outcome:?}");
    }
}

#[test]
fn read_schema_explains_adaptive_complete_json_pages() {
    let definition = ToolRegistry::new(ToolPolicy::minimum())
        .tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "read_file")
        .unwrap();
    assert_eq!(
        definition.parameters["properties"]["limit"]["default"],
        4096
    );
    assert_eq!(
        definition.parameters["properties"]["limit"]["maximum"],
        16384
    );
    assert!(definition.description.contains("7000"));
    assert!(definition.description.contains("next_offset"));
}

#[test]
fn escaped_control_and_cjk_read_pages_are_complete_json_with_lossless_progress() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    for text in ["\u{0001}".repeat(10_000), "漢字界".repeat(2000)] {
        fs::write(&path, &text).unwrap();
        let mut offset = 0;
        let mut collected = String::new();
        loop {
            let output = run(
                &root,
                "read_file",
                json!({"path":path,"offset":offset,"limit":16384}),
            )
            .unwrap();
            let encoded = serde_json::to_vec(&output).unwrap();
            assert!(encoded.len() <= 7000);
            let decoded: Value = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(decoded["sha256"], digest(text.as_bytes()));
            assert_eq!(decoded["offset"], offset);
            assert_eq!(decoded["total_bytes"], text.len());
            let content = decoded["content"].as_str().unwrap();
            collected.push_str(content);
            match decoded["next_offset"].as_u64() {
                Some(next) => {
                    assert!(next as usize > offset);
                    assert_eq!(next as usize, offset + content.len());
                    assert!(text.is_char_boundary(next as usize));
                    offset = next as usize;
                }
                None => break,
            }
        }
        assert_eq!(collected, text);
        let payload = root.join("request.json");
        fs::write(
            &payload,
            request(&root, "read_file", json!({"path":path,"limit":16384}))
                .to_bytes()
                .unwrap(),
        )
        .unwrap();
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_agent-runner-file-tool"))
            .arg(payload)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.len() <= 7000);
        let decoded: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(decoded["sha256"], digest(text.as_bytes()));
        assert!(decoded["next_offset"].as_u64().is_some());
    }
}

#[test]
fn oversized_mutation_metadata_is_rejected_before_file_effects() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let mut parent = root.clone();
    for _ in 0..12 {
        parent = parent.join("\u{0001}".repeat(100));
        fs::create_dir(&parent).unwrap();
    }
    let target = parent.join("text");
    let error = run(&root, "write_file", json!({"path":target,"content":"safe"})).unwrap_err();
    assert!(error.to_string().contains("encoded output bound"));
    assert!(!target.exists());
    assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
}

#[test]
fn long_escaped_list_find_and_search_pages_preserve_all_sorted_entries() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    for index in 0..100 {
        let name = format!("{index:03}-{}", "\u{0001}".repeat(100));
        fs::write(root.join(name), "\u{0001}".repeat(700)).unwrap();
    }
    for (tool, extra, field) in [
        ("list_dir", json!({}), "entries"),
        ("find_files", json!({"pattern":""}), "files"),
        ("search_files", json!({"query":"\u{0001}"}), "matches"),
    ] {
        let mut offset = 0;
        let mut collected = Vec::new();
        loop {
            let mut args = extra.clone();
            args["path"] = json!(root);
            args["offset"] = json!(offset);
            args["limit"] = json!(256);
            let output = run(&root, tool, args).unwrap();
            let encoded = serde_json::to_vec(&output).unwrap();
            assert!(encoded.len() <= 7000);
            let decoded: Value = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(decoded["offset"], offset);
            assert_eq!(decoded["total"], 100);
            let items = decoded[field].as_array().unwrap();
            assert!(!items.is_empty());
            collected.extend(items.iter().cloned());
            match decoded["next_offset"].as_u64() {
                Some(next) => {
                    assert_eq!(next as usize, offset + items.len());
                    offset = next as usize;
                }
                None => break,
            }
        }
        assert_eq!(collected.len(), 100);
        for (index, item) in collected.iter().enumerate() {
            let path = match tool {
                "list_dir" => item["name"].as_str().unwrap(),
                "find_files" => item.as_str().unwrap(),
                _ => item["path"].as_str().unwrap(),
            };
            assert!(
                Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with(&format!("{index:03}-"))
            );
        }
    }
}

#[test]
fn single_search_entry_too_large_for_complete_json_returns_bounded_error() {
    let dir = fixture();
    let root = dir.path().canonicalize().unwrap();
    let mut scope = root.clone();
    for _ in 0..10 {
        scope = scope.join("d".repeat(200));
        fs::create_dir(&scope).unwrap();
    }
    fs::write(scope.join("text"), "\u{0001}".repeat(1024)).unwrap();
    let error = run(
        &root,
        "search_files",
        json!({"path":scope,"query":"\u{0001}","limit":1}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("single entry"));
    assert!(error.to_string().len() < 300);
}

#[test]
fn private_payload_directory_and_file_have_explicit_private_modes() {
    let container = fixture();
    let infrastructure = fixture();
    fs::create_dir(container.path().join("workspace")).unwrap();
    let root = container.path().join("workspace").canonicalize().unwrap();
    let helper = infrastructure.path().canonicalize().unwrap().join("helper");
    // Trusted construction fixture, not an isolation test or actual helper.
    fs::write(&helper, "#!/usr/bin/bash\nprintf '{\"file\":\"%s\",\"directory\":\"%s\"}\\n' \"$(stat -c %a -- \"$1\")\" \"$(stat -c %a -- \"$(dirname -- \"$1\")\")\"\n").unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    let executor = HostExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum()).with_file_helper(helper),
        root,
    );
    let intent = ToolIntent {
        id: "../hostile".to_owned(),
        provider_id: None,
        name: "read_file".to_owned(),
        arguments: json!({"path":"/workspace/text"}),
    };
    let out = executor
        .execute(&intent, &CancellationToken::new())
        .unwrap();
    assert_eq!(out.status, Some(0), "{}", out.error_text(4096));
    let modes: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(modes["file"], "600");
    assert_eq!(modes["directory"], "700");
    assert_eq!(fs::read_dir(container.path()).unwrap().count(), 1);
}

#[test]
fn registry_scope_and_canonical_denials_are_policy_errors_not_argument_errors() {
    let dir = fixture();
    let registry = ToolRegistry::new(ToolPolicy::minimum());
    for tool in ["write_file", "edit_file", "read_file"] {
        for path in [
            "/workspace/../escape",
            "/workspace/./text",
            "/workspace//text",
        ] {
            let args = if tool == "write_file" {
                json!({"path":path,"content":"x"})
            } else if tool == "edit_file" {
                json!({"path":path,"old_text":"x","new_text":"y","expected_sha256":digest(b"x")})
            } else {
                json!({"path":path})
            };
            let intent = ToolIntent {
                id: "test".to_owned(),
                provider_id: None,
                name: tool.to_owned(),
                arguments: args,
            };
            let error = registry
                .render(&intent, &ExecContext::new(dir.path(), "test"))
                .unwrap_err();
            assert!(matches!(error, agent_runner::Error::ToolPolicy { .. }));
            assert_eq!(error.exit_code(), 3);
        }
    }
    for tool in ["write_file", "edit_file"] {
        for path in ["/outside/text", "/workspace-evil/text"] {
            let args = if tool == "write_file" {
                json!({"path":path,"content":"x"})
            } else {
                json!({"path":path,"old_text":"x","new_text":"y","expected_sha256":digest(b"x")})
            };
            let intent = ToolIntent {
                id: "test".to_owned(),
                provider_id: None,
                name: tool.to_owned(),
                arguments: args,
            };
            let error = registry
                .render(&intent, &ExecContext::new(dir.path(), "test"))
                .unwrap_err();
            assert!(matches!(error, agent_runner::Error::ToolPolicy { .. }));
            assert_eq!(error.exit_code(), 3);
        }
    }
    for arguments in [
        json!({"path":false,"content":"x"}),
        json!({"path":"/outside/text","content":"x","extra":true}),
        json!({"path":"/workspace/../text","content":null}),
    ] {
        let intent = ToolIntent {
            id: "test".to_owned(),
            provider_id: None,
            name: "write_file".to_owned(),
            arguments,
        };
        assert!(matches!(
            registry.render(&intent, &ExecContext::new(dir.path(), "test")),
            Err(agent_runner::Error::ToolRender { .. })
        ));
    }
}

#[test]
fn bare_registry_defers_helper_location_but_production_resolution_supplies_it() {
    struct NoLanguages;
    impl agent_runner::ToolchainProbe for NoLanguages {
        fn is_available(&self, _: agent_runner::ToolProfile) -> bool {
            false
        }
    }
    let dir = fixture();
    let intent = ToolIntent {
        id: "test".to_owned(),
        provider_id: None,
        name: "read_file".to_owned(),
        arguments: json!({"path":"/workspace/text"}),
    };
    let bare = ToolRegistry::new(ToolPolicy::minimum());
    let rendered = bare
        .render(&intent, &ExecContext::new(dir.path(), "test"))
        .unwrap();
    assert!(rendered.file_helper.is_none());
    assert!(rendered.file_request.is_some());
    let resolved = ToolRegistry::resolve(
        ToolPolicy::minimum(),
        &Default::default(),
        &NoLanguages,
        None,
    )
    .unwrap();
    let rendered = resolved
        .render(&intent, &ExecContext::new(dir.path(), "test"))
        .unwrap();
    assert_eq!(
        rendered.file_helper.unwrap(),
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("agent-runner-file-tool")
    );
    assert_eq!(
        bare.tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>(),
        [
            "shell",
            "read_file",
            "write_file",
            "list_dir",
            "find_files",
            "search_files",
            "edit_file"
        ]
    );
}
