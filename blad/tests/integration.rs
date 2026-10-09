//! Integration test: Skott writes, Blad reads.
//!
//! Verifies that Blad can read Skott's structured session transcripts.

use std::fs;
use std::path::Path;

fn create_test_session(base_dir: &Path, id: &str) {
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
        "created_at": "2026-10-09T10:00:00Z",
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
    create_test_session(&log_dir, session_id);

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

    create_test_session(&log_dir, "2026-10-09T10-00-00Z-1234-0");
    create_test_session(&log_dir, "2026-10-09T11-00-00Z-1235-0");
    create_test_session(&log_dir, "2026-10-09T12-00-00Z-1236-0");

    let sessions = blad::session::list_sessions(&log_dir, 10);
    assert_eq!(sessions.len(), 3);
    // Newest first
    assert!(sessions[0].id.starts_with("2026-10-09T12"));
    assert!(sessions[1].id.starts_with("2026-10-09T11"));
    assert!(sessions[2].id.starts_with("2026-10-09T10"));
}

#[test]
fn blad_loads_session_by_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let log_dir = temp.path().join("runs");
    fs::create_dir_all(&log_dir).unwrap();

    let session_id = "2026-10-09T10-00-00Z-1234-0";
    create_test_session(&log_dir, session_id);

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
    create_test_session(&log_dir, session_id);

    let loaded = blad::session::load_session(&log_dir, session_id).unwrap();

    // Export to file
    let output = temp.path().join("export.md");
    blad::display::export_session(&loaded, Some(&output));
    let exported = fs::read_to_string(&output).unwrap();

    assert!(exported.contains("Session Transcript"));
    assert!(exported.contains("Please help me with this task."));
    assert!(exported.contains("solution"));
    assert!(exported.contains("User"));
    assert!(exported.contains("Assistant"));
}
