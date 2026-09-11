use agent_runtime::{TrajectoryEvent, TrajectoryRecorder, compute_action_hash, replay_trajectory};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn record_and_replay_full_trajectory() {
    let dir = tempdir().expect("temp dir");
    let journal_path = dir.path().join("session_123.jsonl");

    let recorder = TrajectoryRecorder::new(&journal_path);

    // 1. Session start
    recorder
        .record_event(&TrajectoryEvent::SessionStart {
            session_id: "sess-01".to_owned(),
            task_id: "task-test".to_owned(),
            timestamp: 1772899200,
        })
        .expect("record session start");

    // 2. Turn 1 start
    recorder
        .record_event(&TrajectoryEvent::TurnStart {
            turn: 1,
            timestamp: 1772899201,
        })
        .expect("record turn 1");

    recorder
        .record_event(&TrajectoryEvent::PromptEval {
            turn: 1,
            cached_tokens: Some(1024),
            new_tokens: Some(150),
            eval_duration_ms: Some(85),
        })
        .expect("record prompt eval");

    recorder
        .record_event(&TrajectoryEvent::ModelReasoning {
            turn: 1,
            reasoning: "I should inspect the configuration file first.".to_owned(),
        })
        .expect("record reasoning");

    let args = json!({"path": "Cargo.toml"});
    let action_hash = compute_action_hash("read_file", &args);
    recorder
        .record_event(&TrajectoryEvent::ToolDispatch {
            turn: 1,
            call_id: "call_01".to_owned(),
            tool: "read_file".to_owned(),
            args,
            action_hash,
        })
        .expect("record tool dispatch");

    recorder
        .record_event(&TrajectoryEvent::ToolResult {
            turn: 1,
            call_id: "call_01".to_owned(),
            tool: "read_file".to_owned(),
            stdout: "[package]\nname = \"test\"".to_owned(),
            stderr: "".to_owned(),
            exit_code: 0,
            bytes: 25,
            state_mutated: false,
        })
        .expect("record tool result");

    recorder
        .record_event(&TrajectoryEvent::TurnFinish {
            turn: 1,
            output_tokens: Some(45),
            finish_reason: "tool_calls".to_owned(),
        })
        .expect("record turn finish");

    // 3. Turn 2 start
    recorder
        .record_event(&TrajectoryEvent::TurnStart {
            turn: 2,
            timestamp: 1772899205,
        })
        .expect("record turn 2");

    recorder
        .record_event(&TrajectoryEvent::SessionFinish {
            session_id: "sess-01".to_owned(),
            task_id: "task-test".to_owned(),
            total_turns: 2,
            total_tokens: 1219,
            success: true,
        })
        .expect("record session finish");

    // Replay full trajectory
    let report = replay_trajectory(&journal_path, None).expect("replay full trajectory");
    assert_eq!(report.session_id.as_deref(), Some("sess-01"));
    assert_eq!(report.task_id.as_deref(), Some("task-test"));
    assert_eq!(report.replayed_turns, 2);
    assert_eq!(report.tool_calls_count, 1);
    assert_eq!(report.final_success, Some(true));
    assert_eq!(report.events.len(), 9);

    // Replay capped at turn 1
    let capped_report = replay_trajectory(&journal_path, Some(1)).expect("replay capped at turn 1");
    assert_eq!(capped_report.replayed_turns, 1);
    assert_eq!(capped_report.tool_calls_count, 1);
    assert!(capped_report.events.len() < 8);
}

#[test]
fn replay_nonexistent_file_returns_error() {
    let dir = tempdir().expect("temp dir");
    let missing_path = dir.path().join("does_not_exist.jsonl");
    assert!(replay_trajectory(&missing_path, None).is_err());
}

#[test]
fn replay_malformed_json_returns_error() {
    let dir = tempdir().expect("temp dir");
    let corrupt_path = dir.path().join("corrupt.jsonl");
    std::fs::write(&corrupt_path, "not valid json\n").expect("write corrupt file");
    let error = replay_trajectory(&corrupt_path, None).expect_err("must fail on invalid json");
    assert!(
        error
            .to_string()
            .contains("malformed trajectory journal line")
    );
}
