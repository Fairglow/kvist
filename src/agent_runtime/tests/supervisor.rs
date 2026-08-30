use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use agent_runtime::{AttemptContext, CommandSpec, SupervisionPolicy, run_supervised};

fn policy(max_retries: u32) -> SupervisionPolicy {
    SupervisionPolicy {
        idle_timeout: Duration::from_secs(5),
        attempt_timeout: None,
        detect_loops: true,
        max_retries,
        max_output_bytes: 1024 * 1024,
    }
}

#[test]
fn successful_process_returns_one_attempt() {
    let descendant_policy = SupervisionPolicy {
        detect_loops: false,
        ..policy(0)
    };
    let report = run_supervised(&descendant_policy, |_| {
        Ok(CommandSpec::new("printf", ["completed"]))
    })
    .expect("supervised command succeeds");

    assert_eq!(report.attempts, 1);
}

#[test]
fn retry_context_warns_about_prior_side_effects() {
    let contexts = Arc::new(Mutex::new(Vec::<AttemptContext>::new()));
    let observed = Arc::clone(&contexts);

    let report = run_supervised(&policy(1), move |context| {
        observed
            .lock()
            .expect("lock contexts")
            .push(context.clone());
        if context.attempt_number == 1 {
            Ok(CommandSpec::new(
                "sh",
                [
                    "-c",
                    "printf 'same-line\\nsame-line\\nsame-line\\nsame-line\\n'; sleep 1",
                ],
            ))
        } else {
            Ok(CommandSpec::new("true", std::iter::empty::<String>()))
        }
    })
    .expect("second attempt succeeds");

    assert_eq!(report.attempts, 2);
    let contexts = contexts.lock().expect("lock contexts");
    let notice = contexts[1].retry_notice().expect("retry notice");
    assert!(notice.contains("attempt 2"));
    assert!(notice.contains("may have changed files or external systems"));
    assert!(notice.contains("repetition loop"));
}

#[test]
fn nonzero_exit_is_not_retried() {
    let invocations = Arc::new(Mutex::new(0_u32));
    let observed = Arc::clone(&invocations);

    let error = run_supervised(&policy(3), move |_| {
        *observed.lock().expect("lock count") += 1;
        Ok(CommandSpec::new("false", std::iter::empty::<String>()))
    })
    .expect_err("nonzero exit fails");

    assert!(error.to_string().contains("exit status"));
    assert_eq!(*invocations.lock().expect("lock count"), 1);
}

#[test]
fn idle_process_is_terminated_before_retry_exhaustion_returns() {
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(1),
        attempt_timeout: None,
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 1024 * 1024,
    };
    let started = Instant::now();

    let error = run_supervised(&policy, |_| Ok(CommandSpec::new("sleep", ["10"])))
        .expect_err("idle timeout fails");

    assert!(error.to_string().contains("idle timeout"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn attempt_timeout_is_independent_of_continuing_output() {
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(5),
        attempt_timeout: Some(Duration::from_millis(200)),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: 1024 * 1024,
    };
    let started = Instant::now();

    let error = run_supervised(&policy, |_| {
        Ok(CommandSpec::new(
            "sh",
            ["-c", "while :; do printf x; sleep 0.05; done"],
        ))
    })
    .expect_err("attempt timeout must be terminal");

    assert!(error.to_string().contains("attempt timeout"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn rejects_unbounded_retry_configuration() {
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(5),
        attempt_timeout: None,
        detect_loops: false,
        max_retries: 11,
        max_output_bytes: 1024 * 1024,
    };

    let error = run_supervised(&policy, |_| {
        Ok(CommandSpec::new("true", std::iter::empty::<String>()))
    })
    .expect_err("invalid retry count");

    assert!(error.to_string().contains("at most 10"));
}

#[test]
fn output_limit_is_terminal_and_not_retried() {
    let invocations = Arc::new(Mutex::new(0_u32));
    let observed = Arc::clone(&invocations);
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(5),
        attempt_timeout: None,
        detect_loops: false,
        max_retries: 3,
        max_output_bytes: 16,
    };

    let error = run_supervised(&policy, move |_| {
        *observed.lock().expect("lock count") += 1;
        Ok(CommandSpec::new("printf", ["more-than-sixteen-bytes"]))
    })
    .expect_err("output limit fails");

    assert!(error.to_string().contains("16-byte output limit"));
    assert_eq!(*invocations.lock().expect("lock count"), 1);
}

#[test]
fn descendants_holding_output_pipes_are_terminated_after_parent_exit() {
    let started = Instant::now();

    let descendant_policy = SupervisionPolicy {
        detect_loops: false,
        ..policy(0)
    };
    let report = run_supervised(&descendant_policy, |_| {
        Ok(CommandSpec::new(
            "/bin/sh",
            ["-c", "/bin/sleep 30 & exit 0"],
        ))
    })
    .expect("parent exit succeeds");

    assert_eq!(report.attempts, 1);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn escaped_descendant_retaining_output_fails_without_hanging() {
    let started = Instant::now();

    let error = run_supervised(&policy(0), |_| {
        Ok(CommandSpec::new(
            "/bin/sh",
            ["-c", "setsid /bin/sh -c '/bin/sleep 5 &'"],
        ))
    })
    .expect_err("retained output must be terminal");

    assert!(error.to_string().contains("output remained open"));
    assert!(started.elapsed() < Duration::from_secs(3));
}
