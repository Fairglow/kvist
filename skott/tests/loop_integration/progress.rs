use super::*;
use skott::HostExecutor;

fn native_host(root: &std::path::Path) -> HostExecutor {
    HostExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum())
            .with_file_helper(env!("CARGO_BIN_EXE_skott-toerne")),
        root.to_owned(),
    )
}

fn run_progress(turns: Vec<ModelTurn>, executor: &impl ToolExecutor) -> (RunSummary, Collector) {
    let mut session = make_session();
    session.push_user("verify each relevant revision without repeating stalled work");
    let sink = Collector::default();
    let summary = Skott::default()
        .run(
            &mut session,
            &ScriptedTransport::new(turns),
            executor,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::for_model(32_768),
            None,
        )
        .unwrap();
    (summary, sink)
}

#[test]
fn phase3_verification_after_actual_native_edits_executes_three_times() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("text"), "first").unwrap();
    let verify = tool_turn("verify", "shell", json!({"command":"printf verified"}));
    let (summary, sink) = run_progress(
        vec![
            verify.clone(),
            tool_turn(
                "edit first revision",
                "write_file",
                json!({"path":"/workspace/text","content":"second"}),
            ),
            verify.clone(),
            tool_turn(
                "edit second revision",
                "write_file",
                json!({"path":"/workspace/text","content":"third"}),
            ),
            verify,
            answer_turn("three revisions verified"),
        ],
        &Arc::new(native_host(&root)),
    );
    assert!(summary.success(), "{summary:?}");
    assert_eq!(summary.tools_executed, 5, "{summary:?}");
    let verified = sink
        .events()
        .iter()
        .filter(|event| {
            matches!(event,
            Event::ToolResult { name, failed, .. }
            if name == "shell" && !failed)
        })
        .count();
    assert_eq!(verified, 3, "{summary:?}");
    assert_eq!(std::fs::read(root.join("text")).unwrap(), b"third");
}

#[test]
fn phase3_verified_read_revision_after_shell_edits_allows_reverification() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("text"), "first").unwrap();
    let read = tool_turn(
        "observe revision",
        "read_file",
        json!({"path":"/workspace/text","offset":0}),
    );
    let verify = tool_turn(
        "verify revision",
        "shell",
        json!({"command":"printf verified"}),
    );
    let (summary, sink) = run_progress(
        vec![
            read.clone(),
            verify.clone(),
            tool_turn(
                "external edit",
                "shell",
                json!({"command":"printf second > /workspace/text"}),
            ),
            read.clone(),
            verify.clone(),
            tool_turn(
                "external edit again",
                "shell",
                json!({"command":"printf third > /workspace/text"}),
            ),
            read,
            verify,
            answer_turn("observed revisions verified"),
        ],
        &Arc::new(native_host(&root)),
    );
    assert!(summary.success(), "{summary:?}");
    assert_eq!(summary.tools_executed, 8, "{summary:?}");
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(
                event, Event::ToolResult { name, failed: false, .. } if name == "shell"
            ))
            .count(),
        5,
    );
    assert_eq!(std::fs::read(root.join("text")).unwrap(), b"third");
}

#[test]
fn phase3_unchanged_native_reads_stall_before_exhausting_script() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("text"), "unchanged").unwrap();
    let read = tool_turn(
        "refresh",
        "read_file",
        json!({"path":"/workspace/text","offset":0}),
    );
    let mut turns = vec![read; 50];
    turns.push(answer_turn("must not falsely complete"));
    let (summary, _) = run_progress(turns, &native_host(&root));
    assert!(summary.failure.is_some(), "{summary:?}");
    assert!(summary.answer.is_none(), "{summary:?}");
    assert!(summary.tools_executed < 20, "{summary:?}");
    assert_eq!(std::fs::read(root.join("text")).unwrap(), b"unchanged");
}

#[test]
fn phase3_noop_native_writes_do_not_allow_repeated_verification() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("text"), "same").unwrap();
    let noop = tool_turn(
        "write unchanged bytes",
        "write_file",
        json!({"path":"/workspace/text","content":"same"}),
    );
    let verify = tool_turn("verify", "shell", json!({"command":"printf verified"}));
    let mut turns = Vec::new();
    for _ in 0..20 {
        turns.extend([verify.clone(), noop.clone()]);
    }
    turns.push(answer_turn("must not falsely complete"));
    let (summary, _) = run_progress(turns, &native_host(&root));
    assert!(summary.failure.is_some(), "{summary:?}");
    assert!(summary.answer.is_none(), "{summary:?}");
    assert!(summary.tools_executed < 20, "{summary:?}");
    assert_eq!(std::fs::read(root.join("text")).unwrap(), b"same");
}

#[test]
fn phase3_duplicate_batch_gets_one_correction_and_a_model_response() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut batch = tool_turn("verify once", "shell", json!({"command":"printf verified"}));
    let proposal = batch.tool_intents[0].clone();
    batch.tool_intents = (0..12)
        .map(|index| ToolIntent {
            id: format!("batch-{index}"),
            ..proposal.clone()
        })
        .collect();
    let (summary, sink) = run_progress(
        vec![batch, answer_turn("corrected batch")],
        &native_host(&root),
    );
    assert!(summary.success(), "{summary:?}");
    assert_eq!(summary.answer.as_deref(), Some("corrected batch"));
    assert_eq!(summary.tools_executed, 1);
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, Event::ToolResult { .. }))
            .count(),
        12,
    );
}

#[test]
fn phase3_progress_bridge_receives_resolved_native_read_pages() {
    struct PageObserver {
        native: NativeReadExecutor,
        offsets: Mutex<Vec<serde_json::Value>>,
    }
    impl ToolExecutor for PageObserver {
        fn execute(
            &self,
            intent: &ToolIntent,
            token: &CancellationToken,
        ) -> skott::Result<ToolOutcome> {
            self.native.execute(intent, token)
        }
        fn progress(
            &self,
            intent: &ToolIntent,
            _: &ToolOutcome,
        ) -> skott::Result<sav::loop_detection::ProgressEvidence> {
            self.offsets
                .lock()
                .unwrap()
                .push(intent.arguments["offset"].clone());
            Ok(sav::loop_detection::ProgressEvidence::None)
        }
    }
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("text");
    std::fs::write(&path, "abcdef").unwrap();
    let executor = PageObserver {
        native: NativeReadExecutor {
            root,
            pages: Mutex::new(Vec::new()),
        },
        offsets: Mutex::new(Vec::new()),
    };
    let read = tool_turn("next page", "read_file", json!({"path":path,"limit":2}));
    let (summary, _) = run_progress(
        vec![
            read.clone(),
            read.clone(),
            read,
            answer_turn("all pages read"),
        ],
        &executor,
    );
    assert!(summary.success(), "{summary:?}");
    assert_eq!(
        *executor.offsets.lock().unwrap(),
        vec![json!(null), json!(2), json!(4)]
    );
    let contents: String = executor
        .native
        .pages
        .lock()
        .unwrap()
        .iter()
        .map(|page| page["content"].as_str().unwrap())
        .collect();
    assert_eq!(contents, "abcdef");
}

#[test]
fn phase3_failed_progress_observation_is_explicit_and_records_the_outcome() {
    struct FailedObserver;
    impl ToolExecutor for FailedObserver {
        fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> skott::Result<ToolOutcome> {
            Ok(success_outcome("completed effect"))
        }
        fn progress(
            &self,
            _: &ToolIntent,
            _: &ToolOutcome,
        ) -> skott::Result<sav::loop_detection::ProgressEvidence> {
            Err(Error::SandboxExec {
                tool: Some("write_file".into()),
                reason: "post-execution revision unavailable; reconcile completed effects".into(),
            })
        }
    }
    let mut session = make_session();
    session.push_user("perform one mutation");
    let mut recorder = FakeRecorder::default();
    let result = Skott::default().run(
        &mut session,
        &ScriptedTransport::new(vec![
            tool_turn(
                "mutate",
                "write_file",
                json!({"path":"/workspace/text","content":"new"}),
            ),
            answer_turn("must not falsely complete"),
        ]),
        &FailedObserver,
        &Collector::default(),
        &CancellationToken::new(),
        &mut ContextManager::for_model(32_768),
        Some(&mut recorder),
    );
    let diagnostic = match result {
        Err(error) => error.describe(),
        Ok(summary) => {
            assert!(summary.answer.is_none(), "{summary:?}");
            summary.failure.unwrap_or_default()
        }
    };
    assert!(
        diagnostic.contains("post-execution revision unavailable"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("reconcile"), "{diagnostic}");
    assert_eq!(
        *recorder.tool_results.lock().unwrap(),
        ["turn 1 write_file failed=false"]
    );
    assert_eq!(recorder.terminal_calls.load(Ordering::SeqCst), 1);
}
