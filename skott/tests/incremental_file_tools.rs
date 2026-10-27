//! Native primitive checks, not isolation evidence.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use sav::{CancellationToken, ToolIntent};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use skott::file_tools::{FileRequest, MAX_FILE_BYTES, execute_file_request};
use skott::session::ToolExecutor;
use skott::{ExecContext, HostExecutor, ToolPolicy, ToolRegistry};

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
    let large = vec![b'x'; MAX_FILE_BYTES];
    fs::write(root.join("large"), &large).unwrap();
    assert!(
        run(
            &root,
            "append_file",
            json!({
                "path":root.join("large"),"content":"x","expected_sha256":digest(&large)
            })
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join("large")).unwrap(), large);
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
            definition.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("expected_sha256"))
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
fn patch_result_bound_and_binary_preimages_fail_before_effects() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    let original = "a\n".repeat(MAX_FILE_BYTES / 2);
    fs::write(&path, &original).unwrap();
    assert!(run(&root, "apply_patch", json!({
        "path":path,"patch":"@@ -0,0 +1 @@\n+extra\n","expected_sha256":digest(original.as_bytes())
    })).unwrap_err().to_string().contains("file byte bound"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
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
