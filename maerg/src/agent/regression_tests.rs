use super::*;
use std::os::unix::{fs::symlink, process::ExitStatusExt};

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
    let names = ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "TODOS.yaml"];
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
        let result = record_turn_outcome(
            &profile,
            &request,
            "effects",
            &log,
            &mut file,
            &redactions,
            &[retained],
            Some(&TurnFailure::from_kvist_error(&error)),
            0,
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
        let mut turn = effect_turn();
        turn.usage = usage;
        turn.tool_intents.clear();
        turn.finish_reason = sav::FinishReason::Stop;
        turns.push(RunTurn {
            turn,
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
    let result = record_turn_outcome(
        &profile,
        &request,
        "usage",
        &path,
        &mut log,
        &[],
        &turns,
        None,
        2,
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
        let proposal = record_decision_evidence(&root, &decision, &[]).expect("safe proposal");
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
