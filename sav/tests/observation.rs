use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sav::observation::{
    ControlAction, ControlReply, ControlRequest, ControlServer, JournalFollower, RunJournal,
    send_control,
};
use serde_json::json;

#[test]
fn follower_sees_live_checkpoints_and_terminal_without_reopening() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run.jsonl");
    let mut writer =
        RunJournal::create(&path, "run-1", json!({"host":"fixture"})).expect("journal");
    let mut reader = JournalFollower::open(&path).expect("reader");
    let start = reader.poll().expect("start");
    assert_eq!(start.len(), 1);
    assert_eq!(start[0].sequence, 1);
    assert!(!reader.is_finished());
    writer
        .record(&json!({"type":"tool_dispatch","data":{"call_id":"one"}}))
        .expect("dispatch");
    writer.checkpoint().expect("sync");
    assert_eq!(reader.poll().expect("live")[0].sequence, 2);
    writer.finish("completed").expect("finish");
    assert_eq!(reader.poll().expect("terminal")[0].event["type"], "run_end");
    assert!(reader.is_finished());
    assert!(writer.record(&json!({"type":"late"})).is_err());
    assert!(RunJournal::create(&path, "run-1", json!({})).is_err());
    assert_eq!(
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn incomplete_tail_is_pending_but_gaps_and_oversized_lines_fail() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("partial");
    fs::write(
        &path,
        b"{\"schema_version\":1,\"run_id\":\"r\",\"sequence\":1,",
    )
    .expect("partial");
    let mut reader = JournalFollower::open(&path).expect("reader");
    assert!(reader.poll().expect("pending").is_empty());
    OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("append")
        .write_all(b"\"event\":{\"type\":\"run_start\"}}\n")
        .expect("complete");
    assert_eq!(reader.poll().expect("complete").len(), 1);
    OpenOptions::new().append(true).open(&path).expect("append")
        .write_all(b"{\"schema_version\":1,\"run_id\":\"r\",\"sequence\":3,\"event\":{\"type\":\"note\"}}\n").expect("gap");
    assert!(reader.poll().is_err());
    let oversized = dir.path().join("oversized");
    fs::write(&oversized, vec![b'a'; 65_537]).expect("oversized");
    assert!(
        JournalFollower::open(&oversized)
            .expect("reader")
            .poll()
            .is_err()
    );
}

#[test]
fn readers_reject_links_special_files_versions_and_mixed_runs() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    let writer = RunJournal::create(&path, "r", json!({})).expect("journal");
    let link = dir.path().join("link");
    symlink(&path, &link).expect("link");
    assert!(JournalFollower::open(&link).is_err());
    assert!(RunJournal::create(&link, "r", json!({})).is_err());
    assert!(JournalFollower::open(dir.path()).is_err());
    drop(writer);
    for record in [
        json!({"schema_version":2,"run_id":"r","sequence":1,"event":{"type":"run_start"}}),
        json!({"schema_version":1,"run_id":"other","sequence":2,"event":{"type":"note"}}),
    ] {
        let mut file = OpenOptions::new().append(true).open(&path).expect("append");
        writeln!(file, "{record}").expect("record");
        assert!(
            JournalFollower::open(&path)
                .expect("reader")
                .poll()
                .is_err()
        );
        let mut journal = fs::read_to_string(&path).expect("read");
        journal.truncate(journal.find('\n').expect("first line") + 1);
        fs::write(&path, journal).expect("restore fixture");
    }
}

#[test]
fn record_validation_is_atomic_and_eof_is_not_completion() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    let mut writer = RunJournal::create(&path, "r", json!({})).expect("journal");
    let before = fs::read(&path).expect("before");
    assert!(
        writer
            .record(&json!({"type":"note","text":"x".repeat(65_536)}))
            .is_err()
    );
    assert!(writer.record(&json!({"type":"run_end"})).is_err());
    assert_eq!(fs::read(&path).expect("unchanged"), before);
    let mut reader = JournalFollower::open(&path).expect("reader");
    assert_eq!(reader.poll().expect("start").len(), 1);
    assert!(!reader.is_finished());
    assert!(reader.poll().expect("eof").is_empty());
    writer
        .record(&json!({"type":"note"}))
        .expect("valid after validation error");
}

#[test]
fn follower_keeps_original_descriptor_and_limits_batches() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    let mut writer = RunJournal::create(&path, "r", json!({})).expect("journal");
    let mut reader = JournalFollower::open(&path).expect("reader");
    for _ in 0..300 {
        writer.record(&json!({"type":"note"})).expect("record");
    }
    assert_eq!(reader.poll().expect("bounded batch").len(), 256);
    fs::rename(&path, dir.path().join("original")).expect("move");
    fs::write(&path, b"not the original").expect("replace");
    assert_eq!(reader.poll().expect("remaining original").len(), 45);
}

#[test]
fn private_controls_are_validated_acknowledged_and_removed() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let server = ControlServer::bind(move |request| {
        captured.lock().expect("requests").push(request.clone());
        match request.action {
            ControlAction::Cancel => ControlReply::cancellation_requested(request.id),
            ControlAction::Guide { .. } => ControlReply::queued(request.id),
        }
    })
    .expect("server");
    let path = server.path().to_owned();
    assert_eq!(
        fs::metadata(path.parent().expect("parent"))
            .expect("directory")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let reply = send_control(
        &path,
        &ControlRequest::guide(1, "retain the original objective"),
        Duration::from_secs(2),
    )
    .expect("guidance");
    assert_eq!(reply.status, "queued");
    assert_eq!(
        send_control(&path, &ControlRequest::cancel(2), Duration::from_secs(2))
            .expect("cancel")
            .status,
        "cancellation_requested"
    );
    assert!(
        send_control(
            &path,
            &ControlRequest::guide(3, " "),
            Duration::from_secs(2)
        )
        .is_err()
    );
    assert!(
        send_control(
            &path,
            &ControlRequest::guide(4, "x".repeat(16_385)),
            Duration::from_secs(2)
        )
        .is_err()
    );
    assert_eq!(seen.lock().expect("requests").len(), 2);
    drop(server);
    assert!(!path.exists());
}

#[test]
fn control_rejects_malformed_requests_without_calling_host_and_bounds_idle_clients() {
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    let count = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&count);
    let server = ControlServer::bind(move |request| {
        captured.fetch_add(1, Ordering::Relaxed);
        ControlReply::queued(request.id)
    })
    .expect("server");
    for request in [
        b"{\"schema_version\":2,\"id\":1,\"action\":{\"type\":\"cancel\"}}\n".as_slice(),
        b"{\"schema_version\":1,\"id\":0,\"action\":{\"type\":\"cancel\"}}\n".as_slice(),
        b"{\"schema_version\":1,\"id\":1,\"action\":{\"type\":\"cancel\"},\"extra\":true}\n"
            .as_slice(),
    ] {
        let mut stream = UnixStream::connect(server.path()).expect("connect");
        stream.write_all(request).expect("malformed request");
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("deadline");
        let mut reply = [0_u8; 1];
        let _ = std::io::Read::read(&mut stream, &mut reply);
    }
    assert_eq!(count.load(Ordering::Relaxed), 0);
    let _idle = UnixStream::connect(server.path()).expect("idle client");
    let start = Instant::now();
    assert_eq!(
        send_control(
            server.path(),
            &ControlRequest::cancel(2),
            Duration::from_secs(2)
        )
        .expect("healthy client after idle")
        .status,
        "queued"
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn journal_enforces_exact_record_and_total_bound_without_partial_append() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    let mut writer = RunJournal::create(&path, "r", json!({})).expect("journal");
    let event = json!({"type":"note","text":"x".repeat(60_000)});
    for _ in 0..1200 {
        let before = fs::metadata(&path).expect("metadata").len();
        match writer.record(&event) {
            Ok(()) => assert!(fs::metadata(&path).expect("metadata").len() <= 64 * 1024 * 1024),
            Err(_) => {
                assert_eq!(fs::metadata(&path).expect("metadata").len(), before);
                assert!(before > 63 * 1024 * 1024);
                return;
            }
        }
    }
    panic!("journal accepted more than its 64 MiB bound");
}
