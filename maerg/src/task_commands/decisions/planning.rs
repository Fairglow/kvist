//! Task-only planning: typed idea intake, scoped evidence and exact approval.

use super::*;
use crate::task_queue::valid_task_id;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QueueDraft {
    schema_version: u32,
    pub rationale: String,
    #[serde(default)]
    pub document_edits: Vec<DocumentEdit>,
    pub items: Vec<WorkItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkItem {
    id: String,
    title: String,
    context: String,
    purpose: String,
    requirements: Vec<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    regression_tests: Vec<String>,
    write_tests: Phase,
    implement_code: Phase,
    security_audit: Phase,
    compliance_review: Phase,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Phase {
    description: String,
    expected_outcome: String,
}

/// Stage an idea or import without changing any controlled files.
pub(crate) fn stage(
    project_dir: &Path,
    component: &Path,
    idea: Option<&str>,
    import: Option<&Path>,
    exception: Option<&str>,
) -> Result<Output> {
    stage_with(project_dir, component, idea, import, exception, ask)
}

pub(super) fn stage_with(
    project_dir: &Path,
    component: &Path,
    idea: Option<&str>,
    import: Option<&Path>,
    exception: Option<&str>,
    mut model: impl FnMut(&TaskContext, &str, &'static str, &Value) -> Result<String>,
) -> Result<Output> {
    let guidance = match (idea, import) {
        (Some(text), None) => {
            meaningful(text, "idea", 16 * 1024)?;
            text.to_owned()
        }
        (None, Some(_)) => "Manual task proposal".to_owned(),
        _ => return Err(decision_error("specify exactly one of --idea or --import")),
    };
    if let Some(reason) = exception {
        meaningful(reason, "intent-review exception", 8192)?;
    }
    let input = engine::capture_queue(project_dir, component)?;
    let context = engine::context_for(project_dir, component)?;
    let mut documents = allowed_documents(&context, &input, &input.files)?;
    let draft: QueueDraft = if let Some(path) = import {
        strict_parse(&bounded_read(path)?)?
    } else {
        let queue = input.queue.tasks.iter().map(|task| {
            serde_json::json!({"id":task.id, "title":task.title, "requirements":task.requirements})
        }).collect::<Vec<_>>();
        let phase = serde_json::json!({
            "description":"Action, bounded scope and exclusions",
            "expected_outcome":"Observable result and fixture, test selector or review evidence"
        });
        let evidence = serde_json::json!({
            "human_request":guidance,
            "allowed_intent":documents,
            "existing_tasks":queue,
            "draft_schema":{
                "schema_version":1,
                "rationale":"Why this work is needed and what is deliberately excluded",
                "document_edits":[{"path":"REQUIREMENTS.md", "before":"unique exact excerpt", "after":"replacement with stable clause"}],
                "items":[{
                    "id":"a-new-kebab-case-stem", "title":"One bounded outcome",
                    "context":"Known prerequisites and starting state", "purpose":"Why the outcome matters",
                    "requirements":["REQUIREMENTS.md#REQ-ID"], "depends_on":[], "regression_tests":[],
                    "write_tests":phase, "implement_code":phase,
                    "security_audit":phase, "compliance_review":phase
                }]
            }
        });
        strict_parse(&model(
            &context,
            "task-planning",
            "decision-queue-plan",
            &evidence,
        )?)?
    };
    if draft.schema_version != 1 || !(1..=16).contains(&draft.items.len()) {
        return Err(decision_error(
            "task draft requires schema_version 1 and 1 to 16 work items",
        ));
    }
    meaningful(&draft.rationale, "task rationale", 8192)?;
    let at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let changes = engine::queue_changes(&context, &input, draft, &at)?;
    let review = if let Some(reason) = exception {
        IntentReview {
            report: reason.to_owned(),
            exception: true,
        }
    } else {
        for name in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md"] {
            if let Some(text) = changes.outputs.get(name) {
                documents.insert(name.to_owned(), text.clone());
            }
        }
        let queue = parse(&changes.outputs["TODOS.yaml"])
            .map_err(|error| decision_error(&error.to_string()))?;
        documents.insert(
            "TASK_PROJECTION".to_owned(),
            super::super::review::task_projection(&queue)?,
        );
        let evidence = serde_json::json!({
            "human_request":guidance, "rationale":changes.rationale, "materialized_candidate":documents,
            "note":"Review actionable scope, resolved clauses, expected evidence and ordering; never certify compliance."
        });
        IntentReview {
            report: model(
                &context,
                "task-planning-review",
                "decision-intent-review",
                &evidence,
            )?,
            exception: false,
        }
    };
    let plan = engine::prepare_changes(
        project_dir,
        input,
        changes,
        Selection {
            option: None,
            guidance,
        },
        review,
        Disposition::Resolve,
        at,
    )?;
    let plan_path = plan_markdown(project_dir, component, &plan.id)?;
    let exact = format!(
        "kvist task apply {} {} --approve {}",
        shell_token(&component.to_string_lossy()),
        plan.id,
        plan.approval
    );
    let actions = engine::shortcuts::plan(&context, &plan).map_err(|error| {
        decision_error(&format!("Plan retained at `{}` but numbered approval could not be published: {error}. After review, use `{exact}`.", plan_path.display()))
    })?;
    Ok(Output {
        message: format!(
            "{}\n\nReview the schedule above or revise your idea and stage another plan. Staging changed no controlled files.{}",
            bounded_read(&plan_path)?,
            engine::shortcuts::markdown(component, &actions)
        ),
        markdown_path: Some(plan_path),
        plan_id: Some(plan.id),
        approval: Some(plan.approval),
        actions,
    })
}

/// Apply one exact scheduling plan, not a paused-work disposition.
pub(crate) fn apply(
    project_dir: &Path,
    component: &Path,
    id: &str,
    approve: &str,
) -> Result<Output> {
    let plan = engine::load_plan(project_dir, component, id)?;
    if plan.input.task.is_some() {
        return Err(decision_error(
            "use `kvist decision COMPONENT apply` for paused work",
        ));
    }
    let plan = engine::apply(project_dir, component, id, approve)?;
    Ok(Output {
        message: format!(
            "Scheduled the work in plan `{id}`. No task ran and no commit was created.\n\nInspect ready work: `kvist task next {}`.",
            shell_token(&component.to_string_lossy()),
        ),
        markdown_path: Some(plan_markdown(project_dir, component, id)?),
        plan_id: Some(plan.id),
        approval: Some(plan.approval),
        actions: Vec::new(),
    })
}

fn allowed_documents(
    context: &TaskContext,
    input: &Capture,
    files: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut docs = BTreeMap::new();
    for name in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md"] {
        docs.insert(name.to_owned(), files[name].clone());
    }
    let root = bounded_read(&context.project_dir.join("ROOT_CONTRACT.md"))?;
    if input.bindings.get("root/ROOT_CONTRACT.md") != Some(&digest(root.as_bytes())) {
        return Err(decision_error(
            "root contract changed during planning; regenerate the plan",
        ));
    }
    docs.insert("ROOT_CONTRACT.md".to_owned(), root);
    if let Some(parent) = &input.queue.component.parent_contract {
        let config = crate::config::load(&context.project_dir)?;
        let root = context.project_dir.join(&config.component_root);
        let (directory, _) = discovery::find_parent_component(&root, &context.component_path)?
            .ok_or_else(|| decision_error("immediate parent contract is missing"))?;
        let text = bounded_read(&directory.join("CONTRACT.md"))?;
        if digest(text.as_bytes()) != parent.revision {
            return Err(decision_error(
                "parent contract changed during planning; regenerate the plan",
            ));
        }
        docs.insert("PARENT_CONTRACT.md".to_owned(), text);
    }
    Ok(docs)
}

pub(super) fn definitions(
    context: &TaskContext,
    input: &Capture,
    files: &BTreeMap<String, String>,
    items: Vec<WorkItem>,
) -> Result<Vec<TaskDefinition>> {
    let docs = allowed_documents(context, input, files)?;
    let locators = crate::task_commands::task_brief::LocatorIndex::new(
        docs.iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
    );
    let local = ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md"];
    if local.iter().any(|name| files[*name] != input.files[*name]) {
        let prior = crate::task_commands::task_brief::LocatorIndex::new(
            local.map(|name| (name, input.files[name].as_str())),
        );
        for task in input
            .queue
            .tasks
            .iter()
            .filter(|task| task.status != TaskStatus::Completed)
        {
            if let Some(reference) = task.requirements.iter().find(|reference| {
                prior.resolve(reference).is_some() && locators.resolve(reference).is_none()
            }) {
                return Err(decision_error(&format!(
                    "document edits remove the referenced clause `{reference}` from open task `{}`; preserve its locator or plan the change through its decision",
                    task.id
                )));
            }
        }
    }
    let mut ids = input
        .queue
        .tasks
        .iter()
        .map(|t| t.id.clone())
        .collect::<BTreeSet<_>>();
    let mut definitions = Vec::with_capacity(items.len() * 4);
    for item in items {
        if !valid_task_id(&item.id) || ids.contains(&item.id) {
            return Err(decision_error(&format!(
                "invalid or existing work-item stem `{}`",
                item.id
            )));
        }
        if !item
            .requirements
            .iter()
            .any(|r| r.starts_with("REQUIREMENTS.md#"))
        {
            return Err(decision_error(&format!(
                "item `{}` needs a local REQUIREMENTS.md locator",
                item.id
            )));
        }
        for reference in &item.requirements {
            if locators.resolve(reference).is_none() {
                return Err(decision_error(&format!(
                    "item `{}` has an unknown, ambiguous or out-of-scope locator `{reference}`",
                    item.id
                )));
            }
        }
        let mut dependencies = item.depends_on;
        if !item.regression_tests.is_empty() && !is_rust_verification(&context.project_dir) {
            return Err(decision_error(
                "named regression evidence is currently supported only for Rust components",
            ));
        }
        if let Some(id) = dependencies.iter().find(|id| !ids.contains(*id)) {
            return Err(decision_error(&format!(
                "item `{}` depends on unknown or later task `{id}`",
                item.id
            )));
        }
        for (suffix, label, kind, phase) in [
            ("write-tests", "Tests", TaskKind::Test, item.write_tests),
            (
                "implement-code",
                "Implementation",
                TaskKind::Implementation,
                item.implement_code,
            ),
            (
                "security-audit",
                "Security audit",
                TaskKind::SecurityAudit,
                item.security_audit,
            ),
            (
                "compliance-review",
                "Compliance review",
                TaskKind::ComplianceReview,
                item.compliance_review,
            ),
        ] {
            let id = format!("{}-{suffix}", item.id);
            if !valid_task_id(&id) || !ids.insert(id.clone()) {
                return Err(decision_error(&format!(
                    "generated task ID `{id}` is too long or already exists"
                )));
            }
            definitions.push(TaskDefinition {
                id: id.clone(),
                title: format!("{label}: {}", item.title),
                description: phase.description,
                context: item.context.clone(),
                purpose: item.purpose.clone(),
                expected_outcome: phase.expected_outcome,
                regression_tests: (matches!(kind, TaskKind::Test | TaskKind::Implementation)
                    && !item.regression_tests.is_empty())
                .then(|| item.regression_tests.clone()),
                kind,
                depends_on: dependencies,
                requirements: item.requirements.clone(),
            });
            dependencies = vec![id];
        }
    }
    Ok(definitions)
}
