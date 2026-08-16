//! Durable task selection and serialized state transitions.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use serde::Serialize;

use crate::{
    KvistError, Result,
    discovery::ComponentArtifact,
    file_io::{replace_file_atomically, write_new_file_atomically},
    filesystem::is_link_like,
    project_state::{self, ComponentState, MAX_ROOT_TEXT_ARTIFACT_BYTES, ProjectState},
    task_queue::{Task, TaskQueue, TaskStatus, Timestamp, parse, serialize},
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
    let context = validate_context(component_path)?;
    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock_path = context.component_dir.join(".kvist-task.lock");
    let lock = TaskLock::create(&lock_path, task_id, &started_at)?;

    let result = (|| {
        let context = validate_context(component_path)?;
        let mut queue = read_queue(&context.component_dir)?;
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
        task.timestamps.updated_at = started_at.clone();
        task.timestamps.completed_at =
            (target == TaskStatus::Completed).then(|| started_at.clone());
        task.blocked_reason = (target == TaskStatus::Blocked).then(|| reason.unwrap().to_owned());

        let serialized = serialize(&queue).map_err(|error| KvistError::TaskQueueUnavailable {
            path: context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            reason: error.to_string(),
        })?;
        let attempt_path = attempt_path(&context.component_dir, task_id)?;
        append_attempt(
            &attempt_path,
            AttemptRecord::new("prepared", task_id, from, target, &started_at, reason),
        )?;
        replace_file_atomically(
            &context
                .component_dir
                .join(ComponentArtifact::TaskQueue.filename()),
            &serialized,
        )?;
        append_attempt(
            &attempt_path,
            AttemptRecord::new("committed", task_id, from, target, &started_at, reason),
        )?;
        Ok(format!("transitioned {task_id} to {}", status_name(target)))
    })();

    let release = lock.release();
    match (result, release) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), _) => Err(error),
    }
}

struct TaskContext {
    component_path: PathBuf,
    component_dir: PathBuf,
}

fn validate_context(component_path: &Path) -> Result<TaskContext> {
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
    if component.state != ComponentState::Current {
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
            "the version-2 state machine does not allow this transition",
        ));
    }
    match target {
        TaskStatus::InProgress if !task_is_ready(task, tasks) => {
            return Err(KvistError::TaskNotReady {
                task_id: task.id.clone(),
                reason: "it must be pending with all dependency-chain tasks completed".to_owned(),
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
    released: bool,
}

impl TaskLock {
    fn create(path: &Path, task_id: &str, started_at: &Timestamp) -> Result<Self> {
        let contents = format!("started_at: {started_at}\ntask_id: {task_id:?}\n");
        match write_new_file_atomically(path, &contents) {
            Ok(()) => Ok(Self {
                path: path.to_path_buf(),
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

    fn release(mut self) -> Result<()> {
        self.released = true;
        fs::remove_file(&self.path).map_err(|source| KvistError::Io {
            operation: "remove component task lock",
            path: self.path.clone(),
            source,
        })
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
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(KvistError::Io {
                operation: "append attempt record",
                path: path.to_path_buf(),
                source: io::Error::other("attempt record must be a regular file"),
            });
        }
    }
    let encoded =
        serde_json::to_string(&record).map_err(|error| KvistError::TaskQueueUnavailable {
            path: path.to_path_buf(),
            reason: format!("cannot serialize attempt record: {error}"),
        })?;
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
            operation: "append attempt record",
            path: path.to_path_buf(),
            source,
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
