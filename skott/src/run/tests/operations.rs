use super::*;

#[test]
fn control_capability_presentation_rejects_unprintable_paths_before_filesystem_access() {
    use std::os::unix::ffi::OsStrExt;
    let workdir = std::path::Path::new(".");
    for path in [
        std::path::Path::new("\u{1b}bad/control.sock"),
        std::path::Path::new(std::ffi::OsStr::from_bytes(b"/bad-\xff/control.sock")),
    ] {
        assert!(matches!(validate_control_path(path,workdir),
            Err(Error::Control{reason}) if reason.contains("printable")));
    }
}

#[test]
fn phase4_cancel_active_unblocks_full_channel_without_shutdown_or_drain() {
    let attempted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let logs = dir.path().join("private");
    let log = crate::session_log::SessionLog::open(&logs, "cancel").unwrap();
    let (handle, rx, prompts) = start(
        test_session(),
        FloodTransport(attempted.clone()),
        NoopExecutor,
        ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
        Some(Box::new(log)),
        RetryPolicy::new(1, Duration::from_millis(1), Duration::from_millis(1)),
        1,
        RunLimits::default(),
    )
    .unwrap();
    prompts.send("cancel saturated producer".into()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while attempted.load(Ordering::SeqCst) < 120 && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        attempted.load(Ordering::SeqCst) >= 120,
        "stream did not reach bounded backpressure: {}",
        attempted.load(Ordering::SeqCst)
    );
    thread::sleep(Duration::from_millis(30));
    handle
        .controls()
        .cancel_active(&handle.cancellation)
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while handle.controls().is_active() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let stopped = !handle.controls().is_active();
    let mut terminal = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        terminal.extend(handle.take_terminal_events().unwrap());
        if !handle.is_running() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    // Teardown remains safe even on the prior implementation's real red.
    handle.shutdown();
    drop((handle, rx, prompts));
    assert!(
        stopped,
        "cancel_active left the full-channel producer blocked without a consumer"
    );
    let journal = std::fs::read_dir(&logs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let journal = std::fs::read_to_string(journal).unwrap();
    assert!(
        journal.contains("session_finish") && journal.contains("\"kind\":\"cancelled\""),
        "{journal}\nTerminal: {terminal:?}"
    );
    assert!(
        terminal
            .iter()
            .any(|event| matches!(event, Event::Failed(message)
        if message.contains("event") && message.contains("full") && message.contains("cancelled"))),
        "{terminal:?}"
    );
    let mut app = crate::tui::app::App::new(
        &[],
        None,
        sav::ReasoningEffort::None,
        None,
        120,
        30,
        crate::tui::Theme::dark(),
    );
    for event in terminal {
        app.push_event(event);
    }
    assert!(!app.running);
}

#[test]
fn phase4_exactly_128_full_slots_return_typed_cancelled_not_silent_success() {
    let (sender, _receiver) = mpsc::sync_channel(128);
    for index in 0..128 {
        sender
            .try_send(Event::Note(format!("queued {index}")))
            .unwrap();
    }
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let terminal = Arc::new(Mutex::new(Vec::new()));
    let sink = ChannelSink {
        sender,
        shutdown: Arc::new(AtomicBool::new(false)),
        cancellation,
        terminal: terminal.clone(),
    };
    let started = std::time::Instant::now();
    assert!(matches!(
        sink.send(Event::Text("blocked".into())),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        sink.send(Event::PromptEnd {
            exhausted: false,
            cancelled: true,
            reason: crate::session::StopReason::Cancelled
        }),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        sink.send(Event::Finished {
            message: "completed answer".into()
        }),
        Err(Error::Cancelled)
    ));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(matches!(
        terminal.lock().unwrap().as_slice(),
        [Event::PromptEnd { .. }, Event::Finished { .. }]
    ));
}
