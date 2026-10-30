use super::*;
use crate::task_queue::{RevalidationState, TaskTimestamps};

pub(super) struct Changes {
    pub outputs: BTreeMap<String, String>,
    pub retired: Vec<Task>,
    pub retirement_reasons: BTreeMap<String, String>,
    pub rationale: String,
}

pub(super) fn outputs(
    context: &TaskContext,
    input: &Capture,
    draft: Draft,
    at: &Timestamp,
) -> Result<Changes> {
    if draft.schema_version != 1 || draft.rationale.trim().is_empty() {
        return Err(fail(
            &context.component_dir,
            "draft requires schema_version 1 and a nonblank rationale",
        ));
    }
    bounded(&encode(&draft)?, Path::new("draft"))?;
    let mut documents =
        documents::materialize(&input.files, draft.documents, draft.document_edits)?;
    let mut queue = input.queue.clone();
    let selected = queue
        .tasks
        .iter()
        .position(|task| task.id == input.task.id)
        .ok_or_else(|| fail(&context.component_dir, "selected task disappeared"))?;
    let mut seen = BTreeSet::new();
    let mut additions = Vec::new();
    let mut retirements = BTreeMap::new();
    for edit in draft.tasks {
        let id = match &edit {
            TaskEdit::Add { task } | TaskEdit::Update { task } => &task.id,
            TaskEdit::Retire { id, .. } => id,
        };
        if !seen.insert(id.clone()) {
            return Err(fail(
                &context.component_dir,
                format!("duplicate/conflicting edit for `{id}`"),
            ));
        }
        match edit {
            TaskEdit::Add { task } => {
                if queue.tasks.iter().any(|existing| existing.id == task.id) {
                    return Err(fail(
                        &context.component_dir,
                        format!("added task `{}` already exists", task.id),
                    ));
                }
                additions.push(new_task(task, at));
            }
            TaskEdit::Update { task } => {
                let existing = queue
                    .tasks
                    .iter_mut()
                    .find(|existing| existing.id == task.id)
                    .ok_or_else(|| {
                        fail(
                            &context.component_dir,
                            format!("unknown updated task `{}`", task.id),
                        )
                    })?;
                editable(existing, &input.task.id, &context.component_dir)?;
                authored(existing, task);
                existing.timestamps.updated_at = at.clone();
            }
            TaskEdit::Retire { id, reason } => {
                let task = queue
                    .tasks
                    .iter()
                    .find(|task| task.id == id)
                    .ok_or_else(|| {
                        fail(
                            &context.component_dir,
                            format!("unknown retired task `{id}`"),
                        )
                    })?;
                if task.recovery_state.is_some()
                    || matches!(task.status, TaskStatus::Completed | TaskStatus::InProgress)
                    || reason.trim().is_empty()
                    || reason.len() > 4096
                {
                    return Err(fail(
                        &context.component_dir,
                        "retirement requires an uncompleted, non-live, non-fenced task and a bounded explicit reason",
                    ));
                }
                retirements.insert(id, reason);
            }
        }
    }
    let mut retired = Vec::new();
    match draft.disposition {
        Disposition::Resolve => {
            if !retirements.is_empty() {
                return Err(fail(
                    &context.component_dir,
                    "resolve cannot retire work; use an explicit withdraw disposition",
                ));
            }
            let task = &mut queue.tasks[selected];
            task.status = TaskStatus::Pending;
            task.blocked_reason = None;
            task.timestamps.updated_at = at.clone();
            task.timestamps.completed_at = None;
            for added in &additions {
                task.depends_on.push(added.id.clone());
            }
            task.depends_on.sort();
            task.depends_on.dedup();
        }
        Disposition::Withdraw => {
            let mut closure = BTreeSet::from([input.task.id.clone()]);
            loop {
                let previous = closure.len();
                for task in &input.queue.tasks {
                    if task.depends_on.iter().any(|id| closure.contains(id)) {
                        closure.insert(task.id.clone());
                    }
                }
                if closure.len() == previous {
                    break;
                }
            }
            if closure != retirements.keys().cloned().collect() {
                return Err(fail(
                    &context.component_dir,
                    "withdraw must explicitly retire the selected task and its entire affected dependant closure, and no unrelated tasks",
                ));
            }
            for task in &input.queue.tasks {
                if closure.contains(&task.id) {
                    retired.push(task.clone());
                }
            }
        }
    }
    queue.tasks.splice(selected..selected, additions);
    queue
        .tasks
        .retain(|task| !retirements.contains_key(&task.id));
    queue.component.requirements_revision = digest(documents["REQUIREMENTS.md"].as_bytes());
    queue.component.contract_revision = digest(documents["CONTRACT.md"].as_bytes());
    queue.component.design_revision = digest(documents["DESIGN.md"].as_bytes());
    queue.component.parent_contract = capture::parent(context)?;
    queue.component.revalidation.state = RevalidationState::Current;
    queue.component.revalidation.checked_at = at.clone();
    queue.component.revalidation.stale_since = None;
    queue.component.revalidation.causes.clear();
    let text = serialize(&queue)
        .map_err(|error| fail(&context.component_dir.join("TODOS.yaml"), error.to_string()))?;
    bounded(&text, Path::new("TODOS.yaml"))?;
    documents.insert("TODOS.yaml".to_owned(), text);
    documents.retain(|name, text| input.files.get(name) != Some(text));
    Ok(Changes {
        outputs: documents,
        retired,
        retirement_reasons: retirements,
        rationale: draft.rationale,
    })
}

fn editable(task: &Task, selected: &str, path: &Path) -> Result<()> {
    if task.recovery_state.is_some()
        || !(task.status == TaskStatus::Pending
            || task.id == selected
                && matches!(
                    task.status,
                    TaskStatus::Blocked | TaskStatus::AwaitingDecision
                ))
    {
        return Err(fail(
            path,
            format!(
                "cannot update completed, live, fenced or unrelated paused task `{}`",
                task.id
            ),
        ));
    }
    Ok(())
}

fn authored(existing: &mut Task, definition: TaskDefinition) {
    existing.title = definition.title;
    existing.description = definition.description;
    existing.context = definition.context;
    existing.purpose = definition.purpose;
    existing.expected_outcome = definition.expected_outcome;
    existing.kind = definition.kind;
    existing.depends_on = definition.depends_on;
    existing.requirements = definition.requirements;
}

fn new_task(definition: TaskDefinition, at: &Timestamp) -> Task {
    let mut task = Task {
        id: definition.id.clone(),
        title: String::new(),
        description: String::new(),
        context: String::new(),
        purpose: String::new(),
        expected_outcome: String::new(),
        kind: definition.kind,
        status: TaskStatus::Pending,
        depends_on: Vec::new(),
        requirements: Vec::new(),
        timestamps: TaskTimestamps {
            created_at: at.clone(),
            updated_at: at.clone(),
            completed_at: None,
            recovered: false,
        },
        blocked_reason: None,
        recovery_state: None,
        acceptance_id: None,
        disposition: None,
    };
    authored(&mut task, definition);
    task
}
