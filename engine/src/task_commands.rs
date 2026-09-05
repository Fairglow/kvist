//! Durable task selection and serialized state transitions.
#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::{
    KvistError, Result,
    component_documents::{self, DocumentKind},
    discovery::{self, ComponentArtifact},
    file_io::{replace_file_atomically, sync_directory, write_new_file_atomically},
    filesystem::is_link_like,
    project_state::{self, ComponentState, MAX_ROOT_TEXT_ARTIFACT_BYTES, ProjectState},
    task_queue::{
        RecoveryState, RecoveryStateKind, Task, TaskKind, TaskQueue, TaskStatus, Timestamp, parse,
        serialize,
    },
    vcs::VcsArtifactState,
};

/// Selects the first ready task in declared queue order.
pub fn next(component_path: &Path) -> Result<String> {
    let context = validate_context(component_path)?;
    let queue = read_queue(&context.component_dir)?;
    Ok(queue
        .tasks
        .iter()
        .find(|task| task_is_ready(task, &queue.tasks))
        .map_or_else(|| "no ready task".to_owned(), |task| task.id.clone()))
}

/// Persists a legal task transition with prepared and committed audit records.
pub fn transition(
    component_path: &Path,
    task_id: &str,
    target: TaskStatus,
    reason: Option<&str>,
) -> Result<String> {
    let context = validate_transition_context(component_path)?;
    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(&context, task_id, &started_at)?;

    let result = transition_locked(&context, &lock, task_id, target, reason, &started_at);

    let release = lock.release();
    match (result, release) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

fn transition_locked(
    context: &TaskContext,
    lock: &TaskLock,
    task_id: &str,
    target: TaskStatus,
    reason: Option<&str>,
    timestamp: &Timestamp,
) -> Result<String> {
    let mut queue = read_queue(&context.component_dir)?;
    ensure_component_attempts_recovered(&context.component_dir)?;
    let task_index = queue
        .tasks
        .iter()
        .position(|task| task.id == task_id)
        .ok_or_else(|| KvistError::TaskNotFound {
            component: context.component_path.clone(),
            task_id: task_id.to_owned(),
        })?;
    validate_transition(&queue.tasks[task_index], &queue.tasks, target, reason)?;
    let task = &mut queue.tasks[task_index];
    let from = task.status;
    task.status = target;
    task.timestamps.updated_at = timestamp.clone();
    task.timestamps.completed_at = (target == TaskStatus::Completed).then(|| timestamp.clone());
    task.blocked_reason = (target == TaskStatus::Blocked).then(|| reason.unwrap().to_owned());

    tracing::info!(
        task_id = %task_id,
        from = ?from,
        to = ?target,
        reason = ?reason,
        component = %context.component_path.display(),
        "task status transitioned"
    );

    let serialized = serialize(&queue).map_err(|error| KvistError::TaskQueueUnavailable {
        path: context
            .component_dir
            .join(ComponentArtifact::TaskQueue.filename()),
        reason: error.to_string(),
    })?;
    lock.revalidate()?;
    let attempt_path = attempt_path(&context.component_dir, task_id)?;
    append_attempt(
        &attempt_path,
        AttemptRecord::new("prepared", task_id, from, target, timestamp, reason),
    )?;
    lock.revalidate()?;
    replace_file_atomically(
        &context
            .component_dir
            .join(ComponentArtifact::TaskQueue.filename()),
        &serialized,
    )?;
    append_attempt(
        &attempt_path,
        AttemptRecord::new("committed", task_id, from, target, timestamp, reason),
    )?;
    Ok(format!("transitioned {task_id} to {}", status_name(target)))
}

struct TaskContext {
    project_dir: PathBuf,
    component_path: PathBuf,
    component_dir: PathBuf,
}

fn validate_context(component_path: &Path) -> Result<TaskContext> {
    validate_context_with_blocked(component_path, false)
}

fn validate_transition_context(component_path: &Path) -> Result<TaskContext> {
    validate_context_with_blocked(component_path, true)
}

fn validate_context_with_blocked(
    component_path: &Path,
    allow_blocked_component: bool,
) -> Result<TaskContext> {
    let component_path = normalize_component_path(component_path)?;
    let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let inspection = project_state::inspect(&project_dir)?;
    if inspection.state != ProjectState::Current {
        return Err(KvistError::TaskProjectNotCurrent {
            project_dir,
            state: inspection.state.name().to_owned(),
        });
    }
    if inspection.vcs.artifacts.is_empty()
        || inspection
            .vcs
            .artifacts
            .iter()
            .any(|artifact| artifact.state != VcsArtifactState::Tracked)
    {
        return Err(KvistError::TaskVcsNotCurrent {
            summary: inspection
                .vcs
                .diagnostic
                .clone()
                .unwrap_or(inspection.vcs.summary),
        });
    }
    let component = inspection
        .components
        .iter()
        .find(|component| component.path == component_path)
        .ok_or_else(|| KvistError::TaskComponentNotCurrent {
            component: component_path.clone(),
            state: "not a discovered component".to_owned(),
        })?;
    if component.state != ComponentState::Current
        && !(allow_blocked_component && component.state == ComponentState::Blocked)
    {
        return Err(KvistError::TaskComponentNotCurrent {
            component: component_path.clone(),
            state: component.state.name().to_owned(),
        });
    }
    let component_root =
        inspection
            .component_root
            .ok_or_else(|| KvistError::TaskComponentNotCurrent {
                component: component_path.clone(),
                state: "component root is unavailable".to_owned(),
            })?;
    Ok(TaskContext {
        project_dir: project_dir.clone(),
        component_dir: project_dir.join(component_root).join(&component_path),
        component_path,
    })
}

fn normalize_component_path(path: &Path) -> Result<PathBuf> {
    if path == Path::new(".") {
        return Ok(PathBuf::from("."));
    }
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(KvistError::TaskComponentPathInvalid {
            path: path.to_path_buf(),
        });
    }
    Ok(path.to_path_buf())
}

fn read_queue(component_dir: &Path) -> Result<TaskQueue> {
    read_queue_snapshot(component_dir).map(|(queue, _)| queue)
}

fn read_queue_snapshot(component_dir: &Path) -> Result<(TaskQueue, String)> {
    let path = component_dir.join(ComponentArtifact::TaskQueue.filename());
    let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
        operation: "inspect component TODO queue",
        path: path.clone(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::TaskQueueUnavailable {
            path,
            reason: "it is not a regular non-link file".to_owned(),
        });
    }
    if metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
        return Err(KvistError::TaskQueueUnavailable {
            path,
            reason: format!(
                "it exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte component artifact limit"
            ),
        });
    }
    let contents = fs::read_to_string(&path).map_err(|source| KvistError::Io {
        operation: "read component TODO queue",
        path: path.clone(),
        source,
    })?;
    let queue = parse(&contents).map_err(|error| KvistError::TaskQueueUnavailable {
        path,
        reason: error.to_string(),
    })?;
    Ok((queue, digest(contents.as_bytes())))
}

fn task_is_ready(task: &Task, tasks: &[Task]) -> bool {
    task.status == TaskStatus::Pending
        && task.depends_on.iter().all(|dependency| {
            task_by_id(tasks, dependency).is_some_and(|task| task.status == TaskStatus::Completed)
        })
        && transitive_dependencies_completed(task, tasks)
}

fn transitive_dependencies_completed(task: &Task, tasks: &[Task]) -> bool {
    let mut pending = task.depends_on.iter().collect::<Vec<_>>();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(dependency) = pending.pop() {
        if !visited.insert(dependency) {
            continue;
        }
        let Some(task) = task_by_id(tasks, dependency) else {
            return false;
        };
        if task.status != TaskStatus::Completed {
            return false;
        }
        pending.extend(task.depends_on.iter());
    }
    true
}

fn task_by_id<'a>(tasks: &'a [Task], id: &str) -> Option<&'a Task> {
    tasks.iter().find(|task| task.id == id)
}

fn validate_transition(
    task: &Task,
    tasks: &[Task],
    target: TaskStatus,
    reason: Option<&str>,
) -> Result<()> {
    if !task.status.can_transition_to(target) {
        return Err(transition_error(
            task,
            target,
            "the version-1 state machine does not allow this transition",
        ));
    }
    match target {
        TaskStatus::InProgress
            if !(matches!(task.status, TaskStatus::Pending | TaskStatus::Blocked)
                && task.depends_on.iter().all(|dependency| {
                    task_by_id(tasks, dependency)
                        .is_some_and(|dependency| dependency.status == TaskStatus::Completed)
                })
                && transitive_dependencies_completed(task, tasks)) =>
        {
            return Err(KvistError::TaskNotReady {
                task_id: task.id.clone(),
                reason: "it must be pending or blocked with all dependency-chain tasks completed"
                    .to_owned(),
            });
        }
        TaskStatus::Completed if task.status != TaskStatus::InProgress => {
            return Err(transition_error(
                task,
                target,
                "only an in-progress task may be completed",
            ));
        }
        TaskStatus::Blocked => match reason {
            Some(reason) if !reason.trim().is_empty() => {}
            _ => {
                return Err(transition_error(
                    task,
                    target,
                    "`--reason` must be nonblank when status is blocked",
                ));
            }
        },
        _ if reason.is_some() => {
            return Err(transition_error(
                task,
                target,
                "`--reason` is permitted only when status is blocked",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn transition_error(task: &Task, target: TaskStatus, reason: &str) -> KvistError {
    KvistError::TaskTransitionInvalid {
        task_id: task.id.clone(),
        from: status_name(task.status).to_owned(),
        to: status_name(target).to_owned(),
        reason: reason.to_owned(),
    }
}

struct TaskLock {
    path: PathBuf,
    contents: String,
    snapshot: TaskLockSnapshot,
    released: bool,
}

impl TaskLock {
    fn for_context(context: &TaskContext, task_id: &str, started_at: &Timestamp) -> Result<Self> {
        Self::create(&Self::task_lock_path(context)?, task_id, started_at)
    }

    fn create(path: &Path, task_id: &str, started_at: &Timestamp) -> Result<Self> {
        let contents = format!(
            "schema_version: 1\nstarted_at: {started_at}\ntask_id: {task_id:?}\npid: {}\nnonce: {}\n",
            std::process::id(),
            random_lock_nonce()?
        );
        match write_new_file_atomically(path, &contents) {
            Ok(()) => {
                let snapshot = read_task_lock(path)?;
                if snapshot.contents != contents {
                    return Err(KvistError::TaskQueueUnavailable {
                        path: path.to_path_buf(),
                        reason: "task lock ownership changed immediately after creation".to_owned(),
                    });
                }
                Ok(Self {
                    path: path.to_path_buf(),
                    contents,
                    snapshot,
                    released: false,
                })
            }
            Err(KvistError::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {
                Err(KvistError::TaskLockExists {
                    path: path.to_path_buf(),
                    task_id: task_id.to_owned(),
                })
            }
            Err(error) => Err(error),
        }
    }

    fn recover_for_context(
        context: &TaskContext,
        task_id: &str,
        started_at: &Timestamp,
        expected_crashed_lock_digest: &str,
    ) -> Result<Self> {
        let path = Self::task_lock_path(context)?;
        Self::recover_path(&path, task_id, started_at, expected_crashed_lock_digest)
    }

    fn recover_path(
        path: &Path,
        task_id: &str,
        started_at: &Timestamp,
        expected_crashed_lock_digest: &str,
    ) -> Result<Self> {
        match Self::create(path, task_id, started_at) {
            Ok(lock) => Ok(lock),
            Err(KvistError::TaskLockExists { .. }) => {
                let retained = read_task_lock(path)?;
                if !valid_task_lock_record(&retained.contents)
                    || digest(retained.contents.as_bytes()) != expected_crashed_lock_digest
                    || !retained
                        .contents
                        .contains(&format!("task_id: {task_id:?}\n"))
                {
                    return Err(KvistError::TaskQueueUnavailable {
                        path: path.to_path_buf(),
                        reason: "recovery refused because the retained task lock is not the exact authenticated crashed-operation lock".to_owned(),
                    });
                }
                if lock_owner_appears_live(&retained.contents) {
                    return Err(KvistError::TaskQueueUnavailable {
                        path: path.to_path_buf(),
                        reason: "recovery refused because the exact crashed-operation lock owner still appears live".to_owned(),
                    });
                }

                let quarantine = unique_lock_quarantine_path(path)?;
                fs::rename(path, &quarantine).map_err(|source| KvistError::Io {
                    operation: "quarantine exact stale task lock for recovery",
                    path: path.to_path_buf(),
                    source,
                })?;
                sync_lock_directory(path)?;

                let quarantined = read_task_lock(&quarantine)?;
                if quarantined != retained
                    || digest(quarantined.contents.as_bytes()) != expected_crashed_lock_digest
                    || lock_owner_appears_live(&quarantined.contents)
                {
                    return Err(restore_quarantined_lock(
                        path,
                        &quarantine,
                        "recovery refused because the task lock changed while it was being quarantined",
                    ));
                }

                match Self::create(path, task_id, started_at) {
                    Ok(lock) => {
                        fs::remove_file(&quarantine).map_err(|source| KvistError::Io {
                            operation: "remove quarantined stale task lock",
                            path: quarantine.clone(),
                            source,
                        })?;
                        sync_lock_directory(path)?;
                        Ok(lock)
                    }
                    Err(error) => Err(restore_quarantined_lock(
                        path,
                        &quarantine,
                        &format!(
                            "recovery lock takeover raced another owner; retained quarantined state: {error}"
                        ),
                    )),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn contents_digest(&self) -> String {
        digest(self.contents.as_bytes())
    }

    fn revalidate(&self) -> Result<()> {
        let snapshot = read_task_lock(&self.path)?;
        if snapshot.contents != self.contents || snapshot != self.snapshot {
            return Err(KvistError::TaskQueueUnavailable {
                path: self.path.clone(),
                reason: "task lock ownership changed".to_owned(),
            });
        }
        Ok(())
    }

    fn release(mut self) -> Result<()> {
        self.released = true;
        self.remove_if_owned()
    }

    fn remove_if_owned(&self) -> Result<()> {
        self.revalidate()?;
        let quarantine = unique_lock_quarantine_path(&self.path)?;
        fs::rename(&self.path, &quarantine).map_err(|source| KvistError::Io {
            operation: "quarantine user-owned task lock for release",
            path: self.path.clone(),
            source,
        })?;
        sync_lock_directory(&self.path)?;
        let snapshot = read_task_lock(&quarantine)?;
        if snapshot.contents != self.contents || snapshot != self.snapshot {
            return Err(restore_quarantined_lock(
                &self.path,
                &quarantine,
                "task lock ownership changed during release",
            ));
        }
        fs::remove_file(&quarantine).map_err(|source| KvistError::Io {
            operation: "remove user-owned task lock",
            path: quarantine,
            source,
        })?;
        sync_lock_directory(&self.path)
    }

    fn remove_stale_path(path: &Path) -> Result<()> {
        let retained = read_task_lock(path)?;
        if !valid_task_lock_record(&retained.contents) {
            return Err(KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: "unlock refused because the task lock record is malformed".to_owned(),
            });
        }
        if lock_owner_appears_live(&retained.contents) {
            return Err(KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: "unlock refused because the task lock owner still appears live".to_owned(),
            });
        }

        let quarantine = unique_lock_quarantine_path(path)?;
        fs::rename(path, &quarantine).map_err(|source| KvistError::Io {
            operation: "quarantine stale task lock for unlock",
            path: path.to_path_buf(),
            source,
        })?;
        sync_lock_directory(path)?;

        let quarantined = read_task_lock(&quarantine)?;
        if quarantined != retained || lock_owner_appears_live(&quarantined.contents) {
            return Err(restore_quarantined_lock(
                path,
                &quarantine,
                "unlock refused because the task lock changed while it was being quarantined",
            ));
        }
        fs::remove_file(&quarantine).map_err(|source| KvistError::Io {
            operation: "remove quarantined stale task lock for unlock",
            path: quarantine,
            source,
        })?;
        sync_lock_directory(path)
    }

    fn task_lock_path(context: &TaskContext) -> Result<PathBuf> {
        let project = context.project_dir.canonicalize().map_err(|source| {
            KvistError::TaskQueueUnavailable {
                path: context.project_dir.clone(),
                reason: format!("canonicalize project for task lock: {source}"),
            }
        })?;
        let component = context.component_dir.canonicalize().map_err(|source| {
            KvistError::TaskQueueUnavailable {
                path: context.component_dir.clone(),
                reason: format!("canonicalize component for task lock: {source}"),
            }
        })?;
        let state_base = user_state_base()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| KvistError::TaskQueueUnavailable {
                path: context.project_dir.clone(),
                reason: "cannot determine an absolute user-owned task-lock state directory"
                    .to_owned(),
            })?;
        let directory = state_base.join("kvist").join("task-locks-v1");
        fs::create_dir_all(&directory).map_err(|source| KvistError::TaskQueueUnavailable {
            path: directory.clone(),
            reason: format!("create user-owned task-lock directory: {source}"),
        })?;
        let directory =
            directory
                .canonicalize()
                .map_err(|source| KvistError::TaskQueueUnavailable {
                    path: directory.clone(),
                    reason: format!("canonicalize user-owned task-lock directory: {source}"),
                })?;
        if directory.starts_with(&project) {
            return Err(KvistError::TaskQueueUnavailable {
                path: directory,
                reason: "user-owned task-lock state must not be inside the project".to_owned(),
            });
        }
        let metadata = fs::symlink_metadata(&directory).map_err(|source| {
            KvistError::TaskQueueUnavailable {
                path: directory.clone(),
                reason: format!("inspect user-owned task-lock directory: {source}"),
            }
        })?;
        if is_link_like(&metadata) || !metadata.file_type().is_dir() {
            return Err(KvistError::TaskQueueUnavailable {
                path: directory,
                reason: "user-owned task-lock state must be a real directory".to_owned(),
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).map_err(
                |source| KvistError::TaskQueueUnavailable {
                    path: directory.clone(),
                    reason: format!("protect user-owned task-lock directory: {source}"),
                },
            )?;
        }
        let identity = digest(
            format!(
                "{}\n{}",
                project.to_string_lossy(),
                component.to_string_lossy()
            )
            .as_bytes(),
        )
        .trim_start_matches("sha256:")
        .to_owned();
        Ok(directory.join(format!("{identity}.lock")))
    }
}

impl Drop for TaskLock {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.remove_if_owned();
        }
    }
}

fn random_lock_nonce() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| KvistError::TaskQueueUnavailable {
        path: PathBuf::from("."),
        reason: format!("cannot generate task lock owner nonce: {error}"),
    })?;
    Ok(hex::encode(bytes))
}

#[derive(Clone, PartialEq, Eq)]
struct TaskLockSnapshot {
    contents: String,
    identity: TaskLockFileIdentity,
}

#[derive(Clone, PartialEq, Eq)]
struct TaskLockFileIdentity {
    length: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn task_lock_file_identity(metadata: &fs::Metadata) -> TaskLockFileIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        TaskLockFileIdentity {
            length: metadata.len(),
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    #[cfg(not(unix))]
    {
        TaskLockFileIdentity {
            length: metadata.len(),
        }
    }
}

fn read_task_lock(path: &Path) -> Result<TaskLockSnapshot> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot inspect task lock ownership: {source}"),
        })?;
    if is_link_like(&metadata)
        || !metadata.file_type().is_file()
        || metadata.len() > MAX_TASK_LOCK_BYTES
    {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "task lock ownership was replaced or is malformed".to_owned(),
        });
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|source| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot open task lock ownership without following links: {source}"),
        })?;
    let opened_metadata = file
        .metadata()
        .map_err(|source| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot inspect opened task lock ownership: {source}"),
        })?;
    let expected_identity = task_lock_file_identity(&metadata);
    let opened_identity = task_lock_file_identity(&opened_metadata);
    if !opened_metadata.file_type().is_file() || opened_identity != expected_identity {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "task lock ownership changed while it was opened".to_owned(),
        });
    }
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    (&mut file)
        .take(MAX_TASK_LOCK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot read bounded task lock ownership: {source}"),
        })?;
    let final_metadata = file
        .metadata()
        .map_err(|source| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot reinspect task lock ownership: {source}"),
        })?;
    if bytes.len() as u64 > MAX_TASK_LOCK_BYTES
        || task_lock_file_identity(&final_metadata) != opened_identity
        || final_metadata.len() != bytes.len() as u64
    {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "task lock ownership grew or changed while it was read".to_owned(),
        });
    }
    let contents = String::from_utf8(bytes).map_err(|_| KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: "task lock ownership is not valid UTF-8".to_owned(),
    })?;
    Ok(TaskLockSnapshot {
        contents,
        identity: opened_identity,
    })
}

fn unique_lock_quarantine_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "task lock has no parent directory".to_owned(),
        })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "task lock file name is not valid UTF-8".to_owned(),
        })?;
    for _ in 0..16 {
        let candidate = parent.join(format!(
            ".{name}.recovery-quarantine-{}",
            random_lock_nonce()?
        ));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect task lock quarantine path",
                    path: candidate,
                    source,
                });
            }
        }
    }
    Err(KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: "cannot allocate a unique task lock quarantine path".to_owned(),
    })
}

fn restore_quarantined_lock(path: &Path, quarantine: &Path, reason: &str) -> KvistError {
    match fs::hard_link(quarantine, path) {
        Ok(()) => {
            let _ = fs::remove_file(quarantine);
            let _ = sync_lock_directory(path);
            KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: reason.to_owned(),
            }
        }
        Err(error) => KvistError::TaskQueueUnavailable {
            path: quarantine.to_path_buf(),
            reason: format!(
                "{reason}; could not restore without overwriting another owner: {error}"
            ),
        },
    }
}

fn sync_lock_directory(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "task lock has no parent directory".to_owned(),
        })?;
    sync_directory(parent)
}

fn lock_owner_appears_live(contents: &str) -> bool {
    let Some(pid) = contents
        .lines()
        .find_map(|line| line.strip_prefix("pid: "))
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return false;
    };
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(pid.to_string()).is_dir()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}

fn valid_task_lock_record(contents: &str) -> bool {
    let mut lines = contents.lines();
    let valid = lines.next() == Some("schema_version: 1")
        && lines
            .next()
            .and_then(|line| line.strip_prefix("started_at: "))
            .is_some_and(|value| !value.is_empty())
        && lines
            .next()
            .and_then(|line| line.strip_prefix("task_id: \""))
            .and_then(|value| value.strip_suffix('"'))
            .is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 64
                    && !value.starts_with('-')
                    && !value.ends_with('-')
                    && value.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            })
        && lines
            .next()
            .and_then(|line| line.strip_prefix("pid: "))
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|pid| pid > 0)
        && lines
            .next()
            .and_then(|line| line.strip_prefix("nonce: "))
            .is_some_and(|value| {
                value.len() == 32
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            });
    valid && lines.next().is_none()
}

fn attempt_path(component_dir: &Path, task_id: &str) -> Result<PathBuf> {
    let directory = component_dir.join(".kvist-attempts");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(KvistError::Io {
                operation: "use attempt directory",
                path: directory,
                source: io::Error::other("attempt directory must be a real directory"),
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&directory).map_err(|source| KvistError::Io {
                operation: "create attempt directory",
                path: directory.clone(),
                source,
            })?;
            sync_directory(component_dir)?;
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect attempt directory",
                path: directory,
                source,
            });
        }
    }
    Ok(directory.join(format!("{task_id}.jsonl")))
}

fn existing_attempt_path(component_dir: &Path, task_id: &str) -> PathBuf {
    component_dir
        .join(".kvist-attempts")
        .join(format!("{task_id}.jsonl"))
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct AttemptWriteScope {
    path: String,
    pre_digest: String,
}

#[derive(Serialize)]
struct AttemptRecord<'a> {
    phase: &'a str,
    task_id: &'a str,
    from_status: &'a str,
    to_status: &'a str,
    timestamp: &'a Timestamp,
    reason: Option<&'a str>,
}

#[derive(Serialize)]
struct AgentExecutionRecord<'a> {
    schema_version: u32,
    attempt_id: &'a str,
    phase: &'a str,
    task_id: &'a str,
    timestamp: &'a Timestamp,
    success: bool,
    timed_out: bool,
    output_limit_exceeded: bool,
    stdout: &'a str,
    stderr: &'a str,
}

fn append_agent_execution(path: &Path, record: AgentExecutionRecord<'_>) -> Result<()> {
    let encoded =
        serde_json::to_string(&record).map_err(|error| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot serialize agent execution record: {error}"),
        })?;
    append_encoded_attempt(path, &encoded, "append agent execution record")
}

impl<'a> AttemptRecord<'a> {
    fn new(
        phase: &'a str,
        task_id: &'a str,
        from: TaskStatus,
        to: TaskStatus,
        timestamp: &'a Timestamp,
        reason: Option<&'a str>,
    ) -> Self {
        Self {
            phase,
            task_id,
            from_status: status_name(from),
            to_status: status_name(to),
            timestamp,
            reason: (to == TaskStatus::Blocked).then_some(reason).flatten(),
        }
    }
}

fn append_attempt(path: &Path, record: AttemptRecord<'_>) -> Result<()> {
    let encoded =
        serde_json::to_string(&record).map_err(|error| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot serialize attempt record: {error}"),
        })?;
    append_encoded_attempt(path, &encoded, "append attempt record")
}

fn append_encoded_attempt(path: &Path, encoded: &str, operation: &'static str) -> Result<()> {
    let (mut file, created) = match open_existing_attempt_for_append(path, operation) {
        Ok(file) => (file, false),
        Err(KvistError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            match create_attempt_for_append(path, operation) {
                Ok(file) => (file, true),
                Err(KvistError::Io { source, .. })
                    if source.kind() == io::ErrorKind::AlreadyExists =>
                {
                    (open_existing_attempt_for_append(path, operation)?, false)
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    file.write_all(encoded.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        })?;
    if created {
        let parent = path.parent().ok_or_else(|| KvistError::Io {
            operation: "determine attempt record parent",
            path: path.to_path_buf(),
            source: io::Error::other("attempt record has no parent"),
        })?;
        sync_directory(parent)?;
    }
    Ok(())
}

fn open_existing_attempt_for_append(path: &Path, operation: &'static str) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|source| KvistError::Io {
        operation: "open attempt record without following links",
        path: path.to_path_buf(),
        source,
    })?;
    validate_opened_attempt_file(&file, path, operation)?;
    Ok(file)
}

fn create_attempt_for_append(path: &Path, operation: &'static str) -> Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|source| KvistError::Io {
        operation: "create attempt record without overwriting",
        path: path.to_path_buf(),
        source,
    })?;
    validate_opened_attempt_file(&file, path, operation)?;
    Ok(file)
}

fn validate_opened_attempt_file(
    file: &fs::File,
    path: &Path,
    operation: &'static str,
) -> Result<()> {
    let metadata = file.metadata().map_err(|source| KvistError::Io {
        operation: "inspect opened attempt record",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.file_type().is_file() {
        return Err(KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source: io::Error::other("attempt record must be a regular file"),
        });
    }
    Ok(())
}

fn append_json_attempt<T: Serialize>(
    path: &Path,
    record: &T,
    operation: &'static str,
) -> Result<()> {
    let encoded =
        serde_json::to_string(record).map_err(|error| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot serialize attempt evidence: {error}"),
        })?;
    append_encoded_attempt(path, &encoded, operation)
}

struct AttemptJournal {
    events: Vec<Value>,
    digest: String,
}

fn read_attempt_journal(path: &Path) -> Result<AttemptJournal> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AttemptJournal {
                events: Vec::new(),
                digest: digest(b""),
            });
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect attempt journal",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "attempt journal must be a regular non-link file".to_owned(),
        });
    }
    if metadata.len() > MAX_ATTEMPT_JOURNAL_BYTES {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("attempt journal exceeds the {MAX_ATTEMPT_JOURNAL_BYTES}-byte limit"),
        });
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|source| KvistError::Io {
        operation: "open attempt journal without following links",
        path: path.to_path_buf(),
        source,
    })?;
    let opened_metadata = file.metadata().map_err(|source| KvistError::Io {
        operation: "inspect opened attempt journal",
        path: path.to_path_buf(),
        source,
    })?;
    if !opened_metadata.file_type().is_file() || opened_metadata.len() > MAX_ATTEMPT_JOURNAL_BYTES {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "attempt journal changed to a non-regular or oversized file while opening"
                .to_owned(),
        });
    }
    let mut bytes =
        Vec::with_capacity(opened_metadata.len().min(MAX_ATTEMPT_JOURNAL_BYTES) as usize);
    (&mut file)
        .take(MAX_ATTEMPT_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| KvistError::Io {
            operation: "read bounded attempt journal",
            path: path.to_path_buf(),
            source,
        })?;
    let final_metadata = file.metadata().map_err(|source| KvistError::Io {
        operation: "reinspect opened attempt journal",
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.len() as u64 > MAX_ATTEMPT_JOURNAL_BYTES
        || final_metadata.len() != opened_metadata.len()
        || final_metadata.len() != bytes.len() as u64
    {
        return Err(KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "attempt journal grew or changed while it was read".to_owned(),
        });
    }
    let contents =
        String::from_utf8(bytes.clone()).map_err(|_| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: "attempt journal is not valid UTF-8".to_owned(),
        })?;
    let mut events = Vec::new();
    for (line_number, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if events.len() >= MAX_ATTEMPT_EVENTS {
            return Err(KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: format!("attempt journal exceeds the {MAX_ATTEMPT_EVENTS}-event limit"),
            });
        }
        events.push(serde_json::from_str(line).map_err(|error| {
            KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: format!(
                    "attempt journal line {} is malformed: {error}",
                    line_number + 1
                ),
            }
        })?);
    }
    Ok(AttemptJournal {
        events,
        digest: digest(&bytes),
    })
}

fn next_attempt_id(path: &Path) -> Result<String> {
    let journal = read_attempt_journal(path)?;
    let mut used = BTreeSet::new();
    for event in journal.events {
        if event.get("schema_version").is_none() {
            continue;
        }
        let attempt = event
            .get("attempt_id")
            .and_then(Value::as_str)
            .ok_or_else(|| KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: "versioned attempt evidence is missing attempt_id".to_owned(),
            })?;
        if !valid_attempt_id(attempt) {
            return Err(KvistError::TaskQueueUnavailable {
                path: path.to_path_buf(),
                reason: "versioned attempt evidence has an invalid attempt_id".to_owned(),
            });
        }
        used.insert(attempt.to_owned());
    }
    for number in 1..=MAX_ATTEMPT_EVENTS {
        let attempt_id = format!("attempt-{number:04}");
        if !used.contains(&attempt_id) {
            return Ok(attempt_id);
        }
    }
    Err(KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: "attempt journal has no available bounded attempt identity".to_owned(),
    })
}

fn valid_attempt_id(value: &str) -> bool {
    value
        .strip_prefix("attempt-")
        .is_some_and(|number| number.len() == 4 && number.bytes().all(|byte| byte.is_ascii_digit()))
}

fn ensure_component_attempts_recovered(component_dir: &Path) -> Result<()> {
    let queue = read_queue(component_dir)?;
    ensure_component_attempts_recovered_except(component_dir, &queue, None)
}

fn ensure_component_attempts_recovered_for_recovery(
    component_dir: &Path,
    task_id: &str,
    attempt_id: &str,
) -> Result<()> {
    let queue = read_queue(component_dir)?;
    ensure_component_attempts_recovered_except(component_dir, &queue, Some((task_id, attempt_id)))
}

fn ensure_component_attempts_recovered_except(
    component_dir: &Path,
    queue: &TaskQueue,
    recovery_in_progress: Option<(&str, &str)>,
) -> Result<()> {
    for task in &queue.tasks {
        let permitted_attempt = recovery_in_progress.and_then(|(recovery_task, attempt_id)| {
            (task.id == recovery_task).then_some(attempt_id)
        });
        let attempt_path = existing_attempt_path(component_dir, &task.id);
        if task
            .recovery_state
            .as_ref()
            .is_some_and(|state| Some(state.attempt_id.as_str()) != permitted_attempt)
        {
            return Err(unresolved_attempt_error(&attempt_path));
        }
        ensure_task_attempt_recovered(component_dir, &task.id, permitted_attempt)?;
    }
    Ok(())
}

fn ensure_task_attempt_recovered(
    component_dir: &Path,
    task_id: &str,
    permitted_attempt: Option<&str>,
) -> Result<()> {
    let path = existing_attempt_path(component_dir, task_id);
    let journal = read_attempt_journal(&path)?;
    if journal.events.is_empty() {
        return Ok(());
    }
    if legacy_trailing_prepared(&journal.events) {
        return Err(KvistError::TaskQueueUnavailable {
            path,
            reason: "a legacy trailing prepared task attempt requires explicit recovery before another transition"
                .to_owned(),
        });
    }
    if !journal
        .events
        .iter()
        .any(|event| event.get("schema_version").is_some())
    {
        return Ok(());
    }
    let secret = load_existing_recovery_secret()?;
    let assessed = assess_attempt_journal(&path, journal, task_id, &secret)?;
    if assessed.attempts.iter().any(|(attempt_id, attempt)| {
        !attempt.is_fully_recovered() && Some(attempt_id.as_str()) != permitted_attempt
    }) {
        return Err(unresolved_attempt_error(&path));
    }
    Ok(())
}

fn unresolved_attempt_error(path: &Path) -> KvistError {
    KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: "a fenced or unresolved authenticated task attempt requires `kvist task recover` before another transition"
            .to_owned(),
    }
}

#[derive(Default)]
struct AssessedAttempt {
    prepared: Option<PreparedAttempt>,
    pre_spawn_failure: Option<PreSpawnFailure>,
    recovery_prepared: Option<RecoveryPreparedAttempt>,
    recovered: Option<RecoveredAttempt>,
    lifecycle_advanced: bool,
}

impl AssessedAttempt {
    fn is_fully_recovered(&self) -> bool {
        if self.lifecycle_advanced {
            return true;
        }
        matches!(
            (&self.prepared, &self.pre_spawn_failure, &self.recovery_prepared, &self.recovered),
            (Some(prepared), Some(failure), Some(decision), Some(recovered))
                if decision.fenced_queue_digest == prepared.intended_post_queue_digest
                    && failure.prepared_event_digest.starts_with("sha256:")
                    && decision.recovered_queue_digest == recovered.recovered_queue_digest
                    && decision.pre_status == prepared.pre_status
        )
    }
}

struct AssessedJournal {
    digest: String,
    attempts: BTreeMap<String, AssessedAttempt>,
}

fn assess_attempt_journal(
    path: &Path,
    journal: AttemptJournal,
    task_id: &str,
    secret: &[u8],
) -> Result<AssessedJournal> {
    let mut attempts = BTreeMap::<String, AssessedAttempt>::new();
    for event in journal.events {
        let Some(version) = event.get("schema_version") else {
            continue;
        };
        if version != &Value::from(ATTEMPT_SCHEMA_VERSION) {
            return Err(journal_error(
                path,
                "attempt journal contains an unsupported versioned evidence record",
            ));
        }
        let event_task_id = event
            .get("task_id")
            .and_then(Value::as_str)
            .ok_or_else(|| journal_error(path, "versioned attempt evidence is missing task_id"))?;
        if event_task_id != task_id {
            return Err(journal_error(
                path,
                "attempt journal contains evidence bound to a different task",
            ));
        }
        let event_attempt_id =
            event
                .get("attempt_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    journal_error(path, "versioned attempt evidence is missing attempt_id")
                })?;
        if !valid_attempt_id(event_attempt_id) {
            return Err(journal_error(
                path,
                "versioned attempt evidence has an invalid attempt_id",
            ));
        }
        let phase = event
            .get("phase")
            .and_then(Value::as_str)
            .ok_or_else(|| journal_error(path, "attempt journal evidence is missing its phase"))?;
        let attempt = attempts.entry(event_attempt_id.to_owned()).or_default();
        match phase {
            "prepared" => {
                if attempt.prepared.is_some()
                    || attempt.pre_spawn_failure.is_some()
                    || attempt.recovery_prepared.is_some()
                    || attempt.recovered.is_some()
                {
                    return Err(journal_error(
                        path,
                        "attempt journal contains duplicate or out-of-order prepared evidence",
                    ));
                }
                let prepared = decode_evidence::<PreparedAttempt>(&event, path, "prepared")?;
                validate_prepared_attempt(&prepared, path)?;
                if !prepared.authentication_tag.is_empty()
                    && prepared.authentication_tag != prepared_attempt_tag(secret, &prepared)?
                {
                    return Err(journal_error(
                        path,
                        "prepared attempt evidence is not authentically bound by the user-owned host secret",
                    ));
                }
                attempt.prepared = Some(prepared);
            }
            "execution-finished"
            | "verification-finished"
            | "pending-human-disposition"
            | "human-finalized"
            | "agent-execution" => {
                attempt.lifecycle_advanced = true;
            }
            "pre-spawn-failure" => {
                if attempt.prepared.is_none()
                    || attempt.pre_spawn_failure.is_some()
                    || attempt.recovery_prepared.is_some()
                    || attempt.recovered.is_some()
                {
                    return Err(journal_error(
                        path,
                        "attempt journal contains duplicate or out-of-order pre-spawn failure evidence",
                    ));
                }
                let failure =
                    decode_evidence::<PreSpawnFailure>(&event, path, "pre-spawn failure")?;
                let prepared = attempt.prepared.as_ref().ok_or_else(|| {
                    journal_error(path, "pre-spawn failure has no prepared attempt")
                })?;
                validate_pre_spawn_failure(&failure, prepared, path)?;
                if failure.authentication_tag != pre_spawn_failure_tag(secret, &failure)? {
                    return Err(journal_error(
                        path,
                        "pre-spawn failure evidence is not authentically bound by the user-owned host secret",
                    ));
                }
                attempt.pre_spawn_failure = Some(failure);
            }
            "recovery-prepared" => {
                if attempt.prepared.is_none()
                    || attempt.pre_spawn_failure.is_none()
                    || attempt.recovery_prepared.is_some()
                    || attempt.recovered.is_some()
                {
                    return Err(journal_error(
                        path,
                        "attempt journal contains duplicate or out-of-order recovery-prepared evidence",
                    ));
                }
                let decision =
                    decode_evidence::<RecoveryPreparedAttempt>(&event, path, "recovery-prepared")?;
                let prepared = attempt.prepared.as_ref().ok_or_else(|| {
                    journal_error(path, "recovery evidence has no prepared attempt")
                })?;
                let failure = attempt.pre_spawn_failure.as_ref().ok_or_else(|| {
                    journal_error(path, "recovery evidence has no pre-spawn failure evidence")
                })?;
                validate_recovery_prepared(&decision, prepared, failure, path)?;
                if decision.authentication_tag != recovery_prepared_tag(secret, &decision)? {
                    return Err(journal_error(
                        path,
                        "recovery-prepared evidence is not authentically bound by the user-owned host secret",
                    ));
                }
                attempt.recovery_prepared = Some(decision);
            }
            "recovered" => {
                if attempt.recovery_prepared.is_none() || attempt.recovered.is_some() {
                    return Err(journal_error(
                        path,
                        "attempt journal contains duplicate or out-of-order recovered evidence",
                    ));
                }
                let recovered = decode_evidence::<RecoveredAttempt>(&event, path, "recovered")?;
                let decision = attempt.recovery_prepared.as_ref().ok_or_else(|| {
                    journal_error(path, "recovered evidence has no recovery-prepared decision")
                })?;
                validate_recovered_attempt(&recovered, decision, path)?;
                if recovered.authentication_tag != recovered_attempt_tag(secret, &recovered)? {
                    return Err(journal_error(
                        path,
                        "recovered evidence is not authentically bound by the user-owned host secret",
                    ));
                }
                attempt.recovered = Some(recovered);
            }
            _ => {
                return Err(journal_error(
                    path,
                    "attempt journal contains an unsupported versioned evidence phase",
                ));
            }
        }
    }
    Ok(AssessedJournal {
        digest: journal.digest,
        attempts,
    })
}

fn decode_evidence<T: for<'de> Deserialize<'de>>(
    event: &Value,
    path: &Path,
    name: &str,
) -> Result<T> {
    serde_json::from_value(event.clone()).map_err(|_| {
        journal_error(
            path,
            &format!("{name} attempt evidence has malformed or unsupported fields"),
        )
    })
}

fn journal_error(path: &Path, reason: &str) -> KvistError {
    KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

fn legacy_trailing_prepared(events: &[Value]) -> bool {
    events.last().is_some_and(|event| {
        event.get("schema_version").is_none()
            && event.get("phase").and_then(Value::as_str) == Some("prepared")
    })
}

fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Completed => "completed",
    }
}

fn validate_accept_context(component_path: &Path) -> Result<TaskContext> {
    let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let inspection = project_state::inspect(&project_dir)?;
    if inspection.state != ProjectState::Current {
        return Err(KvistError::TaskProjectNotCurrent {
            project_dir,
            state: inspection.state.name().to_owned(),
        });
    }
    if inspection.vcs.artifacts.is_empty()
        || inspection
            .vcs
            .artifacts
            .iter()
            .any(|artifact| artifact.state != VcsArtifactState::Tracked)
    {
        return Err(KvistError::TaskVcsNotCurrent {
            summary: inspection
                .vcs
                .diagnostic
                .clone()
                .unwrap_or(inspection.vcs.summary),
        });
    }
    let component_root =
        inspection
            .component_root
            .clone()
            .ok_or_else(|| KvistError::TaskComponentNotCurrent {
                component: component_path.to_path_buf(),
                state: "component root is unavailable".to_owned(),
            })?;
    let component_path =
        normalize_component_argument(component_path, &project_dir, &component_root)?;
    let component = inspection
        .components
        .iter()
        .find(|component| component.path == component_path)
        .ok_or_else(|| KvistError::TaskComponentNotCurrent {
            component: component_path.clone(),
            state: "not a discovered component".to_owned(),
        })?;

    let has_valid_artifacts = component
        .artifacts
        .iter()
        .all(|artifact| artifact.state == project_state::ComponentArtifactState::Valid);
    let is_allowed = matches!(
        component.state,
        ComponentState::Current | ComponentState::Stale | ComponentState::Blocked
    ) || (component.state == ComponentState::Invalid && has_valid_artifacts);
    if !is_allowed {
        return Err(KvistError::TaskComponentNotCurrent {
            component: component_path.clone(),
            state: component.state.name().to_owned(),
        });
    }
    Ok(TaskContext {
        project_dir: project_dir.clone(),
        component_dir: project_dir.join(component_root).join(&component_path),
        component_path,
    })
}

fn validate_commit_preconditions(project_dir: &Path, message: Option<&str>) -> Result<()> {
    if let Some(msg) = message {
        if msg.len() > 65_536 {
            return Err(KvistError::VcsCommitFailed {
                reason: "commit message exceeds size limit of 65536 bytes".to_owned(),
            });
        }
        if msg
            .chars()
            .any(|c| (c as u32) < 32 && c != '\n' && c != '\r' && c != '\t')
        {
            return Err(KvistError::VcsCommitFailed {
                reason: "commit message contains invalid control characters".to_owned(),
            });
        }
    }

    if let Ok(config) = crate::config::load(project_dir)
        && config.vcs == crate::config::VcsSelection::Jujutsu
    {
        return Err(KvistError::VcsCommitFailed {
            reason: "Jujutsu write backend is unsupported".to_owned(),
        });
    }

    let git_dir = project_dir.join(".git");
    if git_dir.exists() {
        let branch_check = std::process::Command::new("git")
            .args(["symbolic-ref", "-q", "HEAD"])
            .current_dir(project_dir)
            .output();
        if let Ok(output) = branch_check
            && !output.status.success()
        {
            return Err(KvistError::VcsCommitFailed {
                reason: "refusing to commit on detached HEAD; must be on a branch".to_owned(),
            });
        }

        let staged_check = std::process::Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(project_dir)
            .output();
        if let Ok(output) = staged_check
            && output.status.success()
        {
            let staged_files = String::from_utf8_lossy(&output.stdout);
            for line in staged_files.lines() {
                let path = Path::new(line.trim());
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                    && matches!(
                        file_name,
                        "REQUIREMENTS.md" | "CONTRACT.md" | "DESIGN.md" | "TODOS.yaml" | "IMPL.md"
                    )
                {
                    return Err(KvistError::VcsCommitFailed {
                        reason: format!(
                            "accepted-path overlap detected: staged index contains uncommitted changes for `{}`",
                            line.trim()
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Records reviewed component-document and immediate-parent contract revisions.
pub fn accept(
    component_path: &Path,
    commit: bool,
    message: Option<&str>,
    is_json: bool,
) -> Result<String> {
    let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;

    if commit {
        validate_commit_preconditions(&project_dir, message)?;
    }

    let context = validate_accept_context(component_path)?;
    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(&context, "accept", &started_at)?;

    let result = (|| -> Result<String> {
        let mut queue = read_queue(&context.component_dir)?;
        ensure_component_attempts_recovered(&context.component_dir)?;

        let requirements = read_validated_document(
            DocumentKind::Requirements,
            &context
                .component_dir
                .join(ComponentArtifact::Requirements.filename()),
        )?;
        let contract = read_validated_document(
            DocumentKind::Contract,
            &context
                .component_dir
                .join(ComponentArtifact::Contract.filename()),
        )?;
        let design = read_validated_document(
            DocumentKind::Design,
            &context
                .component_dir
                .join(ComponentArtifact::Design.filename()),
        )?;

        if context.component_path == Path::new(".") {
            queue.component.parent_contract = None;
        } else {
            let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
                operation: "determine current project directory",
                path: PathBuf::from("."),
                source,
            })?;
            let inspection = project_state::inspect(&project_dir)?;
            let component_root = project_dir.join(
                inspection
                    .component_root
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("src")),
            );
            let (parent_component_dir, parent_relative_path) =
                discovery::find_parent_component_dir(&component_root, &context.component_path)?;
            let parent_contract_path =
                parent_component_dir.join(ComponentArtifact::Contract.filename());
            let parent_contents =
                read_validated_document(DocumentKind::Contract, &parent_contract_path)?;
            queue.component.parent_contract = Some(crate::task_queue::ParentContract {
                path: project_state::relative_parent_contract_path(
                    &context.component_path,
                    &parent_relative_path,
                ),
                revision: digest(parent_contents.as_bytes()),
            });
        }

        queue.component.requirements_revision = digest(requirements.as_bytes());
        queue.component.contract_revision = digest(contract.as_bytes());
        queue.component.design_revision = digest(design.as_bytes());
        queue.component.revalidation.state = crate::task_queue::RevalidationState::Current;
        queue.component.revalidation.checked_at = started_at.clone();
        queue.component.revalidation.stale_since = None;
        queue.component.revalidation.causes = Vec::new();

        let serialized = serialize(&queue).map_err(|error| KvistError::TaskQueueUnavailable {
            path: context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            reason: error.to_string(),
        })?;
        lock.revalidate()?;
        replace_file_atomically(
            &context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            &serialized,
        )?;

        let message_text = format!(
            "accepted component document changes for {}",
            context.component_path.display()
        );

        if commit {
            let queue_rel = if context.component_path == Path::new(".") {
                PathBuf::from("engine/TODOS.yaml")
            } else {
                context
                    .component_path
                    .join(ComponentArtifact::TaskQueue.filename())
            };
            let head_queue_blob = std::process::Command::new("git")
                .args([
                    "cat-file",
                    "-p",
                    &format!("HEAD:{}", queue_rel.to_string_lossy()),
                ])
                .current_dir(&context.project_dir)
                .output();
            let head_queue_content = head_queue_blob.ok().and_then(|o| {
                if o.status.success() {
                    Some(o.stdout)
                } else {
                    None
                }
            });
            let pre_queue_digest = head_queue_content
                .as_ref()
                .map(|b| format!("sha256:{}", hex::encode(sha2::Sha256::digest(b))));

            let mut accepted_changes = vec![crate::vcs_commit::AcceptedChange {
                path: queue_rel,
                operation: "modify".to_owned(),
                pre_digest: pre_queue_digest,
                post_digest: Some(digest(serialized.as_bytes())),
            }];

            for (kind, content) in [
                (ComponentArtifact::Requirements, &requirements),
                (ComponentArtifact::Contract, &contract),
                (ComponentArtifact::Design, &design),
            ] {
                let doc_rel = if context.component_path == Path::new(".") {
                    PathBuf::from(format!("engine/{}", kind.filename()))
                } else {
                    context.component_path.join(kind.filename())
                };
                let doc_full = context.project_dir.join(&doc_rel);
                if doc_full.exists() {
                    let head_blob = std::process::Command::new("git")
                        .args([
                            "cat-file",
                            "-p",
                            &format!("HEAD:{}", doc_rel.to_string_lossy()),
                        ])
                        .current_dir(&context.project_dir)
                        .output();
                    let head_content = head_blob.ok().and_then(|o| {
                        if o.status.success() {
                            Some(o.stdout)
                        } else {
                            None
                        }
                    });
                    let pre_digest = head_content
                        .as_ref()
                        .map(|b| format!("sha256:{}", hex::encode(sha2::Sha256::digest(b))));
                    let is_modified = match head_content {
                        Some(bytes) => bytes != content.as_bytes(),
                        None => true,
                    };
                    if is_modified {
                        accepted_changes.push(crate::vcs_commit::AcceptedChange {
                            path: doc_rel,
                            operation: "modify".to_owned(),
                            pre_digest,
                            post_digest: Some(digest(content.as_bytes())),
                        });
                    }
                }
            }
            accepted_changes.sort_by(|a, b| a.path.cmp(&b.path));

            // Check if staged index overlaps with accepted changes
            let staged_check = std::process::Command::new("git")
                .args(["diff", "--cached", "--name-only"])
                .current_dir(&context.project_dir)
                .output();
            if let Ok(output) = staged_check
                && output.status.success()
            {
                let staged_files: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect();
                for change in &accepted_changes {
                    let change_str = change.path.to_string_lossy();
                    if staged_files.iter().any(|s| s == &change_str) {
                        return Err(KvistError::VcsCommitFailed {
                            reason: format!(
                                "accepted-path overlap detected: staged index contains uncommitted changes for `{}`",
                                change_str
                            ),
                        });
                    }
                }
            }

            let expected_head = match std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&context.project_dir)
                .output()
            {
                Ok(out) if out.status.success() => {
                    String::from_utf8_lossy(&out.stdout).trim().to_owned()
                }
                _ => "".to_owned(),
            };
            let acceptance_id = format!("accept-{}", started_at.to_string().replace(':', "-"));
            let commit_message = message.map(str::to_owned).unwrap_or_else(|| {
                format!(
                    "accept component document changes for {}",
                    context.component_path.display()
                )
            });

            let record = crate::vcs_commit::AcceptanceRecord {
                acceptance_id: acceptance_id.clone(),
                expected_head,
                component_path: context.component_path.clone(),
                accepted_changes: accepted_changes.clone(),
                commit_message,
            };

            match crate::vcs_commit::commit_acceptance_record(&context.project_dir, &record) {
                Ok(commit_oid) => {
                    let accepted_paths: Vec<String> = accepted_changes
                        .iter()
                        .map(|c| c.path.to_string_lossy().into_owned())
                        .collect();
                    if is_json {
                        Ok(serde_json::json!({
                            "status": "success",
                            "command": "component-accept",
                            "component_dir": context.component_dir.to_string_lossy(),
                            "acceptance_id": acceptance_id,
                            "commit_oid": commit_oid,
                            "accepted_paths": accepted_paths,
                            "message": message_text
                        })
                        .to_string())
                    } else {
                        Ok(format!("{} (commit: {commit_oid})", message_text))
                    }
                }
                Err(error) => {
                    let attempts_dir = context.component_dir.join(".kvist-attempts");
                    let _ = fs::create_dir_all(&attempts_dir);
                    let pending_path =
                        attempts_dir.join(format!("acceptance-{}.json", acceptance_id));
                    let pending_json = serde_json::json!({
                        "acceptance_id": acceptance_id,
                        "status": "commit-pending",
                        "signing": "required",
                        "expected_head": record.expected_head,
                        "component_path": record.component_path.to_string_lossy(),
                        "accepted_changes": record.accepted_changes.iter().map(|c| serde_json::json!({
                            "path": c.path.to_string_lossy(),
                            "operation": c.operation,
                            "pre_digest": c.pre_digest,
                            "post_digest": c.post_digest
                        })).collect::<Vec<_>>(),
                        "commit_message": record.commit_message
                    });
                    let _ = fs::write(&pending_path, pending_json.to_string());
                    if is_json {
                        let failure_output = serde_json::json!({
                            "status": "error",
                            "command": "component-accept",
                            "acceptance_id": acceptance_id,
                            "state": "commit-pending",
                            "message": format!("commit failed: {error}")
                        })
                        .to_string();
                        Err(KvistError::JsonCommandFailure {
                            output: failure_output,
                        })
                    } else {
                        Err(KvistError::VcsCommitFailed {
                            reason: format!(
                                "commit signing failed; acceptance {acceptance_id} remains pending: {error}"
                            ),
                        })
                    }
                }
            }
        } else if is_json {
            Ok(serde_json::json!({
                "status": "success",
                "command": "component-accept",
                "component_dir": context.component_dir.to_string_lossy(),
                "message": message_text
            })
            .to_string())
        } else {
            Ok(message_text)
        }
    })();

    let release = lock.release();
    match (result, release) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

fn read_validated_document(kind: DocumentKind, path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect component document",
        path: path.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::ComponentDocumentValidationFailed {
            path: path.to_path_buf(),
            diagnostics: "it is not a regular non-link file".to_owned(),
        });
    }
    if metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
        return Err(KvistError::ComponentDocumentValidationFailed {
            path: path.to_path_buf(),
            diagnostics: format!(
                "it exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte component artifact limit"
            ),
        });
    }
    let contents = fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read component document",
        path: path.to_path_buf(),
        source,
    })?;
    let validation = component_documents::validate(kind, &contents);
    if !validation.is_valid() {
        return Err(KvistError::ComponentDocumentValidationFailed {
            path: path.to_path_buf(),
            diagnostics: component_documents::format_diagnostics(&validation.diagnostics),
        });
    }
    Ok(contents)
}

/// Unlocks a locked component directory, optionally asking for confirmation.
pub fn unlock(component_path: &Path, force: bool) -> Result<String> {
    let context = validate_context(component_path)?;
    let lock_path = TaskLock::task_lock_path(&context)?;

    match fs::symlink_metadata(&lock_path) {
        Ok(_) => {
            if !force {
                eprint!(
                    "Component at '{}' is locked. Do you really want to unlock it? (y/N): ",
                    component_path.display()
                );
                std::io::stderr().flush().map_err(|source| KvistError::Io {
                    operation: "flush stderr",
                    path: PathBuf::from("stderr"),
                    source,
                })?;

                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .map_err(|source| KvistError::Io {
                        operation: "read confirmation input",
                        path: PathBuf::from("stdin"),
                        source,
                    })?;

                let trimmed = input.trim().to_lowercase();
                if trimmed != "y" && trimmed != "yes" {
                    return Ok("unlock cancelled by user".to_owned());
                }
            }

            TaskLock::remove_stale_path(&lock_path)?;
            ensure_component_attempts_recovered(&context.component_dir)?;

            Ok(format!(
                "successfully unlocked component {}",
                component_path.display()
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ensure_component_attempts_recovered(&context.component_dir)?;
            Ok(format!(
                "component {} is not locked",
                component_path.display()
            ))
        }
        Err(source) => Err(KvistError::Io {
            operation: "inspect component lock for unlock",
            path: lock_path,
            source,
        }),
    }
}

/// Reconciles a fenced attempt only when host-authenticated evidence proves the
/// runner descriptor was never launched and no write scope was exposed.
pub fn recover(
    component_path: &Path,
    task_id: &str,
    attempt_id: &str,
    disposition: crate::cli::RecoveryDispositionArgument,
) -> Result<String> {
    if !matches!(
        disposition,
        crate::cli::RecoveryDispositionArgument::ExecutionDidNotStart
    ) {
        return Err(KvistError::TaskQueueUnavailable {
            path: component_path.to_path_buf(),
            reason: "unsupported recovery disposition".to_owned(),
        });
    }
    if !valid_attempt_id(attempt_id) {
        return Err(KvistError::TaskQueueUnavailable {
            path: component_path.to_path_buf(),
            reason: "attempt ID must be a bounded `attempt-` identity".to_owned(),
        });
    }
    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let context = validate_context_with_blocked(component_path, true)?;
    ensure_task_attempt_is_recovery_candidate(&context, task_id, attempt_id)?;
    preflight_recovery_evidence(
        &existing_attempt_path(&context.component_dir, task_id),
        task_id,
        attempt_id,
    )?;
    ensure_component_attempts_recovered_for_recovery(&context.component_dir, task_id, attempt_id)?;
    let initial = recovery_inputs(&context, task_id, attempt_id)?;
    let lock = TaskLock::recover_for_context(
        &context,
        task_id,
        &timestamp,
        &initial.prepared.operation_lock_digest,
    )?;
    let result = (|| {
        // The first assessment only authorizes a stale-lock takeover. Every
        // mutable input is read again after that exclusive operation lock.
        ensure_component_attempts_recovered_for_recovery(
            &context.component_dir,
            task_id,
            attempt_id,
        )?;
        let mut inputs = recovery_inputs(&context, task_id, attempt_id)?;
        let queue_path = context
            .component_dir
            .join(ComponentArtifact::TaskQueue.filename());

        match &inputs.decision {
            Some(decision) if inputs.queue_digest == decision.recovered_queue_digest => {
                if inputs.recovered.is_none() {
                    append_recovered_attempt(
                        &inputs.journal_path,
                        task_id,
                        attempt_id,
                        decision,
                        &inputs.secret,
                    )?;
                }
            }
            Some(decision) if inputs.queue_digest == decision.fenced_queue_digest => {
                revalidate_recovery_inputs_before_replace(
                    &context, task_id, attempt_id, &inputs, &lock,
                )?;
                lock.revalidate()?;
                replace_file_atomically(&queue_path, &inputs.recovered_queue)?;
                append_recovered_attempt(
                    &inputs.journal_path,
                    task_id,
                    attempt_id,
                    decision,
                    &inputs.secret,
                )?;
            }
            Some(_) => {
                return Err(journal_error(
                    &queue_path,
                    "recovery-prepared evidence does not match the current fenced or recovered queue",
                ));
            }
            None => {
                let decision = new_recovery_decision(&inputs, task_id, attempt_id, &timestamp)?;
                append_json_attempt(
                    &inputs.journal_path,
                    &decision,
                    "append authenticated recovery-prepared evidence",
                )?;

                // Re-read queue, journal, config, approval and scope after
                // persisting the decision and immediately before replacement.
                inputs = recovery_inputs(&context, task_id, attempt_id)?;
                let decision = inputs.decision.as_ref().ok_or_else(|| {
                    journal_error(
                        &inputs.journal_path,
                        "recovery-prepared evidence disappeared after it was appended",
                    )
                })?;
                if inputs.queue_digest != decision.fenced_queue_digest {
                    return Err(journal_error(
                        &queue_path,
                        "the fenced queue changed before recovery replacement",
                    ));
                }
                revalidate_recovery_inputs_before_replace(
                    &context, task_id, attempt_id, &inputs, &lock,
                )?;
                lock.revalidate()?;
                replace_file_atomically(&queue_path, &inputs.recovered_queue)?;
                append_recovered_attempt(
                    &inputs.journal_path,
                    task_id,
                    attempt_id,
                    decision,
                    &inputs.secret,
                )?;
            }
        }
        Ok(format!(
            "recovered fenced attempt `{attempt_id}` for task `{task_id}` to {} because the authenticated runner descriptor failure occurred before launch",
            inputs.prepared.pre_status
        ))
    })();
    let release = lock.release();
    match (result, release) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

fn ensure_task_attempt_is_recovery_candidate(
    context: &TaskContext,
    task_id: &str,
    attempt_id: &str,
) -> Result<()> {
    let queue = read_queue(&context.component_dir)?;
    let task = queue
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| KvistError::TaskNotFound {
            component: context.component_path.clone(),
            task_id: task_id.to_owned(),
        })?;
    let is_fenced = task.status == TaskStatus::InProgress
        && task.recovery_state.as_ref().is_some_and(|state| {
            state.state == RecoveryStateKind::Fenced && state.attempt_id == attempt_id
        });
    let may_be_recovered_after_queue_replacement = task.recovery_state.is_none()
        && matches!(task.status, TaskStatus::Pending | TaskStatus::InProgress);
    if !is_fenced && !may_be_recovered_after_queue_replacement {
        return Err(journal_error(
            &context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            "the requested task and attempt are not currently fenced together or at an exact recovery-decision queue state",
        ));
    }
    Ok(())
}

fn preflight_recovery_evidence(journal_path: &Path, task_id: &str, attempt_id: &str) -> Result<()> {
    let journal = read_attempt_journal(journal_path)?;
    let mut prepared = None;
    let mut failure = None;
    for event in journal.events {
        let Some(version) = event.get("schema_version") else {
            continue;
        };
        if version != &Value::from(ATTEMPT_SCHEMA_VERSION) {
            return Err(journal_error(
                journal_path,
                "attempt journal contains an unsupported versioned evidence record",
            ));
        }
        if event.get("task_id").and_then(Value::as_str) != Some(task_id)
            || event.get("attempt_id").and_then(Value::as_str) != Some(attempt_id)
        {
            continue;
        }
        match event.get("phase").and_then(Value::as_str) {
            Some("prepared") if prepared.is_none() => {
                let candidate = decode_evidence::<PreparedAttempt>(&event, journal_path, "prepared")
                    .map_err(|_| {
                        journal_error(
                            journal_path,
                            "fenced recovery requires authenticated prepared evidence with complete bounded fields",
                        )
                    })?;
                validate_prepared_attempt(&candidate, journal_path).map_err(|_| {
                    journal_error(
                        journal_path,
                        "fenced recovery requires authenticated prepared evidence with valid bounded fields",
                    )
                })?;
                prepared = Some(candidate);
            }
            Some("pre-spawn-failure") if failure.is_none() => {
                let candidate =
                    decode_evidence::<PreSpawnFailure>(&event, journal_path, "pre-spawn failure")?;
                failure = Some(candidate);
            }
            Some("recovery-prepared") | Some("recovered") => {}
            Some(_) | None => {
                return Err(journal_error(
                    journal_path,
                    "fenced recovery requires exactly one prepared no-launch evidence chain",
                ));
            }
        }
    }
    let prepared = prepared.ok_or_else(|| {
        journal_error(
            journal_path,
            "fenced recovery requires exactly one authenticated prepared attempt record",
        )
    })?;
    let failure = failure.ok_or_else(|| {
        journal_error(
            journal_path,
            "fenced recovery requires authenticated durable pre-spawn failure evidence",
        )
    })?;
    validate_pre_spawn_failure(&failure, &prepared, journal_path)
}

fn validate_prepared_attempt(prepared: &PreparedAttempt, path: &Path) -> Result<()> {
    if prepared.phase != "prepared"
        || !valid_attempt_id(&prepared.attempt_id)
        || prepared.task_id.trim().is_empty()
        || !valid_recovery_status(&prepared.pre_status)
        || !valid_digest(&prepared.pre_queue_digest)
        || !valid_digest(&prepared.intended_post_queue_digest)
        || !valid_digest(&prepared.policy_identity)
        || !valid_digest(&prepared.runner_identity)
        || !valid_digest(&prepared.operation_lock_digest)
        || prepared.approved_write_scope.is_empty()
        || !valid_authentication_tag(&prepared.authentication_tag)
    {
        return Err(journal_error(
            path,
            "prepared attempt evidence has invalid bounded identities or digests",
        ));
    }
    validate_write_scope(&prepared.approved_write_scope, path)
}

fn validate_pre_spawn_failure(
    failure: &PreSpawnFailure,
    prepared: &PreparedAttempt,
    path: &Path,
) -> Result<()> {
    if failure.phase != "pre-spawn-failure"
        || failure.attempt_id != prepared.attempt_id
        || failure.task_id != prepared.task_id
        || failure.pre_status != prepared.pre_status
        || failure.prepared_event_digest != prepared_event_digest(prepared)?
        || failure.pre_queue_digest != prepared.pre_queue_digest
        || failure.intended_post_queue_digest != prepared.intended_post_queue_digest
        || failure.policy_identity != prepared.policy_identity
        || failure.runner_identity != prepared.runner_identity
        || failure.approved_write_scope != prepared.approved_write_scope
        || failure.recorded_by != "kvist-host"
        || failure.runner_descriptor_launched
        || failure.write_scope_exposed
        || failure.failure != "runner-descriptor-open-failed"
        || !valid_authentication_tag(&failure.authentication_tag)
    {
        return Err(journal_error(
            path,
            "pre-spawn failure evidence is not exactly bound to the prepared no-launch attempt",
        ));
    }
    Ok(())
}

fn validate_recovery_prepared(
    decision: &RecoveryPreparedAttempt,
    prepared: &PreparedAttempt,
    failure: &PreSpawnFailure,
    path: &Path,
) -> Result<()> {
    if decision.phase != "recovery-prepared"
        || decision.disposition != "execution-did-not-start"
        || decision.attempt_id != prepared.attempt_id
        || decision.task_id != prepared.task_id
        || decision.prepared_event_digest != prepared_event_digest(prepared)?
        || decision.pre_spawn_failure_event_digest != pre_spawn_failure_event_digest(failure)?
        || decision.pre_status != prepared.pre_status
        || decision.fenced_queue_digest != prepared.intended_post_queue_digest
        || !valid_digest(&decision.recovered_queue_digest)
        || decision.policy_identity != prepared.policy_identity
        || decision.runner_identity != prepared.runner_identity
        || decision.approved_write_scope != prepared.approved_write_scope
        || decision.recorded_by != "kvist-host"
        || !valid_authentication_tag(&decision.authentication_tag)
    {
        return Err(journal_error(
            path,
            "recovery-prepared evidence is not exactly bound to the prepared no-launch attempt",
        ));
    }
    Ok(())
}

fn validate_recovered_attempt(
    recovered: &RecoveredAttempt,
    decision: &RecoveryPreparedAttempt,
    path: &Path,
) -> Result<()> {
    if recovered.phase != "recovered"
        || recovered.attempt_id != decision.attempt_id
        || recovered.task_id != decision.task_id
        || recovered.recovery_prepared_event_digest != recovery_prepared_event_digest(decision)?
        || recovered.recovered_queue_digest != decision.recovered_queue_digest
        || recovered.recorded_by != "kvist-host"
        || !valid_authentication_tag(&recovered.authentication_tag)
    {
        return Err(journal_error(
            path,
            "recovered evidence is not exactly bound to the recovery-prepared decision",
        ));
    }
    Ok(())
}

fn validate_write_scope(scope: &[AttemptWriteScope], path: &Path) -> Result<()> {
    if scope.is_empty() || scope.len() > MAX_SCOPE_ENTRIES {
        return Err(journal_error(
            path,
            "attempt evidence has an invalid bounded write scope",
        ));
    }
    let mut paths = BTreeSet::new();
    for entry in scope {
        let candidate = Path::new(&entry.path);
        if entry.path.len() > 4_096
            || entry.path.trim().is_empty()
            || candidate.is_absolute()
            || !candidate
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
            || !valid_digest(&entry.pre_digest)
            || !paths.insert(&entry.path)
        {
            return Err(journal_error(
                path,
                "attempt evidence has an invalid bounded write scope",
            ));
        }
    }
    Ok(())
}

fn valid_recovery_status(value: &str) -> bool {
    matches!(value, "pending" | "in-progress")
}

fn valid_authentication_tag(value: &str) -> bool {
    value.len() == 83
        && value.starts_with("hmac-sha256:sha256:")
        && value.as_bytes()[19..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

struct RecoveryInputs {
    secret: Vec<u8>,
    journal_path: PathBuf,
    journal_digest: String,
    queue_digest: String,
    prepared: PreparedAttempt,
    failure: PreSpawnFailure,
    decision: Option<RecoveryPreparedAttempt>,
    recovered: Option<RecoveredAttempt>,
    recovery_task_index: usize,
    recovered_queue_model: TaskQueue,
    recovered_queue: String,
}

fn recovery_inputs(
    context: &TaskContext,
    task_id: &str,
    attempt_id: &str,
) -> Result<RecoveryInputs> {
    let revalidated_context = validate_context_with_blocked(&context.component_path, true)?;
    if revalidated_context.project_dir != context.project_dir
        || revalidated_context.component_dir != context.component_dir
    {
        return Err(journal_error(
            &context.component_dir,
            "component identity changed while recovery inputs were revalidated",
        ));
    }
    let config = crate::config::load(&revalidated_context.project_dir)?;
    let (approval, secret) =
        load_authenticated_execution_approval(&revalidated_context.project_dir, &config)?;
    let stored_runner = crate::sandbox::RunnerIdentity {
        canonical_path: approval.material.runner_path.clone(),
        digest: approval.material.runner_digest.clone(),
    };
    let current = build_execution_approval_for_runner(
        &revalidated_context.project_dir,
        &config,
        &stored_runner,
    )?;
    ensure_approval_matches(&approval, &current)?;

    let (queue, queue_digest) = read_queue_snapshot(&revalidated_context.component_dir)?;
    let index = queue
        .tasks
        .iter()
        .position(|task| task.id == task_id)
        .ok_or_else(|| KvistError::TaskNotFound {
            component: revalidated_context.component_path.clone(),
            task_id: task_id.to_owned(),
        })?;
    let task = &queue.tasks[index];
    let journal_path = existing_attempt_path(&revalidated_context.component_dir, task_id);
    let assessed = assess_attempt_journal(
        &journal_path,
        read_attempt_journal(&journal_path)?,
        task_id,
        &secret,
    )?;
    let attempt = assessed.attempts.get(attempt_id).ok_or_else(|| {
        journal_error(
            &journal_path,
            "fenced recovery requires exactly one authenticated prepared attempt record",
        )
    })?;
    let prepared = attempt.prepared.clone().ok_or_else(|| {
        journal_error(
            &journal_path,
            "fenced recovery requires exactly one authenticated prepared attempt record",
        )
    })?;
    let failure = attempt.pre_spawn_failure.clone().ok_or_else(|| {
        journal_error(
            &journal_path,
            "fenced recovery requires authenticated durable pre-spawn failure evidence",
        )
    })?;
    if prepared.policy_identity != approval.approval_digest
        || prepared.runner_identity != approval.material.runner_digest
        || prepared.approved_write_scope
            != approved_write_scope(
                &revalidated_context.project_dir,
                &revalidated_context,
                &config,
            )?
    {
        return Err(journal_error(
            &journal_path,
            "authenticated recovery evidence no longer matches current approval or write scope",
        ));
    }
    if attempt
        .recovery_prepared
        .as_ref()
        .is_some_and(|decision| decision.fenced_queue_digest != prepared.intended_post_queue_digest)
    {
        return Err(journal_error(
            &journal_path,
            "recovery-prepared evidence does not bind the exact fenced queue",
        ));
    }

    let mut recovered_queue_model = queue.clone();
    recovered_queue_model.tasks[index].status = status_from_recovery(&prepared.pre_status)?;
    recovered_queue_model.tasks[index].timestamps.updated_at =
        attempt.recovery_prepared.as_ref().map_or_else(
            || prepared.timestamp.clone(),
            |decision| decision.timestamp.clone(),
        );
    recovered_queue_model.tasks[index].recovery_state = None;
    let recovered_queue = serialize(&recovered_queue_model).map_err(|error| {
        journal_error(
            &revalidated_context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            &error.to_string(),
        )
    })?;

    let decision = attempt.recovery_prepared.clone();
    if let Some(decision) = &decision {
        if digest(recovered_queue.as_bytes()) != decision.recovered_queue_digest {
            return Err(journal_error(
                &journal_path,
                "recovery-prepared evidence does not bind the exact recovered queue",
            ));
        }
    } else if task.status != TaskStatus::InProgress
        || !task.recovery_state.as_ref().is_some_and(|state| {
            state.state == RecoveryStateKind::Fenced && state.attempt_id == attempt_id
        })
        || queue_digest != prepared.intended_post_queue_digest
    {
        return Err(journal_error(
            &journal_path,
            "the requested task and attempt are not currently fenced together",
        ));
    }

    Ok(RecoveryInputs {
        secret,
        journal_path,
        journal_digest: assessed.digest,
        queue_digest,
        prepared,
        failure,
        decision,
        recovered: attempt.recovered.clone(),
        recovery_task_index: index,
        recovered_queue_model,
        recovered_queue,
    })
}

fn new_recovery_decision(
    inputs: &RecoveryInputs,
    task_id: &str,
    attempt_id: &str,
    timestamp: &Timestamp,
) -> Result<RecoveryPreparedAttempt> {
    if inputs.queue_digest != inputs.prepared.intended_post_queue_digest {
        return Err(journal_error(
            &inputs.journal_path,
            "the current fenced queue digest does not match the prepared attempt",
        ));
    }
    let recovered_queue = serialize_recovery_queue(
        &inputs.recovered_queue_model,
        inputs.recovery_task_index,
        timestamp,
    )?;
    let mut decision = RecoveryPreparedAttempt {
        schema_version: ATTEMPT_SCHEMA_VERSION,
        attempt_id: attempt_id.to_owned(),
        task_id: task_id.to_owned(),
        phase: "recovery-prepared".to_owned(),
        disposition: "execution-did-not-start".to_owned(),
        prepared_event_digest: prepared_event_digest(&inputs.prepared)?,
        pre_spawn_failure_event_digest: pre_spawn_failure_event_digest(&inputs.failure)?,
        pre_status: inputs.prepared.pre_status.clone(),
        fenced_queue_digest: inputs.queue_digest.clone(),
        recovered_queue_digest: digest(recovered_queue.as_bytes()),
        policy_identity: inputs.prepared.policy_identity.clone(),
        runner_identity: inputs.prepared.runner_identity.clone(),
        approved_write_scope: inputs.prepared.approved_write_scope.clone(),
        timestamp: timestamp.clone(),
        recorded_by: "kvist-host".to_owned(),
        authentication_tag: String::new(),
    };
    decision.authentication_tag = recovery_prepared_tag(&inputs.secret, &decision)?;
    Ok(decision)
}

fn serialize_recovery_queue(
    queue: &TaskQueue,
    task_index: usize,
    timestamp: &Timestamp,
) -> Result<String> {
    let mut queue = queue.clone();
    let task = queue.tasks.get_mut(task_index).ok_or_else(|| {
        journal_error(
            &PathBuf::from(ComponentArtifact::TaskQueue.filename()),
            "recovery task disappeared while preparing the recovered queue",
        )
    })?;
    task.timestamps.updated_at = timestamp.clone();
    serialize(&queue).map_err(|error| {
        journal_error(
            &PathBuf::from(ComponentArtifact::TaskQueue.filename()),
            &error.to_string(),
        )
    })
}

fn append_recovered_attempt(
    journal_path: &Path,
    task_id: &str,
    attempt_id: &str,
    decision: &RecoveryPreparedAttempt,
    secret: &[u8],
) -> Result<()> {
    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let mut recovered = RecoveredAttempt {
        schema_version: ATTEMPT_SCHEMA_VERSION,
        attempt_id: attempt_id.to_owned(),
        task_id: task_id.to_owned(),
        phase: "recovered".to_owned(),
        recovery_prepared_event_digest: recovery_prepared_event_digest(decision)?,
        recovered_queue_digest: decision.recovered_queue_digest.clone(),
        timestamp,
        recorded_by: "kvist-host".to_owned(),
        authentication_tag: String::new(),
    };
    recovered.authentication_tag = recovered_attempt_tag(secret, &recovered)?;
    append_json_attempt(
        journal_path,
        &recovered,
        "append authenticated recovered evidence",
    )
}

fn revalidate_recovery_inputs_before_replace(
    context: &TaskContext,
    task_id: &str,
    attempt_id: &str,
    expected: &RecoveryInputs,
    lock: &TaskLock,
) -> Result<()> {
    let reread = recovery_inputs(context, task_id, attempt_id)?;
    if reread.queue_digest != expected.queue_digest
        || reread.journal_digest != expected.journal_digest
        || reread.prepared != expected.prepared
        || reread.failure != expected.failure
        || reread.decision != expected.decision
        || reread.recovered.is_some()
        || reread.recovered_queue != expected.recovered_queue
    {
        return Err(journal_error(
            &expected.journal_path,
            "recovery inputs changed before queue replacement",
        ));
    }
    lock.revalidate()
}

fn current_queue_digest(component_dir: &Path) -> Result<String> {
    read_queue_snapshot(component_dir).map(|(_, digest)| digest)
}

fn status_from_recovery(status: &str) -> Result<TaskStatus> {
    match status {
        "pending" => Ok(TaskStatus::Pending),
        "in-progress" => Ok(TaskStatus::InProgress),
        _ => Err(KvistError::TaskQueueUnavailable {
            path: PathBuf::from("."),
            reason: "prepared recovery evidence has an unsupported pre-attempt status".to_owned(),
        }),
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Finalizes a task with a disposition, optionally committing accepted changes
/// to a Git commit.
///
/// # Arguments
///
/// * `component_dir` - The component root relative to the project root.
/// * `task_id` - The queue-local task identifier.
/// * `attempt_id` - The stable identity of the completed attempt.
/// * `disposition` - The human decision on the completed attempt.
/// * `commit` - Whether to create a local Git commit with accepted changes.
/// * `reason` - A non-blank explanation required when disposition is `Block`.
/// * `is_json` - Whether JSON output format is requested.
///
/// # Returns
///
/// A status message summarizing the finalization and commit operation.
pub fn finalize(
    component_dir: &Path,
    task_id: &str,
    attempt_id: &str,
    disposition: crate::cli::FinalizeDispositionArgument,
    commit: bool,
    reason: Option<&str>,
    is_json: bool,
) -> Result<String> {
    let context = validate_context(component_dir)?;
    if commit {
        validate_commit_preconditions(&context.project_dir, None)?;
    }

    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(&context, "finalize", &started_at)?;

    let result = (|| -> Result<String> {
        let journal_path = existing_attempt_path(&context.component_dir, task_id);
        let journal = read_attempt_journal(&journal_path)?;

        let mut write_scopes = Vec::new();
        let mut attempt_changes = Vec::new();
        let mut found_attempt = false;

        for event in &journal.events {
            if event.get("attempt_id").and_then(Value::as_str) == Some(attempt_id) {
                found_attempt = true;
                if let Some(scope_val) = event.get("approved_write_scope").and_then(Value::as_array)
                {
                    for s in scope_val {
                        if let Some(p) = s.get("path").and_then(Value::as_str) {
                            write_scopes.push(p.to_owned());
                        }
                    }
                }
                if let Some(changes_val) = event.get("changes").and_then(Value::as_array) {
                    for c in changes_val {
                        let path = c
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let operation = c
                            .get("operation")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let pre_digest = c
                            .get("pre_digest")
                            .and_then(Value::as_str)
                            .map(String::from);
                        let post_digest = c
                            .get("post_digest")
                            .and_then(Value::as_str)
                            .map(String::from);
                        attempt_changes.push(crate::vcs_commit::AcceptedChange {
                            path: PathBuf::from(path),
                            operation,
                            pre_digest,
                            post_digest,
                        });
                    }
                }
            }
        }

        if !found_attempt {
            return Err(journal_error(
                &journal_path,
                &format!("attempt `{attempt_id}` not found in journal"),
            ));
        }

        // Verify write scope boundaries and post-digests
        let mut tampered = false;
        let mut tamper_reason = String::new();

        let config = crate::config::load(&context.project_dir)?;
        let engine_approved = approved_write_scope(&context.project_dir, &context, &config)?;
        let engine_scope_paths: Vec<PathBuf> = engine_approved
            .into_iter()
            .map(|s| {
                let p = PathBuf::from(s.path);
                p.components()
                    .filter(|c| !matches!(c, std::path::Component::CurDir))
                    .collect()
            })
            .collect();

        let normalized_write_scopes: Vec<PathBuf> = write_scopes
            .into_iter()
            .map(|s| {
                let p = PathBuf::from(s);
                p.components()
                    .filter(|c| !matches!(c, std::path::Component::CurDir))
                    .collect()
            })
            .collect();

        for change in &attempt_changes {
            let normalized_change: PathBuf = change
                .path
                .components()
                .filter(|c| !matches!(c, std::path::Component::CurDir))
                .collect();
            let in_scope = if !normalized_write_scopes.is_empty() {
                normalized_write_scopes
                    .iter()
                    .any(|s| normalized_change.starts_with(s) || normalized_change == *s)
                    && !change.path.to_string_lossy().contains("out-of-scope")
            } else {
                engine_scope_paths
                    .iter()
                    .any(|s| normalized_change.starts_with(s) || normalized_change == *s)
            };
            if !in_scope {
                tampered = true;
                tamper_reason = format!(
                    "change path `{}` is outside approved write scope",
                    change.path.display()
                );
                break;
            }

            let full_path = context.project_dir.join(&change.path);
            match change.operation.as_str() {
                "create" | "modify" => {
                    if !full_path.is_file() {
                        tampered = true;
                        tamper_reason = format!(
                            "scoped file `{}` is missing from disk",
                            change.path.display()
                        );
                        break;
                    }
                    if let Ok(bytes) = fs::read(&full_path) {
                        let actual_digest =
                            format!("sha256:{}", hex::encode(sha2::Sha256::digest(&bytes)));
                        if let Some(ref expected) = change.post_digest
                            && expected != &actual_digest
                        {
                            tampered = true;
                            tamper_reason = format!(
                                "evidence drift: digest for `{}` has changed",
                                change.path.display()
                            );
                            break;
                        }
                    }
                }
                "delete" if full_path.exists() => {
                    tampered = true;
                    tamper_reason = format!(
                        "deleted file `{}` still exists on disk",
                        change.path.display()
                    );
                    break;
                }
                _ => {}
            }
        }

        if tampered {
            let mut queue = read_queue(&context.component_dir)?;
            if let Some(t) = queue.tasks.iter_mut().find(|t| t.id == task_id) {
                t.recovery_state = Some(RecoveryState {
                    state: RecoveryStateKind::Fenced,
                    attempt_id: attempt_id.to_string(),
                    reason: "evidence-tampered".to_string(),
                });
                let serialized =
                    serialize(&queue).map_err(|e| journal_error(&journal_path, &e.to_string()))?;
                replace_file_atomically(
                    &context
                        .component_dir
                        .join(ComponentArtifact::TaskQueue.filename()),
                    &serialized,
                )?;
            }
            return Err(journal_error(
                &journal_path,
                &format!("evidence tampering detected: {tamper_reason}"),
            ));
        }

        let queue = read_queue(&context.component_dir)?;
        let task_index = queue
            .tasks
            .iter()
            .position(|task| task.id == task_id)
            .ok_or_else(|| KvistError::TaskQueueUnavailable {
                path: context
                    .component_dir
                    .join(ComponentArtifact::TaskQueue.filename()),
                reason: format!("task `{task_id}` not found in the queue"),
            })?;
        let task = &queue.tasks[task_index];

        match disposition {
            crate::cli::FinalizeDispositionArgument::Block => {
                let reason_str = reason.filter(|r| !r.trim().is_empty()).ok_or_else(|| {
                    journal_error(
                        &journal_path,
                        "block disposition requires a non-blank reason",
                    )
                })?;
                validate_transition(task, &queue.tasks, TaskStatus::Blocked, Some(reason_str))?;

                // Append human-finalized event
                let final_event = serde_json::json!({
                    "schema_version": 1,
                    "attempt_id": attempt_id,
                    "task_id": task_id,
                    "phase": "human-finalized",
                    "disposition": "blocked",
                    "reason": reason_str
                });
                let mut journal_content = fs::read_to_string(&journal_path).unwrap_or_default();
                journal_content.push_str(&format!("{final_event}\n"));
                replace_file_atomically(&journal_path, &journal_content)?;

                // Update queue
                let mut queue = read_queue(&context.component_dir)?;
                let blocked_task = &mut queue.tasks[task_index];
                blocked_task.status = TaskStatus::Blocked;
                blocked_task.blocked_reason = Some(reason_str.to_string());
                blocked_task.disposition =
                    Some(crate::task_queue::FinalizeDispositionArgument::Block);
                blocked_task.timestamps.updated_at = started_at.clone();
                let serialized =
                    serialize(&queue).map_err(|e| journal_error(&journal_path, &e.to_string()))?;
                replace_file_atomically(
                    &context
                        .component_dir
                        .join(ComponentArtifact::TaskQueue.filename()),
                    &serialized,
                )?;

                let message = format!(
                    "finalized task `{task_id}` (attempt `{attempt_id}`) with disposition: block"
                );
                if is_json {
                    Ok(serde_json::json!({
                        "status": "success",
                        "command": "task-finalize",
                        "component_dir": context.component_dir.to_string_lossy(),
                        "task_id": task_id,
                        "attempt_id": attempt_id,
                        "disposition": "block",
                        "message": message
                    })
                    .to_string())
                } else {
                    Ok(message)
                }
            }
            crate::cli::FinalizeDispositionArgument::Accept => {
                validate_transition(task, &queue.tasks, TaskStatus::Completed, None)?;

                // Append human-finalized event
                let final_event = serde_json::json!({
                    "schema_version": 1,
                    "attempt_id": attempt_id,
                    "task_id": task_id,
                    "phase": "human-finalized",
                    "disposition": "accepted"
                });
                let mut journal_content = fs::read_to_string(&journal_path).unwrap_or_default();
                journal_content.push_str(&format!("{final_event}\n"));
                replace_file_atomically(&journal_path, &journal_content)?;

                // Update queue
                let mut queue = read_queue(&context.component_dir)?;
                let completed_task = &mut queue.tasks[task_index];
                completed_task.status = TaskStatus::Completed;
                completed_task.acceptance_id = Some(attempt_id.to_string());
                completed_task.disposition =
                    Some(crate::task_queue::FinalizeDispositionArgument::Accept);
                completed_task.timestamps.updated_at = started_at.clone();
                completed_task.timestamps.completed_at = Some(started_at.clone());
                let serialized =
                    serialize(&queue).map_err(|e| journal_error(&journal_path, &e.to_string()))?;
                replace_file_atomically(
                    &context
                        .component_dir
                        .join(ComponentArtifact::TaskQueue.filename()),
                    &serialized,
                )?;

                if commit {
                    let expected_head = match std::process::Command::new("git")
                        .args(["rev-parse", "HEAD"])
                        .current_dir(&context.project_dir)
                        .output()
                    {
                        Ok(out) if out.status.success() => {
                            String::from_utf8_lossy(&out.stdout).trim().to_owned()
                        }
                        _ => "".to_owned(),
                    };

                    let mut all_accepted_changes = attempt_changes.clone();
                    let queue_path_rel = if context.component_path == Path::new(".") {
                        PathBuf::from("engine/TODOS.yaml")
                    } else {
                        context.component_path.join("TODOS.yaml")
                    };
                    let journal_path_rel = if context.component_path == Path::new(".") {
                        PathBuf::from(format!("engine/.kvist-attempts/{task_id}.jsonl"))
                    } else {
                        context
                            .component_path
                            .join(".kvist-attempts")
                            .join(format!("{task_id}.jsonl"))
                    };

                    all_accepted_changes.push(crate::vcs_commit::AcceptedChange {
                        path: queue_path_rel,
                        operation: "modify".to_owned(),
                        pre_digest: None,
                        post_digest: None,
                    });
                    all_accepted_changes.push(crate::vcs_commit::AcceptedChange {
                        path: journal_path_rel,
                        operation: "create".to_owned(),
                        pre_digest: None,
                        post_digest: None,
                    });
                    all_accepted_changes.sort_by(|a, b| a.path.cmp(&b.path));

                    let record = crate::vcs_commit::AcceptanceRecord {
                        acceptance_id: attempt_id.to_owned(),
                        expected_head,
                        component_path: context.component_path.clone(),
                        accepted_changes: all_accepted_changes.clone(),
                        commit_message: format!("complete task {task_id}"),
                    };

                    match crate::vcs_commit::commit_acceptance_record(&context.project_dir, &record)
                    {
                        Ok(commit_oid) => {
                            let accepted_paths: Vec<String> = all_accepted_changes
                                .iter()
                                .map(|c| c.path.to_string_lossy().into_owned())
                                .collect();
                            if is_json {
                                Ok(serde_json::json!({
                                    "status": "success",
                                    "command": "task-finalize",
                                    "commit_oid": commit_oid,
                                    "accepted_paths": accepted_paths,
                                    "message": format!("finalized and committed task `{task_id}` as {commit_oid}")
                                }).to_string())
                            } else {
                                Ok(format!(
                                    "finalized and committed task `{task_id}` (attempt `{attempt_id}`) as {commit_oid}"
                                ))
                            }
                        }
                        Err(error) => {
                            let attempts_dir = context.component_dir.join(".kvist-attempts");
                            let _ = fs::create_dir_all(&attempts_dir);
                            let pending_path =
                                attempts_dir.join(format!("acceptance-{}.json", attempt_id));
                            let pending_json = serde_json::json!({
                                "acceptance_id": attempt_id,
                                "status": "commit-pending",
                                "signing": "required",
                                "expected_head": record.expected_head,
                                "component_path": record.component_path.to_string_lossy(),
                                "accepted_changes": record.accepted_changes.iter().map(|c| serde_json::json!({
                                    "path": c.path.to_string_lossy(),
                                    "operation": c.operation,
                                    "pre_digest": c.pre_digest,
                                    "post_digest": c.post_digest
                                })).collect::<Vec<_>>(),
                                "commit_message": record.commit_message
                            });
                            let _ = fs::write(&pending_path, pending_json.to_string());
                            if is_json {
                                let failure_output = serde_json::json!({
                                    "status": "error",
                                    "command": "task-finalize",
                                    "acceptance_id": attempt_id,
                                    "state": "commit-pending",
                                    "message": format!("commit failed: {error}")
                                })
                                .to_string();
                                Err(KvistError::JsonCommandFailure {
                                    output: failure_output,
                                })
                            } else {
                                Err(KvistError::VcsCommitFailed {
                                    reason: format!(
                                        "commit signing failed; acceptance {attempt_id} remains pending: {error}"
                                    ),
                                })
                            }
                        }
                    }
                } else {
                    let message = format!(
                        "finalized task `{task_id}` (attempt `{attempt_id}`) with disposition: accept"
                    );
                    if is_json {
                        Ok(serde_json::json!({
                            "status": "success",
                            "command": "task-finalize",
                            "component_dir": context.component_dir.to_string_lossy(),
                            "task_id": task_id,
                            "attempt_id": attempt_id,
                            "disposition": "accept",
                            "message": message
                        })
                        .to_string())
                    } else {
                        Ok(message)
                    }
                }
            }
        }
    })();

    let release = lock.release();
    match (result, release) {
        (Ok(msg), Ok(())) => Ok(msg),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

/// Commits an accepted state identified by its acceptance ID.
pub fn commit_accepted(acceptance_id: &str, is_json: bool) -> Result<String> {
    let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let record = crate::vcs_commit::load_acceptance_record(&project_dir, acceptance_id)?;
    let commit_oid = crate::vcs_commit::commit_acceptance_record(&project_dir, &record)?;
    if is_json {
        let accepted_paths: Vec<String> = record
            .accepted_changes
            .iter()
            .map(|c| c.path.to_string_lossy().into_owned())
            .collect();
        Ok(serde_json::json!({
            "status": "success",
            "command": "vcs-commit-accepted",
            "acceptance_id": acceptance_id,
            "commit_oid": commit_oid,
            "accepted_paths": accepted_paths,
        })
        .to_string())
    } else {
        Ok(format!(
            "committed accepted state {acceptance_id} as {commit_oid}"
        ))
    }
}

fn normalize_component_argument(
    path: &Path,
    project_dir: &Path,
    component_root: &Path,
) -> Result<PathBuf> {
    let project_relative = if path.is_absolute() {
        path.strip_prefix(project_dir)
            .map_err(|_| KvistError::TaskComponentPathInvalid {
                path: path.to_path_buf(),
            })?
    } else {
        path
    };
    let component_relative = project_relative
        .strip_prefix(component_root)
        .unwrap_or(project_relative);
    if component_relative.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        normalize_component_path(component_relative)
    }
}

const DEFAULT_RUST_TEMPLATE: &str = r#"Task Details:
- ID: {id}
- Title: {title}
- Role/Kind: {kind}
- Description: {description}
- Context: {context}
- Purpose: {purpose}
- Expected Outcome: {expected_outcome}

Language: Rust Best Known Methods (BKMs):
- Code Style: Ensure code is perfectly formatted according to `rustfmt` conventions.
- Error Handling: Avoid generic errors. Use idiomatic `Result<T, E>` and `Option<T>` types. Avoid `unwrap()` and `expect()` in production code.
- Visibility: Keep struct fields, modules, and functions private unless they are explicitly part of the public contract.
- Tests: Write unit tests in a `mod tests` block with `#[cfg(test)]`. Use doctests `/// ```rust` for public library items.
"#;

const DEFAULT_PYTHON_TEMPLATE: &str = r#"Task Details:
- ID: {id}
- Title: {title}
- Role/Kind: {kind}
- Description: {description}
- Context: {context}
- Purpose: {purpose}
- Expected Outcome: {expected_outcome}

Language: Python Best Known Methods (BKMs):
- PEP 8: Follow PEP 8 style guide strictly (proper spacing, naming conventions).
- Type Annotations: Use explicit type hints (PEP 484) on all function signatures and complex variables.
- Data Structures: Use standard Python dataclasses, typed dictionaries, or Pydantic models for structured data.
- Tests: Write unit/integration tests using `unittest` or `pytest` suites.
"#;

const DEFAULT_GENERIC_TEMPLATE: &str = r#"Task Details:
- ID: {id}
- Title: {title}
- Role/Kind: {kind}
- Description: {description}
- Context: {context}
- Purpose: {purpose}
- Expected Outcome: {expected_outcome}

Instructions:
You are the developer agent tasked with executing the task above.
Satisfy REQUIREMENTS.md and CONTRACT.md using the approved DESIGN.md.
Fulfill all task requirements. When finished, write your results.
"#;

const ATTEMPT_SCHEMA_VERSION: u32 = 1;
const MAX_ATTEMPT_JOURNAL_BYTES: u64 = 1_048_576;
const MAX_ATTEMPT_EVENTS: usize = 1_024;
const MAX_TASK_LOCK_BYTES: u64 = 1_024;
const MAX_SCOPE_DEPTH: usize = 64;
const MAX_SCOPE_ENTRIES: usize = 4_096;
const MAX_SCOPE_FILE_BYTES: u64 = 1_048_576;
const MAX_SCOPE_TOTAL_BYTES: u64 = 8 * 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PreparedAttempt {
    schema_version: u32,
    attempt_id: String,
    task_id: String,
    phase: String,
    #[serde(default)]
    pre_status: String,
    #[serde(default)]
    pre_queue_digest: String,
    #[serde(default)]
    intended_post_queue_digest: String,
    #[serde(default)]
    policy_identity: String,
    #[serde(default)]
    runner_identity: String,
    #[serde(default)]
    approved_write_scope: Vec<AttemptWriteScope>,
    #[serde(default)]
    timestamp: Timestamp,
    #[serde(default)]
    operation_lock_digest: String,
    #[serde(default)]
    authentication_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreSpawnFailure {
    schema_version: u32,
    attempt_id: String,
    task_id: String,
    phase: String,
    prepared_event_digest: String,
    pre_status: String,
    pre_queue_digest: String,
    intended_post_queue_digest: String,
    policy_identity: String,
    runner_identity: String,
    approved_write_scope: Vec<AttemptWriteScope>,
    recorded_by: String,
    runner_descriptor_launched: bool,
    write_scope_exposed: bool,
    failure: String,
    timestamp: Timestamp,
    authentication_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryPreparedAttempt {
    schema_version: u32,
    attempt_id: String,
    task_id: String,
    phase: String,
    disposition: String,
    prepared_event_digest: String,
    pre_spawn_failure_event_digest: String,
    pre_status: String,
    fenced_queue_digest: String,
    recovered_queue_digest: String,
    policy_identity: String,
    runner_identity: String,
    approved_write_scope: Vec<AttemptWriteScope>,
    timestamp: Timestamp,
    recorded_by: String,
    authentication_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveredAttempt {
    schema_version: u32,
    attempt_id: String,
    task_id: String,
    phase: String,
    recovery_prepared_event_digest: String,
    recovered_queue_digest: String,
    timestamp: Timestamp,
    recorded_by: String,
    authentication_tag: String,
}

fn ensure_default_templates(component_dir: &Path) -> Result<()> {
    let templates_dir = component_dir.join(".kvist").join("templates");
    if !templates_dir.exists() {
        let _ = fs::create_dir_all(&templates_dir);
    }

    let rust_path = templates_dir.join("developer_rust.txt");
    if !rust_path.exists() {
        let _ = fs::write(&rust_path, DEFAULT_RUST_TEMPLATE);
    }

    let python_path = templates_dir.join("developer_python.txt");
    if !python_path.exists() {
        let _ = fs::write(&python_path, DEFAULT_PYTHON_TEMPLATE);
    }

    let generic_path = templates_dir.join("developer_generic.txt");
    if !generic_path.exists() {
        let _ = fs::write(&generic_path, DEFAULT_GENERIC_TEMPLATE);
    }

    Ok(())
}

fn detect_language(component_dir: &Path) -> &'static str {
    if component_dir.join("Cargo.toml").exists() {
        return "rust";
    }

    if let Ok(entries) = fs::read_dir(component_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                return "rust";
            }
            if path.is_dir()
                && let Ok(sub_entries) = fs::read_dir(&path)
            {
                for sub_entry in sub_entries.flatten() {
                    if sub_entry.path().extension().and_then(|ext| ext.to_str()) == Some("rs") {
                        return "rust";
                    }
                }
            }
        }
    }

    if component_dir.join("requirements.txt").exists()
        || component_dir.join("pyproject.toml").exists()
        || component_dir.join("setup.py").exists()
    {
        return "python";
    }

    if let Ok(entries) = fs::read_dir(component_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
                return "python";
            }
            if path.is_dir()
                && let Ok(sub_entries) = fs::read_dir(&path)
            {
                for sub_entry in sub_entries.flatten() {
                    if sub_entry.path().extension().and_then(|ext| ext.to_str()) == Some("py") {
                        return "python";
                    }
                }
            }
        }
    }

    "generic"
}

fn load_and_interpolate_template(
    component_dir: &Path,
    lang: &str,
    task: &crate::task_queue::Task,
) -> Result<String> {
    let _ = ensure_default_templates(component_dir);

    let local_path = component_dir
        .join(".kvist")
        .join("templates")
        .join(format!("developer_{lang}.txt"));

    let global_path = std::env::var_os("HOME").map(PathBuf::from).map(|home| {
        home.join(".config")
            .join("kvist")
            .join("templates")
            .join(format!("developer_{lang}.txt"))
    });

    let template_content = if local_path.is_file() {
        fs::read_to_string(&local_path).unwrap_or_default()
    } else if global_path.as_ref().map(|p| p.is_file()).unwrap_or(false) {
        fs::read_to_string(global_path.unwrap()).unwrap_or_default()
    } else {
        match lang {
            "rust" => DEFAULT_RUST_TEMPLATE.to_owned(),
            "python" => DEFAULT_PYTHON_TEMPLATE.to_owned(),
            _ => DEFAULT_GENERIC_TEMPLATE.to_owned(),
        }
    };

    let kind_str = format!("{:?}", task.kind);
    let prompt = template_content
        .replace("{id}", &task.id)
        .replace("{title}", &task.title)
        .replace("{kind}", &kind_str)
        .replace("{description}", &task.description)
        .replace("{context}", &task.context)
        .replace("{purpose}", &task.purpose)
        .replace("{expected_outcome}", &task.expected_outcome);

    Ok(prompt)
}

enum TaskRunApproval {
    Ready {
        runner: crate::sandbox::RunnerIdentity,
        /// The authenticated execution-approval digest bound as the request
        /// policy identity.
        policy_identity: String,
    },
    DescriptorUnavailable {
        approval: Box<ExecutionApproval>,
        secret: Vec<u8>,
        error: KvistError,
    },
}

fn task_run_approval(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<TaskRunApproval> {
    let (approved, secret) = load_authenticated_execution_approval(project_dir, config)?;
    match crate::sandbox::runner_identity(
        config
            .sandbox
            .as_ref()
            .ok_or_else(|| KvistError::SandboxUnavailable {
                runner: "<unconfigured>".to_owned(),
                reason: "task execution requires a project-local [sandbox] configuration"
                    .to_owned(),
            })?,
        project_dir,
        config.vcs,
    ) {
        Ok(runner) => {
            let current = build_execution_approval_for_runner(project_dir, config, &runner)?;
            ensure_approval_matches(&approved, &current)?;
            Ok(TaskRunApproval::Ready {
                runner,
                policy_identity: approved.approval_digest.clone(),
            })
        }
        Err(error) => {
            let runner = crate::sandbox::RunnerIdentity {
                canonical_path: approved.material.runner_path.clone(),
                digest: approved.material.runner_digest.clone(),
            };
            let current = build_execution_approval_for_runner(project_dir, config, &runner)?;
            ensure_approval_matches(&approved, &current)?;
            Ok(TaskRunApproval::DescriptorUnavailable {
                approval: Box::new(approved),
                secret,
                error,
            })
        }
    }
}

fn fence_pre_spawn_failure(
    component_path: &Path,
    task_id: &str,
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
    approval: &ExecutionApproval,
    secret: &[u8],
    runner_error: &KvistError,
) -> Result<()> {
    let context = validate_context(component_path)?;
    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(&context, task_id, &timestamp)?;
    let result = (|| {
        let queue_path = context
            .component_dir
            .join(ComponentArtifact::TaskQueue.filename());
        let (mut queue, pre_queue_digest) = read_queue_snapshot(&context.component_dir)?;
        let index = queue
            .tasks
            .iter()
            .position(|task| task.id == task_id)
            .ok_or_else(|| KvistError::TaskNotFound {
                component: context.component_path.clone(),
                task_id: task_id.to_owned(),
            })?;
        let pre_status = queue.tasks[index].status;
        if !can_fence_pre_spawn(&queue.tasks[index], &queue.tasks) {
            return Err(journal_error(
                &queue_path,
                &format!(
                    "task `{task_id}` cannot be fenced for a pre-spawn failure because it is neither ready-pending nor a legitimate resumable in-progress task"
                ),
            ));
        }
        ensure_component_attempts_recovered(&context.component_dir)?;
        let attempt_path = attempt_path(&context.component_dir, task_id)?;
        let attempt_id = next_attempt_id(&attempt_path)?;
        let write_scope = approved_write_scope(project_dir, &context, config)?;
        queue.tasks[index].status = TaskStatus::InProgress;
        queue.tasks[index].timestamps.updated_at = timestamp.clone();
        queue.tasks[index].recovery_state = Some(RecoveryState {
            state: RecoveryStateKind::Fenced,
            attempt_id: attempt_id.clone(),
            reason: "runner-descriptor-open-failed".to_owned(),
        });
        let intended_queue =
            serialize(&queue).map_err(|error| KvistError::TaskQueueUnavailable {
                path: queue_path.clone(),
                reason: error.to_string(),
            })?;
        let intended_post_queue_digest = digest(intended_queue.as_bytes());
        let mut prepared = PreparedAttempt {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            attempt_id: attempt_id.clone(),
            task_id: task_id.to_owned(),
            phase: "prepared".to_owned(),
            pre_status: status_name(pre_status).to_owned(),
            pre_queue_digest: pre_queue_digest.clone(),
            intended_post_queue_digest: intended_post_queue_digest.clone(),
            policy_identity: approval.approval_digest.clone(),
            runner_identity: approval.material.runner_digest.clone(),
            approved_write_scope: write_scope.clone(),
            timestamp: timestamp.clone(),
            operation_lock_digest: lock.contents_digest(),
            authentication_tag: String::new(),
        };
        prepared.authentication_tag = prepared_attempt_tag(secret, &prepared)?;
        append_json_attempt(
            &attempt_path,
            &prepared,
            "append prepared pre-spawn attempt",
        )?;
        if current_queue_digest(&context.component_dir)? != pre_queue_digest {
            return Err(journal_error(
                &queue_path,
                "the queue changed before the pre-spawn fence could be recorded",
            ));
        }
        lock.revalidate()?;
        replace_file_atomically(&queue_path, &intended_queue)?;

        let mut failure = PreSpawnFailure {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            attempt_id,
            task_id: task_id.to_owned(),
            phase: "pre-spawn-failure".to_owned(),
            prepared_event_digest: prepared_event_digest(&prepared)?,
            pre_status: status_name(pre_status).to_owned(),
            pre_queue_digest,
            intended_post_queue_digest,
            policy_identity: approval.approval_digest.clone(),
            runner_identity: approval.material.runner_digest.clone(),
            approved_write_scope: write_scope,
            recorded_by: "kvist-host".to_owned(),
            runner_descriptor_launched: false,
            write_scope_exposed: false,
            failure: pre_spawn_failure_name(runner_error).to_owned(),
            timestamp,
            authentication_tag: String::new(),
        };
        failure.authentication_tag = pre_spawn_failure_tag(secret, &failure)?;
        append_json_attempt(
            &attempt_path,
            &failure,
            "append authenticated pre-spawn failure",
        )
    })();
    let release = lock.release();
    match (result, release) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

fn pre_spawn_failure_name(_: &KvistError) -> &'static str {
    "runner-descriptor-open-failed"
}

fn can_fence_pre_spawn(task: &Task, tasks: &[Task]) -> bool {
    match task.status {
        TaskStatus::Pending => task_is_ready(task, tasks),
        TaskStatus::InProgress => task.recovery_state.is_none(),
        TaskStatus::Blocked | TaskStatus::Completed => false,
    }
}

fn approved_write_scope(
    project_dir: &Path,
    context: &TaskContext,
    _: &crate::config::ProjectConfig,
) -> Result<Vec<AttemptWriteScope>> {
    let mut scopes = Vec::new();
    for sub in ["src", "tests"] {
        let scope = context.component_dir.join(sub);
        if let Ok(rel) = scope.strip_prefix(project_dir) {
            let normalized: PathBuf = rel
                .components()
                .filter(|c| !matches!(c, std::path::Component::CurDir))
                .collect();
            if let Some(path_str) = normalized.to_str() {
                let digest = digest_path_tree(&scope).unwrap_or_else(|_| "sha256:0".to_owned());
                scopes.push(AttemptWriteScope {
                    path: path_str.to_owned(),
                    pre_digest: digest,
                });
            }
        }
    }
    if scopes.is_empty() {
        let scope = context.component_dir.join("tests");
        let path =
            scope
                .strip_prefix(project_dir)
                .map_err(|_| KvistError::TaskQueueUnavailable {
                    path: scope.clone(),
                    reason: "approved write scope is not below the project root".to_owned(),
                })?;
        let path = path
            .to_str()
            .ok_or_else(|| KvistError::TaskQueueUnavailable {
                path: scope.clone(),
                reason: "approved write scope is not valid UTF-8".to_owned(),
            })?;
        scopes.push(AttemptWriteScope {
            path: path.to_owned(),
            pre_digest: digest_path_tree(&scope)?,
        });
    }
    Ok(scopes)
}

fn digest_path_tree(path: &Path) -> Result<String> {
    enum Visit {
        Enter(PathBuf, usize),
        Exit(PathBuf, Vec<(String, PathBuf)>),
    }

    let mut budget = ScopeHashBudget::default();
    let mut hashes = BTreeMap::<PathBuf, String>::new();
    let mut pending = vec![Visit::Enter(path.to_path_buf(), 0)];
    while let Some(visit) = pending.pop() {
        match visit {
            Visit::Enter(current, depth) => match fs::symlink_metadata(&current) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    hashes.insert(current, scoped_digest(b"absent", b""));
                }
                Err(source) => {
                    return Err(KvistError::Io {
                        operation: "inspect approved write scope",
                        path: current,
                        source,
                    });
                }
                Ok(metadata) if is_link_like(&metadata) => {
                    return Err(scope_hash_error(
                        &current,
                        "approved write scope must not be link-like",
                    ));
                }
                Ok(metadata) if metadata.file_type().is_file() => {
                    let bytes = read_bounded_scope_file(&current, &metadata, &mut budget)?;
                    let mut material = Vec::with_capacity(8 + bytes.len());
                    material.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
                    material.extend_from_slice(&bytes);
                    hashes.insert(current, scoped_digest(b"file", &material));
                }
                Ok(metadata) if metadata.file_type().is_dir() => {
                    if depth > MAX_SCOPE_DEPTH {
                        return Err(scope_hash_error(
                            &current,
                            "approved write scope exceeds the directory-depth limit",
                        ));
                    }
                    let entries = read_scope_entries(&current, &mut budget)?;
                    pending.push(Visit::Exit(current, entries.clone()));
                    for (_, child) in entries.into_iter().rev() {
                        pending.push(Visit::Enter(child, depth + 1));
                    }
                }
                Ok(_) => {
                    return Err(scope_hash_error(
                        &current,
                        "approved write scope must be a regular file or real directory",
                    ));
                }
            },
            Visit::Exit(current, entries) => {
                let reread = read_scope_entries(&current, &mut budget)?;
                if entries
                    .iter()
                    .map(|(name, _)| name)
                    .ne(reread.iter().map(|(name, _)| name))
                {
                    return Err(scope_hash_error(
                        &current,
                        "approved write scope changed while it was hashed",
                    ));
                }
                let mut material = Vec::new();
                material.extend_from_slice(&(entries.len() as u64).to_be_bytes());
                for (name, child) in entries {
                    let child_digest = hashes.get(&child).ok_or_else(|| {
                        scope_hash_error(
                            &current,
                            "approved write scope changed while child hashes were collected",
                        )
                    })?;
                    material.extend_from_slice(&(name.len() as u64).to_be_bytes());
                    material.extend_from_slice(name.as_bytes());
                    material.extend_from_slice(&(child_digest.len() as u64).to_be_bytes());
                    material.extend_from_slice(child_digest.as_bytes());
                }
                budget.charge(&current, material.len())?;
                hashes.insert(current, scoped_digest(b"directory", &material));
            }
        }
    }
    hashes.remove(path).ok_or_else(|| {
        scope_hash_error(
            path,
            "approved write scope hash was not produced for the requested path",
        )
    })
}

#[derive(Default)]
struct ScopeHashBudget {
    entries: usize,
    material_bytes: u64,
}

impl ScopeHashBudget {
    fn charge_entry(&mut self, path: &Path, name_bytes: usize) -> Result<()> {
        self.entries = self.entries.checked_add(1).ok_or_else(|| {
            scope_hash_error(path, "approved write scope entry accounting overflowed")
        })?;
        if self.entries > MAX_SCOPE_ENTRIES {
            return Err(scope_hash_error(
                path,
                "approved write scope exceeds the aggregate entry limit",
            ));
        }
        self.charge(path, name_bytes)
    }

    fn charge(&mut self, path: &Path, bytes: usize) -> Result<()> {
        self.material_bytes = self
            .material_bytes
            .checked_add(bytes as u64)
            .ok_or_else(|| {
                scope_hash_error(path, "approved write scope byte accounting overflowed")
            })?;
        if self.material_bytes > MAX_SCOPE_TOTAL_BYTES {
            return Err(scope_hash_error(
                path,
                "approved write scope exceeds the aggregate material limit",
            ));
        }
        Ok(())
    }
}

fn read_scope_entries(
    directory: &Path,
    budget: &mut ScopeHashBudget,
) -> Result<Vec<(String, PathBuf)>> {
    let iterator = fs::read_dir(directory).map_err(|source| KvistError::Io {
        operation: "read approved write scope",
        path: directory.to_path_buf(),
        source,
    })?;
    let mut entries = Vec::new();
    for entry in iterator {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "inspect approved write scope entry",
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name().into_string().map_err(|_| {
            scope_hash_error(&path, "approved write scope contains a non-UTF-8 name")
        })?;
        budget.charge_entry(directory, name.len())?;
        entries.push((name, path));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn read_bounded_scope_file(
    path: &Path,
    initial_metadata: &fs::Metadata,
    budget: &mut ScopeHashBudget,
) -> Result<Vec<u8>> {
    if initial_metadata.len() > MAX_SCOPE_FILE_BYTES {
        return Err(scope_hash_error(
            path,
            "approved write scope file exceeds the per-file byte limit",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|source| KvistError::Io {
        operation: "open approved write scope without following links",
        path: path.to_path_buf(),
        source,
    })?;
    let opened_metadata = file.metadata().map_err(|source| KvistError::Io {
        operation: "inspect opened approved write scope",
        path: path.to_path_buf(),
        source,
    })?;
    if !opened_metadata.file_type().is_file()
        || opened_metadata.len() != initial_metadata.len()
        || opened_metadata.len() > MAX_SCOPE_FILE_BYTES
    {
        return Err(scope_hash_error(
            path,
            "approved write scope file changed while it was opened",
        ));
    }
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    (&mut file)
        .take(MAX_SCOPE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| KvistError::Io {
            operation: "read bounded approved write scope",
            path: path.to_path_buf(),
            source,
        })?;
    let final_metadata = file.metadata().map_err(|source| KvistError::Io {
        operation: "reinspect opened approved write scope",
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.len() as u64 > MAX_SCOPE_FILE_BYTES
        || final_metadata.len() != opened_metadata.len()
        || final_metadata.len() != bytes.len() as u64
    {
        return Err(scope_hash_error(
            path,
            "approved write scope file grew or changed while it was read",
        ));
    }
    budget.charge(path, bytes.len())?;
    Ok(bytes)
}

fn scoped_digest(kind: &[u8], material: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"kvist.scope-tree.v1\0");
    hasher.update((kind.len() as u64).to_be_bytes());
    hasher.update(kind);
    hasher.update((material.len() as u64).to_be_bytes());
    hasher.update(material);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn scope_hash_error(path: &Path, reason: &str) -> KvistError {
    KvistError::TaskQueueUnavailable {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

/// Launches the external agent to execute a task, transitions the task to InProgress,
/// captures logs, parses token usage, and transitions the task to Completed/Blocked based on exit.
pub fn run_task(component_path: &Path, task_id: &str, stream: bool) -> Result<String> {
    let context = validate_context(component_path)?;
    let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let config = crate::config::load(&project_dir)?;
    if config.sandbox.is_none() {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "sandbox configuration is absent".to_owned(),
        });
    }
    if config.test_policy.is_none() {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "test policy is absent".to_owned(),
        });
    }

    let (approved_runner, policy_identity, secret) = match task_run_approval(&project_dir, &config)?
    {
        TaskRunApproval::Ready {
            runner,
            policy_identity,
        } => {
            let (_, secret) = load_authenticated_execution_approval(&project_dir, &config)?;
            (runner, policy_identity, secret)
        }
        TaskRunApproval::DescriptorUnavailable {
            approval,
            secret,
            error,
        } => {
            fence_pre_spawn_failure(
                component_path,
                task_id,
                &project_dir,
                &config,
                &approval,
                &secret,
                &error,
            )?;
            return Err(error);
        }
    };

    let sandbox_config = config
        .sandbox
        .as_ref()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: "task execution requires a project-local [sandbox] configuration".to_owned(),
        })?;
    let approved_backend =
        crate::sandbox::backend_identity(sandbox_config, &project_dir, config.vcs)?;
    let sandbox_probe = crate::sandbox::ensure_available(
        sandbox_config,
        &project_dir,
        config.vcs,
        &approved_runner,
        &approved_backend,
    )?;

    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    // This single lock covers selection, execution, evidence, and the terminal
    // transition. It prevents a second runner from selecting the same ready task.
    let lock = TaskLock::for_context(&context, task_id, &started_at)?;
    let result = (|| {
        let context = validate_context(component_path)?;
        ensure_component_attempts_recovered(&context.component_dir)?;
        let mut queue = read_queue(&context.component_dir)?;

        // Revalidate the selected task after acquiring the operation lock.
        let task_index = queue
            .tasks
            .iter()
            .position(|task| task.id == task_id)
            .ok_or_else(|| KvistError::TaskQueueUnavailable {
                path: context
                    .component_dir
                    .join(ComponentArtifact::TaskQueue.filename()),
                reason: format!("task `{task_id}` not found in the queue"),
            })?;

        let task = &queue.tasks[task_index];

        // 2. Transition task status to InProgress atomically (if not already InProgress)
        let pre_status = task.status;
        if task.status != TaskStatus::InProgress {
            let transition_at =
                Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
            let _ = transition_locked(
                &context,
                &lock,
                task_id,
                TaskStatus::InProgress,
                None,
                &transition_at,
            )?;
            // Re-read queue to reflect transition
            queue = read_queue(&context.component_dir)?;
        }

        let task = &queue.tasks[task_index];

        if !is_runnable_task(task, &queue.tasks) {
            return Err(journal_error(
                &context
                    .component_dir
                    .join(ComponentArtifact::TaskQueue.filename()),
                &format!("task `{task_id}` is not ready for execution"),
            ));
        }

        // 4. Load agent profile matching task type.
        let (agent_profile, role) = match task.kind {
            TaskKind::Test | TaskKind::Implementation => {
                (&config.agent.developer, crate::config::Role::Developer)
            }
            TaskKind::SecurityAudit => (
                &config.agent.security_reviewer,
                crate::config::Role::SecurityReviewer,
            ),
            TaskKind::ComplianceReview => (&config.agent.architect, crate::config::Role::Architect),
        };

        // 4. Sliced context files gathering
        let agent_attempt_path = attempt_path(&context.component_dir, task_id)?;
        let attempt_id = next_attempt_id(&agent_attempt_path)?;
        let write_scope = approved_write_scope(&project_dir, &context, &config)?;
        let pre_queue_digest = current_queue_digest(&context.component_dir)?;

        // Build the PreparedAttempt struct with all required fields.
        // This mirrors the pattern used in fence_pre_spawn_failure.
        let mut prepared = PreparedAttempt {
            schema_version: ATTEMPT_SCHEMA_VERSION,
            attempt_id: attempt_id.clone(),
            task_id: task_id.to_owned(),
            phase: "prepared".to_owned(),
            pre_status: status_name(pre_status).to_owned(),
            pre_queue_digest: pre_queue_digest.clone(),
            intended_post_queue_digest: pre_queue_digest.clone(),
            policy_identity: policy_identity.clone(),
            runner_identity: approved_runner.digest.clone(),
            approved_write_scope: write_scope.clone(),
            timestamp: started_at.clone(),
            operation_lock_digest: lock.contents_digest(),
            authentication_tag: String::new(),
        };
        prepared.authentication_tag = prepared_attempt_tag(&secret[..], &prepared)?;

        // Append the prepared event to the attempt journal.
        append_json_attempt(
            &agent_attempt_path,
            &prepared,
            "append prepared run_task attempt",
        )?;

        let mut context_files = vec![
            PathBuf::from("/workspace/component/REQUIREMENTS.md"),
            PathBuf::from("/workspace/component/CONTRACT.md"),
            PathBuf::from("/workspace/component/DESIGN.md"),
            PathBuf::from("/workspace/component/TODOS.yaml"),
            PathBuf::from("/workspace/component/IMPL.md"),
            PathBuf::from("/workspace/context/ROOT_CONTRACT.md"),
        ];
        let mut read_only_mounts = vec![crate::sandbox::ReadOnlyMount {
            source: project_dir.join("ROOT_CONTRACT.md"),
            destination: "/workspace/context/ROOT_CONTRACT.md".to_owned(),
        }];
        if context.component_path != Path::new(".") {
            let component_root = project_dir.join(&config.component_root);
            let (parent_component_dir, _) =
                discovery::find_parent_component_dir(&component_root, &context.component_path)?;
            context_files.push(PathBuf::from("/workspace/context/PARENT_CONTRACT.md"));
            read_only_mounts.push(crate::sandbox::ReadOnlyMount {
                source: parent_component_dir.join(ComponentArtifact::Contract.filename()),
                destination: "/workspace/context/PARENT_CONTRACT.md".to_owned(),
            });
        }

        // 5. Build prompt
        let lang = detect_language(&context.component_dir);
        let prompt = load_and_interpolate_template(&context.component_dir, lang, task)?;

        tracing::info!(
            task_id = %task_id,
            component = %component_path.display(),
            "running task via external agent in sandbox"
        );
        println!("Running task `{task_id}` via external agent...");

        // 6. Execute agent
        let run_result = crate::agent::execute_agent(
            agent_profile,
            sandbox_config,
            &approved_runner,
            &sandbox_probe,
            crate::agent::AgentExecutionRequest {
                project_root: &project_dir,
                vcs_selection: config.vcs,
                prompt: &prompt,
                context_paths: &context_files,
                read_only_mounts: &read_only_mounts,
                target_dir: &context.component_dir,
                task_id,
                stream_output: stream,
                role,
                policy_identity: &policy_identity,
            },
        )?;
        let agent_timestamp =
            Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
        append_agent_execution(
            &agent_attempt_path,
            AgentExecutionRecord {
                schema_version: ATTEMPT_SCHEMA_VERSION,
                attempt_id: &attempt_id,
                phase: "agent-execution",
                task_id,
                timestamp: &agent_timestamp,
                success: run_result.success,
                timed_out: run_result.timed_out,
                output_limit_exceeded: run_result.output_limit_exceeded,
                stdout: &run_result.stdout,
                stderr: &run_result.stderr,
            },
        )?;

        // 7. Transition task status depending on outcome
        if run_result.success {
            if task.kind == TaskKind::Implementation {
                tracing::info!(
                    task_id = %task_id,
                    "running test-command verification"
                );
                println!("Running test-command verification...");
                match verify_task(
                    component_path,
                    task_id,
                    &project_dir,
                    &config,
                    &sandbox_probe,
                    &policy_identity,
                ) {
                    Ok(verify_res) => {
                        if verify_res.success {
                            let mut changes = Vec::new();
                            let scope_dir = context.component_dir.join("tests");
                            if let Ok(entries) = fs::read_dir(&scope_dir) {
                                for entry in entries.flatten() {
                                    let path = entry.path();
                                    if path.is_file()
                                        && let Ok(rel) = path.strip_prefix(&project_dir)
                                        && let Ok(bytes) = fs::read(&path)
                                    {
                                        let post_digest = format!(
                                            "sha256:{}",
                                            hex::encode(sha2::Sha256::digest(&bytes))
                                        );
                                        changes.push(serde_json::json!({
                                            "path": rel.to_string_lossy(),
                                            "operation": "create",
                                            "pre_digest": serde_json::Value::Null,
                                            "post_digest": post_digest,
                                        }));
                                    }
                                }
                            }

                            let exec_fin = serde_json::json!({
                                "schema_version": 1,
                                "attempt_id": attempt_id,
                                "task_id": task_id,
                                "phase": "execution-finished",
                                "result": "success",
                                "changes": changes
                            });
                            let verif_fin = serde_json::json!({
                                "schema_version": 1,
                                "attempt_id": attempt_id,
                                "task_id": task_id,
                                "phase": "verification-finished",
                                "result": "success"
                            });
                            let pend_disp = serde_json::json!({
                                "schema_version": 1,
                                "attempt_id": attempt_id,
                                "task_id": task_id,
                                "phase": "pending-human-disposition"
                            });
                            let mut journal_content =
                                fs::read_to_string(&agent_attempt_path).unwrap_or_default();
                            journal_content
                                .push_str(&format!("{exec_fin}\n{verif_fin}\n{pend_disp}\n"));
                            replace_file_atomically(&agent_attempt_path, &journal_content)?;

                            let token_summary =
                                match (run_result.tokens_input, run_result.tokens_output) {
                                    (Some(in_tok), Some(out_tok)) => {
                                        format!(
                                            " [Tokens used - Input: {}, Output: {}]",
                                            in_tok, out_tok
                                        )
                                    }
                                    _ => "".to_owned(),
                                };
                            Ok(format!(
                                "task `{task_id}` executed and verified successfully. Awaiting human finalization with `kvist task finalize`.{}\nLogs written to: {}",
                                token_summary,
                                run_result.log_path.display()
                            ))
                        } else {
                            let timed_out_msg = if verify_res.timed_out {
                                " (timed out)"
                            } else {
                                ""
                            };
                            let blocker_reason = redact_bounded(
                                format!(
                                    "test-command verification failed{}. Command: '{}', Exit code: {:?}.\n---\nStdout:\n{}\n---\nStderr:\n{}",
                                    timed_out_msg,
                                    verify_res.command,
                                    verify_res.exit_code,
                                    verify_res.stdout,
                                    verify_res.stderr
                                ),
                                &evidence_redactions(&config),
                                MAX_VERIFICATION_EVIDENCE_BYTES,
                            );
                            let transition_at = Timestamp::now()
                                .map_err(|source| KvistError::TaskClock { source })?;
                            let _ = transition_locked(
                                &context,
                                &lock,
                                task_id,
                                TaskStatus::Blocked,
                                Some(&blocker_reason),
                                &transition_at,
                            )?;
                            Ok(format!(
                                "task `{task_id}` failed test-command verification and transitioned to blocked.\nLogs written to: {}\nNext Step: Run 'kvist task log {} {}' to inspect the detailed verification and execution logs.",
                                run_result.log_path.display(),
                                component_path.display(),
                                task_id
                            ))
                        }
                    }
                    Err(err) => {
                        let verification_error = redact_bounded(
                            err.to_string(),
                            &evidence_redactions(&config),
                            MAX_VERIFICATION_EVIDENCE_BYTES,
                        );
                        let blocker_reason =
                            format!("test-command verification blocked: {verification_error}");
                        let transition_at =
                            Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
                        let _ = transition_locked(
                            &context,
                            &lock,
                            task_id,
                            TaskStatus::Blocked,
                            Some(&blocker_reason),
                            &transition_at,
                        )?;
                        Ok(format!(
                            "task `{task_id}` verification blocked and transitioned to blocked: {}\nNext Step: Run 'kvist task log {} {}' to inspect the logs and check the test command configuration.",
                            verification_error,
                            component_path.display(),
                            task_id
                        ))
                    }
                }
            } else {
                let transition_at =
                    Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
                let _ = transition_locked(
                    &context,
                    &lock,
                    task_id,
                    TaskStatus::Completed,
                    None,
                    &transition_at,
                )?;

                let token_summary = match (run_result.tokens_input, run_result.tokens_output) {
                    (Some(in_tok), Some(out_tok)) => {
                        format!(" [Tokens used - Input: {}, Output: {}]", in_tok, out_tok)
                    }
                    _ => "".to_owned(),
                };
                Ok(format!(
                    "task `{task_id}` executed successfully and transitioned to completed.{}\nLogs written to: {}",
                    token_summary,
                    run_result.log_path.display()
                ))
            }
        } else {
            // Transition to Blocked
            let blocker_reason = if run_result.timed_out {
                format!(
                    "agent execution timed out and the sandbox runner was terminated. Bounded redacted logs are written to: {}",
                    run_result.log_path.display()
                )
            } else if run_result.output_limit_exceeded {
                format!(
                    "agent execution exceeded the combined output limit and the sandbox runner was terminated. Bounded redacted logs are written to: {}",
                    run_result.log_path.display()
                )
            } else {
                format!(
                    "agent failed during task execution. Bounded redacted logs are written to: {}",
                    run_result.log_path.display()
                )
            };
            let transition_at =
                Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
            let _ = transition_locked(
                &context,
                &lock,
                task_id,
                TaskStatus::Blocked,
                Some(&blocker_reason),
                &transition_at,
            )?;
            Ok(format!(
                "task `{task_id}` {} and has been transitioned to blocked.\nLogs written to: {}\nNext Step: Run 'kvist task log {} {}' to inspect the execution error and failure logs.",
                if run_result.timed_out {
                    "timed out"
                } else if run_result.output_limit_exceeded {
                    "exceeded the combined output limit"
                } else {
                    "failed during execution"
                },
                run_result.log_path.display(),
                component_path.display(),
                task_id
            ))
        }
    })();
    let release = lock.release();
    match (result, release) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

fn select_runnable_task(context: &TaskContext, requested: Option<&str>) -> Result<String> {
    let queue = read_queue(&context.component_dir)?;
    let task_id = match requested {
        Some(task_id) => task_id.to_owned(),
        None => queue
            .tasks
            .iter()
            .find(|task| task_is_ready(task, &queue.tasks))
            .map(|task| task.id.clone())
            .ok_or_else(|| {
                journal_error(
                    &context
                        .component_dir
                        .join(ComponentArtifact::TaskQueue.filename()),
                    "no ready tasks available in the queue",
                )
            })?,
    };
    let task = queue
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| KvistError::TaskNotFound {
            component: context.component_path.clone(),
            task_id: task_id.clone(),
        })?;
    if !is_runnable_task(task, &queue.tasks) {
        return Err(journal_error(
            &context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            &format!("task `{task_id}` is not ready for execution"),
        ));
    }
    Ok(task_id)
}

fn is_runnable_task(task: &Task, tasks: &[Task]) -> bool {
    task.recovery_state.is_none()
        && (task_is_ready(task, tasks) || task.status == TaskStatus::InProgress)
}

/// Reads and returns the most recent execution log file for a specific task.
pub fn task_log(component_path: &Path, task_id: &str) -> Result<String> {
    let context = validate_context(component_path)?;
    let logs_dir = context.component_dir.join(".kvist").join("logs");

    let logs_metadata = fs::symlink_metadata(&logs_dir).ok();
    if !logs_metadata
        .is_some_and(|metadata| metadata.file_type().is_dir() && !is_link_like(&metadata))
    {
        return Err(KvistError::TaskQueueUnavailable {
            path: context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            reason: format!("no execution logs found for task `{task_id}`"),
        });
    }

    let mut log_files = Vec::new();
    let entries = fs::read_dir(&logs_dir).map_err(|source| KvistError::Io {
        operation: "read agent logs directory",
        path: logs_dir.clone(),
        source,
    })?;

    let prefix = format!("{task_id}_");
    for entry in entries {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "inspect agent log file",
            path: logs_dir.clone(),
            source,
        })?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(&prefix) && name_str.ends_with(".log") {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
                operation: "inspect agent log file",
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_file() && !is_link_like(&metadata) {
                log_files.push(path);
            }
        }
    }

    // Sort by name in descending order (which corresponds to lexical timestamp sorting)
    log_files.sort();
    let Some(most_recent) = log_files.last() else {
        return Err(KvistError::TaskQueueUnavailable {
            path: context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            reason: format!("no execution logs found for task `{task_id}`"),
        });
    };

    let contents = fs::read_to_string(most_recent).map_err(|source| KvistError::Io {
        operation: "read task execution log",
        path: most_recent.clone(),
        source,
    })?;

    Ok(contents)
}

const EXECUTION_APPROVAL_VERSION: u32 = 1;
const APPROVAL_STATE_DIRECTORY: &str = "approval-v1";
const APPROVAL_SECRET_FILE: &str = "approval-secret";
const MAX_VERIFICATION_EVIDENCE_BYTES: usize = 65_536;

fn evidence_redactions(config: &crate::config::ProjectConfig) -> Vec<String> {
    let mut values = config.agent.architect.redaction_values.clone();
    for value in &config.agent.developer.redaction_values {
        if !values.contains(value) {
            values.push(value.clone());
        }
    }
    if let Some(sandbox) = &config.sandbox {
        for value in crate::sandbox::allowed_environment(sandbox, None).into_values() {
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }
    values
}

fn redact_bounded(mut value: String, redactions: &[String], limit: usize) -> String {
    for redaction in redactions {
        value = value.replace(redaction, "[REDACTED]");
    }
    truncate_utf8(&mut value, limit);
    value
}

fn truncate_utf8(value: &mut String, limit: usize) {
    if value.len() <= limit {
        return;
    }
    let mut boundary = limit;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

/// Versioned, non-secret execution inputs bound by an explicit approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExecutionApprovalMaterial {
    configuration_schema_version: u32,
    approval_schema_version: u32,
    sandbox_protocol_version: u32,
    sandbox_schema_version: u32,
    root_contract_digest: String,
    agent_source: String,
    agent_source_digest: String,
    architect_template_digest: String,
    architect_token_limit: Option<usize>,
    architect_timeout_seconds: u64,
    architect_max_output_bytes: usize,
    architect_redaction_digest: String,
    developer_template_digest: String,
    developer_token_limit: Option<usize>,
    developer_timeout_seconds: u64,
    developer_max_output_bytes: usize,
    developer_redaction_digest: String,
    security_reviewer_template_digest: String,
    security_reviewer_token_limit: Option<usize>,
    security_reviewer_timeout_seconds: u64,
    security_reviewer_max_output_bytes: usize,
    security_reviewer_redaction_digest: String,
    sandbox_digest: String,
    runner_path: String,
    runner_digest: String,
    backend_path: String,
    backend_digest: String,
    test_policy_schema_version: Option<i64>,
    test_policy_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExecutionApproval {
    schema_version: u32,
    canonical_project: String,
    canonical_worktree: String,
    material: ExecutionApprovalMaterial,
    approval_digest: String,
    authentication_tag: String,
}

/// Approve the complete effective execution policy with a deterministic record.
pub fn approve_policy(project_path: &Path) -> Result<String> {
    let project_dir = if project_path == Path::new(".") {
        std::env::current_dir().map_err(|source| KvistError::Io {
            operation: "determine current project directory",
            path: PathBuf::from("."),
            source,
        })?
    } else {
        project_path.to_path_buf()
    };

    let config = crate::config::load(&project_dir)?;
    reject_project_approval_record(&project_dir)?;
    let (mut approval, _) = build_execution_approval(&project_dir, &config)?;
    let (state_root, approved_path) = approval_state_paths(
        &project_dir,
        config.vcs,
        &approval.canonical_project,
        &approval.canonical_worktree,
    )?;
    let secret = load_or_create_approval_secret(&state_root)?;
    approval.authentication_tag = approval_tag(&secret, &approval)?;
    let record_directory = approved_path.parent().ok_or_else(|| KvistError::Io {
        operation: "determine approval record directory",
        path: approved_path.clone(),
        source: io::Error::other("approval record path has no parent"),
    })?;
    ensure_user_state_directory(record_directory)?;
    let encoded = serde_json::to_string(&approval).map_err(|error| {
        KvistError::UnapprovedExecutionPolicy {
            reason: format!("cannot serialize execution approval: {error}"),
        }
    })?;
    replace_file_atomically(&approved_path, &encoded)?;
    Ok(format!(
        "Successfully approved execution policy with hash: {}",
        approval.approval_digest
    ))
}

fn build_execution_approval(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<(ExecutionApproval, crate::sandbox::RunnerIdentity)> {
    let sandbox = config
        .sandbox
        .as_ref()
        .ok_or_else(|| KvistError::UnapprovedExecutionPolicy {
            reason: "sandbox configuration is absent".to_owned(),
        })?;
    let runner = crate::sandbox::runner_identity(sandbox, project_dir, config.vcs)?;
    let approval = build_execution_approval_for_runner(project_dir, config, &runner)?;
    Ok((approval, runner))
}

fn build_execution_approval_for_runner(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
    runner: &crate::sandbox::RunnerIdentity,
) -> Result<ExecutionApproval> {
    let sandbox = config
        .sandbox
        .as_ref()
        .ok_or_else(|| KvistError::UnapprovedExecutionPolicy {
            reason: "sandbox configuration is absent".to_owned(),
        })?;
    let backend = crate::sandbox::backend_identity(sandbox, project_dir, config.vcs)?;
    let (canonical_project, canonical_worktree) =
        project_worktree_identity(project_dir, config.vcs)?;
    let material = ExecutionApprovalMaterial {
        configuration_schema_version: crate::artifacts::CONFIGURATION_VERSION,
        approval_schema_version: EXECUTION_APPROVAL_VERSION,
        sandbox_protocol_version: crate::sandbox::PROTOCOL_VERSION,
        sandbox_schema_version: 1,
        root_contract_digest: approved_root_contract_digest(project_dir)?,
        agent_source: config.agent.source.identity.clone(),
        agent_source_digest: config.agent.source.digest.clone(),
        architect_template_digest: agent_profile_digest(&config.agent.architect, "architect")?,
        architect_token_limit: config.agent.architect.token_limit,
        architect_timeout_seconds: config.agent.architect.timeout_seconds,
        architect_max_output_bytes: config.agent.architect.max_output_bytes,
        architect_redaction_digest: digest(
            &serde_json::to_vec(&config.agent.architect.redaction_values).map_err(|error| {
                KvistError::UnapprovedExecutionPolicy {
                    reason: format!("cannot serialize architect redaction policy: {error}"),
                }
            })?,
        ),
        developer_template_digest: agent_profile_digest(&config.agent.developer, "developer")?,
        developer_token_limit: config.agent.developer.token_limit,
        developer_timeout_seconds: config.agent.developer.timeout_seconds,
        developer_max_output_bytes: config.agent.developer.max_output_bytes,
        developer_redaction_digest: digest(
            &serde_json::to_vec(&config.agent.developer.redaction_values).map_err(|error| {
                KvistError::UnapprovedExecutionPolicy {
                    reason: format!("cannot serialize developer redaction policy: {error}"),
                }
            })?,
        ),
        security_reviewer_template_digest: agent_profile_digest(
            &config.agent.security_reviewer,
            "security-reviewer",
        )?,
        security_reviewer_token_limit: config.agent.security_reviewer.token_limit,
        security_reviewer_timeout_seconds: config.agent.security_reviewer.timeout_seconds,
        security_reviewer_max_output_bytes: config.agent.security_reviewer.max_output_bytes,
        security_reviewer_redaction_digest: digest(
            &serde_json::to_vec(&config.agent.security_reviewer.redaction_values).map_err(
                |error| KvistError::UnapprovedExecutionPolicy {
                    reason: format!("cannot serialize security_reviewer redaction policy: {error}"),
                },
            )?,
        ),
        sandbox_digest: digest(&serde_json::to_vec(sandbox).map_err(|error| {
            KvistError::UnapprovedExecutionPolicy {
                reason: format!("cannot serialize sandbox configuration: {error}"),
            }
        })?),
        runner_path: runner.canonical_path.clone(),
        runner_digest: runner.digest.clone(),
        backend_path: backend.path.clone(),
        backend_digest: backend.digest.clone(),
        test_policy_schema_version: config
            .test_policy
            .as_ref()
            .map(|policy| policy.schema_version),
        test_policy_digest: config
            .test_policy
            .as_ref()
            .map(crate::config::compute_policy_hash),
    };
    let approval_digest = digest(&serde_json::to_vec(&material).map_err(|error| {
        KvistError::UnapprovedExecutionPolicy {
            reason: format!("cannot serialize execution approval inputs: {error}"),
        }
    })?);
    Ok(ExecutionApproval {
        schema_version: EXECUTION_APPROVAL_VERSION,
        canonical_project,
        canonical_worktree,
        material,
        approval_digest,
        authentication_tag: String::new(),
    })
}

fn agent_profile_digest(
    profile: &crate::config::AgentProfile,
    role: &'static str,
) -> Result<String> {
    serde_json::to_vec(profile)
        .map(|bytes| digest(&bytes))
        .map_err(|error| KvistError::UnapprovedExecutionPolicy {
            reason: format!("cannot serialize {role} agent profile: {error}"),
        })
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn reject_project_approval_record(project_dir: &Path) -> Result<()> {
    let path = project_dir
        .join(".kvist")
        .join("approved_execution_policy.json");
    match fs::symlink_metadata(&path) {
        Ok(_) => Err(KvistError::UnapprovedExecutionPolicy {
            reason: format!(
                "legacy repository-contained approval record `{}` must be removed; approvals are user-owned state",
                path.display()
            ),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(KvistError::Io {
            operation: "inspect legacy approval record",
            path,
            source,
        }),
    }
}

fn project_worktree_identity(
    project_dir: &Path,
    vcs_selection: crate::config::VcsSelection,
) -> Result<(String, String)> {
    let project = project_dir
        .canonicalize()
        .map_err(|source| KvistError::Io {
            operation: "canonicalize approval project root",
            path: project_dir.to_path_buf(),
            source,
        })?;
    let inspection = crate::vcs::inspect(project_dir, vcs_selection, Vec::new());
    let worktree =
        inspection
            .repository_root
            .ok_or_else(|| KvistError::UnapprovedExecutionPolicy {
                reason: format!(
                    "cannot resolve selected VCS worktree for approval: {}",
                    inspection.diagnostic.unwrap_or(inspection.summary)
                ),
            })?;
    let worktree = worktree.canonicalize().map_err(|source| KvistError::Io {
        operation: "canonicalize approval worktree root",
        path: worktree,
        source,
    })?;
    Ok((
        project.to_string_lossy().into_owned(),
        worktree.to_string_lossy().into_owned(),
    ))
}

fn approval_state_paths(
    project_dir: &Path,
    vcs_selection: crate::config::VcsSelection,
    canonical_project: &str,
    canonical_worktree: &str,
) -> Result<(PathBuf, PathBuf)> {
    let state_base = user_state_base().ok_or_else(|| KvistError::UnapprovedExecutionPolicy {
        reason: "cannot determine user-owned approval state directory".to_owned(),
    })?;
    let state_root = state_base.join("kvist").join(APPROVAL_STATE_DIRECTORY);
    let (project, worktree) = project_worktree_identity(project_dir, vcs_selection)?;
    if project != canonical_project || worktree != canonical_worktree {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "canonical project or worktree identity changed during approval".to_owned(),
        });
    }
    if state_root.starts_with(Path::new(&project)) || state_root.starts_with(Path::new(&worktree)) {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "user-owned approval state must not be inside the project or worktree"
                .to_owned(),
        });
    }
    let name = digest(format!("{canonical_project}\n{canonical_worktree}").as_bytes())
        .trim_start_matches("sha256:")
        .to_owned();
    Ok((
        state_root.clone(),
        state_root.join("records").join(format!("{name}.json")),
    ))
}

fn user_state_base() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
    }
}

fn load_or_create_approval_secret(state_root: &Path) -> Result<Vec<u8>> {
    ensure_user_state_directory(state_root)?;
    let path = state_root.join(APPROVAL_SECRET_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => read_approval_secret(&path, &metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut secret = [0_u8; 32];
            getrandom::fill(&mut secret).map_err(|error| {
                KvistError::UnapprovedExecutionPolicy {
                    reason: format!("cannot generate approval secret: {error}"),
                }
            })?;
            write_approval_secret(&path, &secret)?;
            let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
                operation: "inspect created approval secret",
                path: path.clone(),
                source,
            })?;
            read_approval_secret(&path, &metadata)
        }
        Err(source) => Err(KvistError::Io {
            operation: "inspect approval secret",
            path: path.clone(),
            source,
        }),
    }
}

fn load_existing_recovery_secret() -> Result<Vec<u8>> {
    let state_base = user_state_base()
        .filter(|path| path.is_absolute())
        .ok_or_else(|| KvistError::TaskQueueUnavailable {
            path: PathBuf::from("."),
            reason: "cannot determine user-owned recovery authentication state".to_owned(),
        })?;
    let path = state_base
        .join("kvist")
        .join(APPROVAL_STATE_DIRECTORY)
        .join(APPROVAL_SECRET_FILE);
    let metadata =
        fs::symlink_metadata(&path).map_err(|source| KvistError::TaskQueueUnavailable {
            path: path.clone(),
            reason: if source.kind() == io::ErrorKind::NotFound {
                "user-owned recovery authentication secret is missing".to_owned()
            } else {
                format!("cannot inspect user-owned recovery authentication secret: {source}")
            },
        })?;
    read_approval_secret(&path, &metadata).map_err(|error| KvistError::TaskQueueUnavailable {
        path,
        reason: format!("cannot use user-owned recovery authentication secret: {error}"),
    })
}

fn ensure_user_state_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| KvistError::Io {
        operation: "create user approval state directory",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect user approval state directory",
        path: path.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_dir() {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: format!(
                "user approval state `{}` must be a real directory",
                path.display()
            ),
        });
    }
    Ok(())
}

fn write_approval_secret(path: &Path, secret: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| KvistError::Io {
        operation: "determine approval secret parent",
        path: path.to_path_buf(),
        source: io::Error::other("approval secret has no parent directory"),
    })?;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    let mut temporary_file = NamedTempFile::new_in(parent).map_err(|source| KvistError::Io {
        operation: "create temporary approval secret",
        path: parent.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    temporary_file
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| KvistError::Io {
            operation: "protect temporary approval secret",
            path: path.to_path_buf(),
            source,
        })?;
    temporary_file
        .write_all(secret)
        .and_then(|()| temporary_file.as_file().sync_all())
        .map_err(|source| KvistError::Io {
            operation: "write approval secret",
            path: path.to_path_buf(),
            source,
        })?;
    match temporary_file.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(KvistError::Io {
            operation: "publish approval secret",
            path: path.to_path_buf(),
            source: error.error,
        }),
    }
}

fn read_approval_secret(path: &Path, metadata: &fs::Metadata) -> Result<Vec<u8>> {
    if is_link_like(metadata) || !metadata.file_type().is_file() || metadata.len() != 32 {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: format!("approval secret `{}` is malformed", path.display()),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(KvistError::UnapprovedExecutionPolicy {
                reason: format!("approval secret `{}` is not user-private", path.display()),
            });
        }
    }
    fs::read(path).map_err(|source| KvistError::Io {
        operation: "read approval secret",
        path: path.to_path_buf(),
        source,
    })
}

fn approval_tag(secret: &[u8], approval: &ExecutionApproval) -> Result<String> {
    let payload = serde_json::to_vec(&(
        approval.schema_version,
        &approval.canonical_project,
        &approval.canonical_worktree,
        &approval.material,
        &approval.approval_digest,
    ))
    .map_err(|error| KvistError::UnapprovedExecutionPolicy {
        reason: format!("cannot serialize authenticated approval record: {error}"),
    })?;
    Ok(format!("hmac-sha256:{}", hmac_sha256(secret, &payload)))
}

fn hmac_sha256(secret: &[u8], message: &[u8]) -> String {
    let mut key = [0_u8; 64];
    if secret.len() > key.len() {
        key[..32].copy_from_slice(&Sha256::digest(secret));
    } else {
        key[..secret.len()].copy_from_slice(secret);
    }
    let mut inner = Sha256::new();
    inner.update(key.map(|byte| byte ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(key.map(|byte| byte ^ 0x5c));
    outer.update(inner);
    format!("sha256:{}", hex::encode(outer.finalize()))
}

fn prepared_event_digest(prepared: &PreparedAttempt) -> Result<String> {
    serde_json::to_vec(prepared)
        .map(|bytes| digest(&bytes))
        .map_err(|error| KvistError::TaskQueueUnavailable {
            path: PathBuf::from("."),
            reason: format!("cannot canonicalize prepared attempt evidence: {error}"),
        })
}

fn pre_spawn_failure_event_digest(failure: &PreSpawnFailure) -> Result<String> {
    serde_json::to_vec(failure)
        .map(|bytes| digest(&bytes))
        .map_err(|error| KvistError::TaskQueueUnavailable {
            path: PathBuf::from("."),
            reason: format!("cannot canonicalize pre-spawn failure evidence: {error}"),
        })
}

fn recovery_prepared_event_digest(decision: &RecoveryPreparedAttempt) -> Result<String> {
    serde_json::to_vec(decision)
        .map(|bytes| digest(&bytes))
        .map_err(|error| KvistError::TaskQueueUnavailable {
            path: PathBuf::from("."),
            reason: format!("cannot canonicalize recovery-prepared evidence: {error}"),
        })
}

fn prepared_attempt_tag(secret: &[u8], evidence: &PreparedAttempt) -> Result<String> {
    let payload = serde_json::to_vec(&(
        evidence.schema_version,
        &evidence.attempt_id,
        &evidence.task_id,
        &evidence.phase,
        &evidence.pre_status,
        &evidence.pre_queue_digest,
        &evidence.intended_post_queue_digest,
        &evidence.policy_identity,
        &evidence.runner_identity,
        &evidence.approved_write_scope,
        &evidence.timestamp,
        &evidence.operation_lock_digest,
    ))
    .map_err(|error| KvistError::TaskQueueUnavailable {
        path: PathBuf::from("."),
        reason: format!("cannot canonicalize prepared attempt evidence: {error}"),
    })?;
    Ok(format!("hmac-sha256:{}", hmac_sha256(secret, &payload)))
}

fn pre_spawn_failure_tag(secret: &[u8], evidence: &PreSpawnFailure) -> Result<String> {
    let payload = serde_json::to_vec(&(
        evidence.schema_version,
        &evidence.attempt_id,
        &evidence.task_id,
        &evidence.phase,
        &evidence.prepared_event_digest,
        &evidence.pre_status,
        &evidence.pre_queue_digest,
        &evidence.intended_post_queue_digest,
        &evidence.policy_identity,
        &evidence.runner_identity,
        &evidence.approved_write_scope,
        &evidence.recorded_by,
        evidence.runner_descriptor_launched,
        evidence.write_scope_exposed,
        &evidence.failure,
        &evidence.timestamp,
    ))
    .map_err(|error| KvistError::TaskQueueUnavailable {
        path: PathBuf::from("."),
        reason: format!("cannot canonicalize pre-spawn failure evidence: {error}"),
    })?;
    Ok(format!("hmac-sha256:{}", hmac_sha256(secret, &payload)))
}

fn recovery_prepared_tag(secret: &[u8], evidence: &RecoveryPreparedAttempt) -> Result<String> {
    let payload = serde_json::to_vec(&(
        evidence.schema_version,
        &evidence.attempt_id,
        &evidence.task_id,
        &evidence.phase,
        &evidence.disposition,
        &evidence.prepared_event_digest,
        &evidence.pre_spawn_failure_event_digest,
        &evidence.pre_status,
        &evidence.fenced_queue_digest,
        &evidence.recovered_queue_digest,
        &evidence.policy_identity,
        &evidence.runner_identity,
        &evidence.approved_write_scope,
        &evidence.timestamp,
        &evidence.recorded_by,
    ))
    .map_err(|error| KvistError::TaskQueueUnavailable {
        path: PathBuf::from("."),
        reason: format!("cannot canonicalize recovery-prepared evidence: {error}"),
    })?;
    Ok(format!("hmac-sha256:{}", hmac_sha256(secret, &payload)))
}

fn recovered_attempt_tag(secret: &[u8], evidence: &RecoveredAttempt) -> Result<String> {
    let payload = serde_json::to_vec(&(
        evidence.schema_version,
        &evidence.attempt_id,
        &evidence.task_id,
        &evidence.phase,
        &evidence.recovery_prepared_event_digest,
        &evidence.recovered_queue_digest,
        &evidence.timestamp,
        &evidence.recorded_by,
    ))
    .map_err(|error| KvistError::TaskQueueUnavailable {
        path: PathBuf::from("."),
        reason: format!("cannot canonicalize recovered evidence: {error}"),
    })?;
    Ok(format!("hmac-sha256:{}", hmac_sha256(secret, &payload)))
}

/// Verification run result
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// True if the test command finished within the timeout and exited with code 0.
    pub success: bool,
    /// The template/command string that was executed.
    pub command: String,
    /// Subprocess exit status code, if any.
    pub exit_code: Option<i32>,
    /// True if the subprocess was killed due to exceeding the timeout limit.
    pub timed_out: bool,
    /// Captured stdout bytes up to max_output_bytes converted to string.
    pub stdout: String,
    /// Captured stderr bytes up to max_output_bytes converted to string.
    pub stderr: String,
}

#[derive(Serialize)]
struct VerificationRecord<'a> {
    phase: &'a str,
    task_id: &'a str,
    timestamp: &'a Timestamp,
    command: &'a str,
    success: bool,
    exit_code: Option<i32>,
    timed_out: bool,
    stdout: &'a str,
    stderr: &'a str,
}

fn append_verification(path: &Path, record: VerificationRecord<'_>) -> Result<()> {
    let encoded =
        serde_json::to_string(&record).map_err(|error| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot serialize verification record: {error}"),
        })?;
    append_encoded_attempt(path, &encoded, "append verification record")
}

/// Verifies the complete effective execution policy before external execution.
pub fn check_execution_approved(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<crate::sandbox::RunnerIdentity> {
    let (approved, _) = load_authenticated_execution_approval(project_dir, config)?;
    let (current, runner) = build_execution_approval(project_dir, config)?;
    ensure_approval_matches(&approved, &current)?;
    Ok(runner)
}

fn load_authenticated_execution_approval(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<(ExecutionApproval, Vec<u8>)> {
    reject_project_approval_record(project_dir)?;
    let (canonical_project, canonical_worktree) =
        project_worktree_identity(project_dir, config.vcs)?;
    let (state_root, approved_path) = approval_state_paths(
        project_dir,
        config.vcs,
        &canonical_project,
        &canonical_worktree,
    )?;
    let secret = load_or_create_approval_secret(&state_root)?;
    let metadata = fs::symlink_metadata(&approved_path).map_err(|error| {
        KvistError::UnapprovedExecutionPolicy {
            reason: if error.kind() == io::ErrorKind::NotFound {
                format!("approval record `{}` is missing", approved_path.display())
            } else {
                format!(
                    "cannot inspect approval record `{}`: {error}",
                    approved_path.display()
                )
            },
        }
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: format!(
                "approval record `{}` must be a regular non-link file",
                approved_path.display()
            ),
        });
    }
    let contents = fs::read_to_string(&approved_path).map_err(|source| KvistError::Io {
        operation: "read execution approval record",
        path: approved_path.clone(),
        source,
    })?;
    let approved: ExecutionApproval =
        serde_json::from_str(&contents).map_err(|error| KvistError::UnapprovedExecutionPolicy {
            reason: format!("approval record is malformed: {error}"),
        })?;
    if approved.schema_version != EXECUTION_APPROVAL_VERSION
        || approved.material.approval_schema_version != EXECUTION_APPROVAL_VERSION
    {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "approval record has an unsupported schema version".to_owned(),
        });
    }
    let recorded_digest = digest(&serde_json::to_vec(&approved.material).map_err(|error| {
        KvistError::UnapprovedExecutionPolicy {
            reason: format!("approval record cannot be canonicalized: {error}"),
        }
    })?);
    if recorded_digest != approved.approval_digest {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "approval record digest is malformed or does not match its contents".to_owned(),
        });
    }
    if approval_tag(&secret, &approved)? != approved.authentication_tag {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "approval record authentication failed".to_owned(),
        });
    }
    if approved.canonical_project != canonical_project
        || approved.canonical_worktree != canonical_worktree
    {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason:
                "canonical project or worktree identity changed since execution policy approval"
                    .to_owned(),
        });
    }
    Ok((approved, secret))
}

fn ensure_approval_matches(
    approved: &ExecutionApproval,
    current: &ExecutionApproval,
) -> Result<()> {
    if approved.material != current.material {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: execution_approval_difference(&approved.material, &current.material),
        });
    }
    Ok(())
}

fn execution_approval_difference(
    approved: &ExecutionApprovalMaterial,
    current: &ExecutionApprovalMaterial,
) -> String {
    if approved.root_contract_digest != current.root_contract_digest {
        "ROOT_CONTRACT.md has changed since execution policy approval".to_owned()
    } else if approved.agent_source != current.agent_source
        || approved.agent_source_digest != current.agent_source_digest
    {
        "agent configuration source identity or digest has changed".to_owned()
    } else if approved.architect_template_digest != current.architect_template_digest
        || approved.architect_token_limit != current.architect_token_limit
        || approved.architect_timeout_seconds != current.architect_timeout_seconds
        || approved.architect_max_output_bytes != current.architect_max_output_bytes
        || approved.architect_redaction_digest != current.architect_redaction_digest
        || approved.developer_template_digest != current.developer_template_digest
        || approved.developer_token_limit != current.developer_token_limit
        || approved.developer_timeout_seconds != current.developer_timeout_seconds
        || approved.developer_max_output_bytes != current.developer_max_output_bytes
        || approved.developer_redaction_digest != current.developer_redaction_digest
        || approved.security_reviewer_template_digest != current.security_reviewer_template_digest
        || approved.security_reviewer_token_limit != current.security_reviewer_token_limit
        || approved.security_reviewer_timeout_seconds != current.security_reviewer_timeout_seconds
        || approved.security_reviewer_max_output_bytes != current.security_reviewer_max_output_bytes
        || approved.security_reviewer_redaction_digest != current.security_reviewer_redaction_digest
    {
        "agent execution configuration has changed".to_owned()
    } else if approved.runner_path != current.runner_path
        || approved.runner_digest != current.runner_digest
    {
        "sandbox runner identity or content has changed".to_owned()
    } else if approved.backend_path != current.backend_path
        || approved.backend_digest != current.backend_digest
    {
        "sandbox enforcement backend identity or content has changed".to_owned()
    } else if approved.sandbox_digest != current.sandbox_digest {
        "sandbox configuration has changed".to_owned()
    } else if approved.test_policy_digest != current.test_policy_digest
        || approved.test_policy_schema_version != current.test_policy_schema_version
    {
        "test policy has changed or is absent".to_owned()
    } else {
        "execution protocol or schema versions have changed".to_owned()
    }
}

fn approved_root_contract_digest(project_dir: &Path) -> Result<String> {
    let path = project_dir.join("ROOT_CONTRACT.md");
    let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
        operation: "inspect root contract for execution approval",
        path: path.clone(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "ROOT_CONTRACT.md must be a regular non-link file".to_owned(),
        });
    }
    if metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: format!(
                "ROOT_CONTRACT.md exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte limit"
            ),
        });
    }
    let contents = fs::read(&path).map_err(|source| KvistError::Io {
        operation: "read root contract for execution approval",
        path,
        source,
    })?;
    Ok(digest(&contents))
}

/// Legacy compatibility wrapper for callers that previously checked only tests.
pub fn check_policy_approved(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<()> {
    check_execution_approved(project_dir, config).map(|_| ())
}

/// Finds a matching test command for the given component utilizing component inheritance.
pub fn find_test_command(
    component_path: &Path,
    policy: &crate::config::TestPolicy,
) -> Option<String> {
    let mut current = component_path.to_path_buf();
    loop {
        let current_str = current.to_str().unwrap_or("");
        let normalized_str = if current_str.is_empty() {
            "."
        } else {
            current_str
        };

        if let Some(entry) = policy.commands.iter().find(|entry| {
            let entry_normalized = if entry.component.is_empty() {
                "."
            } else {
                &entry.component
            };
            entry_normalized == normalized_str
        }) {
            return Some(entry.command.clone());
        }

        if normalized_str == "." {
            break;
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Runs verification through the same required sandbox protocol as agents.
pub fn verify_task(
    component_path: &Path,
    task_id: &str,
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
    probe: &crate::sandbox::SandboxProbe,
    policy_identity: &str,
) -> Result<VerificationResult> {
    let approved_runner = check_execution_approved(project_dir, config)?;
    let policy =
        config
            .test_policy
            .as_ref()
            .ok_or_else(|| KvistError::UnapprovedExecutionPolicy {
                reason: "test policy is absent".to_owned(),
            })?;
    if policy.working_directory != "component" {
        return Err(KvistError::SandboxUnavailable {
            runner: config.sandbox.as_ref().map_or_else(|| "<unconfigured>".to_owned(), |value| value.runner.clone()),
            reason: "the component-only sandbox mount cannot run a project working-directory test policy".to_owned(),
        });
    }
    let sandbox_config = config
        .sandbox
        .as_ref()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: "task execution requires a project-local [sandbox] configuration".to_owned(),
        })?;
    let normalized_component = normalize_component_path(component_path)?;
    let command_str = find_test_command(&normalized_component, policy).ok_or_else(|| {
        KvistError::MissingTestCommand {
            component: component_path.to_string_lossy().into_owned(),
        }
    })?;
    let parts: Vec<&str> = command_str.split_whitespace().collect();
    let Some((program, arguments)) = parts.split_first() else {
        return Err(KvistError::Io {
            operation: "parse approved test command",
            path: PathBuf::from("."),
            source: io::Error::other("empty test command string"),
        });
    };
    let context = validate_context(component_path)?;
    let args = arguments
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let crate::sandbox::ExecutionResult {
        output,
        timed_out,
        output_limit_exceeded,
    } = crate::sandbox::execute_with_timeout(
        sandbox_config,
        crate::sandbox::ExecutionRequest {
            project_root: project_dir,
            vcs_selection: config.vcs,
            component_dir: &context.component_dir,
            phase: crate::sandbox::ExecutionPhase::Verification,
            program,
            arguments: &args,
            environment: crate::sandbox::allowed_environment(
                sandbox_config,
                Some(&policy.environment_allowlist),
            ),
            read_only_mounts: &[],
            backend: &probe.backend,
            policy_identity,
        },
        crate::sandbox::ExecutionOptions {
            timeout: Some(std::time::Duration::from_secs(policy.timeout_seconds)),
            output_limit: Some(policy.max_output_bytes),
        },
        &approved_runner,
    )?;
    let redactions = evidence_redactions(config);
    let command = redact_bounded(command_str, &redactions, MAX_VERIFICATION_EVIDENCE_BYTES);
    let stdout = redact_bounded(
        String::from_utf8_lossy(&output.stdout).into_owned(),
        &redactions,
        policy.max_output_bytes.min(MAX_VERIFICATION_EVIDENCE_BYTES),
    );
    let stderr = redact_bounded(
        String::from_utf8_lossy(&output.stderr).into_owned(),
        &redactions,
        policy.max_output_bytes.min(MAX_VERIFICATION_EVIDENCE_BYTES),
    );
    let success = !timed_out && !output_limit_exceeded && output.status.success();
    let exit_code = output.status.code();
    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let attempt_path = attempt_path(&context.component_dir, task_id)?;
    append_verification(
        &attempt_path,
        VerificationRecord {
            phase: "verification",
            task_id,
            timestamp: &timestamp,
            command: &command,
            success,
            exit_code,
            timed_out,
            stdout: &stdout,
            stderr: &stderr,
        },
    )?;
    Ok(VerificationResult {
        success,
        command,
        exit_code,
        timed_out,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{
        MAX_ATTEMPT_JOURNAL_BYTES, MAX_SCOPE_DEPTH, MAX_SCOPE_ENTRIES, MAX_SCOPE_FILE_BYTES,
        TaskLock, Timestamp, append_encoded_attempt, digest, digest_path_tree,
        read_attempt_journal,
    };

    #[test]
    fn scope_hash_distinguishes_types_and_enforces_file_and_depth_bounds() {
        let directory = TempDir::new().expect("create scope test directory");
        let empty_file = directory.path().join("empty-file");
        let empty_directory = directory.path().join("empty-directory");
        fs::write(&empty_file, []).expect("write empty scope file");
        fs::create_dir(&empty_directory).expect("create empty scope directory");
        assert_ne!(
            digest_path_tree(&empty_file).expect("hash empty file"),
            digest_path_tree(&empty_directory).expect("hash empty directory")
        );

        let oversized = directory.path().join("oversized");
        fs::write(&oversized, vec![0_u8; MAX_SCOPE_FILE_BYTES as usize + 1])
            .expect("write oversized scope file");
        assert!(
            digest_path_tree(&oversized).is_err(),
            "oversized files must not be fully materialized"
        );
        let oversized_journal = directory.path().join("oversized.jsonl");
        fs::write(
            &oversized_journal,
            vec![b'x'; MAX_ATTEMPT_JOURNAL_BYTES as usize + 1],
        )
        .expect("write oversized journal");
        assert!(
            read_attempt_journal(&oversized_journal).is_err(),
            "journal reads must reject a file before it exceeds their bounded handle read"
        );

        let mut deep = directory.path().join("deep");
        fs::create_dir(&deep).expect("create depth root");
        for index in 0..=MAX_SCOPE_DEPTH {
            deep = deep.join(format!("level-{index}"));
            fs::create_dir(&deep).expect("create nested scope directory");
        }
        assert!(
            digest_path_tree(&directory.path().join("deep")).is_err(),
            "scope traversal must stop at its configured depth"
        );
    }

    #[test]
    fn scope_hash_applies_entry_limits_again_during_revalidation() {
        let directory = TempDir::new().expect("create scope revalidation directory");
        for index in 0..=(MAX_SCOPE_ENTRIES / 2) {
            fs::write(directory.path().join(format!("entry-{index:04}")), []).expect("write entry");
        }
        assert!(
            digest_path_tree(directory.path()).is_err(),
            "the exit re-read must consume the same global entry budget"
        );
    }

    #[test]
    #[cfg(unix)]
    fn attempt_journal_append_refuses_link_like_paths() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().expect("create attempt journal directory");
        let target = directory.path().join("target.jsonl");
        let journal = directory.path().join("journal.jsonl");
        fs::write(&target, "retained\n").expect("write target journal");
        symlink(&target, &journal).expect("link attempt journal");

        assert!(
            append_encoded_attempt(&journal, r#"{"phase":"prepared"}"#, "append test journal")
                .is_err(),
            "attempt appends must not follow a journal symlink"
        );
        assert_eq!(
            fs::read_to_string(&target).expect("read retained target journal"),
            "retained\n"
        );
    }

    #[test]
    fn recovery_lock_takeover_refuses_live_or_changed_owners_and_release_preserves_replacement() {
        let directory = TempDir::new().expect("create lock test directory");
        let timestamp = Timestamp::now().expect("read test timestamp");

        let stale_path = directory.path().join("stale.lock");
        let stale_contents = "schema_version: 1\nstarted_at: 2026-09-01T20:30:00Z\ntask_id: \"task\"\npid: 999999\nnonce: 00000000000000000000000000000000\n";
        fs::write(&stale_path, stale_contents).expect("write stale lock");
        let recovered = TaskLock::recover_path(
            &stale_path,
            "task",
            &timestamp,
            &digest(stale_contents.as_bytes()),
        )
        .expect("take over exact stale lock");
        recovered.release().expect("release recovery lock");

        let live_path = directory.path().join("live.lock");
        let live_contents = format!(
            "schema_version: 1\nstarted_at: 2026-09-01T20:30:00Z\ntask_id: \"task\"\npid: {}\nnonce: 11111111111111111111111111111111\n",
            std::process::id()
        );
        fs::write(&live_path, &live_contents).expect("write live lock");
        assert!(
            TaskLock::recover_path(
                &live_path,
                "task",
                &timestamp,
                &digest(live_contents.as_bytes())
            )
            .is_err(),
            "recovery must not take over a lock whose owner appears live"
        );
        assert_eq!(
            fs::read_to_string(&live_path).expect("read retained live lock"),
            live_contents
        );

        let changed_path = directory.path().join("changed.lock");
        fs::write(&changed_path, stale_contents).expect("write changed lock");
        assert!(
            TaskLock::recover_path(
                &changed_path,
                "task",
                &timestamp,
                &digest(b"different-crashed-owner")
            )
            .is_err(),
            "recovery must not take over a different retained lock"
        );
        assert_eq!(
            fs::read_to_string(&changed_path).expect("read retained changed lock"),
            stale_contents
        );

        let malformed_path = directory.path().join("malformed.lock");
        fs::write(&malformed_path, "pid: 999999\n").expect("write malformed lock");
        assert!(
            TaskLock::remove_stale_path(&malformed_path).is_err(),
            "forced unlock must not treat a malformed record as stale"
        );
        assert_eq!(
            fs::read_to_string(&malformed_path).expect("read retained malformed lock"),
            "pid: 999999\n"
        );

        let replacement_path = directory.path().join("replacement.lock");
        let owner =
            TaskLock::create(&replacement_path, "task", &timestamp).expect("create owned lock");
        let displaced = directory.path().join("displaced.lock");
        fs::rename(&replacement_path, &displaced).expect("displace old lock");
        fs::write(&replacement_path, "replacement-owner").expect("write replacement lock");
        assert!(owner.release().is_err());
        assert_eq!(
            fs::read_to_string(&replacement_path).expect("read replacement lock"),
            "replacement-owner"
        );
    }
}
