use super::*;

fn repeated_passage() -> String {
    (0..150)
        .map(|index| format!("word{index}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn phase3_cross_turn_repeated_passage_with_unique_successful_noops_stalls() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    for index in 1..=12 {
        fs::write(dir.path().join(format!("src/{index}.rs")), "// preserved\n").expect("file");
    }
    let passage = repeated_passage();
    let (result, calls) = resilience_run_scripted(
        dir.path(),
        |index, _| {
            (
                if index <= 12 {
                    serde_json::json!([{"function":{"name":"write_file","arguments":{
                        "destination":format!("src/{index}.rs"),"content":"// preserved\n"
                    }}}])
                } else {
                    serde_json::json!([])
                },
                passage.clone(),
            )
        },
        None,
    );
    assert!(
        !result.success,
        "unique no-op calls concealed repeated generation"
    );
    assert!(calls <= 12);
    assert_eq!(
        events(dir.path())
            .iter()
            .filter(|event| event["type"] == "generation_recovery")
            .count(),
        2
    );
}

#[test]
fn phase3_fresh_observations_clear_cross_turn_passage_history() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    for index in 1..=12 {
        fs::write(
            dir.path().join(format!("src/{index}.rs")),
            format!("// novel page {index}\n"),
        )
        .expect("file");
    }

    let passage = repeated_passage();
    let (result, calls) = resilience_run_scripted(
        dir.path(),
        |index, _| {
            (
                if index <= 12 {
                    read_call(&format!("src/{index}.rs"))
                } else {
                    serde_json::json!([])
                },
                passage.clone(),
            )
        },
        None,
    );
    assert!(
        result.success,
        "fresh observations triggered a false generation loop: {}",
        result.stderr
    );
    assert_eq!(calls, 13);
    assert!(recoveries(dir.path()).is_empty());
}

#[test]
fn phase3_generation_and_tool_stalls_share_one_correction_allowance() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    for index in 1..=3 {
        fs::write(dir.path().join(format!("src/{index}.rs")), "// preserved\n").expect("file");
    }
    fs::write(dir.path().join("src/page.rs"), "// observation\n").expect("page");
    let passage = repeated_passage();
    let (result, _) = resilience_run_scripted(
        dir.path(),
        |index, _| {
            (
                if index <= 3 {
                    serde_json::json!([{"function":{"name":"write_file","arguments":{
                        "destination":format!("src/{index}.rs"),"content":"// preserved\n"
                    }}}])
                } else if index <= 12 {
                    read_call("src/page.rs")
                } else {
                    serde_json::json!([])
                },
                if index <= 3 {
                    passage.clone()
                } else {
                    format!("inspect step {index}")
                },
            )
        },
        None,
    );
    assert!(!result.success);
    let corrections = events(dir.path())
        .into_iter()
        .filter(|event| {
            matches!(
                event["type"].as_str(),
                Some("generation_recovery" | "tool_recovery")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(corrections.len(), 2);
    assert_eq!(corrections[0]["type"], "generation_recovery");
    assert_eq!(corrections[0]["data"]["attempt"], 1);
    assert_eq!(corrections[1]["type"], "tool_recovery");
    assert_eq!(corrections[1]["data"]["attempt"], 2);
}

fn events(workdir: &Path) -> Vec<serde_json::Value> {
    let journal = fs::read_dir(workdir.join(".kvist/runs"))
        .expect("runs")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
        .expect("journal");
    fs::read_to_string(journal)
        .expect("journal records")
        .lines()
        .map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).expect("event");
            record["event"].clone()
        })
        .collect()
}

fn recoveries(workdir: &Path) -> Vec<serde_json::Value> {
    events(workdir)
        .into_iter()
        .filter(|event| event["type"] == "tool_recovery")
        .collect()
}

fn scripted_loop(
    workdir: &Path,
    mut tools: impl FnMut(usize) -> serde_json::Value + Send,
) -> (kvist::agent::AgentRunResult, usize) {
    resilience_run_scripted(
        workdir,
        |index, _| {
            (
                if index <= 80 {
                    tools(index)
                } else {
                    serde_json::json!([])
                },
                format!("investigation step {index}"),
            )
        },
        None,
    )
}

#[test]
fn phase3_repeatable_failed_reads_get_two_corrections_then_stall() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let (result, calls) = scripted_loop(dir.path(), |_| read_call("src/missing.rs"));
    assert!(
        !result.success,
        "failed observations must not loop to completion"
    );
    assert!(calls < 80, "repeated failures were unbounded: {calls}");
    assert!(result.stderr.contains("stalled"), "{}", result.stderr);
    assert_eq!(recoveries(dir.path()).len(), 2);
    assert!(
        events(dir.path())
            .iter()
            .filter(|event| event["type"] == "tool_result")
            .all(|event| event["data"]["applied"] != true)
    );
}

#[test]
fn phase3_repeatable_failed_discovery_gets_two_corrections_then_stalls() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let (result, calls) = scripted_loop(
        dir.path(),
        |_| serde_json::json!([{"function":{"name":"list_files","arguments":{"path":"missing"}}}]),
    );
    assert!(!result.success, "failed discovery must not run forever");
    assert!(calls < 80);
    assert_eq!(recoveries(dir.path()).len(), 2);
}

#[test]
fn phase3_nine_action_cycle_is_bounded_and_receives_distinct_corrections() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    for index in 0..9 {
        fs::write(
            dir.path().join(format!("src/{index}.rs")),
            format!("// page {index}\n"),
        )
        .expect("page");
    }
    let (result, calls) = scripted_loop(dir.path(), |index| {
        read_call(&format!("src/{}.rs", (index - 1) % 9))
    });
    assert!(!result.success);
    assert!(calls <= 128, "cycle exceeded bounded history: {calls}");
    let corrections = recoveries(dir.path());
    assert_eq!(corrections.len(), 2);
    assert!(
        corrections.iter().any(|event| event["data"]["period"] == 9),
        "{corrections:?}"
    );
}

#[test]
fn phase3_successful_noop_does_not_clear_failed_observation_stagnation() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    fs::write(dir.path().join("src/kept.rs"), "// preserved\n").expect("file");
    let (result, calls) = scripted_loop(dir.path(), |index| {
        if index % 2 == 1 {
            read_call("src/missing.rs")
        } else {
            serde_json::json!([{"function":{"name":"write_file","arguments":{
                "destination":"src/kept.rs","content":"// preserved\n"
            }}}])
        }
    });
    assert!(!result.success, "successful no-op output hid stagnation");
    assert!(calls < 80);
    assert_eq!(recoveries(dir.path()).len(), 2);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/kept.rs")).expect("file"),
        "// preserved\n"
    );
}

#[test]
fn phase3_five_duplicate_batch_calls_allow_both_feedback_opportunities_and_pairing() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    fs::write(dir.path().join("src/page.rs"), "// observed page\n").expect("file");
    let mut requests = Vec::new();
    let (result, calls) = resilience_run_scripted(
        dir.path(),
        |index, request| {
            requests.push(request.clone());
            (
                if index <= 10 {
                    serde_json::Value::Array((0..5).map(|_| {
                        serde_json::json!({"function":{"name":"read_file","arguments":{"path":"src/page.rs"}}})
                    }).collect())
                } else {
                    serde_json::json!([])
                },
                format!("batch {index}"),
            )
        },
        None,
    );
    assert!(!result.success);
    assert!(
        calls >= 3,
        "batch exhausted recovery before model could respond: {calls}"
    );
    let corrections = recoveries(dir.path());
    assert_eq!(corrections.len(), 2);
    for (index, correction) in corrections.iter().enumerate() {
        assert_eq!(correction["data"]["attempt"], index + 1);
        assert_eq!(correction["data"]["limit"], 2);
        let feedback = correction["data"]["feedback"].as_str().expect("feedback");
        assert!(
            requests.iter().any(|request| request["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .any(|message| message["role"] == "user" && message["content"] == feedback)),
            "model never received correction {}: {requests:?}",
            index + 1
        );
    }

    let journal = events(dir.path());
    let mut results = std::collections::BTreeMap::<String, usize>::new();
    let mut result_count = 0;
    for event in &journal {
        if event["type"] == "model_request" && !results.is_empty() {
            assert_eq!(results.len(), 5);
            assert!(results.values().all(|count| *count == 1), "{results:?}");
            results.clear();
        }
        if event["type"] == "tool_result" {
            result_count += 1;
            *results
                .entry(
                    event["data"]["call_id"]
                        .as_str()
                        .expect("identity")
                        .to_owned(),
                )
                .or_default() += 1;
        }
    }
    assert_eq!(result_count, calls * 5);
    assert_eq!(results.len(), 5);
    assert!(results.values().all(|count| *count == 1), "{results:?}");
    assert!(
        journal
            .iter()
            .any(|event| event["type"] == "tool_result" && event["data"]["not_executed"] == true)
    );
}

fn with_effects(
    workdir: &Path,
    answer: impl FnMut(usize, &serde_json::Value) -> (serde_json::Value, String) + Send,
    mut execute: impl FnMut() + Send,
) -> (kvist::agent::AgentRunResult, usize, usize) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let gate = EffectGate::new(workdir);
    let stopped = AtomicBool::new(false);
    thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let mut dispatched = 0;
            while !stopped.load(Ordering::SeqCst) {
                match gate.listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .expect("gate deadline");
                        let mut ready = [0; 6];
                        stream.read_exact(&mut ready).expect("effect started");
                        assert_eq!(&ready, b"ready\n");
                        execute();
                        dispatched += 1;
                        stream.write_all(b"continue\n").expect("release");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("gate: {error}"),
                }
            }
            dispatched
        });
        let (result, calls) = resilience_run_scripted(workdir, answer, Some(&gate));
        stopped.store(true, Ordering::SeqCst);
        (result, calls, worker.join().expect("effects"))
    })
}

#[test]
fn phase3_successful_runner_claim_without_revision_does_not_allow_repeated_effects() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let (result, calls, dispatched) = with_effects(
        dir.path(),
        |index, _| {
            (
                if index <= 8 {
                    serde_json::json!([{"function":{"name":"write_file","arguments":{
                        "destination":"src/changed.rs","content":"// proposed revision\n"
                    }}}])
                } else {
                    serde_json::json!([])
                },
                format!("effect attempt {index}"),
            )
        },
        || {},
    );
    assert!(
        !result.success,
        "runner success alone claimed useful state change"
    );
    assert!(!dir.path().join("src/changed.rs").exists());
    assert_eq!(dispatched, 1, "repeated effect reached dispatch");
    assert!(calls >= 3);
    assert_eq!(recoveries(dir.path()).len(), 2);
}

#[test]
fn phase3_actual_changed_state_allows_verification_of_the_same_effect_intent() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let path = dir.path().join("src/changed.rs");
    let (result, calls, dispatched) = with_effects(
        dir.path(),
        |index, _| {
            (
                if index == 1 || index == 3 {
                    serde_json::json!([{"function":{"name":"write_file","arguments":{
                        "destination":"src/changed.rs","content":"// approved revision\n"
                    }}}])
                } else if index == 2 {
                    read_call("src/changed.rs")
                } else {
                    serde_json::json!([])
                },
                format!("changed state verification {index}"),
            )
        },
        || {
            let staged = fs::read_dir(dir.path().join(".kvist/authoring"))
                .expect("staging")
                .map(|entry| entry.expect("entry").path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .expect("staged effect");
            kvist::authoring::apply::apply_intent(dir.path(), &staged)
                .expect("actual bound effect");
        },
    );
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 4);
    assert_eq!(dispatched, 1);
    assert_eq!(
        fs::read_to_string(path).expect("revision"),
        "// approved revision\n"
    );
    assert!(recoveries(dir.path()).is_empty());
}
