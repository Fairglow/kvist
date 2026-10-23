use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use serde_json::Value;
use skott::session_transcript::SessionTranscript;
use skott::transcript_writer::TranscriptWriter;
use skott::{Recorder, RunSummary};

#[test]
fn structured_recording_cannot_redirect_host_writes_through_links() {
    let fixture = tempfile::tempdir().expect("fixture");
    let logs = fixture.path().join("logs");
    let victim = fixture.path().join("victim");
    fs::write(&victim, "sentinel").expect("victim");
    let mut writer = TranscriptWriter::new("probe", &logs, None).expect("writer");
    symlink(&victim, writer.session_dir().join("session.json")).expect("manifest link");
    writer
        .record_notice("first")
        .expect("atomic manifest replacement");
    assert_eq!(fs::read_to_string(&victim).expect("victim"), "sentinel");
    assert!(
        !fs::symlink_metadata(writer.session_dir().join("session.json"))
            .expect("manifest")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::metadata(writer.session_dir().join("session.json"))
            .expect("private manifest")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    symlink(&victim, writer.session_dir().join("messages/000002.md")).expect("message link");
    assert!(writer.record_notice("must not overwrite").is_err());
    assert_eq!(fs::read_to_string(victim).expect("victim"), "sentinel");
}

#[test]
fn structured_recording_pins_its_original_directory_descriptors() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut writer =
        TranscriptWriter::new("probe", &fixture.path().join("logs"), None).expect("writer");
    let original = writer.session_dir().join("messages");
    let held = writer.session_dir().join("original-messages");
    let unrelated = fixture.path().join("unrelated");
    fs::create_dir(&unrelated).expect("unrelated directory");
    fs::rename(&original, &held).expect("rename original");
    symlink(&unrelated, &original).expect("replacement link");
    writer.record_notice("held descriptor").expect("record");
    assert!(!unrelated.join("000001.md").exists());
    assert_eq!(
        fs::read_to_string(held.join("000001.md")).expect("held record"),
        "held descriptor"
    );
}

#[test]
fn continuing_native_transcript_has_one_final_and_resets_its_lifecycle() {
    let fixture = tempfile::tempdir().expect("fixture");
    let logs = fixture.path().join("logs");
    let mut recorder = SessionTranscript::open(&logs, "fixture", None).expect("recorder");
    recorder.on_prompt("first").expect("prompt");
    recorder.session_start().expect("start");
    recorder
        .session_finish(&RunSummary {
            answer: Some("done".into()),
            ..Default::default()
        })
        .expect("finish");
    let manifest = fs::read_dir(&logs)
        .expect("logs")
        .map(|entry| entry.expect("entry").path().join("session.json"))
        .find(|path| path.is_file())
        .expect("structured manifest");
    let read = || -> Value {
        serde_json::from_slice(&fs::read(&manifest).expect("manifest")).expect("JSON")
    };
    let completed = read();
    assert_eq!(
        completed["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .filter(|entry| entry["kind"] == "assistant" && entry["disposition"] == "final")
            .count(),
        1
    );
    assert_eq!(completed["status"], "completed");
    recorder.on_prompt("second").expect("next prompt");
    let active = read();
    assert_eq!(active["status"], "streaming");
    assert!(active["completed_at"].is_null());
}

#[test]
fn unicode_prompt_slug_is_bounded_without_splitting_a_character() {
    let fixture = tempfile::tempdir().expect("fixture");
    let mut log =
        skott::session_log::SessionLog::open(&fixture.path().join("logs"), "unicode").expect("log");
    log.annotate_prompt(&"界".repeat(14)).expect("Unicode slug");
    assert!(log.journal_path().exists());
}

#[test]
fn structured_sessions_are_single_component_and_no_clobber() {
    let fixture = tempfile::tempdir().expect("fixture");
    let logs = fixture.path().join("logs");
    let _writer = TranscriptWriter::new("probe", &logs, None).expect("writer");
    assert!(TranscriptWriter::new("probe", &logs, None).is_err());
    assert!(TranscriptWriter::new("../escape", &logs, None).is_err());
    assert!(!fixture.path().join("escape").exists());
}
