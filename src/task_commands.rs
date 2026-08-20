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
            "the version-1 state machine does not allow this transition",
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
        if task.kind == TaskKind::Implementation {
            println!("Running test-command verification...");
            match verify_task(component_path, &task_id, &project_dir, &config) {
                Ok(verify_res) => {
                    if verify_res.success {
                        let _ = transition(component_path, &task_id, TaskStatus::Completed, None)?;
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
                        let blocker_reason = format!(
                            "test-command verification failed{}. Command: '{}', Exit code: {:?}.\n---\nStdout:\n{}\n---\nStderr:\n{}",
                            timed_out_msg,
                            verify_res.command,
                            verify_res.exit_code,
                            verify_res.stdout,
                            verify_res.stderr
                        );
                        let _ = transition(
                            component_path,
                            &task_id,
                            TaskStatus::Blocked,
                            Some(&blocker_reason),
                        )?;
                        Ok(format!(
                            "task `{task_id}` failed test-command verification and transitioned to blocked.\nLogs written to: {}",
                            run_result.log_path.display()
                        ))
                    }
                }
                Err(err) => {
                    let blocker_reason = format!("test-command verification blocked: {err}");
                    let _ = transition(
                        component_path,
                        &task_id,
                        TaskStatus::Blocked,
                        Some(&blocker_reason),
                    )?;
                    Ok(format!(
                        "task `{task_id}` verification blocked and transitioned to blocked: {err}"
                    ))
                }
            }
        } else {
            // Transition to Completed
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
        }
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

/// Approve the current test-command policy by recording its canonical hash locally.
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
    let Some(policy) = &config.test_policy else {
        return Err(KvistError::InvalidProjectConfiguration {
            path: project_dir.join("kvist.toml"),
            reason: "no `[test_policy]` section found in project configuration".to_owned(),
        });
    };

    let current_hash = crate::config::compute_policy_hash(policy);

    let kvist_dir = project_dir.join(".kvist");
    if !kvist_dir.exists() {
        fs::create_dir_all(&kvist_dir).map_err(|source| KvistError::Io {
            operation: "create .kvist directory",
            path: kvist_dir.clone(),
            source,
        })?;
    }

    let approved_path = kvist_dir.join("approved_policy.sha256");
    fs::write(&approved_path, &current_hash).map_err(|source| KvistError::Io {
        operation: "write approved policy hash",
        path: approved_path.clone(),
        source,
    })?;

    Ok(format!(
        "Successfully approved test-command policy with hash: {current_hash}"
    ))
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

/// Verifies whether the current test policy has been approved.
pub fn check_policy_approved(
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<()> {
    let Some(policy) = &config.test_policy else {
        return Err(KvistError::UnapprovedTestPolicy {
            current_hash: "none".to_owned(),
            expected_hash: None,
        });
    };

    let current_hash = crate::config::compute_policy_hash(policy);
    let approved_path = project_dir.join(".kvist").join("approved_policy.sha256");
    if !approved_path.exists() {
        return Err(KvistError::UnapprovedTestPolicy {
            current_hash,
            expected_hash: None,
        });
    }

    let approved_hash = fs::read_to_string(&approved_path)
        .map_err(|source| KvistError::Io {
            operation: "read approved policy hash",
            path: approved_path.clone(),
            source,
        })?
        .trim()
        .to_owned();

    if current_hash != approved_hash {
        return Err(KvistError::UnapprovedTestPolicy {
            current_hash,
            expected_hash: Some(approved_hash),
        });
    }

    Ok(())
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

/// Runs verification on a component task using the approved test command.
pub fn verify_task(
    component_path: &Path,
    task_id: &str,
    project_dir: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<VerificationResult> {
    // 1. Verify policy is approved
    check_policy_approved(project_dir, config)?;

    let policy = config.test_policy.as_ref().unwrap();

    // 2. Locate the command
    let normalized_component = normalize_component_path(component_path)?;
    let command_str = find_test_command(&normalized_component, policy).ok_or_else(|| {
        KvistError::MissingTestCommand {
            component: component_path.to_string_lossy().into_owned(),
        }
    })?;

    // 3. Resolve working directory
    let context = validate_context(component_path)?;
    let working_dir = match policy.working_directory.as_str() {
        "component" => context.component_dir.clone(),
        _ => project_dir.to_path_buf(),
    };

    // 4. Split and prepare command
    let parts: Vec<&str> = command_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err(KvistError::Io {
            operation: "parse approved test command",
            path: PathBuf::from("."),
            source: io::Error::other("empty test command string"),
        });
    }
    let program = parts[0];
    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(&args);
    cmd.current_dir(&working_dir);
    cmd.env_clear();
    for env_var in &policy.environment_allowlist {
        if let Ok(val) = std::env::var(env_var) {
            cmd.env(env_var, val);
        }
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // 5. Spawn test subprocess
    let mut child = cmd.spawn().map_err(|source| KvistError::Io {
        operation: "spawn approved test command",
        path: PathBuf::from(program),
        source,
    })?;

    let mut stdout_pipe = child.stdout.take().ok_or_else(|| KvistError::Io {
        operation: "take test command stdout",
        path: PathBuf::from(program),
        source: io::Error::other("cannot take stdout pipe"),
    })?;

    let mut stderr_pipe = child.stderr.take().ok_or_else(|| KvistError::Io {
        operation: "take test command stderr",
        path: PathBuf::from(program),
        source: io::Error::other("cannot take stderr pipe"),
    })?;

    let max_output_bytes = policy.max_output_bytes;

    // Spawn reader threads
    use std::io::Read;
    use std::thread;

    let stdout_handle = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut buffer = [0u8; 1024];
        let mut out = Vec::new();
        loop {
            if out.len() >= max_output_bytes {
                break;
            }
            let chunk_size = std::cmp::min(buffer.len(), max_output_bytes - out.len());
            let bytes_read = stdout_pipe.read(&mut buffer[..chunk_size])?;
            if bytes_read == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..bytes_read]);
        }
        Ok(out)
    });

    let stderr_handle = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut buffer = [0u8; 1024];
        let mut err = Vec::new();
        loop {
            if err.len() >= max_output_bytes {
                break;
            }
            let chunk_size = std::cmp::min(buffer.len(), max_output_bytes - err.len());
            let bytes_read = stderr_pipe.read(&mut buffer[..chunk_size])?;
            if bytes_read == 0 {
                break;
            }
            err.extend_from_slice(&buffer[..bytes_read]);
        }
        Ok(err)
    });

    // Wait with timeout
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(policy.timeout_seconds);
    let mut exit_status = None;
    let mut timed_out = false;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_status = Some(status);
                break;
            }
            Ok(None) => {
                if start_time.elapsed() >= timeout {
                    let _ = child.kill();
                    timed_out = true;
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "wait for test command",
                    path: PathBuf::from(program),
                    source,
                });
            }
        }
    }

    // Join reader threads
    let stdout_bytes = match stdout_handle.join() {
        Ok(Ok(bytes)) => bytes,
        _ => Vec::new(),
    };

    let stderr_bytes = match stderr_handle.join() {
        Ok(Ok(bytes)) => bytes,
        _ => Vec::new(),
    };

    let stdout_str = String::from_utf8_lossy(&stdout_bytes).into_owned();
    let stderr_str = String::from_utf8_lossy(&stderr_bytes).into_owned();

    let success = !timed_out && exit_status.is_some_and(|s| s.success());
    let exit_code = exit_status.and_then(|s| s.code());

    // 6. Record results against task attempt
    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let attempt_path = attempt_path(&context.component_dir, task_id)?;
    append_verification(
        &attempt_path,
        VerificationRecord {
            phase: "verification",
            task_id,
            timestamp: &timestamp,
            command: &command_str,
            success,
            exit_code,
            timed_out,
            stdout: &stdout_str,
            stderr: &stderr_str,
        },
    )?;

    Ok(VerificationResult {
        success,
        command: command_str,
        exit_code,
        timed_out,
        stdout: stdout_str,
        stderr: stderr_str,
    })
}
