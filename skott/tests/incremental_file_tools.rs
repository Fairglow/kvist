//! Native primitive checks, not isolation evidence.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use sav::{CancellationToken, ToolIntent};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use skott::file_tools::{FileRequest, execute_file_request};
use skott::session::ToolExecutor;
use skott::{ExecContext, HostExecutor, ToolPolicy, ToolRegistry};

#[test]
fn live_large_reads_adjust_moved_utf8_offsets_and_shrunken_files() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let large = "x".repeat(2 * 1024 * 1024);
    fs::write(&path, &large).unwrap();
    let out = run(
        &root,
        "read_file",
        json!({"path":path,"offset":1048577,"limit":3}),
    )
    .unwrap();
    assert_eq!(out["content"], "xxx");
    assert_eq!(out["total_bytes"], large.len());
    assert!(out["sha256"].is_null());
    assert_eq!(out["page_sha256"], digest(b"xxx"));
    fs::write(&path, "aéz").unwrap();
    let moved = run(&root, "read_file", json!({"path":path,"offset":2})).unwrap();
    assert_eq!(moved["offset"], 3);
    assert_eq!(moved["requested_offset"], 2);
    assert_eq!(moved["offset_adjustment"], "utf8_boundary");
    assert_eq!(moved["content"], "z");
    let shrunk = run(&root, "read_file", json!({"path":path,"offset":1048577})).unwrap();
    assert_eq!(shrunk["offset"], 4);
    assert_eq!(shrunk["offset_adjustment"], "past_eof");
    assert_eq!(shrunk["content"], "");
}

#[test]
fn live_large_mutations_are_optional_guarded_streaming_and_mode_preserving() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let before = format!("{}needle\r\nlast", "x\n".repeat(1_048_580));
    fs::write(&path, &before).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    run(
        &root,
        "edit_file",
        json!({"path":path,"old_text":"needle","new_text":"changed"}),
    )
    .unwrap();
    run(
        &root,
        "apply_patch",
        json!({
            "path":path,"patch":"@@ -1048581 +1048581 @@\n-changed\r\n+patched\r\n"
        }),
    )
    .unwrap();
    let result = run(&root, "append_file", json!({"path":path,"content":" tail"})).unwrap();
    let expected = before.replace("needle", "patched") + " tail";
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    assert_eq!(result["sha256"], digest(expected.as_bytes()));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    run(
        &root,
        "write_file",
        json!({"path":path,"content":"small","expected_sha256":result["sha256"]}),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "small");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn live_large_edits_match_across_chunks_and_reject_late_overlaps_without_publication() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let source = format!(
        "{}aéneedle{}",
        "x".repeat(8190),
        "y".repeat(2 * 1024 * 1024)
    );
    fs::write(&path, &source).unwrap();
    run(
        &root,
        "edit_file",
        json!({"path":path,"old_text":"aéneedle","new_text":"replacement"}),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        source.replace("aéneedle", "replacement")
    );
    let ambiguous = format!("{}aaa", "x".repeat(2 * 1024 * 1024));
    fs::write(&path, &ambiguous).unwrap();
    assert!(
        run(
            &root,
            "edit_file",
            json!({"path":path,"old_text":"aa","new_text":"replacement"})
        )
        .unwrap_err()
        .to_string()
        .contains("overlapping")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), ambiguous);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn live_search_continuation_reaches_past_budget_and_cross_boundary_multiline_queries() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let budget = 8 * 1024 * 1024;
    let mut before = "x".repeat(budget - 3);
    before.push_str("aé\nneedle\n");
    before.push_str(&"x".repeat(8190));
    before.push_str("aé\nneedle\n");
    fs::write(&path, before).unwrap();
    let first = run(
        &root,
        "search_files",
        json!({"path":path,"query":"aé\nneedle"}),
    )
    .unwrap();
    assert_eq!(first["scan_limit_reached"], true);
    let next = &first["next_scan"];
    assert_eq!(next["path"], json!(path));
    assert!(next["offset"].as_u64().unwrap() > 0);
    let second = run(
        &root,
        "search_files",
        json!({"path":next["path"],"scan_offset":next["offset"],"query":"aé\nneedle"}),
    )
    .unwrap();
    let offsets: Vec<u64> = first["matches"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["matches"].as_array().unwrap())
        .map(|m| m["match_byte_offset"].as_u64().unwrap())
        .collect();
    assert_eq!(offsets, vec![(budget - 3) as u64, (budget + 8198) as u64]);
    assert_eq!(second["scan_limit_reached"], false);
    assert!(second["next_scan"].is_null());
    assert_eq!(
        second["complete"], false,
        "a resumed scan does not cover the omitted prefix"
    );
}

#[test]
fn live_sparse_large_search_has_bounded_work_and_no_size_skip() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("sparse");
    fs::File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let result = run(&root, "search_files", json!({"path":path,"query":"absent"})).unwrap();
    assert_eq!(result["skipped_oversized"], 0);
    assert_eq!(result["skipped_binary"], 1);
    assert!(result["scanned_bytes"].as_u64().unwrap() <= 8 * 1024 * 1024);
}

#[test]
fn live_search_split_buffer_queries_keep_exact_line_context_and_truthful_deep_offsets() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let text = format!("{}\naé\nneedle\r\nlast", "x".repeat(8188));
    fs::write(&path, &text).unwrap();
    let result = run(
        &root,
        "search_files",
        json!({"path":path,"query":"aé\nneedle"}),
    )
    .unwrap();
    assert_eq!(result["matches"][0]["line"], 2);
    assert_eq!(result["matches"][0]["match_byte_offset"], 8189);
    assert_eq!(result["matches"][0]["content"], "aé");
    assert_eq!(result["matches"][0]["truncated"], false);
    let deep = format!("{}aé\nneedle\r\nlast", "x".repeat(8 * 1024 * 1024 + 8190));
    fs::write(&path, &deep).unwrap();
    let result = run(
        &root,
        "search_files",
        json!({"path":path,"query":"aé\nneedle","scan_offset":8 * 1024 * 1024 + 8190}),
    )
    .unwrap();
    assert!(result["matches"][0]["line"].is_null());
    assert_eq!(result["matches"][0]["line_in_scan"], 1);
    assert_eq!(
        result["matches"][0]["preview_offset"],
        8 * 1024 * 1024 + 8190
    );
    assert_eq!(result["prefix_scanned_bytes"], 0);
    assert_eq!(result["complete"], false);
    for args in [
        json!({"path":root,"query":"needle","scan_offset":0}),
        json!({"path":path,"query":"needle","scan_offset":null}),
    ] {
        assert!(run(&root, "search_files", args).is_err());
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn run(root: &Path, tool: &str, args: Value) -> skott::Result<Value> {
    execute_file_request(&FileRequest::new(root.to_str().unwrap(), tool, args)?)
}

#[test]
fn append_is_verbatim_atomic_mode_preserving_and_digest_bound() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    fs::write(&path, "aé\r\nlast").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let args =
        json!({"path":path,"content":" tail\n","expected_sha256":digest("aé\r\nlast".as_bytes())});
    let out = run(&root, "append_file", args.clone()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), "aé\r\nlast tail\n".as_bytes());
    assert_eq!(out["sha256"], digest("aé\r\nlast tail\n".as_bytes()));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(run(&root, "append_file", args).is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn patch_applies_multiple_exact_hunks_preserving_crlf_and_no_final_newline() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let original = "first\r\nold\r\nmiddle\r\nlast";
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o751)).unwrap();
    let patch = "@@ -1,2 +1,3 @@ section\n first\r\n-old\r\n+new\r\n+extra\r\n@@ -4 +5 @@\n-last\n\\ No newline at end of file\n+end\n\\ No newline at end of file\n";
    let out = run(
        &root,
        "apply_patch",
        json!({
            "path":path,"patch":patch,"expected_sha256":digest(original.as_bytes())
        }),
    )
    .unwrap();
    let expected = b"first\r\nnew\r\nextra\r\nmiddle\r\nend";
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(out["sha256"], digest(expected));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o751
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn patch_handles_empty_file_insertions_deletions_and_default_counts() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    for (before, patch, after) in [
        ("", "@@ -0,0 +1,2 @@\n+a\n+b\n", "a\nb\n"),
        ("a\nb\n", "@@ -2 +1,0 @@\n-b\n", "a\n"),
        ("a\n", "@@ -1 +1 @@\n-a\n+é\n", "é\n"),
        (
            "a",
            "@@ -1 +1,2 @@\n-a\n\\ No newline at end of file\n+a\n+b\n",
            "a\nb\n",
        ),
        ("a\n", "@@ -1,0 +2 @@\n+b\n", "a\nb\n"),
        ("a\n", "@@ -1 +0,0 @@\n-a\n", ""),
    ] {
        fs::write(&path, before).unwrap();
        run(
            &root,
            "apply_patch",
            json!({
                "path":path,"patch":patch,"expected_sha256":digest(before.as_bytes())
            }),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), after, "{patch}");
    }
}

#[test]
fn invalid_patches_never_change_the_preimage() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let original = b"a\nb\n";
    fs::write(&path, original).unwrap();
    for patch in [
        "",
        "--- /outside\n+++ /workspace/text\n@@ -1 +1 @@\n-a\n+x\n",
        "@@ -1 +1 @@\n-wrong\n+x\n",
        "@@ -2 +1 @@\n-b\n+x\n",
        "@@ -1,2 +1 @@\n-a\n+x\n",
        "@@ -1 +1,2 @@\n-a\n+x\n",
        "@@ -0 +1 @@\n-a\n+x\n",
        "@@ -999999999999999999999999999 +1 @@\n-a\n+x\n",
        "@@ -1 +1 @@\n-a\n+x\n@@ -1 +1 @@\n-a\n+y\n",
        "@@ -1 +1 @@\n-a\n+x\n@@ -2 +2 @@\n-wrong\n+y\n",
        "@@ -1 +1 @@\n-a\n+x", // transport line missing LF
        "@@ -1 +1 @@\n\\ No newline at end of file\n-a\n+x\n",
        "@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+x\n",
        "@@ -1 +1 @@\n a\n", // no actual change
    ] {
        assert!(
            run(
                &root,
                "apply_patch",
                json!({
                    "path":path,"patch":patch,"expected_sha256":digest(original)
                })
            )
            .is_err(),
            "{patch:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }
}

#[test]
fn incremental_tools_reject_links_bounds_stale_and_missing_preimages() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("real")).unwrap();
    fs::write(root.join("real/text"), b"a\n").unwrap();
    symlink("real", root.join("link")).unwrap();
    symlink("real/text", root.join("alias")).unwrap();
    for tool in ["append_file", "apply_patch"] {
        for path in [
            root.join("link/text"),
            root.join("alias"),
            root.join("missing"),
        ] {
            let mut args = json!({"path":path,"expected_sha256":digest(b"a\n")});
            args[if tool == "append_file" {
                "content"
            } else {
                "patch"
            }] = json!(if tool == "append_file" {
                "x"
            } else {
                "@@ -1 +1 @@\n-a\n+x\n"
            });
            assert!(run(&root, tool, args).is_err());
        }
        let field = if tool == "append_file" {
            "content"
        } else {
            "patch"
        };
        for value in [Value::Null, json!("x".repeat(65537))] {
            let mut args = json!({"path":root.join("real/text"),"expected_sha256":digest(b"a\n")});
            args[field] = value;
            assert!(FileRequest::new(root.to_str().unwrap(), tool, args).is_err());
        }
        let mut args = json!({"path":root.join("real/text"),"expected_sha256":digest(b"stale")});
        args[field] = json!(if tool == "append_file" {
            "x"
        } else {
            "@@ -1 +1 @@\n-a\n+x\n"
        });
        assert!(run(&root, tool, args).is_err());
    }
    let large = vec![b'x'; 1024 * 1024];
    fs::write(root.join("large"), &large).unwrap();
    run(
        &root,
        "append_file",
        json!({
            "path":root.join("large"),"content":"x","expected_sha256":digest(&large)
        }),
    )
    .unwrap();
    assert_eq!(
        fs::metadata(root.join("large")).unwrap().len(),
        large.len() as u64 + 1
    );
    assert_eq!(fs::read(root.join("real/text")).unwrap(), b"a\n");
}

#[test]
fn new_tools_render_without_effects_and_use_the_installed_host_helper() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::write(root.join("text"), b"a\n").unwrap();
    let registry = ToolRegistry::new(ToolPolicy::minimum())
        .with_file_helper(env!("CARGO_BIN_EXE_skott-file-tool"));
    let definitions = registry.tool_definitions();
    for name in ["append_file", "apply_patch"] {
        let definition = definitions.iter().find(|d| d.name == name).unwrap();
        assert_eq!(definition.parameters["additionalProperties"], false);
        assert!(
            !definition.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("expected_sha256"))
        );
        assert!(
            definition.parameters["properties"]
                .get("expected_sha256")
                .is_some()
        );
        let field = if name == "append_file" {
            "content"
        } else {
            "patch"
        };
        let mut args = json!({"path":"/workspace/text","expected_sha256":digest(b"a\n")});
        args[field] = json!(if name == "append_file" {
            "b\n"
        } else {
            "@@ -1 +1 @@\n-a\n+x\n"
        });
        let intent = ToolIntent {
            id: "../hostile".into(),
            provider_id: None,
            name: name.into(),
            arguments: args,
        };
        let rendered = registry
            .render(&intent, &ExecContext::new(&root, "id"))
            .unwrap();
        assert_eq!(rendered.argv, ["/context/1", "/context/0"]);
        assert!(rendered.file_request.is_some());
        for path in [
            "/outside/text",
            "/workspace-evil/text",
            "/workspace/../text",
        ] {
            let mut denied = intent.clone();
            denied.arguments["path"] = json!(path);
            assert!(matches!(
                registry.render(&denied, &ExecContext::new(&root, "id")),
                Err(skott::Error::ToolPolicy { .. })
            ));
        }
        fs::write(root.join("text"), b"a\n").unwrap();
        let outcome = HostExecutor::new(registry.clone(), root.clone())
            .execute(&intent, &CancellationToken::new())
            .unwrap();
        assert_eq!(outcome.status, Some(0), "{}", outcome.error_text(4096));
        assert_eq!(
            fs::read(root.join("text")).unwrap(),
            if name == "append_file" {
                b"a\nb\n".as_slice()
            } else {
                b"x\n".as_slice()
            }
        );
    }
}

#[test]
fn read_digest_guard_rejects_stale_continuation() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    fs::write(&path, b"changed").unwrap();
    let error = run(
        &root,
        "read_file",
        json!({
            "path":path,"offset":2,"expected_sha256":digest(b"original")
        }),
    )
    .unwrap_err();
    assert!(error.to_string().contains("offset: 0"), "{error}");
}

#[test]
fn patch_can_grow_large_results_and_binary_preimages_fail_before_effects() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let original = "a\n".repeat(1024 * 1024 / 2);
    fs::write(&path, &original).unwrap();
    run(&root, "apply_patch", json!({
        "path":path,"patch":"@@ -0,0 +1 @@\n+extra\n","expected_sha256":digest(original.as_bytes())
    })).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("extra\n{original}")
    );
    fs::write(&path, [0xff]).unwrap();
    for (tool, extra) in [
        ("append_file", json!({"content":"x"})),
        ("apply_patch", json!({"patch":"@@ -1 +1 @@\n-a\n+x\n"})),
    ] {
        let mut args = extra;
        args["path"] = json!(path);
        args["expected_sha256"] = json!(digest(&[0xff]));
        assert!(
            run(&root, tool, args)
                .unwrap_err()
                .to_string()
                .contains("UTF-8")
        );
        assert_eq!(fs::read(&path).unwrap(), [0xff]);
    }
}
