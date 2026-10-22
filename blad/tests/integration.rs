//! Integration test: Skott writes, Blad reads.
//!
//! Verifies that Blad can read Skott's structured session transcripts.

use std::fs;
use std::path::Path;

fn create_test_session(base_dir: &Path, id: &str, created_at: &str) {
    let session_dir = base_dir.join(id);
    fs::create_dir_all(&session_dir).unwrap();

    // Create session.json manifest
    let manifest = serde_json::json!({
        "schema_version": 1,
        "kind": "skott-conversation",
        "session_id": id,
        "canonical_evidence": false,
        "status": "completed",
        "markdown_profile": "commonmark-gfm-subset-v1",
        "model": "test-model",
        "created_at": created_at,
        "completed_at": "2026-10-09T10:05:00Z",
        "entries": [
            {
                "id": "message-000001",
                "sequence": 1,
                "kind": "user",
                "media_type": "text/markdown",
                "path": "messages/000001.md",
                "bytes": 30,
                "complete": true,
                "truncated": false
            },
            {
                "id": "message-000002",
                "sequence": 2,
                "kind": "assistant",
                "disposition": "final",
                "media_type": "text/markdown",
                "path": "messages/000002.md",
                "bytes": 50,
                "complete": true,
                "truncated": false
            }
        ]
    });
    fs::write(
        session_dir.join("session.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Create messages
    let messages_dir = session_dir.join("messages");
    fs::create_dir_all(&messages_dir).unwrap();
    fs::write(
        messages_dir.join("000001.md"),
        "Please help me with this task.",
    )
    .unwrap();
    fs::write(
        messages_dir.join("000002.md"),
        "Here's the solution to your problem.\n\n```rust\nfn main() { println!(\"hello\"); }\n```",
    )
    .unwrap();
}

#[test]
fn blad_reads_skott_structured_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let log_dir = temp.path().join("runs");
    fs::create_dir_all(&log_dir).unwrap();

    let session_id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(&log_dir, session_id, "2026-10-09T10:00:00Z");

    // Test listing
    let sessions = blad::session::list_sessions(&log_dir, 10);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session_id);
    assert_eq!(sessions[0].message_count, 2);
    assert_eq!(sessions[0].model, Some("test-model".to_string()));
    assert!(sessions[0].completed);

    // Test loading
    let loaded = blad::session::load_session(&log_dir, session_id).unwrap();
    assert_eq!(loaded.id, session_id);

    // Test message loading
    let messages = blad::session::load_messages(&loaded);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind, "user");
    assert_eq!(messages[0].content, "Please help me with this task.");
    assert_eq!(messages[1].kind, "assistant");
    assert!(messages[1].content.contains("solution"));
}

#[test]
fn blad_lists_sessions_newest_first() {
    let temp = tempfile::tempdir().unwrap();
    let log_dir = temp.path().join("runs");
    fs::create_dir_all(&log_dir).unwrap();

    // All three sessions share the same created_at; the listing must
    // therefore deterministically tie-break by id, independent of directory
    // enumeration order.
    create_test_session(
        &log_dir,
        "2026-10-09T10-00-00Z-1234-0",
        "2026-10-09T10:00:00Z",
    );
    create_test_session(
        &log_dir,
        "2026-10-09T11-00-00Z-1235-0",
        "2026-10-09T10:00:00Z",
    );
    create_test_session(
        &log_dir,
        "2026-10-09T12-00-00Z-1236-0",
        "2026-10-09T10:00:00Z",
    );

    let sessions = blad::session::list_sessions(&log_dir, 10);
    assert_eq!(sessions.len(), 3);
    // Newest first (equal timestamps tie-break by id descending)
    assert!(sessions[0].id.starts_with("2026-10-09T12"));
    assert!(sessions[1].id.starts_with("2026-10-09T11"));
    assert!(sessions[2].id.starts_with("2026-10-09T10"));
}

#[test]
fn blad_lists_sessions_by_created_at_when_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let log_dir = temp.path().join("runs");
    fs::create_dir_all(&log_dir).unwrap();

    // Distinct created_at values, deliberately not matching the id
    // timestamps, to prove the primary sort key is created_at.
    create_test_session(
        &log_dir,
        "2026-10-09T10-00-00Z-1234-0",
        "2026-10-09T08:00:00Z",
    );
    create_test_session(
        &log_dir,
        "2026-10-09T11-00-00Z-1235-0",
        "2026-10-09T09:00:00Z",
    );
    create_test_session(
        &log_dir,
        "2026-10-09T12-00-00Z-1236-0",
        "2026-10-09T07:00:00Z",
    );

    let sessions = blad::session::list_sessions(&log_dir, 10);
    assert_eq!(sessions.len(), 3);
    // Newest created_at first, regardless of id order
    assert!(sessions[0].id.starts_with("2026-10-09T11"));
    assert!(sessions[1].id.starts_with("2026-10-09T10"));
    assert!(sessions[2].id.starts_with("2026-10-09T12"));
}

#[test]
fn blad_loads_session_by_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let log_dir = temp.path().join("runs");
    fs::create_dir_all(&log_dir).unwrap();

    let session_id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(&log_dir, session_id, "2026-10-09T10:00:00Z");

    // Should match by prefix
    let loaded = blad::session::load_session(&log_dir, "2026-10-09T10").unwrap();
    assert_eq!(loaded.id, session_id);
}

#[test]
fn blad_export_session() {
    let temp = tempfile::tempdir().unwrap();
    let log_dir = temp.path().join("runs");
    fs::create_dir_all(&log_dir).unwrap();

    let session_id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(&log_dir, session_id, "2026-10-09T10:00:00Z");

    let loaded = blad::session::load_session(&log_dir, session_id).unwrap();

    // Export to file
    let output = temp.path().join("export.md");
    blad::display::export_session(&loaded, Some(&output)).unwrap();
    let exported = fs::read_to_string(&output).unwrap();

    assert!(exported.contains("Session Transcript"));
    assert!(exported.contains("Please help me with this task."));
    assert!(exported.contains("solution"));
    assert!(exported.contains("User"));
    assert!(exported.contains("Assistant"));
}

#[test]
fn every_session_route_uses_the_same_escape_free_dump_policy() {
    let temp = tempfile::tempdir().unwrap();
    let id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(temp.path(), id, "2026-10-09T10:00:00Z");
    fs::write(
        temp.path().join(id).join("messages/000002.md"),
        "# Answer\n\n**bold**\n\n```rust\nfn main() {}\n```\n\nH~2~O $x^2$\n\n```mermaid\nflowchart LR\nA[Input] --> B[Output]\n```",
    )
    .unwrap();
    for route in [vec!["show", id], vec!["stream", id], vec![id]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_blad"))
            .arg("--log-dir")
            .arg(temp.path())
            .args(["--theme", "light", "--color", "always", "--dump"])
            .args(route)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains('\x1b'), "{text:?}");
        assert!(text.contains("# Answer"));
        assert!(text.contains("bold") && !text.contains("**bold**"));
        assert!(text.contains("fn main()"));
        assert!(text.contains("H₂O x²") && text.contains('┌'), "{text:?}");
    }
}

#[test]
fn listing_session_metadata_cannot_emit_terminal_controls() {
    let temp = tempfile::tempdir().unwrap();
    let id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(temp.path(), id, "2026-10-09T10:00:00Z");
    let manifest_path = temp.path().join(id).join("session.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["model"] = serde_json::json!("model\u{1b}[2J\u{7}");
    manifest["session_id"] = serde_json::json!("id\u{1b}[31m\u{7}");
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_blad"))
        .arg("--log-dir")
        .arg(temp.path())
        .args(["--dump", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains('\x1b') && !text.contains('\x07'), "{text:?}");
    assert!(text.contains("\\u{1b}[2J\\u{7}"));
    assert!(text.contains("id\\u{1b}[31m\\u{7}"), "{text:?}");
}

#[test]
fn export_preserves_source_math_scripts_and_diagrams_even_with_render_flags() {
    let temp = tempfile::tempdir().unwrap();
    let id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(temp.path(), id, "2026-10-09T10:00:00Z");
    let source = "H~2~O $\\frac{1}{2}$\n\n```mermaid\nflowchart LR\nA --> B\n```";
    fs::write(temp.path().join(id).join("messages/000002.md"), source).unwrap();
    let path = temp.path().join("export.md");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_blad"))
        .arg("--log-dir")
        .arg(temp.path())
        .args([
            "--dump",
            "--math",
            "unicode",
            "--mermaid",
            "text",
            "export",
            id,
            "--output",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let exported = fs::read_to_string(path).unwrap();
    assert!(exported.contains(source) && !exported.contains('┌') && !exported.contains("H₂O"));
}

#[test]
fn export_keeps_a_final_line_without_a_blank_line_from_becoming_a_setext_heading() {
    // A message whose last line is not blank, immediately followed by the
    // `---` separator, must not be read back as a setext heading.
    let temp = tempfile::tempdir().unwrap();
    let id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(temp.path(), id, "2026-10-09T10:00:00Z");
    fs::write(
        temp.path().join(id).join("messages/000001.md"),
        "Please help me with this task.", // no trailing newline
    )
    .unwrap();
    let path = temp.path().join("export.md");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_blad"))
        .arg("--log-dir")
        .arg(temp.path())
        .args(["--dump", "export", id, "--output"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let exported = fs::read_to_string(path).unwrap();
    // The message body must remain a paragraph, not a `## Please help me...`
    // heading, and must be separated from the `---` rule by a blank line.
    assert!(
        exported.contains("Please help me with this task.\n\n---"),
        "message body must be separated from the rule by a blank line:\n{exported}"
    );
    assert!(
        !exported.contains("## Please help"),
        "message body must not become a setext heading:\n{exported}"
    );
}

#[test]
fn dump_shows_interim_disposition_and_sanitises_unknown_kinds() {
    let temp = tempfile::tempdir().unwrap();
    let id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(temp.path(), id, "2026-10-09T10:00:00Z");
    // Give the assistant message an `interim` disposition and add a message
    // whose kind carries a terminal control escape.
    let messages_dir = temp.path().join(id).join("messages");
    fs::write(messages_dir.join("000002.md"), "Here is an interim answer.").unwrap();
    fs::write(messages_dir.join("000003.md"), "tool output").unwrap();
    let manifest_path = temp.path().join(id).join("session.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["entries"][1]["disposition"] = serde_json::json!("interim");
    manifest["entries"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "message-000003",
            "sequence": 3,
            "kind": "tool\x1b[2J\x07result",
            "media_type": "text/markdown",
            "path": "messages/000003.md",
            "bytes": 5,
            "complete": true,
            "truncated": false
        }));
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_blad"))
        .arg("--log-dir")
        .arg(temp.path())
        .args(["--dump", id])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let text = String::from_utf8(output.stdout).unwrap();
    // The interim disposition must be visible, not silently dropped.
    assert!(
        text.contains("[interim]"),
        "interim disposition must be shown:\n{text}"
    );
    // The control codes in the unknown kind must be escaped, not emitted raw.
    assert!(
        !text.contains('\x1b') && !text.contains('\x07'),
        "control codes in an unknown kind must be sanitised:\n{text:?}"
    );
    assert!(text.contains("tool\\u{1b}[2J\\u{7}result"), "{text:?}");
}
