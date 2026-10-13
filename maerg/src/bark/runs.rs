//! Execution-run discovery for the workspace shell.
//!
//! Each agent execution leaves a run record (`<task>_<timestamp>.json`) and a
//! trajectory journal (`<task>_<timestamp>.trajectory.jsonl`) under the
//! component's `.kvist/runs` directory. This module discovers runs across the
//! project root and every discovered component, pairs each record with its
//! trajectory, and treats file names and record contents as untrusted bounded
//! input. It is shared by the streaming result stage (feedback correlation)
//! and the `last` builtin (run history).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::project_state;

/// One discovered execution run.
#[derive(Debug, Clone, Default)]
pub struct RecentRun {
    /// Relative component path of the run; `None` for the project root.
    pub component: Option<String>,
    /// Task ID parsed from the run file names.
    pub task_id: Option<String>,
    /// Timestamp string parsed from the run file names.
    pub timestamp: Option<String>,
    /// Whether the run succeeded, when the run record says so.
    pub success: Option<bool>,
    pub tokens_input: Option<u64>,
    pub tokens_output: Option<u64>,
    pub record_path: Option<PathBuf>,
    pub trajectory_path: Option<PathBuf>,
}

/// What a result stage should correlate with, when anything.
pub enum FeedbackTarget<'a> {
    /// No feedback section (e.g. a free prompt run records no trajectory).
    None,
    /// Correlate with runs of one task ID (exact or item prefix match).
    Task(&'a str),
    /// Correlate with the most recent run of any task.
    Latest,
}

/// Every runs directory to scan: the project root plus every discovered
/// component, labelled with the component's relative path (`None` for the
/// project root). The same physical directory is scanned at most once, so a
/// root component that shares the project-root `.kvist` is not double-counted.
fn runs_dirs(project_root: &Path) -> Vec<(Option<String>, PathBuf)> {
    let mut dirs: Vec<(Option<String>, PathBuf)> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

    let add = |dirs: &mut Vec<(Option<String>, PathBuf)>,
               seen: &mut BTreeSet<PathBuf>,
               component: Option<String>,
               path: PathBuf| {
        if !path.is_dir() {
            return;
        }
        // Canonicalize so that spelling differences (e.g. `a/./b`) collapse.
        let key = fs::canonicalize(&path).unwrap_or(path.clone());
        if seen.insert(key) {
            dirs.push((component, path));
        }
    };

    add(
        &mut dirs,
        &mut seen,
        None,
        project_root.join(".kvist").join("runs"),
    );
    if let Ok(inspection) = project_state::inspect(project_root)
        && let Some(component_root) = &inspection.component_root
    {
        for component in &inspection.components {
            let component_dir = project_root.join(component_root).join(&component.path);
            let runs = component_dir.join(".kvist").join("runs");
            let label = if component.path.as_os_str().is_empty() {
                Some(".".to_owned())
            } else {
                Some(component.path.to_string_lossy().into_owned())
            };
            add(&mut dirs, &mut seen, label, runs);
        }
    }

    dirs.sort_by(|a, b| a.1.cmp(&b.1));
    dirs
}

/// Scans the project root and every discovered component for execution runs,
/// pairing each run record with its trajectory.
///
/// Returns at most `limit` runs, newest first. Only files whose names parse
/// as `<task_id>_<timestamp>` are treated as runs; the timestamp never
/// contains an underscore, so the final underscore separates the two parts.
pub fn scan_recent_runs(project_root: &Path, limit: usize) -> Vec<RecentRun> {
    #[derive(Default)]
    struct Acc {
        task_id: Option<String>,
        timestamp: Option<String>,
        success: Option<bool>,
        tokens_input: Option<u64>,
        tokens_output: Option<u64>,
        record_path: Option<PathBuf>,
        trajectory_path: Option<PathBuf>,
        newest: Option<SystemTime>,
    }

    let mut by_base: BTreeMap<(Option<String>, String), Acc> = BTreeMap::new();

    for (component, runs_dir) in runs_dirs(project_root) {
        let Ok(entries) = fs::read_dir(&runs_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let (base, kind) = if let Some(base) = name.strip_suffix(".trajectory.jsonl") {
                (base.to_owned(), "trajectory")
            } else if let Some(base) = name.strip_suffix(".json") {
                (base.to_owned(), "record")
            } else {
                continue;
            };
            let Some((task_id, timestamp)) = base
                .rsplit_once('_')
                .map(|(task, ts)| (task.to_owned(), ts.to_owned()))
            else {
                // Not a run file (no `<task_id>_<timestamp>` shape); ignore.
                continue;
            };
            if task_id.is_empty() || timestamp.is_empty() {
                continue;
            }

            let modified = path.metadata().ok().and_then(|m| m.modified().ok());
            let slot = by_base.entry((component.clone(), base)).or_default();
            slot.task_id = Some(task_id.to_owned());
            slot.timestamp = Some(timestamp.to_owned());
            if let Some(newest) = modified
                && slot.newest.as_ref().is_none_or(|best| newest > *best)
            {
                slot.newest = Some(newest);
            }
            if kind == "trajectory" {
                slot.trajectory_path = Some(path);
            } else {
                // Read before moving the path into the slot.
                let contents = fs::read_to_string(&path).ok();
                slot.record_path = Some(path);
                if let Some(contents) = contents
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(&contents)
                {
                    slot.success = v
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .map(|s| s == "success" || s == "completed");
                    slot.tokens_input = v.get("tokens_input").and_then(serde_json::Value::as_u64);
                    slot.tokens_output = v.get("tokens_output").and_then(serde_json::Value::as_u64);
                }
            }
        }
    }

    let mut runs: Vec<RecentRun> = by_base
        .into_iter()
        .filter_map(|((component, _base), acc)| {
            if acc.trajectory_path.is_none() && acc.record_path.is_none() {
                return None;
            }
            Some(RecentRun {
                component,
                task_id: acc.task_id,
                timestamp: acc.timestamp,
                success: acc.success,
                tokens_input: acc.tokens_input,
                tokens_output: acc.tokens_output,
                record_path: acc.record_path,
                trajectory_path: acc.trajectory_path,
            })
        })
        .collect();
    // Order newest first. The run's own timestamp (parsed from the file name,
    // a fixed-width RFC 3339 string) is the deterministic recency key; file
    // mtime only breaks exact ties (e.g. two components' runs in the same
    // second), since coarse-granularity filesystems can stamp several writes
    // with identical mtimes.
    runs.sort_by(|a, b| {
        let mtime = |run: &RecentRun| -> Option<SystemTime> {
            run.trajectory_path
                .iter()
                .chain(run.record_path.iter())
                .filter_map(|path| path.metadata().ok())
                .filter_map(|meta| meta.modified().ok())
                .max()
        };
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| mtime(b).cmp(&mtime(a)))
    });
    runs.truncate(limit);
    runs
}

/// Selects the run to correlate with, by the requested target, from a
/// newest-first run list.
pub fn select_recent_run<'a>(
    runs: &'a [RecentRun],
    target: &FeedbackTarget<'_>,
) -> Option<&'a RecentRun> {
    match target {
        FeedbackTarget::None => None,
        FeedbackTarget::Task(task_id) => runs.iter().find(|run| {
            run.task_id
                .as_deref()
                .is_some_and(|id| id == *task_id || id.starts_with(&format!("{task_id}-")))
        }),
        FeedbackTarget::Latest => runs.first(),
    }
}

/// Test helper: writes a one-turn trajectory journal with known metrics.
#[cfg(test)]
pub fn write_trajectory(runs_dir: &Path, task_id: &str, ts: &str) -> PathBuf {
    let traj_path = runs_dir.join(format!("{task_id}_{ts}.trajectory.jsonl"));
    let journal = format!(
        r#"{{"event":"session_start","session_id":"s1","task_id":"{task_id}","timestamp":1700000000}}
{{"event":"turn_start","turn":1,"timestamp":1700000001}}
{{"event":"prompt_eval","turn":1,"new_tokens":50,"cached_tokens":100}}
{{"event":"model_reasoning","turn":1,"reasoning":"Inspecting test suite."}}
{{"event":"tool_dispatch","turn":1,"call_id":"c1","tool":"read_file","args":{{}},"action_hash":"h1"}}
{{"event":"tool_result","turn":1,"call_id":"c1","tool":"read_file","stdout":"","stderr":"","exit_code":0,"bytes":0,"state_mutated":false}}
{{"event":"turn_finish","turn":1,"output_tokens":25,"finish_reason":"stop"}}
{{"event":"session_finish","session_id":"s1","task_id":"{task_id}","total_turns":1,"total_tokens":75,"success":true}}
"#
    );
    fs::write(&traj_path, journal).unwrap();
    traj_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_returns_newest_first_and_pairs_records() {
        let dir = tempfile::tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        write_trajectory(&runs_dir, "task-a", "2026-09-11T20-00-00Z");
        let newer = write_trajectory(&runs_dir, "task-b", "2026-09-11T22-00-00Z");
        fs::write(
            runs_dir.join("task-b_2026-09-11T22-00-00Z.json"),
            r#"{"status":"success","tokens_input":10,"tokens_output":5}"#,
        )
        .unwrap();
        // A stray file without the `<task_id>_<timestamp>` shape is ignored.
        fs::write(runs_dir.join("other.json"), "{}").unwrap();

        let runs = scan_recent_runs(dir.path(), 10);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].task_id.as_deref(), Some("task-b"));
        assert_eq!(runs[1].task_id.as_deref(), Some("task-a"));
        let main = &runs[0];
        assert_eq!(main.success, Some(true));
        assert_eq!(main.tokens_input, Some(10));
        assert_eq!(main.tokens_output, Some(5));
        assert_eq!(main.trajectory_path, Some(newer));
        assert!(main.record_path.is_some());
        assert_eq!(main.timestamp.as_deref(), Some("2026-09-11T22-00-00Z"));
    }

    #[test]
    fn scan_is_empty_for_a_non_project() {
        // project_state::inspect only recognizes components in a Current
        // project, so a bare directory with no runs dir yields nothing.
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_recent_runs(dir.path(), 10).is_empty());
    }

    #[test]
    fn select_prefers_the_requested_task_over_newer_other_runs() {
        let dir = tempfile::tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        write_trajectory(&runs_dir, "task-a", "2026-09-11T20-00-00Z");
        write_trajectory(&runs_dir, "task-b", "2026-09-11T22-00-00Z");

        let runs = scan_recent_runs(dir.path(), 10);
        let picked = select_recent_run(&runs, &FeedbackTarget::Task("task-a"))
            .expect("the requested task run");
        assert_eq!(picked.task_id.as_deref(), Some("task-a"));
        let latest = select_recent_run(&runs, &FeedbackTarget::Latest).expect("a run");
        assert_eq!(latest.task_id.as_deref(), Some("task-b"));
        assert!(select_recent_run(&runs, &FeedbackTarget::None).is_none());
    }

    #[test]
    fn scan_limit_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        for i in 0..5 {
            write_trajectory(
                &runs_dir,
                &format!("task-{i}"),
                &format!("2026-09-11T2{i}-00-00Z"),
            );
        }
        assert_eq!(scan_recent_runs(dir.path(), 2).len(), 2);
    }
}
