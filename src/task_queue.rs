//! Versioned, durable task-queue parsing and semantic validation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::artifacts::TODO_QUEUE_VERSION;

const MAX_TASK_ID_CHARS: usize = 64;
const MAX_TITLE_CHARS: usize = 120;
const MAX_DETAIL_CHARS: usize = 4096;
const SHA256_PREFIX: &str = "sha256:";
const SHA256_HEX_LENGTH: usize = 64;

/// A parsed version-2 component task queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskQueue {
    /// Schema version used to parse this durable queue.
    pub schema_version: u32,
    /// Component-level revision and revalidation evidence.
    pub component: ComponentState,
    /// Tasks in the explicit deterministic authoring order.
    pub tasks: Vec<Task>,
}

/// The specification revisions against which a component plan was reviewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentState {
    /// SHA-256 revision of this component's reviewed `SPEC.md`.
    pub specification_revision: String,
    /// Immediate-parent specification revision, or `null` for the root.
    pub parent_specification: Option<ParentSpecification>,
    /// Evidence that determines whether tasks may be selected.
    pub revalidation: Revalidation,
}

/// The only upstream specification allowed in a component context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParentSpecification {
    /// Relative path to the immediate parent's specification.
    pub path: String,
    /// SHA-256 revision reviewed by this queue.
    pub revision: String,
}

/// Current or stale revalidation evidence for a component plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revalidation {
    /// Whether this plan is eligible for task selection.
    pub state: RevalidationState,
    /// Most recent time a revision comparison completed.
    pub checked_at: Timestamp,
    /// First time the current stale state was recorded.
    pub stale_since: Option<Timestamp>,
    /// Exact revision differences that require revalidation.
    pub causes: Vec<StalenessCause>,
}

/// Revalidation eligibility for the complete component plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevalidationState {
    /// Reviewed specifications still match recorded revisions.
    Current,
    /// A local or immediate-parent specification revision changed.
    Stale,
}

/// Attributable evidence of a specification revision mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StalenessCause {
    /// Whether the changed specification is local or the immediate parent.
    pub kind: StalenessCauseKind,
    /// Component-relative specification path that was compared.
    pub path: String,
    /// Revision recorded when this queue was reviewed.
    pub expected_revision: String,
    /// Revision observed during later inspection.
    pub observed_revision: String,
}

/// The two specification changes that can stale a component plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StalenessCauseKind {
    /// The component's own `SPEC.md` changed.
    ComponentSpecificationRevisionChanged,
    /// The immediate parent's `SPEC.md` changed.
    ParentSpecificationRevisionChanged,
}

/// One actionable, traceable unit of component work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Stable, queue-local task identity.
    pub id: String,
    /// One-line human-readable task label.
    pub title: String,
    /// Bounded description of the work to perform.
    pub description: String,
    /// Background or condition that made the task necessary.
    pub context: String,
    /// Architectural value or risk addressed by the task.
    pub purpose: String,
    /// Observable completion condition.
    pub expected_outcome: String,
    /// Lifecycle role controlling execution and review ordering.
    pub kind: TaskKind,
    /// Durable workflow state.
    pub status: TaskStatus,
    /// Earlier queue-local tasks that must complete first.
    pub depends_on: Vec<String>,
    /// Requirement locators supplying traceable task intent.
    pub requirements: Vec<String>,
    /// Durable transition timestamps.
    pub timestamps: TaskTimestamps,
    /// Actionable blocker explanation when the task is blocked.
    pub blocked_reason: Option<String>,
}

/// Mandatory lifecycle roles for an executable task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    /// Creates or updates executable verification before behavior changes.
    Test,
    /// Changes the component to meet its specified outcome.
    Implementation,
    /// Independently examines security-relevant boundaries and invariants.
    SecurityAudit,
    /// Performs the clean-slate and source-blind compliance lifecycle.
    ComplianceReview,
}

/// The finite, durable states of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    /// The task is defined but no attempt is active.
    Pending,
    /// One authorized attempt is active.
    InProgress,
    /// The task cannot progress until its recorded blocker is resolved.
    Blocked,
    /// The task achieved its expected outcome and retains its evidence.
    Completed,
}

impl TaskStatus {
    /// Returns whether a durable state update may move from `self` to `next`.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::InProgress | Self::Blocked)
                | (
                    Self::InProgress,
                    Self::Pending | Self::Blocked | Self::Completed
                )
                | (Self::Blocked, Self::Pending | Self::InProgress)
        )
    }
}

/// Timestamps that make a task's state understandable without file metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTimestamps {
    /// Creation time of this version-2 task record.
    pub created_at: Timestamp,
    /// Most recent durable task update.
    pub updated_at: Timestamp,
    /// Completion transition time, present only for completed tasks.
    pub completed_at: Option<Timestamp>,
}

/// A canonical whole-second UTC RFC 3339 instant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(String);

impl Timestamp {
    fn validate(&self, field: &str) -> std::result::Result<(), TaskQueueError> {
        let value = self.0.as_bytes();
        if value.len() != 20
            || value[4] != b'-'
            || value[7] != b'-'
            || value[10] != b'T'
            || value[13] != b':'
            || value[16] != b':'
            || value[19] != b'Z'
            || !value
                .iter()
                .enumerate()
                .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19))
                .all(|(_, byte)| byte.is_ascii_digit())
        {
            return Err(TaskQueueError::invalid(format!(
                "`{field}` must be a UTC RFC 3339 instant with whole seconds"
            )));
        }

        let year = decimal(value, 0, 4);
        let month = decimal(value, 5, 7);
        let day = decimal(value, 8, 10);
        let hour = decimal(value, 11, 13);
        let minute = decimal(value, 14, 16);
        let second = decimal(value, 17, 19);
        if year == 0
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(TaskQueueError::invalid(format!(
                "`{field}` is not a real UTC calendar instant"
            )));
        }
        Ok(())
    }
}

/// A parser, validation, or serialization error for `TODOS.yaml`.
#[derive(Debug, Error)]
pub enum TaskQueueError {
    /// YAML does not conform to the typed version-2 shape.
    #[error("invalid TODO queue YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// A well-formed queue declares a version this binary cannot use.
    #[error("unsupported TODO queue version {found}; this binary supports version {supported}")]
    UnsupportedVersion {
        /// Declared schema version.
        found: u32,
        /// Supported schema version.
        supported: u32,
    },
    /// A parsed queue violates its semantic contract.
    #[error("invalid TODO queue: {0}")]
    Invalid(String),
}

impl TaskQueueError {
    fn invalid(reason: impl Into<String>) -> Self {
        Self::Invalid(reason.into())
    }
}

/// Parses and fully validates a version-2 queue without reading the filesystem.
pub fn parse(contents: &str) -> std::result::Result<TaskQueue, TaskQueueError> {
    let version: VersionProbe = serde_yaml::from_str(contents)?;
    if version.schema_version != TODO_QUEUE_VERSION {
        return Err(TaskQueueError::UnsupportedVersion {
            found: version.schema_version,
            supported: TODO_QUEUE_VERSION,
        });
    }
    let queue: TaskQueue = serde_yaml::from_str(contents)?;
    validate(&queue)?;
    Ok(queue)
}

/// Validates a parsed queue's schema-independent semantic invariants.
pub fn validate(queue: &TaskQueue) -> std::result::Result<(), TaskQueueError> {
    if queue.schema_version != TODO_QUEUE_VERSION {
        return Err(TaskQueueError::UnsupportedVersion {
            found: queue.schema_version,
            supported: TODO_QUEUE_VERSION,
        });
    }

    validate_component(&queue.component)?;
    validate_tasks(&queue.tasks)
}

/// Serializes a validated queue in its deterministic canonical YAML form.
pub fn serialize(queue: &TaskQueue) -> std::result::Result<String, TaskQueueError> {
    validate(queue)?;
    let mut output = format!("schema_version: {}\ncomponent:\n", queue.schema_version);
    append_component(&mut output, &queue.component, 2);
    output.push_str("tasks:");
    if queue.tasks.is_empty() {
        output.push_str(" []\n");
        return Ok(output);
    }
    output.push('\n');
    for task in &queue.tasks {
        append_task(&mut output, task);
    }
    Ok(output)
}

fn append_component(output: &mut String, component: &ComponentState, indentation: usize) {
    line(
        output,
        indentation,
        &format!(
            "specification_revision: {}",
            yaml_string(&component.specification_revision)
        ),
    );
    match &component.parent_specification {
        Some(parent) => {
            line(output, indentation, "parent_specification:");
            line(
                output,
                indentation + 2,
                &format!("path: {}", yaml_string(&parent.path)),
            );
            line(
                output,
                indentation + 2,
                &format!("revision: {}", yaml_string(&parent.revision)),
            );
        }
        None => line(output, indentation, "parent_specification: null"),
    }
    line(output, indentation, "revalidation:");
    line(
        output,
        indentation + 2,
        &format!(
            "state: {}",
            revalidation_state_name(component.revalidation.state)
        ),
    );
    line(
        output,
        indentation + 2,
        &format!(
            "checked_at: {}",
            yaml_string(&component.revalidation.checked_at.0)
        ),
    );
    match &component.revalidation.stale_since {
        Some(timestamp) => line(
            output,
            indentation + 2,
            &format!("stale_since: {}", yaml_string(&timestamp.0)),
        ),
        None => line(output, indentation + 2, "stale_since: null"),
    }
    if component.revalidation.causes.is_empty() {
        line(output, indentation + 2, "causes: []");
    } else {
        line(output, indentation + 2, "causes:");
        for cause in &component.revalidation.causes {
            line(
                output,
                indentation + 4,
                &format!("- kind: {}", staleness_cause_kind_name(cause.kind)),
            );
            line(
                output,
                indentation + 6,
                &format!("path: {}", yaml_string(&cause.path)),
            );
            line(
                output,
                indentation + 6,
                &format!(
                    "expected_revision: {}",
                    yaml_string(&cause.expected_revision)
                ),
            );
            line(
                output,
                indentation + 6,
                &format!(
                    "observed_revision: {}",
                    yaml_string(&cause.observed_revision)
                ),
            );
        }
    }
}

fn append_task(output: &mut String, task: &Task) {
    line(output, 2, &format!("- id: {}", yaml_string(&task.id)));
    for (field, value) in [
        ("title", &task.title),
        ("description", &task.description),
        ("context", &task.context),
        ("purpose", &task.purpose),
        ("expected_outcome", &task.expected_outcome),
    ] {
        line(output, 4, &format!("{field}: {}", yaml_string(value)));
    }
    line(output, 4, &format!("kind: {}", task_kind_name(task.kind)));
    line(
        output,
        4,
        &format!("status: {}", task_status_name(task.status)),
    );
    append_string_list(output, 4, "depends_on", &task.depends_on);
    append_string_list(output, 4, "requirements", &task.requirements);
    line(output, 4, "timestamps:");
    line(
        output,
        6,
        &format!("created_at: {}", yaml_string(&task.timestamps.created_at.0)),
    );
    line(
        output,
        6,
        &format!("updated_at: {}", yaml_string(&task.timestamps.updated_at.0)),
    );
    match &task.timestamps.completed_at {
        Some(timestamp) => line(
            output,
            6,
            &format!("completed_at: {}", yaml_string(&timestamp.0)),
        ),
        None => line(output, 6, "completed_at: null"),
    }
    match &task.blocked_reason {
        Some(reason) => line(
            output,
            4,
            &format!("blocked_reason: {}", yaml_string(reason)),
        ),
        None => line(output, 4, "blocked_reason: null"),
    }
}

fn append_string_list(output: &mut String, indentation: usize, field: &str, values: &[String]) {
    if values.is_empty() {
        line(output, indentation, &format!("{field}: []"));
        return;
    }
    line(output, indentation, &format!("{field}:"));
    for value in values {
        line(
            output,
            indentation + 2,
            &format!("- {}", yaml_string(value)),
        );
    }
}

fn line(output: &mut String, indentation: usize, contents: &str) {
    output.push_str(&" ".repeat(indentation));
    output.push_str(contents);
    output.push('\n');
}

fn yaml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\0'..='\u{1f}' => {
                use std::fmt::Write;

                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn validate_component(component: &ComponentState) -> std::result::Result<(), TaskQueueError> {
    validate_revision(
        &component.specification_revision,
        "component.specification_revision",
    )?;
    if let Some(parent) = &component.parent_specification {
        if parent.path != "../SPEC.md" {
            return Err(TaskQueueError::invalid(
                "`component.parent_specification.path` must be `../SPEC.md`",
            ));
        }
        validate_revision(&parent.revision, "component.parent_specification.revision")?;
    }

    component
        .revalidation
        .checked_at
        .validate("component.revalidation.checked_at")?;
    match component.revalidation.state {
        RevalidationState::Current => {
            if component.revalidation.stale_since.is_some()
                || !component.revalidation.causes.is_empty()
            {
                return Err(TaskQueueError::invalid(
                    "current revalidation state requires `stale_since: null` and `causes: []`",
                ));
            }
        }
        RevalidationState::Stale => {
            let Some(stale_since) = &component.revalidation.stale_since else {
                return Err(TaskQueueError::invalid(
                    "stale revalidation state requires `stale_since`",
                ));
            };
            stale_since.validate("component.revalidation.stale_since")?;
            if stale_since > &component.revalidation.checked_at {
                return Err(TaskQueueError::invalid(
                    "`component.revalidation.stale_since` must not be later than `checked_at`",
                ));
            }
            if component.revalidation.causes.is_empty() {
                return Err(TaskQueueError::invalid(
                    "stale revalidation state requires at least one cause",
                ));
            }
        }
    }

    for (index, cause) in component.revalidation.causes.iter().enumerate() {
        let prefix = format!("component.revalidation.causes[{index}]");
        if cause.path.is_empty() {
            return Err(TaskQueueError::invalid(format!(
                "`{prefix}.path` must not be empty"
            )));
        }
        validate_revision(
            &cause.expected_revision,
            &format!("{prefix}.expected_revision"),
        )?;
        validate_revision(
            &cause.observed_revision,
            &format!("{prefix}.observed_revision"),
        )?;
        if cause.expected_revision == cause.observed_revision {
            return Err(TaskQueueError::invalid(format!(
                "`{prefix}` must record different expected and observed revisions"
            )));
        }
    }
    Ok(())
}

fn validate_tasks(tasks: &[Task]) -> std::result::Result<(), TaskQueueError> {
    let mut positions = BTreeMap::new();
    for (index, task) in tasks.iter().enumerate() {
        validate_task(task)?;
        if positions.insert(task.id.as_str(), index).is_some() {
            return Err(TaskQueueError::invalid(format!(
                "duplicate task ID `{}`",
                task.id
            )));
        }
    }

    for task in tasks {
        for dependency in &task.depends_on {
            if dependency == &task.id {
                return Err(TaskQueueError::invalid(format!(
                    "task `{}` cannot depend on itself",
                    task.id
                )));
            }
            if !positions.contains_key(dependency.as_str()) {
                return Err(TaskQueueError::invalid(format!(
                    "task `{}` depends on unknown task `{dependency}`",
                    task.id
                )));
            }
        }
    }

    validate_acyclic(tasks, &positions)?;
    for (index, task) in tasks.iter().enumerate() {
        for dependency in &task.depends_on {
            if positions[dependency.as_str()] >= index {
                return Err(TaskQueueError::invalid(format!(
                    "task `{}` must depend only on an earlier declared task",
                    task.id
                )));
            }
        }
        validate_lifecycle_dependencies(task, tasks, &positions)?;
    }
    Ok(())
}

fn validate_task(task: &Task) -> std::result::Result<(), TaskQueueError> {
    if !valid_task_id(&task.id) {
        return Err(TaskQueueError::invalid(format!(
            "task ID `{}` must be 1-{MAX_TASK_ID_CHARS} lowercase kebab-case characters",
            task.id
        )));
    }
    validate_text(&task.title, "title", MAX_TITLE_CHARS, true)?;
    for (name, value) in [
        ("description", &task.description),
        ("context", &task.context),
        ("purpose", &task.purpose),
        ("expected_outcome", &task.expected_outcome),
    ] {
        validate_text(value, name, MAX_DETAIL_CHARS, false)?;
    }
    validate_sorted_unique(&task.depends_on, "depends_on")?;
    validate_sorted_unique(&task.requirements, "requirements")?;
    for requirement in &task.requirements {
        let Some((source, locator)) = requirement.split_once('#') else {
            return Err(TaskQueueError::invalid(format!(
                "task `{}` requirement `{requirement}` must contain a source and `#` locator",
                task.id
            )));
        };
        if source.trim().is_empty() || locator.trim().is_empty() {
            return Err(TaskQueueError::invalid(format!(
                "task `{}` requirement `{requirement}` must have nonblank source and locator",
                task.id
            )));
        }
    }

    task.timestamps
        .created_at
        .validate(&format!("task `{}` timestamps.created_at", task.id))?;
    task.timestamps
        .updated_at
        .validate(&format!("task `{}` timestamps.updated_at", task.id))?;
    if task.timestamps.updated_at < task.timestamps.created_at {
        return Err(TaskQueueError::invalid(format!(
            "task `{}` updated_at must not precede created_at",
            task.id
        )));
    }
    match (task.status, &task.timestamps.completed_at) {
        (TaskStatus::Completed, Some(completed_at)) => {
            completed_at.validate(&format!("task `{}` timestamps.completed_at", task.id))?;
            if completed_at < &task.timestamps.updated_at {
                return Err(TaskQueueError::invalid(format!(
                    "task `{}` completed_at must not precede updated_at",
                    task.id
                )));
            }
        }
        (TaskStatus::Completed, None) => {
            return Err(TaskQueueError::invalid(format!(
                "completed task `{}` requires timestamps.completed_at",
                task.id
            )));
        }
        (_, Some(_)) => {
            return Err(TaskQueueError::invalid(format!(
                "non-completed task `{}` requires timestamps.completed_at: null",
                task.id
            )));
        }
        (_, None) => {}
    }
    match (task.status, task.blocked_reason.as_deref()) {
        (TaskStatus::Blocked, Some(reason)) if !reason.trim().is_empty() => {}
        (TaskStatus::Blocked, _) => {
            return Err(TaskQueueError::invalid(format!(
                "blocked task `{}` requires a nonblank blocked_reason",
                task.id
            )));
        }
        (_, None) => {}
        (_, Some(_)) => {
            return Err(TaskQueueError::invalid(format!(
                "non-blocked task `{}` requires blocked_reason: null",
                task.id
            )));
        }
    }
    Ok(())
}

fn validate_acyclic(
    tasks: &[Task],
    positions: &BTreeMap<&str, usize>,
) -> std::result::Result<(), TaskQueueError> {
    let mut states = vec![VisitState::Unvisited; tasks.len()];
    for index in 0..tasks.len() {
        visit_task(index, tasks, positions, &mut states)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Unvisited,
    Visiting,
    Visited,
}

fn visit_task(
    index: usize,
    tasks: &[Task],
    positions: &BTreeMap<&str, usize>,
    states: &mut [VisitState],
) -> std::result::Result<(), TaskQueueError> {
    match states[index] {
        VisitState::Visited => return Ok(()),
        VisitState::Visiting => {
            return Err(TaskQueueError::invalid(format!(
                "dependency cycle includes task `{}`",
                tasks[index].id
            )));
        }
        VisitState::Unvisited => {}
    }
    states[index] = VisitState::Visiting;
    for dependency in &tasks[index].depends_on {
        visit_task(positions[dependency.as_str()], tasks, positions, states)?;
    }
    states[index] = VisitState::Visited;
    Ok(())
}

fn validate_lifecycle_dependencies(
    task: &Task,
    tasks: &[Task],
    positions: &BTreeMap<&str, usize>,
) -> std::result::Result<(), TaskQueueError> {
    let required_predecessor = match task.kind {
        TaskKind::Test => return Ok(()),
        TaskKind::Implementation => TaskKind::Test,
        TaskKind::SecurityAudit => TaskKind::Implementation,
        TaskKind::ComplianceReview => TaskKind::SecurityAudit,
    };
    if has_transitive_dependency_of_kind(task, required_predecessor, tasks, positions) {
        Ok(())
    } else {
        Err(TaskQueueError::invalid(format!(
            "{} task `{}` must depend on a preceding {} task",
            task_kind_name(task.kind),
            task.id,
            task_kind_name(required_predecessor)
        )))
    }
}

fn has_transitive_dependency_of_kind(
    task: &Task,
    expected: TaskKind,
    tasks: &[Task],
    positions: &BTreeMap<&str, usize>,
) -> bool {
    let mut pending = task
        .depends_on
        .iter()
        .map(|dependency| positions[dependency.as_str()])
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if !visited.insert(index) {
            continue;
        }
        let dependency = &tasks[index];
        if dependency.kind == expected {
            return true;
        }
        pending.extend(
            dependency
                .depends_on
                .iter()
                .map(|next| positions[next.as_str()]),
        );
    }
    false
}

fn validate_revision(value: &str, field: &str) -> std::result::Result<(), TaskQueueError> {
    let Some(hex) = value.strip_prefix(SHA256_PREFIX) else {
        return Err(TaskQueueError::invalid(format!(
            "`{field}` must start with `{SHA256_PREFIX}`"
        )));
    };
    if hex.len() != SHA256_HEX_LENGTH
        || !hex
            .bytes()
            .all(|character| character.is_ascii_digit() || (b'a'..=b'f').contains(&character))
    {
        return Err(TaskQueueError::invalid(format!(
            "`{field}` must contain {SHA256_HEX_LENGTH} lowercase hexadecimal digits"
        )));
    }
    Ok(())
}

fn valid_task_id(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_TASK_ID_CHARS
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.bytes().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == b'-'
        })
        && !value.contains("--")
}

fn validate_text(
    value: &str,
    field: &str,
    maximum_chars: usize,
    single_line: bool,
) -> std::result::Result<(), TaskQueueError> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.chars().count() > maximum_chars
        || (single_line && value.contains(['\n', '\r']))
    {
        return Err(TaskQueueError::invalid(format!(
            "`{field}` must be nonblank, trimmed{} and at most {maximum_chars} characters",
            if single_line { ", one line" } else { "" }
        )));
    }
    Ok(())
}

fn validate_sorted_unique(
    values: &[String],
    field: &str,
) -> std::result::Result<(), TaskQueueError> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(TaskQueueError::invalid(format!(
            "`{field}` entries must be nonblank"
        )));
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(TaskQueueError::invalid(format!(
            "`{field}` must be lexically sorted and duplicate-free"
        )));
    }
    Ok(())
}

fn task_kind_name(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Test => "test",
        TaskKind::Implementation => "implementation",
        TaskKind::SecurityAudit => "security-audit",
        TaskKind::ComplianceReview => "compliance-review",
    }
}

fn task_status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Completed => "completed",
    }
}

fn revalidation_state_name(state: RevalidationState) -> &'static str {
    match state {
        RevalidationState::Current => "current",
        RevalidationState::Stale => "stale",
    }
}

fn staleness_cause_kind_name(kind: StalenessCauseKind) -> &'static str {
    match kind {
        StalenessCauseKind::ComponentSpecificationRevisionChanged => {
            "component-specification-revision-changed"
        }
        StalenessCauseKind::ParentSpecificationRevisionChanged => {
            "parent-specification-revision-changed"
        }
    }
}

fn decimal(bytes: &[u8], start: usize, end: usize) -> u32 {
    bytes[start..end]
        .iter()
        .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    schema_version: u32,
}
