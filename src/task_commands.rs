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
    let context = validate_context(component_path)?;
    let started_at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock_path = context.component_dir.join(".kvist-task.lock");
    let lock = TaskLock::create(&lock_path, task_id, &started_at)?;

    let result = (|| {
        let context = validate_context(component_path)?;
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
    let existed = if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(KvistError::Io {
                operation: "append attempt record",
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
    let lock_path = context.component_dir.join(".kvist-task.lock");
    let lock = TaskLock::create(&lock_path, "accept", &started_at)?;

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
        component_dir: project_dir.join(component_root).join(&component_path),
        component_path,
    })
}

/// Launches the external agent to execute a task, transitions the task to InProgress,
/// captures logs, parses token usage, and transitions the task to Completed/Blocked based on exit.
pub fn run_task(component_path: &Path, task_id_opt: Option<&str>, stream: bool) -> Result<String> {
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
        // Transition to InProgress
        let _ = transition(component_path, &task_id, TaskStatus::InProgress, None)?;
        // Re-read queue to reflect transition
        queue = read_queue(&context.component_dir)?;
    }

    let task = &queue.tasks[task_index];

    // 3. Load agent profile matching task type
    let project_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let config = crate::config::load(&project_dir)?;
    let agent_profile = match task.kind {
        TaskKind::Test | TaskKind::Implementation => &config.agent.developer,
        TaskKind::SecurityAudit | TaskKind::ComplianceReview => &config.agent.architect,
    };

    // 4. Sliced context files gathering
    let spec_path = context
        .component_dir
        .join(ComponentArtifact::Specification.filename());
    let queue_path = context
        .component_dir
        .join(ComponentArtifact::TaskQueue.filename());
    let root_contract = project_dir.join("ROOT_CONTRACT.md");
    let mut context_files = vec![spec_path, queue_path, root_contract];

    // If there is a parent specification, add it to context too
    if queue.component.parent_specification.is_some() {
        let parent_spec = context.component_dir.join("../SPEC.md");
        if parent_spec.exists() {
            context_files.push(parent_spec);
        }
    }

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
        &prompt,
        &context_files,
        &context.component_dir,
        &task_id,
        stream,
    )?;

    // 7. Transition task status depending on outcome
    if run_result.success {
        // Transition to Completed
        // In P2-05, we will add test-command verification, but for now we transition to Completed!
        let _ = transition(component_path, &task_id, TaskStatus::Completed, None)?;

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
    } else {
        // Transition to Blocked
        let blocker_reason = format!(
            "agent failed during task execution. Raw execution logs are written to: {}",
            run_result.log_path.display()
        );
        let _ = transition(
            component_path,
            &task_id,
            TaskStatus::Blocked,
            Some(&blocker_reason),
        )?;
        Ok(format!(
            "task `{task_id}` failed during execution and has been transitioned to blocked.\nLogs written to: {}",
            run_result.log_path.display()
        ))
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

    if !logs_dir.is_dir() {
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
            log_files.push(entry.path());
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
