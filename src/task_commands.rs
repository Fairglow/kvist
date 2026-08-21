//! Durable task selection and serialized state transitions.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    KvistError, Result,
    discovery::ComponentArtifact,
    file_io::{replace_file_atomically, sync_directory, write_new_file_atomically},
    filesystem::is_link_like,
    project_state::{self, ComponentState, MAX_ROOT_TEXT_ARTIFACT_BYTES, ProjectState},
    task_queue::{Task, TaskKind, TaskQueue, TaskStatus, Timestamp, parse, serialize},
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
    ensure_attempt_recovered(&context.component_dir, task_id)?;
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
            summary: inspection.vcs.summary,
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
    parse(&contents).map_err(|error| KvistError::TaskQueueUnavailable {
        path,
        reason: error.to_string(),
    })
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
    released: bool,
}

impl TaskLock {
    fn for_context(context: &TaskContext, task_id: &str, started_at: &Timestamp) -> Result<Self> {
        Self::create(&Self::task_lock_path(context)?, task_id, started_at)
    }

    fn create(path: &Path, task_id: &str, started_at: &Timestamp) -> Result<Self> {
        let contents = format!("started_at: {started_at}\ntask_id: {task_id:?}\n");
        match write_new_file_atomically(path, &contents) {
            Ok(()) => Ok(Self {
                path: path.to_path_buf(),
                contents,
                released: false,
            }),
            Err(KvistError::Io { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists => {
                Err(KvistError::TaskLockExists {
                    path: path.to_path_buf(),
                    task_id: task_id.to_owned(),
                })
            }
            Err(error) => Err(error),
        }
    }

    fn revalidate(&self) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.path).map_err(|source| {
            KvistError::TaskQueueUnavailable {
                path: self.path.clone(),
                reason: format!("cannot revalidate task lock ownership: {source}"),
            }
        })?;
        if is_link_like(&metadata) || !metadata.file_type().is_file() {
            return Err(KvistError::TaskQueueUnavailable {
                path: self.path.clone(),
                reason: "task lock ownership was replaced".to_owned(),
            });
        }
        let contents =
            fs::read_to_string(&self.path).map_err(|source| KvistError::TaskQueueUnavailable {
                path: self.path.clone(),
                reason: format!("cannot read task lock ownership: {source}"),
            })?;
        if contents != self.contents {
            return Err(KvistError::TaskQueueUnavailable {
                path: self.path.clone(),
                reason: "task lock ownership changed".to_owned(),
            });
        }
        Ok(())
    }

    fn release(mut self) -> Result<()> {
        self.released = true;
        fs::remove_file(&self.path).map_err(|source| KvistError::Io {
            operation: "remove user-owned task lock",
            path: self.path.clone(),
            source,
        })
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
            let _ = fs::remove_file(&self.path);
        }
    }
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
    let existed = if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(KvistError::Io {
                operation,
                path: path.to_path_buf(),
                source: io::Error::other("attempt record must be a regular file"),
            });
        }
        true
    } else {
        false
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| KvistError::Io {
            operation: "open attempt record",
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(encoded.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        })?;
    if !existed {
        let parent = path.parent().ok_or_else(|| KvistError::Io {
            operation: "determine attempt record parent",
            path: path.to_path_buf(),
            source: io::Error::other("attempt record has no parent"),
        })?;
        sync_directory(parent)?;
    }
    Ok(())
}

fn ensure_attempt_recovered(component_dir: &Path, task_id: &str) -> Result<()> {
    let path = component_dir
        .join(".kvist-attempts")
        .join(format!("{task_id}.jsonl"));
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "read attempt record",
                path,
                source,
            });
        }
    };
    if contents
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.contains(r#""phase":"prepared""#))
    {
        return Err(KvistError::TaskQueueUnavailable {
            path,
            reason: "a prepared task attempt requires explicit recovery before another transition"
                .to_owned(),
        });
    }
    Ok(())
}

fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Completed => "completed",
    }
}

/// Revalidates the component specification, updating the specification hash inside TODOS.yaml
/// and resetting the revalidation state back to Current.
pub fn accept(component_path: &Path) -> Result<String> {
    let context = validate_accept_context(component_path)?;
    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(&context, "accept", &started_at)?;

    let result = (|| {
        let context = validate_accept_context(component_path)?;
        let mut queue = read_queue(&context.component_dir)?;

        // 1. Read and validate the local specification
        let spec_path = context
            .component_dir
            .join(ComponentArtifact::Specification.filename());
        let spec_metadata = fs::symlink_metadata(&spec_path).map_err(|source| KvistError::Io {
            operation: "inspect component specification",
            path: spec_path.clone(),
            source,
        })?;
        if is_link_like(&spec_metadata) || !spec_metadata.file_type().is_file() {
            return Err(KvistError::SpecificationValidationFailed {
                path: spec_path,
                diagnostics: "it is not a regular non-link file".to_owned(),
            });
        }
        if spec_metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
            return Err(KvistError::SpecificationValidationFailed {
                path: spec_path,
                diagnostics: format!(
                    "it exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte component artifact limit"
                ),
            });
        }
        let spec_contents = fs::read_to_string(&spec_path).map_err(|source| KvistError::Io {
            operation: "read component specification",
            path: spec_path.clone(),
            source,
        })?;

        // Rigorously validate specification structure
        let spec_validation = crate::specification::validate(&spec_contents);
        if !spec_validation.is_valid() {
            return Err(KvistError::SpecificationValidationFailed {
                path: spec_path,
                diagnostics: crate::specification::format_diagnostics(&spec_validation.diagnostics),
            });
        }

        // 2. Compute the new specification SHA-256 revision
        use sha2::{Digest, Sha256};
        let new_hash = format!("sha256:{:x}", Sha256::digest(spec_contents.as_bytes()));

        // 3. If there is an immediate parent, update its revision to current parent's revision
        if let Some(ref mut parent) = queue.component.parent_specification {
            let parent_spec_path = context.component_dir.join("../SPEC.md");
            let parent_metadata =
                fs::symlink_metadata(&parent_spec_path).map_err(|source| KvistError::Io {
                    operation: "inspect parent specification",
                    path: parent_spec_path.clone(),
                    source,
                })?;
            if is_link_like(&parent_metadata) || !parent_metadata.file_type().is_file() {
                return Err(KvistError::SpecificationValidationFailed {
                    path: parent_spec_path,
                    diagnostics: "parent specification is not a regular non-link file".to_owned(),
                });
            }
            if parent_metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
                return Err(KvistError::SpecificationValidationFailed {
                    path: parent_spec_path,
                    diagnostics: format!(
                        "parent specification exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte component artifact limit"
                    ),
                });
            }
            let parent_contents =
                fs::read_to_string(&parent_spec_path).map_err(|source| KvistError::Io {
                    operation: "read parent specification",
                    path: parent_spec_path.clone(),
                    source,
                })?;
            // Rigorously validate parent specification structure too
            let parent_validation = crate::specification::validate(&parent_contents);
            if !parent_validation.is_valid() {
                return Err(KvistError::SpecificationValidationFailed {
                    path: parent_spec_path,
                    diagnostics: crate::specification::format_diagnostics(
                        &parent_validation.diagnostics,
                    ),
                });
            }
            let parent_hash = format!("sha256:{:x}", Sha256::digest(parent_contents.as_bytes()));
            parent.revision = parent_hash;
        }

        // 4. Update the queue component state and clear revalidation causes
        queue.component.specification_revision = new_hash.clone();
        queue.component.revalidation.state = crate::task_queue::RevalidationState::Current;
        queue.component.revalidation.checked_at = started_at.clone();
        queue.component.revalidation.stale_since = None;
        queue.component.revalidation.causes = Vec::new();

        // 5. Serialize and replace the YAML queue atomically
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

        Ok(format!(
            "accepted specification change for component {}",
            context.component_path.display()
        ))
    })();

    let release = lock.release();
    match (result, release) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

fn validate_accept_context(component_path: &Path) -> Result<TaskContext> {
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
            summary: inspection.vcs.summary,
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

    // We allow Current, Stale, and Blocked component states for accepting spec changes.
    let is_allowed = matches!(
        component.state,
        ComponentState::Current | ComponentState::Stale | ComponentState::Blocked
    );
    if !is_allowed {
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

/// Launches the external agent to execute a task, transitions the task to InProgress,
/// captures logs, parses token usage, and transitions the task to Completed/Blocked based on exit.
pub fn run_task(component_path: &Path, task_id_opt: Option<&str>, stream: bool) -> Result<String> {
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
    let approved_runner = check_execution_approved(&project_dir, &config)?;
    if config.test_policy.is_none() {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: "test policy is absent".to_owned(),
        });
    }
    let sandbox_config = config
        .sandbox
        .as_ref()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: "task execution requires a project-local [sandbox] configuration".to_owned(),
        })?;
    crate::sandbox::ensure_available(sandbox_config, &project_dir, config.vcs, &approved_runner)?;
    let context = validate_context(component_path)?;
    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    // This single lock covers selection, execution, evidence, and the terminal
    // transition. Its provisional task ID deliberately prevents a second runner
    // from selecting the same ready task.
    let lock = TaskLock::for_context(&context, task_id_opt.unwrap_or("run"), &started_at)?;
    let result = (|| {
        let context = validate_context(component_path)?;
        let mut queue = read_queue(&context.component_dir)?;

        // 1. Determine the task to run
        let task_id = match task_id_opt {
            Some(id) => id.to_owned(),
            None => {
                // Pick next ready task
                let ready_tasks = get_ready_tasks(&queue);
                let Some(first_ready) = ready_tasks.first() else {
                    return Err(KvistError::TaskQueueUnavailable {
                        path: context
                            .component_dir
                            .join(ComponentArtifact::TaskQueue.filename()),
                        reason: "no ready tasks available in the queue".to_owned(),
                    });
                };
                first_ready.id.clone()
            }
        };

        // Verify task is ready/valid to run
        let task_index = queue
            .tasks
            .iter()
            .position(|t| t.id == task_id)
            .ok_or_else(|| KvistError::TaskQueueUnavailable {
                path: context
                    .component_dir
                    .join(ComponentArtifact::TaskQueue.filename()),
                reason: format!("task `{task_id}` not found in the queue"),
            })?;

        let task = &queue.tasks[task_index];

        // Ensure task is ready or in-progress
        let ready_ids: Vec<String> = get_ready_tasks(&queue).into_iter().map(|t| t.id).collect();
        if task.status != TaskStatus::InProgress && !ready_ids.contains(&task_id) {
            return Err(KvistError::TaskQueueUnavailable {
                path: context
                    .component_dir
                    .join(ComponentArtifact::TaskQueue.filename()),
                reason: format!("task `{task_id}` is not ready for execution"),
            });
        }

        // 2. Transition task status to InProgress atomically (if not already InProgress)
        if task.status != TaskStatus::InProgress {
            let transition_at =
                Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
            let _ = transition_locked(
                &context,
                &lock,
                &task_id,
                TaskStatus::InProgress,
                None,
                &transition_at,
            )?;
            // Re-read queue to reflect transition
            queue = read_queue(&context.component_dir)?;
        }

        let task = &queue.tasks[task_index];

        // 3. Load agent profile matching task type.
        let agent_profile = match task.kind {
            TaskKind::Test | TaskKind::Implementation => &config.agent.developer,
            TaskKind::SecurityAudit | TaskKind::ComplianceReview => &config.agent.architect,
        };

        // 4. Sliced context files gathering
        // The sandbox receives only paths in its component mount. Root and parent
        // files are described by task text, never exposed as host-path context.
        let context_files = vec![
            PathBuf::from("/workspace/component/SPEC.md"),
            PathBuf::from("/workspace/component/TODOS.yaml"),
            PathBuf::from("/workspace/component/IMPL.md"),
        ];

        // 5. Build prompt
        let prompt = format!(
            "Task Details:\n\
         - ID: {}\n\
         - Title: {}\n\
         - Role/Kind: {:?}\n\
         - Description: {}\n\
         - Context: {}\n\
         - Purpose: {}\n\
         - Expected Outcome: {}\n\n\
         Instructions:\n\
         You are the developer agent tasked with executing the task above. \n\
         Ensure all invariants defined in SPEC.md are maintained. \n\
         Fulfill all task requirements. When finished, write your results.",
            task.id,
            task.title,
            task.kind,
            task.description,
            task.context,
            task.purpose,
            task.expected_outcome
        );

        println!("Running task `{task_id}` via external agent...");

        // 6. Execute agent
        let run_result = crate::agent::execute_agent(
            agent_profile,
            sandbox_config,
            &approved_runner,
            crate::agent::AgentExecutionRequest {
                project_root: &project_dir,
                vcs_selection: config.vcs,
                prompt: &prompt,
                context_paths: &context_files,
                target_dir: &context.component_dir,
                task_id: &task_id,
                stream_output: stream,
            },
        )?;
        let agent_timestamp =
            Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
        let agent_attempt_path = attempt_path(&context.component_dir, &task_id)?;
        append_agent_execution(
            &agent_attempt_path,
            AgentExecutionRecord {
                phase: "agent-execution",
                task_id: &task_id,
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
                println!("Running test-command verification...");
                match verify_task(component_path, &task_id, &project_dir, &config) {
                    Ok(verify_res) => {
                        if verify_res.success {
                            let transition_at = Timestamp::now()
                                .map_err(|source| KvistError::TaskClock { source })?;
                            let _ = transition_locked(
                                &context,
                                &lock,
                                &task_id,
                                TaskStatus::Completed,
                                None,
                                &transition_at,
                            )?;
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
                                "task `{task_id}` executed and verified successfully and transitioned to completed.{}\nLogs written to: {}",
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
                                &task_id,
                                TaskStatus::Blocked,
                                Some(&blocker_reason),
                                &transition_at,
                            )?;
                            Ok(format!(
                                "task `{task_id}` failed test-command verification and transitioned to blocked.\nLogs written to: {}",
                                run_result.log_path.display()
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
                            &task_id,
                            TaskStatus::Blocked,
                            Some(&blocker_reason),
                            &transition_at,
                        )?;
                        Ok(format!(
                            "task `{task_id}` verification blocked and transitioned to blocked: {verification_error}"
                        ))
                    }
                }
            } else {
                let transition_at =
                    Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
                let _ = transition_locked(
                    &context,
                    &lock,
                    &task_id,
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
                &task_id,
                TaskStatus::Blocked,
                Some(&blocker_reason),
                &transition_at,
            )?;
            Ok(format!(
                "task `{task_id}` {} and has been transitioned to blocked.\nLogs written to: {}",
                if run_result.timed_out {
                    "timed out"
                } else if run_result.output_limit_exceeded {
                    "exceeded the combined output limit"
                } else {
                    "failed during execution"
                },
                run_result.log_path.display(),
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

fn get_ready_tasks(queue: &TaskQueue) -> Vec<Task> {
    // A task is ready if its status is Pending or InProgress, and all its depends_on tasks are Completed
    let mut ready = Vec::new();
    for task in &queue.tasks {
        if task.status == TaskStatus::Pending || task.status == TaskStatus::InProgress {
            // Check dependencies
            let mut deps_complete = true;
            for dep_id in &task.depends_on {
                if let Some(dep_task) = queue.tasks.iter().find(|t| &t.id == dep_id) {
                    if dep_task.status != TaskStatus::Completed {
                        deps_complete = false;
                        break;
                    }
                } else {
                    deps_complete = false;
                    break;
                }
            }
            if deps_complete {
                ready.push(task.clone());
            }
        }
    }
    ready
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
    sandbox_digest: String,
    runner_path: String,
    runner_digest: String,
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
    let (canonical_project, canonical_worktree) =
        project_worktree_identity(project_dir, config.vcs)?;
    let material = ExecutionApprovalMaterial {
        configuration_schema_version: crate::artifacts::CONFIGURATION_VERSION,
        approval_schema_version: EXECUTION_APPROVAL_VERSION,
        sandbox_protocol_version: crate::sandbox::PROTOCOL_VERSION,
        sandbox_schema_version: 1,
        agent_source: config.agent.source.identity.clone(),
        agent_source_digest: config.agent.source.digest.clone(),
        architect_template_digest: digest(config.agent.architect.command_template.as_bytes()),
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
        developer_template_digest: digest(config.agent.developer.command_template.as_bytes()),
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
        sandbox_digest: digest(&serde_json::to_vec(sandbox).map_err(|error| {
            KvistError::UnapprovedExecutionPolicy {
                reason: format!("cannot serialize sandbox configuration: {error}"),
            }
        })?),
        runner_path: runner.canonical_path.clone(),
        runner_digest: runner.digest.clone(),
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
    Ok((
        ExecutionApproval {
            schema_version: EXECUTION_APPROVAL_VERSION,
            canonical_project,
            canonical_worktree,
            material,
            approval_digest,
            authentication_tag: String::new(),
        },
        runner,
    ))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
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
            path,
            source,
        }),
    }
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
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options.open(path) {
        Ok(mut file) => file
            .write_all(secret)
            .and_then(|()| file.sync_all())
            .map_err(|source| KvistError::Io {
                operation: "write approval secret",
                path: path.to_path_buf(),
                source,
            }),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(KvistError::Io {
            operation: "create approval secret",
            path: path.to_path_buf(),
            source,
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
    format!("{:x}", outer.finalize())
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
    let existed = if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(KvistError::Io {
                operation: "append verification record",
                path: path.to_path_buf(),
                source: io::Error::other("attempt record must be a regular file"),
            });
        }
        true
    } else {
        false
    };
    let encoded =
        serde_json::to_string(&record).map_err(|error| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot serialize verification record: {error}"),
        })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| KvistError::Io {
            operation: "open verification record",
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(encoded.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| KvistError::Io {
            operation: "append verification record",
            path: path.to_path_buf(),
            source,
        })?;
    if !existed {
        let parent = path.parent().ok_or_else(|| KvistError::Io {
            operation: "determine verification record parent",
            path: path.to_path_buf(),
            source: io::Error::other("verification record has no parent"),
        })?;
        sync_directory(parent)?;
    }
    Ok(())
}

/// Verifies the complete effective execution policy before external execution.
pub fn check_execution_approved(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<crate::sandbox::RunnerIdentity> {
    reject_project_approval_record(project_dir)?;
    let (current, runner) = build_execution_approval(project_dir, config)?;
    let (state_root, approved_path) = approval_state_paths(
        project_dir,
        config.vcs,
        &current.canonical_project,
        &current.canonical_worktree,
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
    if approved.canonical_project != current.canonical_project
        || approved.canonical_worktree != current.canonical_worktree
        || approved.material != current.material
    {
        return Err(KvistError::UnapprovedExecutionPolicy {
            reason: execution_approval_difference(&approved.material, &current.material),
        });
    }
    Ok(runner)
}

fn execution_approval_difference(
    approved: &ExecutionApprovalMaterial,
    current: &ExecutionApprovalMaterial,
) -> String {
    if approved.agent_source != current.agent_source
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
    {
        "agent execution configuration has changed".to_owned()
    } else if approved.runner_path != current.runner_path
        || approved.runner_digest != current.runner_digest
    {
        "sandbox runner identity or content has changed".to_owned()
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
            program,
            arguments: &args,
            environment: crate::sandbox::allowed_environment(
                sandbox_config,
                Some(&policy.environment_allowlist),
            ),
            context_files: &[],
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
