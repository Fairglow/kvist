use std::collections::VecDeque;
#[cfg(test)]
use std::path::Path;
use std::sync::Mutex;

use sav::{CancellationToken, ToolIntent, loop_detection::ProgressEvidence};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::file_tools::FileRequest;
use crate::sandbox::ToolOutcome;

const MAX_PENDING: usize = 128;

struct Observation {
    identity: [u8; 32],
    evidence: Result<ProgressEvidence>,
}

/// Evidence is paired with the exact intent and outcome, consumed once and
/// bounded even when an embedding host executes without requesting progress.
#[derive(Default)]
pub(crate) struct NativeProgress(Mutex<VecDeque<Observation>>);

fn failed(phase: &str, error: Error) -> Error {
    Error::SandboxExec {
        tool: Some("file_tool".into()),
        reason: format!(
            "native mutation revision observation {phase} failed: {error}; \
             reconcile the target before further work; no automatic replay"
        ),
    }
}

fn ambiguous() -> Error {
    Error::SandboxExec {
        tool: Some("file_tool".into()),
        reason: "ambiguous matching native mutation observations; evidence cannot identify \
                 this execution; effects may exist, reconcile before further work; no automatic replay"
            .into(),
    }
}

fn identity(intent: &ToolIntent, outcome: &ToolOutcome) -> Result<[u8; 32]> {
    let intent = serde_json::to_vec(intent).map_err(|_| Error::SandboxExec {
        tool: Some("file_tool".into()),
        reason: "cannot identify native mutation observation".into(),
    })?;
    let mut digest = Sha256::new();
    digest.update(intent);
    digest.update([
        u8::from(outcome.exited),
        u8::from(outcome.timed_out),
        u8::from(outcome.output_limit_exceeded),
        u8::from(outcome.cancelled),
        u8::from(outcome.status.is_some()),
    ]);
    digest.update(outcome.status.unwrap_or_default().to_le_bytes());
    digest.update((outcome.stdout.len() as u64).to_le_bytes());
    digest.update(&outcome.stdout);
    digest.update((outcome.stderr.len() as u64).to_le_bytes());
    digest.update(&outcome.stderr);
    Ok(digest.finalize().into())
}

impl NativeProgress {
    fn observations(&self) -> Result<std::sync::MutexGuard<'_, VecDeque<Observation>>> {
        self.0.lock().map_err(|_| Error::SandboxExec {
            tool: Some("file_tool".into()),
            reason: "native mutation observation state is poisoned; reconcile before retrying"
                .into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn execute(
        &self,
        intent: &ToolIntent,
        request: Option<&FileRequest>,
        workdir: &Path,
        cancellation: &CancellationToken,
        execute: impl FnOnce() -> Result<ToolOutcome>,
    ) -> Result<ToolOutcome> {
        super::check_cancelled(cancellation)
            .map_err(|error| failed("before execution (not executed)", error))?;
        let prepared = request
            .map(|request| request.host_mapped(workdir))
            .transpose()
            .map_err(|error| failed("before execution (not executed)", error))?;
        self.execute_prepared(intent, prepared.as_ref(), cancellation, execute)
    }

    /// Uses the exact host-mapped request staged for the helper, without
    /// resolving the workspace selection a second time.
    pub(crate) fn execute_prepared(
        &self,
        intent: &ToolIntent,
        request: Option<&FileRequest>,
        cancellation: &CancellationToken,
        execute: impl FnOnce() -> Result<ToolOutcome>,
    ) -> Result<ToolOutcome> {
        super::check_cancelled(cancellation)
            .map_err(|error| failed("before execution (not executed)", error))?;
        let observation = request
            .map(|request| request.observe_mutation(cancellation))
            .transpose()
            .map_err(|error| failed("before execution (not executed)", error))?
            .flatten();
        if let Some((observer, _)) = &observation {
            observer
                .check_binding(cancellation)
                .map_err(|error| failed("before execution (not executed)", error))?;
        }
        super::check_cancelled(cancellation)
            .map_err(|error| failed("before execution (not executed)", error))?;
        let outcome = execute()?;
        if let Some((observer, before)) = observation {
            let mut evidence = observer
                .revision(cancellation)
                .map_err(|error| failed("after execution (effects may be present)", error))
                .map(|after| {
                    if before != after {
                        ProgressEvidence::StateChanged
                    } else {
                        ProgressEvidence::None
                    }
                });
            let mut observations = self.observations()?;
            let identity = identity(intent, &outcome)?;
            for entry in observations
                .iter_mut()
                .filter(|entry| entry.identity == identity)
            {
                entry.evidence = Err(ambiguous());
                evidence = Err(ambiguous());
            }
            if observations.len() == MAX_PENDING {
                observations.pop_front();
            }
            observations.push_back(Observation { identity, evidence });
        }
        Ok(outcome)
    }

    pub(crate) fn progress(
        &self,
        intent: &ToolIntent,
        outcome: &ToolOutcome,
    ) -> Result<ProgressEvidence> {
        let identity = identity(intent, outcome)?;
        let mut observations = self.observations()?;
        if let Some(index) = observations
            .iter()
            .position(|entry| entry.identity == identity)
            && let Some(entry) = observations.remove(index)
        {
            return entry.evidence;
        }
        Ok(ProgressEvidence::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn intent() -> ToolIntent {
        ToolIntent {
            id: "mutation".into(),
            provider_id: None,
            name: "write_file".into(),
            arguments: json!({"path":"/workspace/text","content":"new"}),
        }
    }

    fn outcome() -> ToolOutcome {
        ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"claimed mutation success".to_vec(),
            stderr: Vec::new(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    fn request(intent: &ToolIntent) -> FileRequest {
        FileRequest::new("/workspace", &intent.name, intent.arguments.clone()).unwrap()
    }

    #[test]
    fn stdout_success_and_inode_only_changes_are_not_mutation_progress() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join("text");
        std::fs::write(&path, "same").unwrap();
        let progress = NativeProgress::default();
        let intent = intent();
        for replace_inode in [false, true] {
            let out = progress
                .execute(
                    &intent,
                    Some(&request(&intent)),
                    &root,
                    &CancellationToken::new(),
                    || {
                        if replace_inode {
                            std::fs::write(root.join("replacement"), "same").unwrap();
                            std::fs::rename(root.join("replacement"), &path).unwrap();
                        }
                        Ok(outcome())
                    },
                )
                .unwrap();
            assert_eq!(
                progress.progress(&intent, &out).unwrap(),
                ProgressEvidence::None
            );
        }
    }

    #[test]
    fn changed_bytes_and_empty_creation_are_observed_even_when_tool_fails() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join("text");
        let progress = NativeProgress::default();
        let intent = intent();
        for content in ["", "changed"] {
            let out = progress
                .execute(
                    &intent,
                    Some(&request(&intent)),
                    &root,
                    &CancellationToken::new(),
                    || {
                        std::fs::write(&path, content).unwrap();
                        let mut out = outcome();
                        out.status = Some(1);
                        Ok(out)
                    },
                )
                .unwrap();
            assert_eq!(
                progress.progress(&intent, &out).unwrap(),
                ProgressEvidence::StateChanged
            );
            assert_eq!(
                progress.progress(&intent, &out).unwrap(),
                ProgressEvidence::None
            );
        }
    }

    #[test]
    fn shell_changes_do_not_trigger_workspace_inspection() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let progress = NativeProgress::default();
        let mut intent = intent();
        intent.name = "shell".into();
        let out = progress
            .execute(&intent, None, &root, &CancellationToken::new(), || {
                std::fs::write(root.join("text"), "changed").unwrap();
                Ok(outcome())
            })
            .unwrap();
        assert_eq!(
            progress.progress(&intent, &out).unwrap(),
            ProgressEvidence::None
        );
        assert!(progress.observations().unwrap().is_empty());
    }

    #[test]
    fn pre_observation_errors_prevent_execution_and_post_errors_are_explicit() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join("text");
        let progress = NativeProgress::default();
        let intent = intent();
        let request = request(&intent);
        std::fs::write(&path, vec![b'x'; crate::file_tools::MAX_FILE_BYTES + 1]).unwrap();
        let error = progress
            .execute(
                &intent,
                Some(&request),
                &root,
                &CancellationToken::new(),
                || {
                    panic!("must not execute after a failed pre-observation");
                },
            )
            .unwrap_err();
        assert!(error.describe().contains("before execution (not executed)"));
        std::fs::write(&path, "bounded").unwrap();
        let out = progress
            .execute(
                &intent,
                Some(&request),
                &root,
                &CancellationToken::new(),
                || {
                    std::fs::remove_file(&path).unwrap();
                    std::fs::create_dir(&path).unwrap();
                    Ok(outcome())
                },
            )
            .unwrap();
        let error = progress.progress(&intent, &out).unwrap_err();
        assert!(
            error
                .describe()
                .contains("after execution (effects may be present)")
        );
        assert!(error.describe().contains("reconcile"));
    }

    #[test]
    fn mutation_observation_rejects_links_without_following_the_target() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("real")).unwrap();
        std::fs::write(root.join("real/text"), "safe").unwrap();
        std::os::unix::fs::symlink("real", root.join("link")).unwrap();
        std::os::unix::fs::symlink("real/text", root.join("text")).unwrap();
        let progress = NativeProgress::default();
        for path in ["/workspace/text", "/workspace/link/text"] {
            let mut intent = intent();
            intent.arguments["path"] = json!(path);
            assert!(
                progress
                    .execute(
                        &intent,
                        Some(&request(&intent)),
                        &root,
                        &CancellationToken::new(),
                        || {
                            panic!("must not execute through links");
                        }
                    )
                    .is_err()
            );
        }
        assert_eq!(std::fs::read(root.join("real/text")).unwrap(), b"safe");
    }

    #[test]
    fn scope_rebinding_cannot_redirect_post_execution_observation() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let workdir = parent.join("work");
        let outside = parent.join("outside");
        std::fs::create_dir(&workdir).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(workdir.join("text"), "same").unwrap();
        std::fs::write(outside.join("text"), "unrelated revision").unwrap();
        let progress = NativeProgress::default();
        let intent = intent();
        let out = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &workdir,
                &CancellationToken::new(),
                || {
                    std::fs::rename(&workdir, parent.join("moved")).unwrap();
                    std::os::unix::fs::symlink(&outside, &workdir).unwrap();
                    Ok(outcome())
                },
            )
            .unwrap();
        let error = progress.progress(&intent, &out).unwrap_err();
        assert!(error.describe().contains("effects may"));
        assert!(error.describe().contains("reconcile"));
        assert!(error.describe().contains("replay"));
        assert_eq!(std::fs::read(parent.join("moved/text")).unwrap(), b"same");
    }

    #[test]
    fn followup_ambiguous_matching_executions_never_receive_each_others_evidence() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let progress = NativeProgress::default();
        let intent = intent();
        let first = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &root,
                &CancellationToken::new(),
                || {
                    std::fs::write(root.join("text"), "new").unwrap();
                    Ok(outcome())
                },
            )
            .unwrap();
        let second = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &root,
                &CancellationToken::new(),
                || {
                    std::fs::write(root.join("text"), "new").unwrap();
                    Ok(outcome())
                },
            )
            .unwrap();
        for out in [&second, &first] {
            let error = progress.progress(&intent, out).unwrap_err();
            assert!(error.describe().contains("ambiguous"), "{error}");
            assert!(error.describe().contains("reconcile"), "{error}");
        }
        let fresh = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &root,
                &CancellationToken::new(),
                || {
                    std::fs::write(root.join("text"), "changed again").unwrap();
                    Ok(outcome())
                },
            )
            .unwrap();
        assert_eq!(
            progress.progress(&intent, &fresh).unwrap(),
            ProgressEvidence::StateChanged
        );
    }

    #[test]
    fn followup_ordinary_directory_rebind_rejects_unbound_effect_observation() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let workdir = parent.join("work");
        std::fs::create_dir(&workdir).unwrap();
        std::fs::write(workdir.join("text"), "before").unwrap();
        let progress = NativeProgress::default();
        let intent = intent();
        let out = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &workdir,
                &CancellationToken::new(),
                || {
                    std::fs::rename(&workdir, parent.join("original")).unwrap();
                    std::fs::create_dir(&workdir).unwrap();
                    std::fs::write(workdir.join("text"), "effect in replacement scope").unwrap();
                    Ok(outcome())
                },
            )
            .unwrap();
        let error = progress.progress(&intent, &out).unwrap_err();
        assert!(error.describe().contains("effects may"), "{error}");
        assert!(error.describe().contains("reconcile"), "{error}");
        assert!(error.describe().contains("replay"), "{error}");
        assert_eq!(
            std::fs::read(parent.join("original/text")).unwrap(),
            b"before"
        );
        assert_eq!(
            std::fs::read(workdir.join("text")).unwrap(),
            b"effect in replacement scope"
        );
    }

    #[test]
    fn followup_observer_uses_the_exact_prepared_helper_scope_not_a_rebound_selection() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let original = parent.join("original");
        let other = parent.join("other");
        let selected = parent.join("selected");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::write(original.join("text"), "before").unwrap();
        std::fs::write(other.join("text"), "unrelated").unwrap();
        std::os::unix::fs::symlink(&original, &selected).unwrap();
        let intent = intent();
        let prepared = request(&intent).host_mapped(&selected).unwrap();
        std::fs::remove_file(&selected).unwrap();
        std::os::unix::fs::symlink(&other, &selected).unwrap();
        let progress = NativeProgress::default();
        let out = progress
            .execute_prepared(&intent, Some(&prepared), &CancellationToken::new(), || {
                crate::file_tools::execute_file_request(&prepared).unwrap();
                Ok(outcome())
            })
            .unwrap();
        assert_eq!(
            progress.progress(&intent, &out).unwrap(),
            ProgressEvidence::StateChanged
        );
        assert_eq!(std::fs::read(original.join("text")).unwrap(), b"new");
        assert_eq!(std::fs::read(other.join("text")).unwrap(), b"unrelated");
    }

    #[test]
    fn evidence_is_exactly_paired_and_unconsumed_observations_are_bounded() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let progress = NativeProgress::default();
        let mut first = None;
        let mut last = None;
        for index in 0..=MAX_PENDING {
            let mut intent = intent();
            intent.id = format!("mutation-{index}");
            let out = progress
                .execute(
                    &intent,
                    Some(&request(&intent)),
                    &root,
                    &CancellationToken::new(),
                    || {
                        std::fs::write(root.join("text"), index.to_string()).unwrap();
                        Ok(outcome())
                    },
                )
                .unwrap();
            if index == 0 {
                first = Some((intent.clone(), out.clone()));
            }
            last = Some((intent, out));
        }
        assert_eq!(progress.observations().unwrap().len(), MAX_PENDING);
        let (first, out) = first.unwrap();
        assert_eq!(
            progress.progress(&first, &out).unwrap(),
            ProgressEvidence::None
        );
        let (last, out) = last.unwrap();
        let mut different = out.clone();
        different.stderr = b"different outcome".to_vec();
        assert_eq!(
            progress.progress(&last, &different).unwrap(),
            ProgressEvidence::None
        );
        assert_eq!(
            progress.progress(&last, &out).unwrap(),
            ProgressEvidence::StateChanged
        );
    }

    #[test]
    fn followup_cancelled_pre_observation_never_executes() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let progress = NativeProgress::default();
        let intent = intent();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let executed = std::cell::Cell::new(false);
        let error = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &root,
                &cancellation,
                || {
                    executed.set(true);
                    std::fs::write(root.join("text"), "must not execute").unwrap();
                    Ok(outcome())
                },
            )
            .unwrap_err();
        assert!(error.describe().contains("cancel"), "{error}");
        assert!(!executed.get());
        assert!(!root.join("text").exists());
    }

    #[test]
    fn followup_cancelled_post_observation_preserves_outcome_but_refuses_evidence() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let root = dir.path().canonicalize().unwrap();
        let progress = NativeProgress::default();
        let intent = intent();
        let cancellation = CancellationToken::new();
        let out = progress
            .execute(
                &intent,
                Some(&request(&intent)),
                &root,
                &cancellation,
                || {
                    std::fs::write(root.join("text"), "completed effect").unwrap();
                    cancellation.cancel();
                    Ok(outcome())
                },
            )
            .unwrap();
        assert!(!out.failed());
        let error = progress.progress(&intent, &out).unwrap_err();
        assert!(error.describe().contains("cancel"), "{error}");
        assert!(error.describe().contains("effects may"), "{error}");
        assert!(error.describe().contains("reconcile"), "{error}");
        assert_eq!(
            std::fs::read(root.join("text")).unwrap(),
            b"completed effect"
        );
    }
}
