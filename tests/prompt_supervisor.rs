use kvist::prompt_supervisor::run_supervised_prompt;

#[test]
#[cfg(target_os = "linux")]
fn test_supervised_prompt_successful_execution() {
    let outcome =
        run_supervised_prompt("echo", &["Hello, Kvist Supervisor".to_owned()], 5, false, 0);
    assert!(outcome.is_ok());
}

#[test]
#[cfg(target_os = "linux")]
fn test_supervised_prompt_idle_timeout() {
    let start = std::time::Instant::now();
    // sleep 10, but idle_timeout is 1 second
    let outcome = run_supervised_prompt("sleep", &["10".to_owned()], 1, false, 0);
    // Should fail because it timed out and has 0 restarts allowed
    assert!(outcome.is_err());
    let elapsed = start.elapsed().as_secs();
    // Verify it was terminated quickly (well before 10 seconds)
    assert!(elapsed < 5);
}

#[test]
#[cfg(target_os = "linux")]
fn test_supervised_prompt_substring_loop_detection() {
    // Output a repeating cycle
    let outcome = run_supervised_prompt(
        "printf",
        &["ABCDEFGHIJABC_ABCDEFGHIJABC_ABCDEFGHIJABC_".to_owned()],
        5,
        true, // Enable loop detection
        0,
    );
    assert!(outcome.is_err());
    assert!(
        outcome
            .unwrap_err()
            .to_string()
            .contains("maximum automatic restarts exceeded due to repetition loop")
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_supervised_prompt_line_loop_detection() {
    let outcome = run_supervised_prompt(
        "printf",
        &["duplicate-line\nduplicate-line\nduplicate-line\nduplicate-line\n".to_owned()],
        5,
        true, // Enable loop detection
        0,
    );
    assert!(outcome.is_err());
    assert!(
        outcome
            .unwrap_err()
            .to_string()
            .contains("maximum automatic restarts exceeded due to repetition loop")
    );
}
