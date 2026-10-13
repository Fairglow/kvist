//! Explicit, bounded project repair.
//!
//! `kvist repair` applies the only content rewrite Kvist defines: sorting and
//! deduplicating TODO-queue dependency and requirement lists, then writing
//! the canonical serialization. It touches a queue only when the queue is
//! invalid because of that defect; parseable queues, fenced queues, and every
//! other artifact problem are reported, never guessed at.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{KvistError, Result, file_io, project_state, task_queue};

/// The result of one TODO queue's repair attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRepair {
    /// Queue path relative to the project root.
    pub path: String,
    /// `repaired`, `unchanged`, `would-repair`, `non-canonical`, `fenced`,
    /// or `unparseable`.
    pub outcome: &'static str,
    /// The defect that was fixed, or why the queue was left untouched.
    pub detail: Option<String>,
}

/// Complete bounded repair report for one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairReport {
    /// Inspected project root.
    pub project_dir: PathBuf,
    /// Project state before any write.
    pub state_before: project_state::ProjectState,
    /// Project state after the writes (equals `state_before` on `--dry-run`).
    pub state_after: project_state::ProjectState,
    /// Per-queue outcomes in stable artifact order.
    pub queues: Vec<QueueRepair>,
    /// Non-valid artifacts remaining after the repair, with reasons.
    pub remaining: Vec<(String, String)>,
    /// Whether the invocation was a preview without writes.
    pub dry_run: bool,
}

impl RepairReport {
    /// Whether the project is fully repaired: no non-valid artifact remains.
    pub fn complete(&self) -> bool {
        self.remaining.is_empty()
    }
}

impl std::fmt::Display for RepairReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "repair: {} (state: {} -> {}, dry-run: {})",
            self.project_dir.display(),
            self.state_before,
            self.state_after,
            self.dry_run
        )?;
        for queue in &self.queues {
            match &queue.detail {
                Some(detail) => writeln!(
                    formatter,
                    "  {}: {} — {}",
                    queue.path, queue.outcome, detail
                )?,
                None => writeln!(formatter, "  {}: {}", queue.path, queue.outcome)?,
            }
        }
        if self.remaining.is_empty() {
            writeln!(formatter, "result: every root artifact is valid")?;
        } else {
            writeln!(
                formatter,
                "result: {} artifact(s) still need manual repair:",
                self.remaining.len()
            )?;
            for (path, status) in &self.remaining {
                writeln!(formatter, "  {}: {}", path, status)?;
            }
            writeln!(
                formatter,
                "hint: run `kvist doctor` for the full inspection; the rewrite `kvist repair` defines is TODO queue set-list canonicalization only"
            )?;
        }
        Ok(())
    }
}

/// Repairs the project at `project_dir` by applying the only defined
/// rewrite: canonical TODO queue serialization of queues whose only defect is
/// unsorted or duplicated set-like lists. Fenced, unparseable, and
/// non-queue artifacts are reported and left untouched.
pub fn repair(project_dir: &Path, dry_run: bool) -> Result<RepairReport> {
    let before = project_state::inspect(project_dir)?;
    let mut queues: Vec<QueueRepair> = Vec::new();
    for relative in project_state::todo_queue_paths(project_dir) {
        let path = project_dir.join(&relative);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "read TODO queue",
                    path,
                    source,
                });
            }
        };
        match task_queue::parse(&contents) {
            Ok(queue) => {
                let canonical = task_queue::serialize(&queue).map_err(|error| {
                    KvistError::TaskQueueUnavailable {
                        path: path.clone(),
                        reason: format!("canonical queue serialization failed: {error}"),
                    }
                })?;
                if canonical == contents {
                    queues.push(QueueRepair {
                        path: relative,
                        outcome: "unchanged",
                        detail: None,
                    });
                } else {
                    queues.push(QueueRepair {
                        path: relative,
                        outcome: "non-canonical",
                        detail: Some(
                            "the queue parses and is valid; its formatting is left untouched"
                                .to_owned(),
                        ),
                    });
                }
            }
            Err(strict_error) => match task_queue::parse_repairable(&contents) {
                Ok(mut queue) => {
                    if queue.tasks.iter().any(|task| task.recovery_state.is_some()) {
                        queues.push(QueueRepair {
                            path: relative,
                            outcome: "fenced",
                            detail: Some(
                                "a fenced task attempt requires `kvist task recover` before the queue may change"
                                    .to_owned(),
                            ),
                        });
                        continue;
                    }
                    task_queue::normalize_set_lists(&mut queue);
                    let canonical = task_queue::serialize(&queue).map_err(|error| {
                        KvistError::TaskQueueUnavailable {
                            path: path.clone(),
                            reason: format!("canonical queue serialization failed: {error}"),
                        }
                    })?;
                    if dry_run {
                        queues.push(QueueRepair {
                            path: relative,
                            outcome: "would-repair",
                            detail: Some(strict_error.to_string()),
                        });
                    } else {
                        file_io::replace_file_atomically(&path, &canonical)?;
                        queues.push(QueueRepair {
                            path: relative,
                            outcome: "repaired",
                            detail: Some(strict_error.to_string()),
                        });
                    }
                }
                Err(error) => queues.push(QueueRepair {
                    path: relative,
                    outcome: "unparseable",
                    detail: Some(error.to_string()),
                }),
            },
        }
    }
    let after = if dry_run {
        before.clone()
    } else {
        project_state::inspect(project_dir)?
    };
    let remaining = after
        .artifacts
        .iter()
        .filter(|artifact| !artifact.is_valid())
        .map(|artifact| (artifact.path.clone(), artifact.status.clone()))
        .collect();
    Ok(RepairReport {
        project_dir: project_dir.to_path_buf(),
        state_before: before.state,
        state_after: after.state,
        queues,
        remaining,
        dry_run,
    })
}
