use sav::loop_detection::{ProgressEvidence, ToolLoopKind, ToolLoopTracker};
use sav::{FinishReason, LocalModelProvider, ModelTurn, ToolIntent, runtime::ToolOutcome};

fn intent(index: usize) -> ToolIntent {
    ToolIntent {
        id: format!("call-{index}"),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": index}),
    }
}

#[test]
fn rejected_continuity_is_rolled_back_before_a_novel_correction() {
    let mut detector = sav::loop_detection::StreamLoopDetector::default();
    let mut turn = ModelTurn {
        text: (0..150).map(|n| format!("word{n} ")).collect(),
        reasoning: None,
        tool_intents: Vec::new(),
        finish_reason: FinishReason::Stop,
        provider: LocalModelProvider::Ollama,
        model: "test".into(),
        response_id: None,
        provider_request_id: None,
        usage: None,
    };
    for _ in 0..2 {
        assert!(detector.observe_turn_while(&turn, &mut || true).is_none());
    }
    let report = detector.observe_turn_while(&turn, &mut || true).unwrap();
    assert_eq!(report.period, 150);
    turn.text = (0..80).map(|n| format!("different{n} ")).collect();
    assert!(detector.observe_turn_while(&turn, &mut || true).is_none());
}
fn outcome(stdout: &[u8], stderr: &[u8], status: i32) -> ToolOutcome {
    ToolOutcome {
        exited: true,
        status: Some(status),
        stdout: stdout.into(),
        stderr: stderr.into(),
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    }
}
#[test]
fn cycles_at_32_but_not_33_actions_and_exact_128_retention() {
    let success = outcome(b"unchanged", b"", 0);
    let mut tracker = ToolLoopTracker::default();
    for n in 0..63 {
        assert!(
            tracker
                .observe(&intent(n % 32), &success, true, ProgressEvidence::None)
                .is_none()
        );
    }
    let report = tracker
        .observe(&intent(31), &success, true, ProgressEvidence::None)
        .unwrap();
    assert_eq!(report.kind, ToolLoopKind::ActionCycle);
    assert_eq!((report.period, report.copies), (32, 2));
    let mut tracker = ToolLoopTracker::default();
    for n in 0..66 {
        assert!(
            tracker
                .observe(&intent(n % 33), &success, true, ProgressEvidence::None)
                .is_none()
        );
    }
    for n in 66..300 {
        tracker.observe(&intent(n), &success, true, ProgressEvidence::None);
    }
    assert_eq!(tracker.history_len(), 128);
}
#[test]
fn only_validated_state_change_clears_effect_history() {
    let mut tracker = ToolLoopTracker::default();
    let success = outcome(b"claimed change", b"", 0);
    tracker.observe(&intent(0), &success, false, ProgressEvidence::None);
    tracker.observe(&intent(1), &success, false, ProgressEvidence::None);
    tracker.observe(&intent(0), &success, false, ProgressEvidence::None);
    assert!(tracker.propose(&intent(0), false).is_some());
    tracker.observe(&intent(2), &success, false, ProgressEvidence::StateChanged);
    assert!(tracker.propose(&intent(0), false).is_none());
    assert_eq!(tracker.history_len(), 1);
}
#[test]
fn failure_status_and_output_boundaries_are_not_conflated() {
    let mut tracker = ToolLoopTracker::default();
    for result in [
        outcome(b"ab", b"c", 1),
        outcome(b"a", b"bc", 1),
        outcome(b"ab", b"c", 2),
    ] {
        assert!(
            tracker
                .observe(&intent(0), &result, true, ProgressEvidence::None)
                .is_none()
        );
    }
}
#[test]
fn failure_recurrence_includes_different_refused_proposals() {
    let mut tracker = ToolLoopTracker::default();
    let failure = ToolOutcome::rejected_with("unsupported capability");
    for n in 0..2 {
        assert!(
            tracker
                .observe(&intent(n), &failure, true, ProgressEvidence::None)
                .is_none()
        );
    }
    let report = tracker
        .observe(&intent(2), &failure, true, ProgressEvidence::None)
        .unwrap();
    assert_eq!(report.kind, ToolLoopKind::RepeatedFailure);
    assert_eq!(report.copies, 3);
}
#[test]
fn changed_bytes_are_observed_before_any_read_rejection() {
    let mut tracker = ToolLoopTracker::default();
    for n in 0..100 {
        assert!(tracker.propose(&intent(0), true).is_none());
        assert!(
            tracker
                .observe(
                    &intent(0),
                    &outcome(format!("revision {n}").as_bytes(), b"", 0),
                    true,
                    ProgressEvidence::None,
                )
                .is_none()
        );
    }
}
