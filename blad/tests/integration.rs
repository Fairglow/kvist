//! Integration tests for Blad.

use std::fs;

use blad::session_manager::{SessionManager, SessionStatus};
use blad::transcript_writer::TranscriptWriter;

#[test]
fn test_session_creation() {
    let temp = tempfile::tempdir().unwrap();
    let base_dir = temp.path().join(".blad/sessions");
    fs::create_dir_all(&base_dir).unwrap();

    let session = SessionManager::new(&base_dir).unwrap();

    assert!(session.session_dir().exists());
    assert!(session.session_dir().join("manifest.json").exists());
    assert!(session.session_dir().join("messages").exists());

    let manifest = session.manifest();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.entry_count, 0);
    assert_eq!(manifest.status, SessionStatus::Streaming);
}

#[test]
fn test_message_addition() {
    let temp = tempfile::tempdir().unwrap();
    let base_dir = temp.path().join(".blad/sessions");
    fs::create_dir_all(&base_dir).unwrap();

    let session = SessionManager::new(&base_dir).unwrap();
    let mut writer = TranscriptWriter::new(session);

    writer.start_message("user", None).unwrap();
    writer.append("Hello, world!").unwrap();
    writer.flush_message(true).unwrap();

    let manifest = writer.session().manifest();
    assert_eq!(manifest.entry_count, 1);

    // Check the message file
    let messages_dir = writer.session().session_dir().join("messages");
    let files: Vec<_> = fs::read_dir(&messages_dir).unwrap().collect();
    assert_eq!(files.len(), 1);

    let entry = files[0].as_ref().unwrap();
    let content = fs::read_to_string(entry.path()).unwrap();
    assert_eq!(content, "Hello, world!");
}
