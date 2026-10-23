use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sav::{
    CancellationToken, CommandSpec, Error, OutputStream, SupervisionPolicy, run_supervised_observed,
};

fn policy() -> SupervisionPolicy {
    SupervisionPolicy {
        idle_timeout: Duration::from_secs(2),
        attempt_timeout: Some(Duration::from_secs(3)),
        max_output_bytes: 4096,
        max_retries: 0,
        detect_loops: false,
    }
}

#[test]
fn opaque_process_streams_once_without_native_tool_interpretation() {
    let mut chunks = Vec::new();
    let report = run_supervised_observed(
        &policy(),
        &CommandSpec::new(
            "/bin/sh",
            ["-c", "printf '{\"tool_calls\":[]}'; printf diagnostic >&2"],
        ),
        &CancellationToken::new(),
        &mut |stream, bytes| {
            chunks.push((stream, bytes.to_vec()));
            Ok(())
        },
    )
    .expect("external command");
    assert_eq!(report.attempts, 1);
    assert_eq!(report.stdout, b"{\"tool_calls\":[]}");
    assert_eq!(report.stderr, b"diagnostic");
    assert!(
        chunks
            .iter()
            .any(|(stream, _)| *stream == OutputStream::Stdout)
    );
    assert!(
        chunks
            .iter()
            .any(|(stream, _)| *stream == OutputStream::Stderr)
    );
}

#[test]
fn cancellation_and_recording_failure_terminate_the_owned_process() {
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let started = Arc::new(Mutex::new(false));
    let signal = Arc::clone(&started);
    let worker = thread::spawn(move || {
        run_supervised_observed(
            &policy(),
            &CommandSpec::new("/bin/sh", ["-c", "printf started; exec sleep 30"]),
            &token,
            &mut |_, _| {
                *signal.lock().expect("started") = true;
                Ok(())
            },
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !*started.lock().expect("started") && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(*started.lock().expect("started"));
    cancellation.cancel();
    assert!(matches!(
        worker.join().expect("worker"),
        Err(Error::Cancelled)
    ));

    let start = Instant::now();
    let failure = run_supervised_observed(
        &policy(),
        &CommandSpec::new("/bin/sh", ["-c", "printf dispatch; exec sleep 30"]),
        &CancellationToken::new(),
        &mut |_, _| {
            Err(Error::Observation {
                reason: "required recorder failed".into(),
            })
        },
    )
    .expect_err("record failure");
    assert!(matches!(failure, Error::Observation { .. }));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn opaque_effects_cannot_be_retried_and_pre_cancel_never_spawns() {
    let mut retries = policy();
    retries.max_retries = 1;
    let command = CommandSpec::new("/program-that-must-not-be-spawned", [] as [&str; 0]);
    assert!(matches!(
        run_supervised_observed(
            &retries,
            &command,
            &CancellationToken::new(),
            &mut |_, _| Ok(())
        ),
        Err(Error::InvalidPolicy { .. })
    ));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        run_supervised_observed(&policy(), &command, &cancellation, &mut |_, _| Ok(())),
        Err(Error::Cancelled)
    ));
}
