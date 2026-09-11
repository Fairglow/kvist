use agent_runtime::loop_detection::{
    ActionHashRing, LoopDecision, compute_action_hash, compute_observation_hash, normalize_json,
};
use serde_json::json;

#[test]
fn normalize_json_is_deterministic_with_sorted_keys() {
    let val1 = json!({
        "zebra": 1,
        "alpha": "two",
        "nested": {
            "d": true,
            "c": null
        }
    });

    let val2 = json!({
        "nested": {
            "c": null,
            "d": true
        },
        "alpha": "two",
        "zebra": 1
    });

    assert_eq!(normalize_json(&val1), normalize_json(&val2));
    assert_eq!(
        normalize_json(&val1),
        r#"{"alpha":"two","nested":{"c":null,"d":true},"zebra":1}"#
    );
}

#[test]
fn compute_action_hash_is_canonical_and_sha256() {
    let args1 = json!({"path": "src/main.rs", "offset": 10});
    let args2 = json!({"offset": 10, "path": "src/main.rs"});

    let hash1 = compute_action_hash("read_file", &args1);
    let hash2 = compute_action_hash("read_file", &args2);

    assert_eq!(hash1, hash2);
    assert!(hash1.starts_with("sha256:"));
    assert_eq!(hash1.len(), 7 + 64);
}

#[test]
fn compute_observation_hash_differs_on_output() {
    let obs1 = compute_observation_hash(b"hello world", b"");
    let obs2 = compute_observation_hash(b"hello world", b"error");
    let obs3 = compute_observation_hash(b"hello world", b"");

    assert_eq!(obs1, obs3);
    assert_ne!(obs1, obs2);
    assert!(obs1.starts_with("sha256:"));
}

#[test]
fn action_hash_ring_detects_consecutive_identical_action() {
    let mut ring = ActionHashRing::new();
    let hash_a = compute_action_hash("cat", &json!({"file": "foo.txt"}));

    // First action proceeds
    assert_eq!(ring.check_proposed_action(&hash_a), LoopDecision::Proceed);
    ring.record_action(hash_a.clone(), "cat".to_owned());
    assert_eq!(
        ring.record_observation(compute_observation_hash(b"contents", b"")),
        LoopDecision::Proceed
    );

    // Consecutive identical action triggers Tier 1 Soft Correction
    match ring.check_proposed_action(&hash_a) {
        LoopDecision::SoftCorrection {
            rejection_message,
            consecutive_stalls,
        } => {
            assert_eq!(consecutive_stalls, 1);
            assert!(rejection_message.contains("[SYSTEM REJECTION]"));
            assert!(rejection_message.contains("You have executed this exact action"));
        }
        other => panic!("expected SoftCorrection, got {:?}", other),
    }
}

#[test]
fn action_hash_ring_escalates_to_temperature_jitter_then_circuit_breaker() {
    let mut ring = ActionHashRing::new();
    ring.set_base_temperature(0.2);
    let hash_a = compute_action_hash("retry_cmd", &json!({}));

    // Turn 1
    assert_eq!(ring.check_proposed_action(&hash_a), LoopDecision::Proceed);
    ring.record_action(hash_a.clone(), "retry_cmd".to_owned());

    // Stall 1: Consecutive identical action -> SoftCorrection
    match ring.check_proposed_action(&hash_a) {
        LoopDecision::SoftCorrection {
            consecutive_stalls, ..
        } => {
            assert_eq!(consecutive_stalls, 1);
        }
        other => panic!("expected SoftCorrection, got {:?}", other),
    }

    // Stall 2: Continued identical action -> TemperatureJitter (+0.5)
    match ring.check_proposed_action(&hash_a) {
        LoopDecision::TemperatureJitter {
            base_temperature,
            jittered_temperature,
            consecutive_stalls,
            ..
        } => {
            assert_eq!(consecutive_stalls, 2);
            assert_eq!(base_temperature, 0.2);
            assert_eq!(jittered_temperature, 0.7);
        }
        other => panic!("expected TemperatureJitter, got {:?}", other),
    }

    // Stall 3: Still repeating -> TemperatureJitter
    match ring.check_proposed_action(&hash_a) {
        LoopDecision::TemperatureJitter {
            consecutive_stalls, ..
        } => {
            assert_eq!(consecutive_stalls, 3);
        }
        other => panic!("expected TemperatureJitter, got {:?}", other),
    }

    // Stall 4: Hard circuit breaker triggered at MAX_STALLED_TURNS (4)
    match ring.check_proposed_action(&hash_a) {
        LoopDecision::CircuitBreaker {
            reason,
            stalled_turns,
        } => {
            assert_eq!(stalled_turns, 4);
            assert!(reason.contains("hard circuit breaker"));
        }
        other => panic!("expected CircuitBreaker, got {:?}", other),
    }
}

#[test]
fn action_hash_ring_detects_tier_2_output_invariants() {
    let mut ring = ActionHashRing::new();
    let obs_hash = compute_observation_hash(b"invariant output", b"");

    // Action 1
    let hash1 = compute_action_hash("ls", &json!({}));
    ring.check_proposed_action(&hash1);
    ring.record_action(hash1, "ls".to_owned());
    assert_eq!(
        ring.record_observation(obs_hash.clone()),
        LoopDecision::Proceed
    );

    // Action 2 (different action, same output)
    let hash2 = compute_action_hash("find", &json!({}));
    ring.check_proposed_action(&hash2);
    ring.record_action(hash2, "find".to_owned());
    assert_eq!(
        ring.record_observation(obs_hash.clone()),
        LoopDecision::Proceed
    );

    // Action 3 (third action, identical output across all 3 -> Tier 2 invariant detected!)
    let hash3 = compute_action_hash("dir", &json!({}));
    ring.check_proposed_action(&hash3);
    ring.record_action(hash3, "dir".to_owned());
    match ring.record_observation(obs_hash) {
        LoopDecision::SoftCorrection {
            rejection_message,
            consecutive_stalls,
        } => {
            assert_eq!(consecutive_stalls, 1);
            assert!(rejection_message.contains("[SYSTEM REJECTION]"));
        }
        other => panic!(
            "expected SoftCorrection on invariant output, got {:?}",
            other
        ),
    }
}

#[test]
fn successful_progress_resets_stalls() {
    let mut ring = ActionHashRing::new();
    let hash_a = compute_action_hash("cmd", &json!({}));
    ring.record_action(hash_a.clone(), "cmd".to_owned());
    ring.record_observation(compute_observation_hash(b"output 1", b""));

    // Trigger stall 1
    assert!(ring.check_proposed_action(&hash_a).is_loop_break());
    assert_eq!(ring.consecutive_stalls(), 1);

    // New action with new mutated observation resets stalls
    let hash_b = compute_action_hash("other_cmd", &json!({}));
    assert_eq!(ring.check_proposed_action(&hash_b), LoopDecision::Proceed);
    ring.record_action(hash_b, "other_cmd".to_owned());
    assert_eq!(
        ring.record_observation(compute_observation_hash(b"new mutated output", b"")),
        LoopDecision::Proceed
    );
    assert_eq!(ring.consecutive_stalls(), 0);
}

#[test]
fn compute_ngram_jaccard_similarity_evaluates_text_overlap() {
    use agent_runtime::loop_detection::compute_ngram_jaccard_similarity;

    // Identical texts
    let text1 = "Let us read file src/main.rs to understand the program entry point.";
    let text2 = "Let us read file src/main.rs to understand the program entry point.";
    assert_eq!(compute_ngram_jaccard_similarity(text1, text2, 4), 1.0);

    // Completely disjoint texts
    let text_a = "apples oranges bananas grapes kiwi melon pineapple";
    let text_b = "quantum physics electrodynamics relativity thermodynamics";
    assert_eq!(compute_ngram_jaccard_similarity(text_a, text_b, 3), 0.0);

    // Minor formatting or synonym changes still yield high similarity
    let r1 = "I need to inspect the file configuration and check if the database port is set.";
    let r2 = "I need to inspect the file configuration and see if the database port is set.";
    let sim = compute_ngram_jaccard_similarity(r1, r2, 3);
    assert!(sim > 0.60, "similarity was {sim}");
}

#[test]
fn action_hash_ring_detects_tier_3_circular_reasoning() {
    let mut ring = ActionHashRing::new();

    let thought1 = "I need to check the configuration file to determine why the test suite is failing with a timeout.";
    assert_eq!(ring.check_reasoning(thought1), LoopDecision::Proceed);

    // Novel thought proceeds
    let thought2 = "Now I will edit src/config.rs to increase the timeout threshold from five to thirty seconds.";
    assert_eq!(ring.check_reasoning(thought2), LoopDecision::Proceed);

    // Nearly identical circular thought triggers Tier 3 loop detection
    let thought3 = "Now I will edit src/config.rs to increase the timeout threshold from five to thirty seconds!";
    match ring.check_reasoning(thought3) {
        LoopDecision::SoftCorrection {
            rejection_message,
            consecutive_stalls,
        } => {
            assert_eq!(consecutive_stalls, 1);
            assert!(rejection_message.contains("[SYSTEM REJECTION]"));
        }
        other => panic!(
            "expected SoftCorrection on circular reasoning, got {:?}",
            other
        ),
    }
}

#[test]
fn action_hash_ring_detects_window_frequency_limit() {
    let mut ring = ActionHashRing::new();
    let action_a = compute_action_hash("read_file", &json!({"path": "a.txt"}));
    let action_b = compute_action_hash("read_file", &json!({"path": "b.txt"}));
    let action_c = compute_action_hash("read_file", &json!({"path": "c.txt"}));

    // Non-consecutive execution of action_a interleaved with other actions
    // 1. A
    assert_eq!(ring.check_proposed_action(&action_a), LoopDecision::Proceed);
    ring.record_action(action_a.clone(), "read_file".to_owned());
    // 2. B
    assert_eq!(ring.check_proposed_action(&action_b), LoopDecision::Proceed);
    ring.record_action(action_b.clone(), "read_file".to_owned());
    // 3. A (2nd time in window)
    assert_eq!(ring.check_proposed_action(&action_a), LoopDecision::Proceed);
    ring.record_action(action_a.clone(), "read_file".to_owned());
    // 4. C
    assert_eq!(ring.check_proposed_action(&action_c), LoopDecision::Proceed);
    ring.record_action(action_c.clone(), "read_file".to_owned());
    // 5. A (3rd time in window -> WINDOW_IDENTICAL_ACTION_LIMIT triggers loop break)
    assert!(ring.check_proposed_action(&action_a).is_loop_break());
}

#[test]
fn action_hash_ring_clear_resets_state() {
    let mut ring = ActionHashRing::new();
    let action = compute_action_hash("test", &json!({}));
    ring.record_action(action.clone(), "test".to_owned());
    assert!(ring.check_proposed_action(&action).is_loop_break());
    assert_eq!(ring.consecutive_stalls(), 1);

    ring.clear();
    assert_eq!(ring.consecutive_stalls(), 0);
    assert!(ring.history().is_empty());
    assert_eq!(ring.check_proposed_action(&action), LoopDecision::Proceed);
}
