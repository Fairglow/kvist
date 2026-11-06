use super::*;

struct MalformedRead;
impl ToolExecutor for MalformedRead {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> skott::Result<ToolOutcome> {
        Ok(ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"complete-but-invalid-page".to_vec(),
            stderr: b"completed diagnostic".to_vec(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}

#[test]
fn phase4_malformed_completed_read_is_recorded_before_observation_failure() {
    let mut recorder = FakeRecorder::default();
    let mut session = make_session();
    session.push_user("read one page");
    let sink = Collector::default();
    let result = Skott::default().run(
        &mut session,
        &ScriptedTransport::new(vec![tool_turn(
            "read",
            "read_file",
            json!({"path":"/workspace/a"}),
        )]),
        &MalformedRead,
        &sink,
        &CancellationToken::new(),
        &mut ContextManager::for_model(8192),
        Some(&mut recorder),
    );
    assert_eq!(
        *recorder.tool_results.lock().unwrap(),
        vec!["turn 1 read_file failed=false"],
        "a completed outcome was lost before validation"
    );
    assert!(result.is_err(), "invalid observation must fail");
    assert!(session.messages().iter().any(|message| matches!(message,
        ModelMessage::ToolResult {content, ..} if content.contains("complete-but-invalid-page")
        && content.contains("completed diagnostic"))));
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, Event::ToolFailure {failure, ..}
        if failure.effects == sav::runtime::EffectCertainty::MayHaveEffects))
    );
}

struct FailingTool(AtomicUsize);
impl ToolExecutor for FailingTool {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> skott::Result<ToolOutcome> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutcome {
            exited: true,
            status: Some(7),
            stdout: vec![],
            stderr: b"bounded diagnostic".to_vec(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}

#[test]
fn phase4_required_failure_notice_error_stops_later_effects_and_attempts_presentation() {
    let mut recorder = FakeRecorder {
        fail_notice: true,
        ..FakeRecorder::default()
    };
    let executor = FailingTool(AtomicUsize::new(0));
    let mut turn = tool_turn("two commands", "shell", json!({"command":"first"}));
    let mut second = turn.tool_intents[0].clone();
    second.id = "second".into();
    second.arguments = json!({"command":"second"});
    turn.tool_intents.push(second);
    let sink = Collector::default();
    let mut session = make_session();
    session.push_user("try one command");
    let result = Skott::default().run(
        &mut session,
        &ScriptedTransport::new(vec![turn]),
        &executor,
        &sink,
        &CancellationToken::new(),
        &mut ContextManager::for_model(8192),
        Some(&mut recorder),
    );
    assert!(
        result.is_err() || result.as_ref().is_ok_and(|summary| !summary.success()),
        "{result:?}"
    );
    assert_eq!(
        executor.0.load(Ordering::SeqCst),
        1,
        "diagnostic recording failure must fence later tools"
    );
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, Event::ToolFailure { .. }))
    );
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, Event::Failed(_)))
    );
}

#[test]
fn phase4_resource_identity_survives_both_error_boundaries() {
    for resource in [
        sav::runtime::BudgetResource::WallTime,
        sav::runtime::BudgetResource::ModelTokens,
        sav::runtime::BudgetResource::Turns,
        sav::runtime::BudgetResource::ModelAttempts,
        sav::runtime::BudgetResource::Context,
    ] {
        let app = Error::from(sav::runtime::Error::RunBudget {
            resource,
            reason: "exhausted".into(),
        });
        assert!(app.describe().contains("review"));
        let shared = sav::runtime::Error::from(app);
        assert!(
            matches!(shared, sav::runtime::Error::RunBudget {resource:actual, ..} if actual == resource)
        );
    }
}

#[test]
fn phase4_generation_exception_keeps_effectful_tool_repetition_protection() {
    let executor = FailingTool(AtomicUsize::new(0));
    let transport = ScriptedTransport::new(
        (0..12)
            .map(|_| {
                tool_turn(
                    "try again",
                    "shell",
                    json!({"command":"same effectful request"}),
                )
            })
            .collect(),
    );
    let mut session = make_session();
    session.push_user("investigate without blind replay");
    let result = Skott::default().with_generation_loop_detection(false).run(
        &mut session,
        &transport,
        &executor,
        &Collector::default(),
        &CancellationToken::new(),
        &mut ContextManager::for_model(8192),
        None,
    );
    assert!(result.is_err() || result.as_ref().is_ok_and(|summary| !summary.success()));
    assert!(
        executor.0.load(Ordering::SeqCst) < 12,
        "generation exception disabled tool protection"
    );
}

#[test]
fn phase4_missing_installed_runner_is_not_executed_prerequisite_not_unknown_io() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert_runner_prerequisite(root.join("missing-runner"), root);
}

#[test]
fn phase4_invalid_installed_runner_preserves_typed_native_prerequisite() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let runner = root.join("directory-runner");
    std::fs::create_dir(&runner).unwrap();
    assert_runner_prerequisite(runner, root);
}

fn assert_runner_prerequisite(runner: PathBuf, root: PathBuf) {
    let executor = skott::SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum()),
        skott::SandboxPaths {
            runner,
            backend: PathBuf::from("/usr/bin/true"),
        },
        root,
    );
    let mut session = make_session();
    session.push_user("run one command");
    let sink = Collector::default();
    let result = Skott::default().run(
        &mut session,
        &ScriptedTransport::new(vec![tool_turn(
            "execute",
            "shell",
            json!({"command":"true"}),
        )]),
        &executor,
        &sink,
        &CancellationToken::new(),
        &mut ContextManager::for_model(8192),
        None,
    );
    assert!(
        result.is_err() || result.as_ref().is_ok_and(|summary| !summary.success()),
        "{result:?}"
    );
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, Event::ToolFailure {failure, ..}
        if failure.category == sav::runtime::ToolFailureCategory::Prerequisite
        && failure.effects == sav::runtime::EffectCertainty::NotExecuted
        && failure.recovery == sav::runtime::RecoveryAction::FixPrerequisite))
    );
}

struct PossiblyEffectfulError;
impl ToolExecutor for PossiblyEffectfulError {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> skott::Result<ToolOutcome> {
        Err(Error::SandboxExec {
            tool: Some("shell".into()),
            reason: "pipe failed after dispatch".into(),
        })
    }
    fn progress(
        &self,
        _: &ToolIntent,
        _: &ToolOutcome,
    ) -> skott::Result<sav::loop_detection::ProgressEvidence> {
        panic!("a synthetic outcome after an execution error is not observable evidence")
    }
}

#[test]
fn phase4_executor_error_never_claims_not_executed_or_replays() {
    let mut session = make_session();
    session.push_user("run one command");
    let sink = Collector::default();
    let result = Skott::default().run(
        &mut session,
        &ScriptedTransport::new(vec![tool_turn(
            "execute",
            "shell",
            json!({"command":"true"}),
        )]),
        &PossiblyEffectfulError,
        &sink,
        &CancellationToken::new(),
        &mut ContextManager::for_model(8192),
        None,
    );
    assert!(
        result.is_err()
            || result
                .as_ref()
                .is_ok_and(|summary| summary.tools_executed == 0)
    );
    let events = serde_json::to_string(&sink.events()).unwrap();
    assert!(
        events.contains("may_have_effects") && events.contains("reconcile"),
        "{events}"
    );
    assert!(!session.messages().iter().any(|message|
        matches!(message, ModelMessage::ToolResult {content, ..} if content.contains("not executed"))));
}
