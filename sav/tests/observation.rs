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
fn oversized_initial_metadata_never_reserves_a_filename_and_serialization_is_bounded() {
    use serde::ser::{Serialize, SerializeSeq, Serializer};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct LazySequence<'a>(&'a AtomicUsize);
    impl Serialize for LazySequence<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(Some(1_000_000))?;
            for _ in 0..1_000_000 {
                self.0.fetch_add(1, Ordering::Relaxed);
                sequence.serialize_element(&0)?;
            }

            sequence.end()
        }
    }
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    assert!(RunJournal::create(&path, "r", json!({"text":"x".repeat(65_536)})).is_err());
    assert!(
        !path.exists(),
        "invalid input must not consume a no-clobber identity"
    );
    let mut writer = RunJournal::create(&path, "r", json!({})).expect("valid retry");
    let before = fs::read(&path).expect("before");
    let count = AtomicUsize::new(0);
    assert!(writer.record(&LazySequence(&count)).is_err());
    assert!(
        count.load(Ordering::Relaxed) < 65_536,
        "stop serializing at the byte bound"
    );
    assert_eq!(fs::read(&path).expect("unchanged"), before);
}

#[test]
fn runtime_control_bridge_never_promotes_late_guidance_to_new_tasks() {
    let controls = sav::runtime::ControlMailbox::default();
    let cancellation = sav::CancellationToken::new();
    let server = sav::observation::bind_runtime_controls(&controls, &cancellation).expect("server");
    let send =
        |request| send_control(server.path(), &request, Duration::from_secs(1)).expect("exchange");
    assert_eq!(
        send(ControlRequest::guide(1, "too early")).status,
        "rejected"
    );
    assert_eq!(send(ControlRequest::cancel(2)).status, "rejected");
    assert!(!cancellation.is_cancelled());
    controls.begin_run().expect("owner");
    let reply = send(ControlRequest::guide(3, "retain the original objective"));
    assert_eq!(reply.status, "queued");
    let guidance = controls.claim().expect("claim");
    assert_eq!(guidance.len(), 1);
    assert!(
        reply
            .message
            .expect("correlation")
            .contains(&format!("#{}", guidance[0].id.0))
    );
    assert_eq!(
        send(ControlRequest::cancel(4)).status,
        "cancellation_requested"
    );
    assert!(cancellation.is_cancelled());
    controls.end_run().expect("end");
    assert_eq!(
        send(ControlRequest::guide(5, "not a replacement")).status,
        "rejected"
    );
    assert!(controls.next_task().expect("tasks").is_none());
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
fn journals_rotate_past_64_mib_without_ending_the_run() {
    let dir = tempfile::tempdir().expect("directory");
    let logs = dir.path().join("logs");
    let retained = dir.path().join("retained");
    let unrelated = dir.path().join("unrelated");
    fs::create_dir(&logs).unwrap();
    fs::create_dir(&unrelated).unwrap();
    let path = logs.join("long.jsonl");
    let mut writer = RunJournal::create(&path, "long-run", json!({})).expect("journal");
    let mut follower = JournalFollower::open(&path).expect("live follower");
    fs::rename(&logs, &retained).unwrap();
    symlink(&unrelated, &logs).unwrap();
    let event = json!({"type":"text","data":{"text":"x".repeat(60 * 1024)}});
    for _ in 0..1100 {
        writer.record(&event).expect("record beyond old total cap");
    }

    writer.finish("completed").expect("terminal");
    for entry in fs::read_dir(&retained).expect("segments") {
        assert!(entry.expect("entry").metadata().expect("metadata").len() <= 64 * 1024 * 1024);
    }
    assert!(retained.join("long.jsonl.part-000001").is_file());
    assert_eq!(
        fs::read_dir(&unrelated).unwrap().count(),
        0,
        "rotation must use the held directory"
    );
    let mut text_records = 0;
    let mut sequence = 0;
    while !follower.is_finished() {
        let rows = follower.poll().expect("cross segment");
        assert!(!rows.is_empty());
        assert!(rows.len() <= 256);
        for row in rows {
            sequence += 1;
            assert_eq!(row.sequence, sequence);
            text_records += usize::from(row.event["type"] == "text");
        }
    }
    follower.end_of_input().expect("complete");
    assert_eq!(text_records, 1100);
    let mut snapshot = JournalFollower::open(&retained.join("long.jsonl")).unwrap();
    while !snapshot.is_finished() {
        assert!(!snapshot.poll().unwrap().is_empty());
    }
    snapshot.end_of_input().unwrap();
}

#[test]
fn terminal_at_poll_boundary_cannot_hide_trailing_data() {
    let dir = tempfile::tempdir().unwrap();
    for ordinary_records in [254, 510] {
        for trailing in ["", "unexpected trailing data\n", "\n"] {
            let path = dir
                .path()
                .join(format!("boundary-{ordinary_records}-{}", trailing.len()));
            let mut writer = RunJournal::create(&path, "boundary", json!({})).unwrap();
            for _ in 0..ordinary_records {
                writer.record(&json!({"type":"note"})).unwrap();
            }
            writer.finish("completed").unwrap();
            OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(trailing.as_bytes())
                .unwrap();
            let mut follower = JournalFollower::open(&path).unwrap();
            let mut rejected = false;
            while !follower.is_finished() {
                match follower.poll() {
                    Ok(records) => assert!(!records.is_empty() && records.len() <= 256),
                    Err(_) => {
                        rejected = true;
                        break;
                    }
                }
            }
            assert_eq!(
                rejected,
                !trailing.is_empty(),
                "terminal after {ordinary_records} ordinary records; trailing {trailing:?}"
            );
            assert_eq!(follower.end_of_input().is_err(), rejected);
        }
    }
}

#[test]
fn followers_reject_invalid_missing_and_linked_continuations() {
    for invalid in [
        "missing", "link", "index", "path", "trailing", "mixed", "gap",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run");
        let mut writer = RunJournal::create(&path, "r", json!({})).unwrap();
        assert!(
            writer
                .record(&json!({"type":"journal_segment","data":{"index":1}}))
                .is_err()
        );
        drop(writer);
        let data = match invalid {
            "index" => json!({"index":2}),
            "path" => json!({"index":1,"path":"elsewhere"}),
            _ => json!({"index":1}),
        };
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(
            file,
            "{}",
            json!({"schema_version":1,"run_id":"r","sequence":2,
            "event":{"type":"journal_segment","data":data}})
        )
        .unwrap();
        if invalid == "trailing" {
            writeln!(file, "unexpected trailing data").unwrap();
        }
        let next = dir.path().join("run.part-000001");
        if invalid == "link" {
            symlink(&path, &next).unwrap();
        } else if invalid != "missing" {
            fs::write(
                &next,
                format!(
                    "{}\n",
                    json!({
                        "schema_version":1, "run_id":if invalid == "mixed" { "other" } else { "r" },
                        "sequence":if invalid == "gap" { 4 } else { 3 },
                        "event":{"type":"run_end","data":{"disposition":"completed"}}
                    })
                ),
            )
            .unwrap();
        }
        assert!(
            JournalFollower::open(&path).unwrap().poll().is_err(),
            "{invalid}"
        );
    }
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
fn journal_rotation_collision_stops_without_partial_append() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    let mut writer = RunJournal::create(&path, "r", json!({})).expect("journal");
    let next = dir.path().join("run.part-000001");
    fs::write(&next, "reserved").expect("existing segment");
    let event = json!({"type":"note","text":"x".repeat(60_000)});
    for _ in 0..1200 {
        let before = fs::metadata(&path).expect("metadata").len();
        match writer.record(&event) {
            Ok(()) => assert!(fs::metadata(&path).expect("metadata").len() <= 64 * 1024 * 1024),
            Err(_) => {
                assert_eq!(fs::metadata(&path).expect("metadata").len(), before);
                assert!(before > 63 * 1024 * 1024);
                assert_eq!(fs::read_to_string(&next).unwrap(), "reserved");
                assert!(writer.record(&json!({"type":"late"})).is_err());
                assert!(writer.checkpoint().is_err());
                return;
            }
        }
    }
    panic!("journal overwrote an existing continuation segment");
}
