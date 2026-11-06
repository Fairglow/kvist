use crate::{Result, agent::AuthoringSequence};
use std::{fs, path::Path};

/// The existing change list is shallow; recursive scope digests remain separate.
pub(super) fn changes(project: &Path, component: &Path) -> Result<Vec<serde_json::Value>> {
    changes_with_read(project, component, super::read_bounded_scope_file)
}

fn changes_with_read(
    project: &Path,
    component: &Path,
    mut read: impl FnMut(&Path, &fs::Metadata, &mut super::ScopeHashBudget) -> Result<Vec<u8>>,
) -> Result<Vec<serde_json::Value>> {
    let mut changes = Vec::new();
    let mut budget = super::ScopeHashBudget::default();
    let scope = component.join("tests");
    let metadata = match fs::symlink_metadata(&scope) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(changes),
        Err(source) => {
            return Err(crate::KvistError::Io {
                operation: "inspect final change inventory directory",
                path: scope,
                source,
            });
        }
        Ok(metadata) => metadata,
    };
    if super::is_link_like(&metadata) || !metadata.is_dir() {
        return Err(super::scope_hash_error(
            &scope,
            "final change inventory must be a non-link directory",
        ));
    }
    for (_, path) in super::read_scope_entries(&scope, &mut budget)? {
        let metadata = fs::symlink_metadata(&path).map_err(|source| crate::KvistError::Io {
            operation: "inspect final change inventory entry",
            path: path.clone(),
            source,
        })?;
        if super::is_link_like(&metadata) {
            return Err(super::scope_hash_error(
                &path,
                "final change inventory must not be link-like",
            ));
        }
        if metadata.is_dir() {
            continue;
        }
        if !metadata.is_file() {
            return Err(super::scope_hash_error(
                &path,
                "final change inventory entry must be regular",
            ));
        }
        let relative = path.strip_prefix(project).map_err(|_| {
            super::scope_hash_error(
                &path,
                "final change inventory entry is outside the selected project",
            )
        })?;
        let relative = relative.to_str().ok_or_else(|| {
            super::scope_hash_error(&path, "final change inventory path must be valid UTF-8")
        })?;
        let bytes = read(&path, &metadata, &mut budget)?;
        let change = serde_json::json!({
            "path": relative,
            "operation": "create",
            "pre_digest": serde_json::Value::Null,
            "post_digest": super::digest(&bytes),
        });
        let encoded = serde_json::to_vec(&change).map_err(|error| {
            crate::KvistError::AgentEvidenceFailed {
                reason: format!("cannot encode final change inventory evidence: {error}"),
            }
        })?;
        budget.charge(&path, encoded.len())?;
        changes.push(change);
    }
    Ok(changes)
}

/// A slow evidence collection must not hide a sequence stop.
pub(super) fn collect<T>(
    sequence: &AuthoringSequence,
    collection: impl FnOnce() -> Result<T>,
) -> Result<T> {
    sequence.check()?;
    let collected = collection()?;
    sequence.check()?;
    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentProfile, Role};
    use sav::CancellationToken;

    fn exercise(expire: bool) {
        let fixture = tempfile::tempdir_in(".").unwrap();
        std::fs::create_dir(fixture.path().join("src")).unwrap();
        std::fs::create_dir(fixture.path().join("tests")).unwrap();
        std::fs::write(fixture.path().join("src/source.rs"), "completed source").unwrap();
        std::fs::write(fixture.path().join("tests/test.rs"), "completed test").unwrap();
        let cancellation = CancellationToken::new();
        let mut profile = AgentProfile::default_for_role(Role::Developer);
        profile.timeout_seconds = if expire { 1 } else { 30 };
        let sequence =
            AuthoringSequence::with_cancellation(&profile, cancellation.clone()).unwrap();
        let mut collected = false;
        let result = collect(&sequence, || {
            let digests = super::super::agent_outcome::source_test_digests(fixture.path())?;
            std::fs::write(
                fixture.path().join("completed-effects.log"),
                "retained completed effects",
            )
            .unwrap();
            collected = true;
            if expire {
                std::thread::sleep(std::time::Duration::from_secs(1));
            } else {
                cancellation.cancel();
            }
            Ok(digests)
        });
        assert!(collected, "the initial resource check must pass");
        let error = result.expect_err("late collection interruption must fence success");
        assert_eq!(
            crate::agent::error_stop(&error),
            if expire {
                sav::runtime::StopReason::Limit(sav::runtime::BudgetResource::WallTime)
            } else {
                sav::runtime::StopReason::Cancelled
            }
        );
        assert!(
            std::fs::read_to_string(fixture.path().join("completed-effects.log"))
                .unwrap()
                .contains("retained completed effects")
        );
        assert_eq!(
            sequence
                .diagnostics(sav::runtime::StopReason::Failed)
                .model_charge,
            0
        );
    }

    #[test]
    fn phase4_final_collection_late_cancellation_fences_success() {
        exercise(false);
    }

    #[test]
    fn phase4_final_collection_late_expiry_fences_success() {
        exercise(true);
    }

    fn inventory_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let project = tempfile::tempdir_in(".").unwrap();
        let component = project.path().join("component");
        fs::create_dir_all(component.join("src")).unwrap();
        fs::create_dir_all(component.join("tests")).unwrap();
        fs::write(component.join("src/source.rs"), "retained source").unwrap();
        (project, component)
    }

    fn inventory_sequence() -> AuthoringSequence {
        AuthoringSequence::with_cancellation(
            &AgentProfile::default_for_role(Role::Developer),
            CancellationToken::new(),
        )
        .unwrap()
    }

    #[test]
    fn phase4_final_inventory_failed_open_is_not_silent_success() {
        let (project, component) = inventory_fixture();
        fs::write(component.join("tests/a.rs"), "observed first test").unwrap();
        fs::write(component.join("tests/b.rs"), "observed later test").unwrap();
        let sequence = inventory_sequence();
        let reads = std::cell::Cell::new(0);
        let result = collect(&sequence, || {
            super::super::agent_outcome::source_test_digests(&component)?;
            fs::write(component.join("completed-effects.log"), "retained effect").unwrap();
            changes_with_read(project.path(), &component, |path, metadata, budget| {
                reads.set(reads.get() + 1);
                if path.file_name().is_some_and(|name| name == "b.rs") {
                    fs::remove_file(path).unwrap();
                }
                super::super::read_bounded_scope_file(path, metadata, budget)
            })
        });
        let error = result.expect_err("a failed observed-file open must fence success");
        assert!(matches!(
            error,
            crate::KvistError::Io { source, .. }
                if source.kind() == std::io::ErrorKind::NotFound
        ));
        assert!(component.join("completed-effects.log").is_file());
        assert_eq!(
            reads.get(),
            2,
            "a completed prefix must not become partial success"
        );
        assert_eq!(
            sequence
                .diagnostics(sav::runtime::StopReason::Failed)
                .model_charge,
            0
        );
    }

    #[test]
    fn phase4_final_inventory_oversized_file_is_not_silent_success() {
        let (project, component) = inventory_fixture();
        let path = component.join("tests/test.rs");
        fs::write(&path, "initially bounded test").unwrap();
        let sequence = inventory_sequence();
        let result = collect(&sequence, || {
            super::super::agent_outcome::source_test_digests(&component)?;
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(super::super::MAX_SCOPE_FILE_BYTES + 1)
                .unwrap();
            changes(project.path(), &component)
        });
        let error = result.expect_err("a late oversized file must fence success");
        assert!(error.to_string().contains("per-file byte limit"), "{error}");
        assert!(path.is_file());
        assert_eq!(
            sequence
                .diagnostics(sav::runtime::StopReason::Failed)
                .model_charge,
            0
        );
    }

    #[test]
    fn phase4_final_inventory_aggregate_bound_is_not_silent_success() {
        let (project, component) = inventory_fixture();
        let sequence = inventory_sequence();
        let result = collect(&sequence, || {
            super::super::agent_outcome::source_test_digests(&component)?;
            for index in 0..9 {
                fs::File::create(component.join(format!("tests/{index}.rs")))
                    .unwrap()
                    .set_len(super::super::MAX_SCOPE_FILE_BYTES)
                    .unwrap();
            }
            changes(project.path(), &component)
        });
        let error = result.expect_err("late aggregate inventory growth must fence success");
        assert!(
            error.to_string().contains("aggregate material limit"),
            "{error}"
        );
    }

    #[test]
    fn phase4_final_inventory_invalid_directory_is_not_silent_success() {
        let (project, component) = inventory_fixture();
        fs::remove_dir(component.join("tests")).unwrap();
        fs::write(component.join("tests"), "not a directory").unwrap();
        let error = changes(project.path(), &component)
            .expect_err("a selected inventory that cannot be enumerated is not empty");
        assert!(error.to_string().contains("directory"), "{error}");
    }

    #[test]
    fn phase4_final_inventory_is_shallow_sorted_and_content_hashed() {
        let (project, component) = inventory_fixture();
        fs::write(component.join("tests/b.rs"), "second").unwrap();
        fs::write(component.join("tests/a.rs"), "first").unwrap();
        fs::create_dir(component.join("tests/nested")).unwrap();
        fs::write(
            component.join("tests/nested/excluded.rs"),
            "not inventoried",
        )
        .unwrap();
        let observed = changes(project.path(), &component).unwrap();
        assert_eq!(
            observed,
            vec![
                serde_json::json!({"path":"component/tests/a.rs", "operation":"create",
                "pre_digest":null,"post_digest":super::super::digest(b"first")}),
                serde_json::json!({"path":"component/tests/b.rs", "operation":"create",
                "pre_digest":null,"post_digest":super::super::digest(b"second")}),
            ]
        );
    }

    #[test]
    fn phase4_final_inventory_absent_directory_remains_empty() {
        let (project, component) = inventory_fixture();
        fs::remove_dir(component.join("tests")).unwrap();
        assert!(changes(project.path(), &component).unwrap().is_empty());
    }

    #[test]
    fn phase4_scope_per_file_boundary_accepts_exact_limit() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let path = fixture.path().join("selected.rs");
        fs::write(&path, "bounded input").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(super::super::MAX_SCOPE_FILE_BYTES)
            .unwrap();
        let mut budget = super::super::ScopeHashBudget::default();
        let bytes = super::super::read_bounded_scope_file(
            &path,
            &fs::metadata(&path).unwrap(),
            &mut budget,
        )
        .unwrap();
        assert_eq!(bytes.len() as u64, super::super::MAX_SCOPE_FILE_BYTES);
        assert_eq!(&bytes[..13], b"bounded input");
        assert_eq!(budget.material_bytes, super::super::MAX_SCOPE_FILE_BYTES);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(super::super::MAX_SCOPE_FILE_BYTES + 1)
            .unwrap();
        let error = super::super::read_bounded_scope_file(
            &path,
            &fs::metadata(&path).unwrap(),
            &mut budget,
        )
        .unwrap_err();
        assert!(error.to_string().contains("per-file byte limit"), "{error}");
        assert_eq!(budget.material_bytes, super::super::MAX_SCOPE_FILE_BYTES);
        fixture
            .close()
            .expect("clean the exact owned regular-file fixture");
        assert!(!path.exists());
    }

    #[test]
    fn phase4_scope_material_boundary_accepts_exact_limit() {
        let mut budget = super::super::ScopeHashBudget::default();
        let path = Path::new("private-material-counter");
        budget
            .charge(path, super::super::MAX_SCOPE_TOTAL_BYTES as usize)
            .unwrap();
        assert_eq!(budget.material_bytes, super::super::MAX_SCOPE_TOTAL_BYTES);
        let error = budget.charge(path, 1).unwrap_err();
        assert!(
            error.to_string().contains("aggregate material limit"),
            "{error}"
        );
    }

    #[test]
    fn phase4_scope_entry_boundary_accepts_exact_limit() {
        let mut budget = super::super::ScopeHashBudget::default();
        let path = Path::new("private-entry-counter");
        for _ in 0..super::super::MAX_SCOPE_ENTRIES {
            budget.charge_entry(path, 1).unwrap();
        }
        assert_eq!(budget.entries, super::super::MAX_SCOPE_ENTRIES);
        let error = budget.charge_entry(path, 1).unwrap_err();
        assert!(
            error.to_string().contains("aggregate entry limit"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn phase4_scope_fifo_substitution_is_rejected_without_writer() {
        use std::io::{Read, Write};
        use std::os::unix::{
            fs::OpenOptionsExt,
            net::{UnixListener, UnixStream},
        };
        use std::process::{Child, Command, Stdio};
        use std::time::{Duration, Instant};

        const ROOT: &str = "KVIST_TEST_SCOPE_FIFO_HELPER_ROOT";
        const TEST: &str = "task_commands::completion::tests::phase4_scope_fifo_substitution_is_rejected_without_writer";

        struct Reaper(Child);
        impl Drop for Reaper {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                }
                let _ = self.0.wait();
            }
        }

        if let Some(root) = std::env::var_os(ROOT) {
            let root = std::path::PathBuf::from(root);
            let path = root.join("selected.rs");
            let metadata = fs::metadata(&path).unwrap();
            let mut channel = UnixStream::connect(root.join("reader.sock")).unwrap();
            channel
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            channel.write_all(b"R").unwrap();
            let mut start = [0];
            channel.read_exact(&mut start).unwrap();
            assert_eq!(start, *b"G");
            channel.write_all(b"E").unwrap();
            let error = super::super::read_bounded_scope_file(
                &path,
                &metadata,
                &mut super::super::ScopeHashBudget::default(),
            )
            .expect_err("a substituted FIFO descriptor must be rejected");
            assert!(matches!(
                error, crate::KvistError::TaskQueueUnavailable { reason, .. }
                    if reason.contains("changed while it was opened")
            ));
            channel.write_all(b"D").unwrap();
            return;
        }

        let fixture = tempfile::tempdir_in(".").unwrap();
        let path = fixture.path().join("selected.rs");
        fs::write(&path, "original regular file").unwrap();
        let listener = UnixListener::bind(fixture.path().join("reader.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut helper = Reaper(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--test-threads=1"])
                .env(ROOT, fixture.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut channel = loop {
            match listener.accept() {
                Ok((channel, _)) => break channel,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        helper.0.try_wait().unwrap().is_none(),
                        "helper exited before metadata handshake"
                    );
                    assert!(
                        Instant::now() < deadline,
                        "helper metadata handshake expired"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("owned helper socket failed: {error}"),
            }
        };
        channel
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut marker = [0];
        channel.read_exact(&mut marker).unwrap();
        assert_eq!(marker, *b"R");
        fs::remove_file(&path).unwrap();
        nix::unistd::mkfifo(
            &path,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .unwrap();
        channel.write_all(b"G").unwrap();
        channel.read_exact(&mut marker).unwrap();
        assert_eq!(marker, *b"E");
        channel
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let timely = match channel.read_exact(&mut marker) {
            Ok(()) => true,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                false
            }
            Err(error) => panic!("owned FIFO helper failed before descriptor rejection: {error}"),
        };
        let _rescue_writer = if !timely {
            let writer = fs::OpenOptions::new()
                .write(true)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
                .open(&path)
                .expect("unblock the owned old-behavior FIFO reader");
            channel
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            channel.read_exact(&mut marker).unwrap();
            Some(writer)
        } else {
            None
        };
        assert_eq!(marker, *b"D");
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = helper.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "owned helper did not exit after descriptor rejection"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(status.success(), "owned helper failed: {status}");
        let root = fixture.path().to_path_buf();
        drop(_rescue_writer);
        drop(channel);
        drop(listener);
        drop(helper);
        fixture
            .close()
            .expect("clean the exact owned FIFO/socket fixture");
        assert!(!root.exists());
        eprintln!(
            "owned FIFO helper reaped; rescue_writer={}; status={status}; exact fixture {} absent",
            !timely,
            root.display()
        );
        assert!(
            timely,
            "bounded scope open waited for a FIFO writer before descriptor validation"
        );
    }
}
