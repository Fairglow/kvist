use super::*;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

fn prose(period: usize) -> String {
    let passage = (0..period)
        .map(|index| format!("investigationword{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} ", vec![passage; 6].join(" "))
}

fn loop_response(
    stream: &mut TcpStream,
    period: usize,
    reasoning: bool,
    streamed: bool,
    length: bool,
    tools: serde_json::Value,
) -> bool {
    let text = prose(period);
    let channel = if reasoning { "thinking" } else { "content" };
    if !streamed {
        let body = serde_json::json!({
            "model":"fixture","message":{"role":"assistant",channel:text,"tool_calls":tools},
            "done":true,"done_reason":if length {"length"} else {"stop"},
            "prompt_eval_count":10,"eval_count":4
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("unary loop");
        return false;
    }
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n"
    )
    .expect("stream header");
    // Split inside words and independently of the repetition cadence.
    for chunk in text.as_bytes().chunks(37) {
        let body = serde_json::json!({
            "model":"fixture","message":{"role":"assistant",channel:std::str::from_utf8(chunk).unwrap()},
            "done":false
        });
        if writeln!(stream, "{body}").is_err() {
            return true;
        }
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("interruption deadline");
    match stream.read(&mut [0]) {
        Ok(0) => return true,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) =>
        {
            return true;
        }
        Ok(_) => panic!("unexpected additional request bytes"),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        Err(error) => panic!("observe provider interruption: {error}"),
    }
    // Old production reaches this final record rather than interrupting.
    let final_record = writeln!(
        stream,
        "{}",
        serde_json::json!({
            "model":"fixture","message":{"role":"assistant","content":"completed report","tool_calls":tools},
            "done":true,"done_reason":if length {"length"} else {"stop"},
            "prompt_eval_count":10,"eval_count":4
        })
    );
    if let Err(error) = final_record {
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ));
        return true;
    }
    false
}

fn run(
    dir: &Path,
    streamed: bool,
    limit: Option<usize>,
    token: &sav::CancellationToken,
    answer: impl FnMut(usize, &serde_json::Value, &mut TcpStream) + Send,
) -> (maerg::Result<maerg::agent::AgentRunResult>, usize) {
    run_controlled(
        dir,
        streamed,
        limit,
        token,
        &sav::runtime::ControlMailbox::default(),
        answer,
    )
}

fn run_controlled(
    dir: &Path,
    streamed: bool,
    limit: Option<usize>,
    token: &sav::CancellationToken,
    controls: &sav::runtime::ControlMailbox,
    mut answer: impl FnMut(usize, &serde_json::Value, &mut TcpStream) + Send,
) -> (maerg::Result<maerg::agent::AgentRunResult>, usize) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let stopped = AtomicBool::new(false);
    thread::scope(|scope| {
        let provider = scope.spawn(|| {
            let mut calls = 0;
            let deadline = Instant::now() + Duration::from_secs(SCRIPTED_TIMEOUT_SECONDS);
            while !stopped.load(Ordering::SeqCst) {
                assert!(Instant::now() < deadline, "missing inference");
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nodelay(true).unwrap();
                        if let Some(request) = read_request(&mut stream) {
                            calls += 1;
                            answer(calls, &request, &mut stream);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
            calls
        });
        let mut selected = shared_profile(&endpoint, 32768);
        selected.timeout_seconds = SCRIPTED_TIMEOUT_SECONDS;
        selected.max_output_bytes = 1024 * 1024;
        selected.token_limit = limit;
        let result = maerg::agent::execute_agent_controlled(
            &selected,
            &SandboxConfig {
                runner: "/usr/bin/true".into(),
                backend: "/usr/bin/true".into(),
                environment_allowlist: vec![],
                acquisition: Default::default(),
            },
            None,
            None,
            AgentExecutionRequest {
                project_root: dir,
                vcs_selection: VcsSelection::Git,
                prompt: "original task: produce the approved implementation",
                context_paths: &[],
                read_only_mounts: &[],
                target_dir: dir,
                task_id: "generation-task",
                stream_output: streamed,
                role: Role::Developer,
                policy_identity: "fixture",
            },
            controls,
            token,
        );
        stopped.store(true, Ordering::SeqCst);
        (result, provider.join().unwrap())
    })
}

fn events(dir: &Path) -> Vec<serde_json::Value> {
    let path = fs::read_dir(dir.join(".kvist/runs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
        .unwrap();
    let mut follower = JournalFollower::open(&path).unwrap();
    let mut events = Vec::new();
    while !follower.is_finished() {
        let batch = follower.poll().unwrap();
        assert!(
            !batch.is_empty(),
            "completed run journal has no terminal record"
        );
        events.extend(batch.into_iter().map(|record| record.event));
    }
    events
}

fn correction(request: &serde_json::Value, attempt: usize) {
    let text = request.to_string();
    assert!(text.contains("original task"));
    assert!(text.contains("approved implementation"));
    assert!(text.contains("repetitive generation"), "{text}");
    assert!(text.contains(&format!("{attempt}/2")), "{text}");
    assert!(
        !text.contains("investigationword"),
        "rejected text leaked into context"
    );
}

#[test]
fn generation_streamed_answer_and_reasoning_interrupt_long_cadences_and_correct_twice() {
    for reasoning in [false, true] {
        for period in [100, 150, 200] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let token = sav::CancellationToken::new();
            let mut interrupted = 0;
            let (result, calls) = run(dir.path(), true, None, &token, |call, request, stream| {
                if call > 1 {
                    correction(request, call - 1);
                }
                if call <= 2 {
                    interrupted += usize::from(loop_response(
                        stream,
                        period,
                        reasoning,
                        true,
                        false,
                        serde_json::json!([]),
                    ));
                } else {
                    respond(stream, 10, 4);
                }
            });
            let result = result.unwrap();
            assert_eq!(
                calls, 3,
                "period={period} reasoning={reasoning}: {}",
                result.stderr
            );
            assert_eq!(interrupted, 2, "must interrupt before provider completion");
            assert!(result.success, "{}", result.stderr);
            assert_eq!(result.stdout, "observed answer");
            assert!(
                !token.is_cancelled(),
                "provider interruption cancelled the parent"
            );
            let events = events(dir.path());
            let reports: Vec<_> = events
                .iter()
                .filter(|event| event["type"] == "generation_repetition")
                .collect();
            assert_eq!(reports.len(), 2);
            assert_eq!(reports[0]["data"]["report"]["period"], period);
            assert_eq!(
                reports[0]["data"]["report"]["channel"],
                if reasoning { "reasoning" } else { "answer" }
            );
            assert!(!reports[0].to_string().contains("investigationword"));
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event["type"] == "generation_recovery")
                    .count(),
                2
            );
            assert!(!events.iter().any(|event| event["type"] == "model_retry"));
            let log = fs::read_to_string(result.log_path).unwrap();
            assert!(
                log.contains("conservatively charged/unknown attempts=2"),
                "{log}"
            );
        }
    }
}

#[test]
fn generation_unary_length_is_checked_before_reserve_growth() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let mut reserves = Vec::new();
    let (result, calls) = run(
        dir.path(),
        false,
        None,
        &sav::CancellationToken::new(),
        |call, request, stream| {
            reserves.push(request["options"]["num_predict"].as_u64().unwrap());
            if call == 1 {
                loop_response(stream, 150, false, false, true, serde_json::json!([]));
            } else {
                correction(request, 1);
                respond(stream, 10, 4);
            }
        },
    );
    let result = result.unwrap();
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 2);
    assert_eq!(reserves, [256, 256], "correction enlarged the allowance");
    assert_eq!(
        result.tokens_input,
        Some(20),
        "discarded unary usage must remain known"
    );
    assert_eq!(result.tokens_output, Some(8));
    assert!(
        !events(dir.path())
            .iter()
            .any(|event| event["type"] == "model_length_retry")
    );
}

#[test]
fn generation_recovery_is_bounded_and_stalled_is_not_success() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (result, calls) = run(
        dir.path(),
        false,
        None,
        &sav::CancellationToken::new(),
        |_, _, stream| {
            loop_response(stream, 100, false, false, false, serde_json::json!([]));
        },
    );
    let result = result.unwrap();
    assert!(!result.success, "loop became completion: {}", result.stdout);
    assert!(!result.surfaced_decision);
    assert_eq!(calls, 3, "must permit exactly two corrections");
    assert!(result.stderr.contains("stalled"), "{}", result.stderr);
    assert!(
        result.stderr.contains("100"),
        "missing actionable metadata: {}",
        result.stderr
    );
    assert!(result.stderr.contains("answer"), "{}", result.stderr);
    assert_eq!(
        events(dir.path())
            .iter()
            .filter(|event| event["type"] == "generation_recovery")
            .count(),
        2
    );
}

#[test]
fn generation_discarded_unary_tools_never_reach_the_broker() {
    let dir = tempfile::tempdir_in(".").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let (result, calls) = run(
        dir.path(),
        false,
        None,
        &sav::CancellationToken::new(),
        |call, request, stream| {
            if call == 1 {
                loop_response(
                    stream,
                    200,
                    false,
                    false,
                    false,
                    serde_json::json!([
                        {"function":{"name":"write_file","arguments":{"destination":"src/rejected.rs","content":"forbidden"}}},
                        {"function":{"name":"propose_decision","arguments":{"summary":"Option A: change scope; Option B: change contract","why":"choose"}}}
                    ]),
                );
            } else {
                correction(request, 1);
                respond(stream, 10, 4);
            }
        },
    );
    let result = result.unwrap();
    assert!(
        result.success,
        "rejected proposals reached authority: {}",
        result.stderr
    );
    assert!(!result.surfaced_decision);
    assert_eq!(calls, 2);
    assert!(!dir.path().join("src/rejected.rs").exists());
    let events = events(dir.path());
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "broker_decision")
            .count(),
        1
    );
    assert!(!events.iter().any(|event| event["type"] == "tool_dispatch"));
}

#[test]
fn generation_parent_cancellation_on_correction_wins_over_repetition() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let token = sav::CancellationToken::new();
    let (result, calls) = run(dir.path(), true, None, &token, |call, _, stream| {
        if call == 1 {
            loop_response(stream, 100, true, true, false, serde_json::json!([]));
        } else {
            token.cancel();
        }
    });
    assert_eq!(calls, 2);
    assert!(
        matches!(
            result,
            Err(maerg::KvistError::Sav(
                sav::Error::Cancelled | sav::Error::ModelTransportCancelled
            ))
        ),
        "{result:?}"
    );
    let events = events(dir.path());
    assert!(!events.iter().any(|event| event["type"] == "tool_dispatch"));
}

#[test]
fn generation_known_rejected_usage_exhausts_the_same_token_allowance() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let (result, calls) = run(
        dir.path(),
        false,
        Some(11000),
        &sav::CancellationToken::new(),
        |_, _, stream| {
            let body = serde_json::json!({
                "model":"fixture","message":{"role":"assistant","content":prose(100)},
                "done":true,"done_reason":"stop","prompt_eval_count":10000,"eval_count":100
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        },
    );
    let result = result.unwrap();
    assert_eq!(calls, 1, "spent tokens replenished for recovery");
    assert!(!result.success);
    assert!(result.stderr.contains("token"), "{}", result.stderr);
    assert_eq!(result.tokens_input, Some(10000));
    assert_eq!(result.tokens_output, Some(100));
}

#[test]
fn generation_interrupted_unknown_usage_remains_charged_before_correction() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let token = sav::CancellationToken::new();
    let (result, calls) = run(dir.path(), true, Some(2000), &token, |_, _, stream| {
        assert!(loop_response(
            stream,
            150,
            true,
            true,
            false,
            serde_json::json!([]),
        ));
    });
    let result = result.unwrap();
    assert_eq!(calls, 1, "interrupted reservation became a free attempt");
    assert!(!result.success);
    assert!(result.stderr.contains("token"), "{}", result.stderr);
    assert_eq!(result.tokens_input, None);
    assert_eq!(result.tokens_output, None);
    assert!(!token.is_cancelled());
    let log = fs::read_to_string(result.log_path).unwrap();
    assert!(
        log.contains("conservatively charged/unknown attempts=1"),
        "{log}"
    );
}

#[test]
fn generation_accepted_inspections_do_not_replenish_corrections_or_lose_context() {
    let dir = tempfile::tempdir_in(".").unwrap();
    fs::write(dir.path().join("first.rs"), "first completed observation").unwrap();
    fs::write(dir.path().join("second.rs"), "second completed observation").unwrap();
    let (result, calls) = run(
        dir.path(),
        false,
        None,
        &sav::CancellationToken::new(),
        |call, request, stream| {
            if call >= 3 {
                assert!(request.to_string().contains("first completed observation"));
            }

            if call == 5 {
                assert!(request.to_string().contains("second completed observation"));
            }
            if call % 2 == 1 {
                loop_response(stream, 100, false, false, false, serde_json::json!([]));
            } else {
                correction(request, call / 2);
                respond_with_tools(
                    stream,
                    10,
                    4,
                    read_call(if call == 2 { "first.rs" } else { "second.rs" }),
                );
            }
        },
    );
    let result = result.unwrap();
    assert!(!result.success);
    assert_eq!(
        calls, 5,
        "accepted turns reset the authoring recovery budget"
    );
    assert_eq!(result.tokens_input, Some(50));
    assert_eq!(result.tokens_output, Some(20));
    let events = events(dir.path());
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "generation_recovery")
            .count(),
        2,
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "tool_result")
            .count(),
        2,
    );
}

#[test]
fn generation_feedback_survives_guidance_and_records_two_distinct_opportunities() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let controls = sav::runtime::ControlMailbox::default();
    let (result, calls) = run_controlled(
        dir.path(),
        false,
        None,
        &sav::CancellationToken::new(),
        &controls,
        |call, request, stream| match call {
            1 | 3 => {
                if call == 3 {
                    correction(request, 1);
                    assert!(request.to_string().contains("new human guidance"));
                }
                loop_response(stream, 100, false, false, false, serde_json::json!([]));
            }
            2 => {
                correction(request, 1);
                controls.steer("new human guidance").unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                assert_eq!(stream.read(&mut [0]).unwrap(), 0);
            }
            4 => {
                correction(request, 1);
                correction(request, 2);
                assert!(request.to_string().contains("new human guidance"));
                respond(stream, 10, 4);
            }
            _ => panic!("unexpected recovery request {call}"),
        },
    );
    let result = result.unwrap();
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 4);
    let events = events(dir.path());
    let notices: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "generation_recovery")
        .collect();
    assert_eq!(notices.len(), 2);
    for (index, notice) in notices.iter().enumerate() {
        assert_eq!(notice["data"]["attempt"], index + 1);
        let feedback = notice["data"]["feedback"]
            .as_str()
            .expect("recorded feedback");
        assert!(feedback.contains(&format!("Recovery {}/2", index + 1)));
        assert!(!feedback.contains("investigationword"));
    }
}

#[test]
fn generation_known_length_attempts_contribute_to_cumulative_progress() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let mut reserves = Vec::new();
    let (result, calls) = run(
        dir.path(),
        false,
        None,
        &sav::CancellationToken::new(),
        |call, request, stream| {
            reserves.push(request["options"]["num_predict"].as_u64().unwrap());
            if call == 1 {
                let body = serde_json::json!({
                    "model":"fixture","message":{"role":"assistant","content":"unfinished segment"},
                    "done":true,"done_reason":"length","prompt_eval_count":11,"eval_count":7
                })
                .to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            } else {
                respond(stream, 13, 5);
            }
        },
    );
    let result = result.unwrap();
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 2);
    assert_eq!(reserves, [256, 512]);
    assert_eq!(result.tokens_input, Some(24));
    assert_eq!(result.tokens_output, Some(12));
    let events = events(dir.path());
    let progress: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "model_accounting")
        .collect();
    assert_eq!(
        progress.len(),
        2,
        "every returned attempt needs progress evidence"
    );
    assert_eq!(progress[1]["data"]["returned_attempts"], 2);
    assert_eq!(progress[1]["data"]["cumulative_usage"]["input_tokens"], 24);
    assert_eq!(progress[1]["data"]["cumulative_usage"]["output_tokens"], 12);
    assert_eq!(progress[1]["data"]["cumulative_usage"]["total_tokens"], 36);
    assert_eq!(progress[1]["data"]["provenance_complete"], true);
}

#[test]
fn generation_unknown_retry_permanently_marks_progress_unavailable_without_refund() {
    for limit in [None, Some(2000)] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let (result, calls) = run(
            dir.path(),
            false,
            limit,
            &sav::CancellationToken::new(),
            |call, _, stream| {
                if call == 1 {
                    stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    respond(stream, 10, 4);
                }
            },
        );
        let result = result.unwrap();
        assert_eq!(calls, if limit.is_some() { 1 } else { 2 });
        assert_eq!(result.success, limit.is_none(), "{}", result.stderr);
        assert_eq!(result.tokens_input, limit.is_none().then_some(10));
        assert_eq!(result.tokens_output, limit.is_none().then_some(4));
        let events = events(dir.path());
        let progress: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "model_accounting")
            .collect();
        assert_eq!(progress.len(), calls);
        for event in &progress {
            assert!(event["data"]["cumulative_usage"].is_null());
            assert_eq!(event["data"]["provenance_complete"], false);
        }
        let last = progress.last().unwrap();
        assert_eq!(last["data"]["uncertain_attempts"], 1);
        if limit.is_some() {
            assert!(last["data"]["charged_tokens"].as_u64().unwrap() > 256);
            assert!(result.stderr.contains("token"));
        } else {
            assert_eq!(last["data"]["known_usage"]["input_tokens"], 10);
            assert_eq!(last["data"]["known_usage"]["output_tokens"], 4);
        }
    }
}
