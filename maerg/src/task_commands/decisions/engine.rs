//! Deterministic, content-bound local decision transactions.

use super::*;

mod capture;
mod documents;
pub(super) mod shortcuts;
mod storage;
mod tasks;
mod transaction;

pub(in crate::task_commands) mod review_storage {
    pub(in crate::task_commands) use super::storage::{directory, read_text, secret, write_new};
}

use storage::{
    absolute, authenticated, bounded, decode, encode, equal, existing_secret, exists, fail, now,
    read_bytes, read_text, remove, replace_controlled, safe_path, secret, write_new,
};

pub(super) fn context_for(project_dir: &Path, component: &Path) -> Result<TaskContext> {
    capture::context_for(project_dir, component)
}

pub(super) fn capture(project_dir: &Path, component: &Path, task: Option<&str>) -> Result<Capture> {
    capture::capture(project_dir, component, task)
}

pub(super) fn capture_queue(project_dir: &Path, component: &Path) -> Result<Capture> {
    capture::capture_queue(project_dir, component)
}

pub(super) use tasks::queue_changes;
pub(super) use transaction::prepare_changes;

pub(super) fn prepare(
    project_dir: &Path,
    input: Capture,
    draft: Draft,
    selection: Selection,
    review: IntentReview,
) -> Result<Plan> {
    transaction::prepare(project_dir, input, draft, selection, review)
}

pub(super) fn load_plan(project_dir: &Path, component: &Path, id: &str) -> Result<Plan> {
    transaction::load_plan(project_dir, component, id)
}

pub(super) fn apply(
    project_dir: &Path,
    component: &Path,
    id: &str,
    approval: &str,
) -> Result<Plan> {
    transaction::apply(project_dir, component, id, approval, None)
}

pub(super) fn ensure_no_pending(component_dir: &Path) -> Result<()> {
    let path = component_dir.join(DIRECTORY).join("pending.json");
    safe_path(&path, true)?;
    if path
        .try_exists()
        .map_err(|error| fail(&path, error.to_string()))?
    {
        return Err(fail(
            &path,
            "pending plan transaction fences ordinary writers; repeat the identical approved apply",
        ));
    }
    Ok(())
}

pub(super) fn recognizes_retired_journal(component: &Path, journal: &Path) -> Result<bool> {
    transaction::recognizes_retired_journal(component, journal)
}

#[cfg(test)]
mod tests;
