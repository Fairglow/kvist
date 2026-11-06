use super::*;

fn repetitive(words: usize) -> String {
    (0..words)
        .map(|n| format!("term{n} "))
        .collect::<String>()
        .repeat(4)
}

#[test]
fn bridge_discards_repetitive_tool_turns_and_preserves_goal_and_pairing() {
    let repeated = tool_turn(&repetitive(150), "shell", json!({"command":"echo partial"}));
    let transport = CapturingTransport::new(vec![
        repeated.clone(),
        repeated,
        answer_turn("Supported completion."),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("Keep the original requested goal.");
    let sink = Collector::default();
    let token = CancellationToken::new();
    let mut recorder = FakeRecorder::default();
    let summary = Skott::default()
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &token,
            &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            Some(&mut recorder),
        )
        .unwrap();
    assert!(summary.success(), "{summary:?}");
    assert!(executor.executed().is_empty());
    assert!(!token.is_cancelled());
    let requests = transport.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| {
        request.max_output_tokens == Some(1024)
            && carries_user_message(&request.messages, "Keep the original requested goal.")
            && unanswered_tool_calls(&request.messages) == 0
    }));
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event,
                Event::Note(text) if text.contains("Recovery ") && text.contains("reasoning")
            ))
            .count(),
        2
    );
    assert!(recorder.session_finished.load(Ordering::SeqCst));
    assert_eq!(recorder.terminal_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn persistent_bridge_repetition_is_failed_not_cancelled_or_successful() {
    let repeated = tool_turn(&repetitive(200), "shell", json!({"command":"echo partial"}));
    let transport = CapturingTransport::new(vec![repeated; 3]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("Finish the requested goal.");
    let sink = Collector::default();
    let token = CancellationToken::new();
    let summary = Skott::default()
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &token,
            &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
        )
        .unwrap();
    assert!(!summary.success(), "{summary:?}");
    assert!(!summary.cancelled);
    assert!(!token.is_cancelled());
    assert!(summary.answer.is_none());
    assert!(summary.failure.as_deref().unwrap().contains("repetitive"));
    assert!(executor.executed().is_empty());
    assert_eq!(transport.requests().len(), 3);
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, Event::Failed { .. }))
            .count(),
        1
    );
}

#[test]
fn explicit_bridge_exception_preserves_intentionally_repeated_output() {
    let text = repetitive(100);
    let transport = CapturingTransport::new(vec![answer_turn(&text)]);
    let mut session = make_session();
    session.push_user("Produce intentionally repeated output.");
    let summary = Skott::default()
        .with_generation_loop_detection(false)
        .run(
            &mut session,
            &transport,
            &RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp")),
            &Collector::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
        )
        .unwrap();
    assert!(summary.success(), "{summary:?}");
    assert_eq!(summary.answer.as_deref(), Some(text.as_str()));
    assert_eq!(transport.requests().len(), 1);
}
