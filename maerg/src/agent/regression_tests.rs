use super::*;
use std::os::unix::{fs::symlink, process::ExitStatusExt};

#[test]
fn generation_completed_stream_repetition_retains_known_usage() {
    assert_completed_repetitive_stream(false);
}

#[test]
fn generation_completed_stream_repetition_keeps_first_cause() {
    assert_completed_repetitive_stream(true);
}

fn assert_completed_repetitive_stream(later_callback: bool) {
    struct CompletedDespiteInterruption<'a> {
        calls: &'a std::sync::atomic::AtomicUsize,
        later_callback: bool,
    }
    impl ModelTransport for CompletedDespiteInterruption<'_> {
        fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
            panic!("streaming fixture cannot use unary completion");
        }

        fn stream(
            &self,
            _: &ModelRequest,
            cancellation: &CancellationToken,
            on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let passage = (0..100)
                .map(|index| format!("investigationword{index} "))
                .collect::<String>();
            assert!(on_event(ModelStreamEvent::TextDelta(passage.repeat(6))).is_err());
            assert!(cancellation.is_cancelled());
            if self.later_callback {
                assert!(
                    on_event(ModelStreamEvent::TextDelta(
                        "A different nonrepetitive assembled answer.".into()
                    ))
                    .is_err()
                );
            }
            let mut turn = effect_turn();
            turn.text = "A different nonrepetitive assembled answer.".into();
            turn.usage = Some(sav::ModelUsage {
                input_tokens: 10000,
                output_tokens: 100,
                total_tokens: 10100,
            });
            Ok(turn)
        }

        fn deadline(&self) -> Duration {
            Duration::from_secs(5)
        }
    }

    let dir = tempfile::tempdir_in(".").unwrap();
    let path = dir.path().join("completed-repetition.events.jsonl");
    let live = LiveRun::create(&path, "completed-repetition", "task", "fixture", &[]).unwrap();
    let parent = CancellationToken::new();
    let controls = sav::runtime::ControlMailbox::default();
    controls.begin_run().unwrap();
    let operation = sav::runtime::Operation::start(&parent, &controls).unwrap();
    let budget = TurnBudget::new(Duration::from_secs(5));
    let request = ModelRequest {
        model: "fixture".into(),
        messages: vec![sav::ModelMessage::User("approved goal ".repeat(1024))],
        tools: vec![],
        tool_choice: sav::ToolChoice::None,
        reasoning_effort: None,
        output_schema: None,
        max_output_tokens: Some(256),
    };
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let mut attempts = 0;
    let mut tokens = TokenBudget::new(Some(11000));
    let result = complete_turn_counted(
        || {
            Ok(live::ObservedTransport {
                transport: CompletedDespiteInterruption {
                    calls: &calls,
                    later_callback,
                },
                live: &live,
                redactions: &[],
            })
        },
        &request,
        true,
        &[],
        &mut attempts,
        &ModelRetry {
            policy: sav::runtime::RetryPolicy::default(),
            budget: &budget,
            cancellation: operation.token(),
            controls: None,
            live: Some(&live),
            phase: "inference",
            model: "fixture",
            endpoint: "fixture",
            redactions: &[],
        },
        &mut tokens,
    );
    assert!(
        matches!(
            result,
            Err(KvistError::Sav(ModelError::RepetitiveGeneration { report }))
                if report.channel == sav::loop_detection::RepetitionChannel::Answer
                    && report.period == 100
        ),
        "the first detected repetition must reject the completed proposal: {result:?}"
    );
    assert_eq!(
        tokens.known_usage().map(|usage| usage.total_tokens),
        Some(10100),
        "successful underlying stream usage was lost during repetition conversion"
    );
    assert!(tokens.preflight(&request).is_err());
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(attempts, 1);
    assert!(operation.token().is_cancelled());
    assert!(!parent.is_cancelled());
    assert!(!dir.path().join("src").exists());
    live.finish("rejected").unwrap();
    let mut follower = sav::observation::JournalFollower::open(&path).unwrap();
    let records = follower.poll().unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.event["type"] == "generation_repetition")
            .count(),
        1
    );
    let repetition = records
        .iter()
        .position(|record| record.event["type"] == "generation_repetition")
        .unwrap();
    let accounting = records
        .iter()
        .position(|record| record.event["type"] == "model_accounting")
        .unwrap();
    assert!(repetition < accounting);
    assert_eq!(
        records[repetition].event["data"]["proposals_executed"],
        false
    );
    let data = &records[accounting].event["data"];
    assert_eq!(data["returned_attempts"], 1);
    assert_eq!(data["known_attempts"], 1);
    assert_eq!(data["uncertain_attempts"], 0);
    assert_eq!(data["provenance_complete"], true);
    assert_eq!(data["charged_tokens"], 10100);
    assert_eq!(data["cumulative_usage"]["input_tokens"], 10000);
    assert_eq!(data["cumulative_usage"]["output_tokens"], 100);
    assert_eq!(data["cumulative_usage"]["total_tokens"], 10100);
    assert!(records.iter().any(|record| {
        record.event["type"] == "model_result"
            && record.event["data"]["tool_count"] == 2
            && record.event["data"]["usage"]["total_tokens"] == 10100
    }));
    assert!(!records.iter().any(|record| {
        matches!(
            record.event["type"].as_str(),
            Some("model_retry" | "broker_decision" | "tool_dispatch")
        )
    }));
    assert!(follower.is_finished());
    drop(operation);
    controls.end_run().unwrap();
}

#[test]
fn generation_completed_usage_reconciles_before_early_steering_return() {
    struct ReturnedWithGuidance<'a>(&'a sav::runtime::ControlMailbox);
    impl ModelTransport for ReturnedWithGuidance<'_> {
        fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
            self.0.steer("new constraint").unwrap();
            let mut turn = effect_turn();
            turn.tool_intents.clear();
            turn.finish_reason = sav::FinishReason::Stop;
            turn.usage = Some(sav::ModelUsage {
                input_tokens: 10000,
                output_tokens: 100,
                total_tokens: 10100,
            });
            Ok(turn)
        }
        fn stream(
            &self,
            request: &ModelRequest,
            cancellation: &CancellationToken,
            _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            self.complete(request, cancellation)
        }
        fn deadline(&self) -> Duration {
            Duration::from_secs(5)
        }
    }
    let dir = tempfile::tempdir_in(".").unwrap();
    let live = LiveRun::create(
        &dir.path().join("usage.events.jsonl"),
        "steered-usage",
        "task",
        "fixture",
        &[],
    )
    .unwrap();
    let controls = sav::runtime::ControlMailbox::default();
    controls.begin_run().unwrap();
    let cancellation = CancellationToken::new();
    let interrupted = std::sync::atomic::AtomicBool::new(false);
    let budget = TurnBudget::new(Duration::from_secs(5));
    let request = ModelRequest {
        model: "fixture".into(),
        messages: vec![sav::ModelMessage::User("approved goal ".repeat(1024))],
        tools: vec![],
        tool_choice: sav::ToolChoice::None,
        reasoning_effort: None,
        output_schema: None,
        max_output_tokens: Some(256),
    };
    let mut tokens = TokenBudget::new(Some(11000));
    let result = complete_turn_counted(
        || Ok(ReturnedWithGuidance(&controls)),
        &request,
        false,
        &[],
        &mut 0,
        &ModelRetry {
            policy: sav::runtime::RetryPolicy::default(),
            budget: &budget,
            cancellation: &cancellation,
            controls: Some(generation::Controls {
                mailbox: &controls,
                interrupted: &interrupted,
            }),
            live: Some(&live),
            phase: "inference",
            model: "fixture",
            endpoint: "fixture",
            redactions: &[],
        },
        &mut tokens,
    );
    assert!(matches!(
        result,
        Err(KvistError::Sav(ModelError::ModelTransportCancelled))
    ));
    assert_eq!(tokens.known_usage().unwrap().input_tokens, 10000);
    assert_eq!(tokens.known_usage().unwrap().output_tokens, 100);
    let accounting = tokens.accounting();
    assert_eq!(accounting.returned_attempts, 1);
    assert_eq!(accounting.known_attempts, 1);
    assert_eq!(accounting.charged_tokens, Some(10100));
    assert_eq!(accounting.cumulative_usage.unwrap().total_tokens, 10100);
    assert!(accounting.provenance_complete);
    assert!(
        tokens.preflight(&request).is_err(),
        "known completed usage was refunded"
    );
    assert!(interrupted.load(std::sync::atomic::Ordering::Relaxed));
    assert!(!cancellation.is_cancelled());
    controls.end_run().unwrap();
    live.finish("interrupted").unwrap();
}

#[test]
fn host_turn_checks_cancellation_and_expiry_before_transport_construction() {
    let dir = tempfile::tempdir().expect("workspace");
    let profile =
        tests::test_profile("curl --json '{\"model\":\"fixture\"}' 'http://127.0.0.1:1/api/chat'");
    let live = LiveRun::create(
        &dir.path().join("events.jsonl"),
        "budget",
        "task",
        "fixture",
        &[],
    )
    .expect("live run");
    for native in [false, true] {
        let mut choice =
            select_model_choice(&profile, crate::config::Role::Developer, None).expect("choice");
        if native {
            choice.native = Some(sav::registry::NativeModel::new(
                LocalModelProvider::Ollama,
                "http://127.0.0.1:1".into(),
                "fixture".into(),
            ));
        }
        for cancelled in [false, true] {
            let token = CancellationToken::new();
            if cancelled {
                token.cancel();
            }
            let budget = TurnBudget::new(Duration::ZERO);
            let request = ModelRequest {
                model: "fixture".into(),
                messages: vec![sav::ModelMessage::User("task".into())],
                tools: vec![],
                tool_choice: sav::ToolChoice::None,
                reasoning_effort: None,
                output_schema: None,
                max_output_tokens: None,
            };
            let error = execute_host_turn(
                &choice,
                &profile,
                &tests::test_execution_request(dir.path()),
                &budget,
                &[],
                &request,
                &token,
                &live,
                &mut 0,
                &mut TokenBudget::new(None),
                None,
            )
            .expect_err("no transport work after cancellation or expiry");
            assert!(
                if cancelled {
                    matches!(error, KvistError::Sav(ModelError::ModelTransportCancelled))
                } else {
                    matches!(error, KvistError::Sav(ModelError::ModelTransportTimedOut))
                },
                "{error:?}"
            );
        }
    }
}

#[test]
fn selected_context_is_bounded_and_never_follows_links_or_expands_source() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let root = dir.path().canonicalize().expect("absolute workspace");
    let selected = [PathBuf::from("/workspace/component/REQUIREMENTS.md")];
    let mut request = tests::test_execution_request(&root);
    request.context_paths = &selected;
    let artifact = root.join("REQUIREMENTS.md");
    fs::write(&artifact, "chosen context").expect("artifact");
    assert!(
        context::system_context(&request, "")
            .expect("context")
            .contains("chosen context")
    );

    fs::remove_file(&artifact).expect("remove");
    fs::write(root.join("outside"), "must not follow").expect("outside");
    symlink(root.join("outside"), &artifact).expect("link");
    assert!(context::system_context(&request, "").is_err());
    fs::remove_file(&artifact).expect("remove link");
    fs::write(&artifact, vec![b'a'; 1024 * 1024 + 1]).expect("large artifact");
    assert!(context::system_context(&request, "").is_err());
    fs::write(&artifact, [0xff]).expect("invalid UTF-8");
    assert!(context::system_context(&request, "").is_err());

    let source = [root.join("outside")];
    request.context_paths = &source;
    assert!(context::system_context(&request, "").is_err());

    let parent = root.join("linked-parent");
    symlink(&root, &parent).expect("directory link");
    request.target_dir = &parent;
    request.context_paths = &selected;
    assert!(context::system_context(&request, "").is_err());
}

#[test]
fn selected_context_aggregate_bound_and_context_window_fail_without_omission() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let root = dir.path().canonicalize().expect("absolute workspace");
    let names = ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "IMPL.md"];
    let mut selected = Vec::new();
    for name in names {
        fs::write(root.join(name), vec![b'a'; 1024 * 1024]).expect("artifact");
        selected.push(Path::new("/workspace/component").join(name));
    }
    let mut request = tests::test_execution_request(&root);
    request.context_paths = &selected;
    let system = context::system_context(&request, "").expect("exactly 4 MiB");
    let mut model_request = ModelRequest {
        model: "fixture".into(),
        messages: vec![
            sav::ModelMessage::System(system),
            sav::ModelMessage::User("task".into()),
        ],
        tools: vec![],
        tool_choice: sav::ToolChoice::None,
        reasoning_effort: None,
        output_schema: None,
        max_output_tokens: None,
    };
    let before = model_request.messages.clone();
    let mut manager = sav::runtime::ContextManager::new(8192, 4);
    assert!(manager.prepare(&mut model_request, 256).is_err());
    assert_eq!(
        model_request.messages, before,
        "selected context must not be dropped to fit"
    );

    fs::write(root.join("ROOT_CONTRACT.md"), "one byte beyond aggregate").expect("root");
    let mut extended = selected.clone();
    extended.push("/workspace/context/ROOT_CONTRACT.md".into());
    let mounts = [sandbox::ReadOnlyMount::file(
        root.join("ROOT_CONTRACT.md"),
        "/workspace/context/ROOT_CONTRACT.md",
    )];
    request.context_paths = &extended;
    request.read_only_mounts = &mounts;
    assert!(context::system_context(&request, "").is_err());
}

#[test]
fn selected_queue_context_contains_only_exact_authored_task_fields() {
    let dir = tempfile::tempdir().expect("workspace");
    let template = crate::artifacts::root_artifacts()
        .iter()
        .find(|artifact| artifact.relative_path == "src/TODOS.yaml")
        .unwrap();
    let mut queue = crate::task_queue::parse(template.contents).unwrap();
    let mut previous = Vec::<String>::new();
    for (id, kind) in [
        ("selected", "test"),
        ("implementation", "implementation"),
        ("audit", "security-audit"),
        ("review", "compliance-review"),
    ] {
        queue.tasks.push(serde_json::from_value(serde_json::json!({
            "id":id, "kind":kind, "title":format!("title {id}"),
            "description":format!("description {id}"),
            "context":if id == "selected" { "exact selected context".into() } else { "unrelated history ".repeat(200).trim().to_owned() },
            "purpose":"fixture purpose", "expected_outcome":"fixture result",
            "requirements":["REQUIREMENTS.md#REQ-FIXTURE"], "depends_on":previous,
            "status":"pending",
            "timestamps":{"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","completed_at":null},
            "blocked_reason":null
        })).unwrap());
        previous = vec![id.to_owned()];
    }
    let contents = crate::task_queue::serialize(&queue).unwrap();
    fs::write(dir.path().join("TODOS.yaml"), &contents).unwrap();
    let mut request = tests::test_execution_request(dir.path());
    let selected = [PathBuf::from("/workspace/component/TODOS.yaml")];
    request.context_paths = &selected;
    request.task_id = "selected";
    let system = context::system_context(&request, "").unwrap();
    assert!(system.contains("exact selected context"));
    assert!(!system.contains("unrelated history"));
    assert!(!system.contains("requirements_revision"));
    assert!(!system.contains("timestamps"));
    assert!(system.len() < 2048, "{} bytes", system.len());
    request.task_id = "absent";
    assert!(context::system_context(&request, "").is_err());
    request.task_id = "selected";
    fs::write(
        dir.path().join("TODOS.yaml"),
        contents.replace("schema_version: 1", "schema_version: 99"),
    )
    .unwrap();
    assert!(context::system_context(&request, "").is_err());
}

fn effect_turn() -> ModelTurn {
    ModelTurn {
        text: "apply two writes".into(),
        reasoning: None,
        tool_intents: (1..=2)
            .map(|index| sav::ToolIntent {
                id: format!("private-value-call-{index}"),
                provider_id: None,
                name: "write_file".into(),
                arguments: serde_json::json!({
                    "destination":format!("src/private-value-{index}.rs"),"content":"fn fixture() {}"
                }),
            })
            .collect(),
        finish_reason: sav::FinishReason::ToolCalls,
        provider: LocalModelProvider::Ollama,
        model: "fixture".into(),
        response_id: None,
        provider_request_id: None,
        usage: Some(sav::ModelUsage {
            input_tokens: 11,
            output_tokens: 7,
            total_tokens: 18,
        }),
    }
}

fn effect_result(cancelled: bool) -> sandbox::ExecutionResult {
    sandbox::ExecutionResult {
        output: std::process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: vec![],
            stderr: vec![],
        },
        timed_out: false,
        output_limit_exceeded: false,
        cancelled,
    }
}

#[test]
fn phase3_terminal_dispatch_failure_precedes_loop_protection() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let root = dir.path().canonicalize().expect("absolute workspace");
    fs::create_dir(root.join("src")).expect("source");
    ensure_logs_directory(&root).expect("state");
    let request = tests::test_execution_request(&root);
    let live = LiveRun::create(
        &root.join("events.jsonl"),
        "effects",
        "fixture",
        "fixture",
        &[],
    )
    .expect("live");
    let controls = sav::runtime::ControlMailbox::default();
    controls.begin_run().expect("begin");
    let token = CancellationToken::new();
    let steering = RunControls {
        mailbox: &controls,
        cancellation: &token,
        live: &live,
        redactions: &[],
    };
    let mut turn = effect_turn();
    turn.tool_intents[1].arguments = turn.tool_intents[0].arguments.clone();
    let plan = authoring::authorize_turn(&root, &turn, &BrokerPolicy::default());
    let mut tracker = sav::loop_detection::ToolLoopTracker::default();
    let mut records = Vec::new();
    let mut dispatched = 0;
    let report = apply_authoring_effects_with(
        &request,
        &plan,
        &turn,
        "failed-effects",
        &live,
        &steering,
        &mut records,
        Some(&mut tracker),
        None,
        |_| {
            dispatched += 1;
            let mut result = effect_result(false);
            result.output.status = std::process::ExitStatus::from_raw(1 << 8);
            result.output.stderr = b"original terminal dispatcher failure".to_vec();
            Ok(result)
        },
    )
    .expect("record completed failure");
    assert!(
        report.is_none(),
        "loop protection overrode the hard failure"
    );
    assert_eq!(dispatched, 1, "later effects followed a terminal failure");
    assert_eq!(records.len(), 1);
    assert!(!records[0].applied);
    assert!(records[0].uncertain);
    assert!(
        records[0]
            .detail
            .contains("original terminal dispatcher failure")
    );
    assert_eq!(
        fs::read_dir(root.join(".kvist/authoring"))
            .expect("staging")
            .count(),
        0
    );
}

#[test]
fn partial_effects_and_usage_survive_cancellation_and_later_dispatch_failure() {
    for interruption in [
        "between-effects",
        "second-dispatch-error",
        "cancelled-dispatch",
    ] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let root = dir.path().canonicalize().expect("absolute workspace");
        fs::create_dir(root.join("src")).expect("source directory");
        ensure_logs_directory(&root).expect("state");
        fs::create_dir(root.join(".kvist/runs")).expect("runs");
        let request = tests::test_execution_request(&root);
        let live = LiveRun::create(
            &root.join("events.jsonl"),
            "effects",
            "fixture",
            "fixture",
            &[],
        )
        .expect("live");
        let controls = sav::runtime::ControlMailbox::default();
        controls.begin_run().expect("begin");
        let token = CancellationToken::new();
        let steering = RunControls {
            mailbox: &controls,
            cancellation: &token,
            live: &live,
            redactions: &[],
        };
        let turn = effect_turn();
        let plan = authoring::authorize_turn(&root, &turn, &BrokerPolicy::default());
        assert_eq!(plan.effects.len(), 2);
        let mut retained = RunTurn {
            turn,
            reads: vec![],
            effects: vec![],
            drops: vec![],
            surfaced_decisions: vec![],
            accepted_requests: vec![],
        };
        let mut calls = 0;
        let error = apply_authoring_effects_with(
            &request,
            &plan,
            &retained.turn,
            "effects",
            &live,
            &steering,
            &mut retained.effects,
            None,
            None,
            |_| {
                calls += 1;
                if interruption == "between-effects" {
                    token.cancel();
                }
                if calls == 2 && interruption == "second-dispatch-error" {
                    return Err(KvistError::AuthoringEffectFailed {
                        call_id: "second".into(),
                        reason: "injected dispatch failure".into(),
                    });
                }
                Ok(effect_result(
                    interruption == "cancelled-dispatch" && calls == 2,
                ))
            },
        )
        .expect_err("must preserve explicit interruption");
        assert!(retained.effects[0].applied);
        assert_eq!(
            calls,
            if interruption == "between-effects" {
                1
            } else {
                2
            }
        );
        if calls == 2 {
            assert!(retained.effects[1].uncertain);
            assert!(!retained.effects[1].applied);
        }
        assert_eq!(
            fs::read_dir(root.join(".kvist/authoring"))
                .expect("staging")
                .count(),
            0
        );

        let mut profile = tests::test_profile("");
        profile.max_output_bytes = 8192;
        let log = root.join("outcome.log");
        let mut file = fs::File::create(&log).expect("log");
        let redactions = vec!["private-value".into(), "write_file".into()];
        let mut tokens = TokenBudget::new(None);
        tokens.finish(0, retained.turn.usage);
        let result = record_turn_outcome(
            &profile,
            &request,
            "effects",
            &log,
            &mut file,
            &redactions,
            &[retained],
            Some(&TurnFailure::from_kvist_error(&error)),
            &tokens,
        )
        .expect("retain evidence");
        assert!(!result.success);
        assert_eq!(result.tokens_input, Some(11));
        assert_eq!(result.tokens_output, Some(7));
        let text = fs::read_to_string(log).expect("log");
        assert!(text.contains("applied [REDACTED]"));
        assert!(!text.contains("private-value"));
        let trajectory = root.join(".kvist/runs/effects.trajectory.jsonl");
        let raw = fs::read_to_string(&trajectory).expect("trajectory");
        assert!(!raw.contains("private-value"));
        assert!(!raw.contains("write_file"));
        let replay = sav::replay_trajectory(&trajectory, None).expect("replay");
        assert_eq!(replay.tool_calls_count, calls);
        let dispatches: Vec<_> = replay
            .events
            .iter()
            .filter_map(|event| match event {
                sav::TrajectoryEvent::ToolDispatch { call_id, tool, .. } => Some((call_id, tool)),
                _ => None,
            })
            .collect();
        let results: Vec<_> = replay
            .events
            .iter()
            .filter_map(|event| match event {
                sav::TrajectoryEvent::ToolResult { call_id, tool, .. } => Some((call_id, tool)),
                _ => None,
            })
            .collect();
        assert_eq!(
            dispatches, results,
            "diagnostic identities must map consistently"
        );
        assert_eq!(replay.final_success, Some(false));
        let mutations = replay
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    sav::TrajectoryEvent::ToolResult {
                        state_mutated: true,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            mutations, calls,
            "uncertain dispatch must not imply no mutation"
        );
        if calls == 2 {
            assert!(raw.contains("may have mutated"));
        }
    }
}

#[test]
fn gateway_probe_observes_precancelled_tokens_without_connecting() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let token = CancellationToken::new();
    token.cancel();
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    assert!(matches!(
        probe_gateway_reachable(&endpoint, &TurnBudget::new(Duration::from_secs(10)), &token),
        Err(KvistError::Sav(ModelError::ModelTransportCancelled))
    ));
    assert_eq!(
        listener.accept().expect_err("no connection").kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn outcome_reports_known_cumulative_usage_without_treating_missing_reports_as_zero() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let root = dir.path().canonicalize().expect("absolute workspace");
    fs::create_dir_all(root.join(".kvist/runs")).expect("runs");
    let mut turns = Vec::new();
    let mut tokens = TokenBudget::new(None);
    for usage in [
        Some(sav::ModelUsage {
            input_tokens: 11,
            output_tokens: 7,
            total_tokens: 18,
        }),
        None,
        Some(sav::ModelUsage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        }),
        Some(sav::ModelUsage {
            input_tokens: 13,
            output_tokens: 5,
            total_tokens: 18,
        }),
    ] {
        tokens.finish(0, usage);
        let mut turn = effect_turn();
        turn.usage = usage;
        turn.tool_intents.clear();
        turn.finish_reason = sav::FinishReason::Stop;
        turns.push(RunTurn {
            turn,
            reads: vec![],
            effects: vec![],
            drops: vec![],
            surfaced_decisions: vec![],
            accepted_requests: vec![],
        });
    }
    let mut profile = tests::test_profile("");
    profile.max_output_bytes = 8192;
    let request = tests::test_execution_request(&root);
    let path = root.join("usage.log");
    let mut log = fs::File::create(&path).expect("log");
    tokens.uncertain_attempts = 2;
    let result = record_turn_outcome(
        &profile,
        &request,
        "usage",
        &path,
        &mut log,
        &[],
        &turns,
        None,
        &tokens,
    )
    .expect("recorded");
    assert_eq!(result.tokens_input, Some(24));
    assert_eq!(result.tokens_output, Some(12));
    let text = fs::read_to_string(path).expect("text");
    assert!(text.contains("reported turns=3, unavailable turns=1"));
    assert!(text.contains("unknown attempts=2"));
    let replay = sav::replay_trajectory(&root.join(".kvist/runs/usage.trajectory.jsonl"), None)
        .expect("trajectory");
    assert!(replay.events.iter().any(|event| matches!(
        event,
        sav::TrajectoryEvent::PromptEval {
            turn: 2,
            new_tokens: None,
            ..
        }
    )));
    assert!(replay.events.iter().any(|event| matches!(
        event,
        sav::TrajectoryEvent::PromptEval {
            turn: 3,
            new_tokens: Some(0),
            ..
        }
    )));
    assert!(replay.events.iter().any(|event| matches!(
        event,
        sav::TrajectoryEvent::SessionFinish {
            total_tokens: 36,
            ..
        }
    )));
}

#[test]
fn provider_call_ids_cannot_select_evidence_filenames_or_escape_their_directory() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let root = dir.path().canonicalize().expect("absolute workspace");
    for call_id in [
        "../escape".to_owned(),
        root.join("absolute").display().to_string(),
        "line\nbreak/é".into(),
    ] {
        let decision = ProposedDecision {
            call_id: call_id.clone(),
            summary: "decision".into(),
            why: "reason".into(),
            patch: None,
        };
        let dependency = DependencyRequest {
            call_id: call_id.clone(),
            name: "fixture".into(),
            origin: "https://example.invalid".into(),
            reason: None,
        };
        let proposal =
            record_decision_evidence(&root, &decision, &[], "fixture:0:0").expect("safe proposal");
        let request = record_dependency_evidence(&root, &dependency, &[]).expect("safe dependency");
        assert_eq!(
            proposal.parent(),
            Some(root.join(".kvist/authoring/proposals").as_path())
        );
        assert_eq!(
            request.parent(),
            Some(root.join(".kvist/authoring/dependencies").as_path())
        );
        for path in [proposal, request] {
            let name = path.file_name().expect("name").to_str().expect("UTF-8");
            assert_eq!(name.len(), 69, "SHA-256 hex plus .json");
            assert!(name[..64].bytes().all(|byte| byte.is_ascii_hexdigit()));
            let stored: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).expect("payload")).expect("JSON");
            assert_eq!(
                stored["call_id"], call_id,
                "payload identity remains operationally unchanged"
            );
        }
    }
    assert!(!root.join(".kvist/authoring/escape.json").exists());
    assert!(!root.join("absolute.json").exists());
}
