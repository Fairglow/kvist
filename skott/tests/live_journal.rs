use sav::{ToolIntent, observation::JournalFollower};
use serde_json::json;
use skott::{Recorder, RunSummary, session_transcript::SessionTranscript};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn native_prompt_journals_are_live_bounded_and_separate_from_canonical_evidence() {
    let dir = tempfile::tempdir().expect("logs");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).expect("private logs");
    let mut recorder = SessionTranscript::open(dir.path(), "fixture", None).expect("recorder");
    recorder
        .on_prompt(&"a".repeat(100_000))
        .expect("bounded prompt");
    recorder.session_start().expect("start");
    let paths = || {
        fs::read_dir(dir.path())
            .expect("logs")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| path.to_string_lossy().ends_with(".events.jsonl"))
            .collect::<Vec<_>>()
    };
    let first = paths().pop().expect("live file");
    let mut viewer = JournalFollower::open(&first).expect("viewer");
    let initial = viewer.poll().expect("initial");
    assert!(!viewer.is_finished());
    assert_eq!(initial[0].event["data"]["canonical_evidence"], false);
    let prompt = initial
        .iter()
        .find(|row| row.event["type"] == "prompt")
        .expect("prompt");
    assert_eq!(
        prompt.event["data"]["text"].as_str().expect("text").len(),
        8192
    );
    assert_eq!(prompt.event["data"]["truncated"], true);
    recorder
        .model_event(&sav::ModelStreamEvent::TextDelta("provisional".into()))
        .expect("stream");
    recorder
        .control(&skott::GuidanceNotice {
            id: skott::GuidanceId(1),
            state: skott::GuidanceState::Delivered,
        })
        .expect("control");
    let intent = ToolIntent {
        id: "call-1".into(),
        provider_id: None,
        name: "shell".into(),
        arguments: json!({"command":"sensitive-command-must-not-leak"}),
    };
    recorder
        .tool_dispatch(1, &intent)
        .expect("dispatch before effect");
    let dispatch = viewer.poll().expect("live effect boundary");
    assert!(
        dispatch
            .iter()
            .any(|row| row.event["type"] == "tool_dispatch")
    );
    assert!(
        dispatch
            .iter()
            .any(|row| row.event["type"] == "text" && row.event["provisional"] == true)
    );
    assert!(
        dispatch
            .iter()
            .any(|row| row.event["type"] == "control" && row.event["data"]["state"] == "delivered")
    );
    assert!(
        !fs::read_to_string(&first)
            .expect("journal")
            .contains("sensitive-command")
    );
    recorder
        .session_finish(&RunSummary {
            answer: Some("done".into()),
            ..Default::default()
        })
        .expect("finish");
    viewer.poll().expect("terminal");
    assert!(viewer.is_finished());
    recorder
        .on_prompt("second prompt")
        .expect("next independent run");
    let second = paths()
        .into_iter()
        .find(|path| path != &first)
        .expect("second journal");
    assert!(
        !JournalFollower::open(&second)
            .expect("viewer")
            .is_finished()
    );
    drop(recorder);
    let mut abandoned = JournalFollower::open(&second).expect("abandoned run");
    abandoned.poll().expect("partial");
    assert!(abandoned.end_of_input().is_err());
}
