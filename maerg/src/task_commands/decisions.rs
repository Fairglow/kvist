//! Human-directed, staged decisions; only exact approved plans change intent.

use super::*;

mod engine;
mod types;
pub use engine::shortcuts::ResponseAction;
pub use types::{Action, Disposition, DocumentEdit, Draft, TaskDefinition, TaskEdit};
use types::{Capture, IntentReview, Plan, Selection};
#[cfg(test)]
mod tests;

const DIRECTORY: &str = ".kvist/decisions";
const MAX_FILE: u64 = 1024 * 1024;
const MAX_TOTAL: usize = 8 * 1024 * 1024;

/// Verifies there is no interrupted decision transaction before ordinary mutation.
pub(crate) fn ensure_no_pending(component_dir: &Path) -> Result<()> {
    engine::ensure_no_pending(component_dir)
}

pub(super) fn recognizes_retired_journal(component_dir: &Path, journal: &Path) -> Result<bool> {
    engine::recognizes_retired_journal(component_dir, journal)
}

/// Both JSON and human command dispatch share this result.
#[derive(Debug, Serialize)]
pub struct Output {
    pub message: String,
    pub markdown_path: Option<PathBuf>,
    pub plan_id: Option<String>,
    pub approval: Option<String>,
    pub actions: Vec<ResponseAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Alternative {
    id: String,
    title: String,
    consequences: String,
    recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Alternatives {
    summary: String,
    options: Vec<Alternative>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    question: String,
    answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conversation {
    schema_version: u32,
    capture: String,
    task_id: String,
    exchanges: Vec<Exchange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: u32,
    capture: String,
    task_id: String,
    alternatives: Alternatives,
}

/// Executes decision support without granting task execution or implementation approval.
pub fn execute(project_dir: &Path, component: &Path, action: Option<Action>) -> Result<Output> {
    let context = engine::context_for(project_dir, component)?;
    for directory in [
        context.component_dir.join(".kvist"),
        context.component_dir.join(DIRECTORY),
    ] {
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() && !is_link_like(&metadata) => {}
            Ok(_) => {
                return Err(decision_error(
                    "decision state ancestors must be real non-link directories",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect decision state ancestor",
                    path: directory,
                    source,
                });
            }
        }
    }
    let action = match action.unwrap_or(Action::Show { task_id: None }) {
        Action::Do { number } => {
            let (command, response) = engine::shortcuts::resolve(&context, number)?;
            eprintln!("Response {number}: {}", response.command);
            match command {
                engine::shortcuts::Command::OptionPlan {
                    task,
                    catalog_digest,
                    option,
                    ..
                } => Action::Plan {
                    task: Some(task),
                    option: Some(option),
                    catalog: Some(catalog_digest),
                    guidance: None,
                    import: None,
                    review_exception: None,
                },
                engine::shortcuts::Command::ApplyPlan {
                    plan_id,
                    approval,
                    continue_run,
                    ..
                } => Action::Apply {
                    plan_id,
                    approve: approval,
                    continue_run,
                },
            }
        }
        action => action,
    };
    match action {
        Action::Actions => {
            let actions = engine::shortcuts::discover(&context)?;
            let mut output = response(engine::shortcuts::markdown(component, &actions), None);
            output.actions = actions;
            Ok(output)
        }
        Action::Show { task_id } => show(project_dir, component, task_id.as_deref()),
        Action::Options { task } => options(project_dir, component, task.as_deref()),
        Action::Discuss { message, task } => {
            discuss(project_dir, component, task.as_deref(), &message)
        }
        Action::Plan {
            task,
            option,
            catalog,
            guidance,
            import,
            review_exception,
        } => prepare(
            project_dir,
            component,
            task.as_deref(),
            Selection {
                option,
                guidance: guidance.unwrap_or_default(),
            },
            import.as_deref(),
            review_exception,
            catalog,
        ),
        Action::Apply {
            plan_id,
            approve,
            continue_run,
        } => {
            let plan = engine::apply(project_dir, component, &plan_id, &approve)?;
            let mut message = format!(
                "Applied exact decision plan `{}`. Intent/scheduling accepted; implementation and findings are not approved. No Git commit was created.",
                plan.id
            );
            if continue_run {
                let outcome = run_task_or_item(project_dir, component, None, false).map_err(|error| {
                    decision_error(&format!(
                        "plan `{}` was applied and recorded, but ordinary continuation stopped: {error}. \
                         Its accepted effects remain; inspect the task diagnostic and decision receipt before resuming.",
                        plan.id
                    ))
                })?;
                message.push_str(&format!("\n{outcome}"));
            }
            Ok(Output {
                message,
                markdown_path: Some(plan_markdown(project_dir, component, &plan.id)?),
                plan_id: Some(plan.id),
                approval: Some(plan.approval),
                actions: Vec::new(),
            })
        }
        Action::Do { .. } => Err(decision_error("unresolved numbered response")),
    }
}

fn response(message: String, path: Option<PathBuf>) -> Output {
    Output {
        message,
        markdown_path: path,
        plan_id: None,
        approval: None,
        actions: Vec::new(),
    }
}

/// Loads authenticated response numbers for completion without writes or inference.
pub fn response_actions(project: &Path, component: &Path) -> Result<Vec<ResponseAction>> {
    engine::shortcuts::list(&engine::context_for(project, component)?)
}

fn show(project_dir: &Path, component: &Path, task_id: Option<&str>) -> Result<Output> {
    let context = engine::context_for(project_dir, component)?;
    let queue = read_queue(&context.component_dir)?;
    let paused = queue
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                TaskStatus::Blocked | TaskStatus::AwaitingDecision
            )
        })
        .collect::<Vec<_>>();
    if task_id.is_none() && paused.len() != 1 {
        let list = paused
            .iter()
            .map(|task| {
                format!(
                    "- `{}` ({}): {}",
                    task.id,
                    status_name(task.status),
                    task.blocked_reason
                        .as_deref()
                        .unwrap_or("No reason recorded.")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(response(
            format!(
                "# Decisions and blockers\n\n{list}\n\n{} paused task(s). Inspect one with `kvist decision {} show TASK_ID`.\nDecision history and any interrupted transaction: `{}`.",
                paused.len(),
                component_argument(component),
                context.component_dir.join(DIRECTORY).display(),
            ),
            None,
        ));
    }
    let task = match task_id {
        Some(id) => queue.tasks.iter().find(|task| task.id == id),
        None => paused.first().copied(),
    }.ok_or_else(|| decision_error("the requested task does not exist; inspect retained decision history for withdrawn work"))?;
    let evidence = context
        .component_dir
        .join(".kvist/attention")
        .join(format!("{}.md", task.id));
    let mut message = format!(
        "# Decision: {}\n\n{}\n\nState: `{}`.\n\n{}\n\n{}",
        task.title,
        task.description,
        status_name(task.status),
        task.blocked_reason
            .as_deref()
            .unwrap_or("No current pause reason."),
        instructions(component, &task.id),
    );
    let evidence = match fs::symlink_metadata(&evidence) {
        Ok(_) => {
            message.push_str(&format!(
                "\nRetained human guidance: `{}`",
                evidence.display()
            ));
            Some(evidence)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect decision guidance",
                path: evidence,
                source,
            });
        }
    };
    let mut actions = engine::shortcuts::list_for_task(&context, &task.id)?;
    if task.status == TaskStatus::AwaitingDecision && actions.is_empty() {
        if let Err(error) = options(project_dir, component, Some(&task.id)) {
            tracing::warn!(
                task_id = %task.id,
                %error,
                "could not auto-generate decision options during decision show; falling back to existing state"
            );
        } else {
            actions = engine::shortcuts::list_for_task(&context, &task.id)?;
        }
    }
    message.push_str(&support_index(&context, &task.id)?);
    message.push_str(&engine::shortcuts::markdown(component, &actions));
    let mut output = response(message, evidence);
    output.actions = actions;
    Ok(output)
}

fn support_index(context: &TaskContext, task: &str) -> Result<String> {
    let directory = context.component_dir.join(DIRECTORY);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !is_link_like(&metadata) => {}
        Ok(_) => {
            return Err(decision_error(
                "decision workspace must be a real non-link directory",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect decision workspace",
                path: directory,
                source,
            });
        }
    }
    let mut links = Vec::new();
    let mut total = 0;
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|source| KvistError::Io {
            operation: "list decision workspace",
            path: directory.clone(),
            source,
        })?
        .enumerate()
    {
        if index >= 4096 {
            return Err(decision_error("decision workspace exceeds 4096 entries"));
        }
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "read decision workspace entry",
            path: directory.clone(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_catalog = name.ends_with(".options.json");
        if !is_catalog && !name.ends_with(".discussion.json") {
            continue;
        }
        let text = bounded_read(&entry.path())?;
        total += text.len();
        if total > MAX_TOTAL {
            return Err(decision_error(
                "decision workspace inspection exceeds 8 MiB",
            ));
        }
        let belongs = if is_catalog {
            let catalog: Catalog = strict_parse(&text)?;
            if catalog.schema_version != 1 {
                return Err(decision_error("unsupported option catalog version"));
            }
            catalog.task_id == task
        } else {
            let conversation: Conversation = strict_parse(&text)?;
            if conversation.schema_version != 1 {
                return Err(decision_error("unsupported discussion version"));
            }
            conversation.task_id == task
        };
        if belongs {
            links.push(entry.path().with_extension("md"));
        }
    }
    links.sort();
    if links.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "\n\n## Retained options and discussion\n\nThese are captured historical revisions; planning validates the current revision.\n\n{}",
        links
            .iter()
            .map(|path| format!("- `{}`", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn instructions(component: &Path, task: &str) -> String {
    let component = component_argument(component);
    let task = quote(task);
    format!(
        "## Discuss, choose, redirect or withdraw\n\n\
         - Ask/clarify: `kvist decision {component} discuss 'Your question' --task {task}`.\n\
         - Get stable options: `kvist decision {component} options --task {task}`.\n\
         - Stage a choice: `kvist decision {component} plan --task {task} --option OPTION_ID [--guidance 'Additional direction']`.\n\
         - Reject/redirect: `kvist decision {component} plan --task {task} --guidance 'Reject the suggestions; instead ...'`.\n\
         - Withdraw: request an explicit withdrawal plan with reasons and the entire dependent closure; this never marks work completed.\n\
         - Manual/offline: `kvist decision {component} plan --task {task} --import draft.json --review-exception 'My explicit intent-review exception'`.\n\n\
         Planning changes nothing controlled. Inspect the generated Markdown and exact JSON plan; \
         use its printed `apply PLAN_ID --approve SHA256 [--continue]` command only after choosing its complete effects. \
         Approval acknowledges advisory findings or the recorded exception, not compliance. \
         Failed/unknown attempts must be recovered before planning. Do not edit TODOS.yaml or delete execution evidence."
    )
}

fn options(project: &Path, component: &Path, task: Option<&str>) -> Result<Output> {
    let input = engine::capture(project, component, task)?;
    let context = engine::context_for(project, component)?;
    let identity = capture_id(&input)?;
    let name = format!("{identity}.options");
    let previous = read_optional_file(
        &context
            .component_dir
            .join(DIRECTORY)
            .join(format!("{name}.json")),
    )?;
    let mut evidence = intent_evidence(&context, &input)?;
    evidence["human_request"] =
        serde_json::json!("Prepare substantive options for this decision; do not apply them.");
    let report = ask(&context, &input.task, "decision-options", &evidence)?;
    let alternatives: Alternatives = strict_parse(&report)?;
    validate_alternatives(&alternatives)?;
    let catalog = Catalog {
        schema_version: 1,
        capture: identity.clone(),
        task_id: input.task.id.clone(),
        alternatives,
    };
    let mut markdown = format!(
        "# Decision alternatives\n\n{}\n\n",
        catalog.alternatives.summary
    );
    for option in &catalog.alternatives.options {
        markdown.push_str(&format!(
            "## {}: {}\n\n{}\n\n**Advisory recommendation:** {}\n\n",
            option.id, option.title, option.consequences, option.recommendation,
        ));
    }
    markdown.push_str(&instructions(component, &input.task.id));
    let path = save_support(&context, &name, &catalog, &markdown, previous.as_deref())?;
    let actions = engine::shortcuts::options(&context, &catalog, &encode(&catalog)?)?;
    let mut output = response(
        format!(
            "{markdown}\n\nMarkdown: `{}`{}",
            path.display(),
            engine::shortcuts::markdown(component, &actions)
        ),
        Some(path),
    );
    output.actions = actions;
    Ok(output)
}

fn discuss(project: &Path, component: &Path, task: Option<&str>, message: &str) -> Result<Output> {
    meaningful(message, "discussion question", 16 * 1024)?;
    let input = engine::capture(project, component, task)?;
    let context = engine::context_for(project, component)?;
    let identity = capture_id(&input)?;
    let name = format!("{identity}.discussion");
    let path = context
        .component_dir
        .join(DIRECTORY)
        .join(format!("{name}.json"));
    let previous = read_optional_file(&path)?;
    let mut conversation = previous
        .as_deref()
        .map(strict_parse::<Conversation>)
        .transpose()?
        .unwrap_or(Conversation {
            schema_version: 1,
            capture: identity.clone(),
            task_id: input.task.id.clone(),
            exchanges: Vec::new(),
        });
    validate_conversation(&conversation)?;
    if conversation.schema_version != 1
        || conversation.capture != identity
        || conversation.task_id != input.task.id
    {
        return Err(decision_error(
            "conversation does not match the frozen decision revision",
        ));
    }
    if conversation.exchanges.len() >= 32 {
        return Err(decision_error(
            "discussion reached 32 exchanges; prepare a plan or preserve this discussion and create a new decision revision. No history was truncated",
        ));
    }
    let mut evidence = intent_evidence(&context, &input)?;
    evidence["conversation"] = serde_json::to_value(&conversation.exchanges).map_err(json_error)?;
    evidence["human_request"] = serde_json::json!(message);
    let answer = ask(&context, &input.task, "decision-discussion", &evidence)?;
    conversation.exchanges.push(Exchange {
        question: message.to_owned(),
        answer,
    });
    if encode(&conversation)?.len() > 256 * 1024 {
        return Err(decision_error(
            "discussion exceeds 256 KiB; no history was truncated or overwritten",
        ));
    }
    let mut markdown =
        "# Decision discussion (advisory)\n\nNo controlled project files have changed.\n\n"
            .to_owned();
    for (index, exchange) in conversation.exchanges.iter().enumerate() {
        markdown.push_str(&format!(
            "## Question {}\n\n{}\n\n### Agent answer\n\n{}\n\n",
            index + 1,
            exchange.question,
            exchange.answer,
        ));
    }
    markdown.push_str(&instructions(component, &input.task.id));
    let path = save_support(
        &context,
        &name,
        &conversation,
        &markdown,
        previous.as_deref(),
    )?;
    let latest = conversation
        .exchanges
        .last()
        .ok_or_else(|| decision_error("missing recorded discussion answer"))?;
    Ok(response(
        format!(
            "{}\n\nDiscussion: `{}`\nNo controlled files changed.",
            latest.answer,
            path.display()
        ),
        Some(path),
    ))
}

fn prepare(
    project: &Path,
    component: &Path,
    task: Option<&str>,
    mut selection: Selection,
    import: Option<&Path>,
    review_exception: Option<String>,
    catalog_pin: Option<String>,
) -> Result<Output> {
    if catalog_pin.is_some() && selection.option.is_none() {
        return Err(decision_error("a catalog pin requires an explicit option"));
    }
    let input = engine::capture(project, component, task)?;
    let context = engine::context_for(project, component)?;
    meaningful(&selection.guidance, "guidance", 16 * 1024).or_else(|error| {
        if selection.guidance.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    })?;
    let mut evidence = intent_evidence(&context, &input)?;
    if let Some(id) = &selection.option {
        let path = context
            .component_dir
            .join(DIRECTORY)
            .join(format!("{}.options.json", capture_id(&input)?));
        let text = read_optional_file(&path)?.ok_or_else(|| decision_error("no current option catalog; run decision COMPONENT options first, or supply custom --guidance"))?;
        if let Some(expected) = &catalog_pin
            && expected != &digest(text.as_bytes())
        {
            return Err(decision_error(
                "pinned option response is stale; request current options; its number was not reassigned",
            ));
        }
        let catalog: Catalog = strict_parse(&text)?;
        if catalog.schema_version != 1
            || catalog.capture != capture_id(&input)?
            || catalog.task_id != input.task.id
        {
            return Err(decision_error(
                "option catalog is stale or has an unsupported version",
            ));
        }
        validate_alternatives(&catalog.alternatives)?;
        let option = catalog
            .alternatives
            .options
            .iter()
            .find(|option| &option.id == id)
            .ok_or_else(|| {
                decision_error("unknown option ID; inspect the current option Markdown")
            })?;
        let chosen = encode(option)?;
        selection.guidance = format!(
            "Selected substantive option:\n{chosen}\n\nAdditional human guidance:\n{}",
            selection.guidance
        );
        evidence["selected_choice"] = serde_json::to_value(option).map_err(json_error)?;
    }
    evidence["human_request"] = serde_json::json!(selection.guidance);
    evidence["draft_schema"] = serde_json::json!({
        "schema_version":1,
        "rationale":"Explain the complete authorized effects and assumptions",
        "documents":{"REQUIREMENTS.md":"optional complete changed text", "CONTRACT.md":"optional complete changed text", "DESIGN.md":"optional complete changed text"},
        "document_edits":[{"path":"DESIGN.md","before":"exact unique excerpt from the supplied document","after":"replacement text"}],
        "tasks":[
            {"action":"add","task":{"id":"new-write-tests","title":"write_tests: ...","description":"...","context":"...","purpose":"...","expected_outcome":"...","kind":"test","depends_on":[],"requirements":["REQUIREMENTS.md#REQ-ID"]}},
            {"action":"update","task":"same complete authored fields for an existing eligible task"},
            {"action":"retire","id":"uncompleted-task","reason":"explicit withdrawal reason"}
        ],
        "disposition":"resolve or withdraw; exactly one word, not both",
        "notes":"Do not include this `notes` key in your actual draft. Keep `documents: {}` and `document_edits: []` when no documents change. Each `{\"action\":\"add\",\"task\":{...}}` item in `tasks` closes with exactly TWO braces `}}`, never `}}}`. Added tasks are inserted BEFORE the paused task as its prerequisites, so a new `-write-tests` task MUST use `\"depends_on\": []` (never the paused task ID). Do not `update` downstream tasks that already depend on the paused task."
    });
    let draft: Draft = if let Some(path) = import {
        let bytes = bounded_read(path)?;
        strict_parse(&bytes)?
    } else {
        if selection.option.is_none() && selection.guidance.trim().is_empty() {
            return Err(decision_error(
                "planning requires --option, --guidance, or --import; no choice was inferred",
            ));
        }
        strict_parse(&ask(&context, &input.task, "decision-plan", &evidence)?)?
    };
    let review = match review_exception {
        Some(reason) => {
            meaningful(&reason, "intent-review exception", 8192)?;
            IntentReview {
                report: reason,
                exception: true,
            }
        }
        None => {
            evidence
                .as_object_mut()
                .ok_or_else(|| decision_error("invalid intent evidence"))?
                .remove("draft_schema");
            evidence["proposed_draft"] = serde_json::to_value(&draft).map_err(json_error)?;
            IntentReview {
                report: ask(&context, &input.task, "decision-intent-review", &evidence)?,
                exception: false,
            }
        }
    };
    let plan = engine::prepare(project, input, draft, selection, review)?;
    let path = plan_markdown(project, component, &plan.id)?;
    let actions = engine::shortcuts::plan(&context, &plan).map_err(|error| decision_error(
        &format!("Plan retained at `{}` but numbered response publication failed: {error}. Use the exact command: kvist decision {} apply {} --approve {}",
            path.display(), component_argument(component), quote(&plan.id), quote(&plan.approval))))?;
    Ok(Output {
        message: format!(
            "Staged a decision plan; no controlled files changed.\nRead the complete changes: `{}`\nUse a numbered approval response below; no hash copying is needed. Approval acknowledges the recorded intent-review findings/exception, not implementation compliance.",
            path.display(),
        ) + &engine::shortcuts::markdown(component, &actions),
        markdown_path: Some(path),
        plan_id: Some(plan.id),
        approval: Some(plan.approval),
        actions,
    })
}

fn intent_evidence(context: &TaskContext, input: &Capture) -> Result<serde_json::Value> {
    let mut documents = input.files.clone();
    documents.remove("TODOS.yaml");
    documents.insert(
        "ROOT_CONTRACT.md".into(),
        bounded_read(&context.project_dir.join("ROOT_CONTRACT.md"))?,
    );
    if let Some(parent) = &input.queue.component.parent_contract {
        let configuration = crate::config::load(&context.project_dir)?;
        let root = context.project_dir.join(&configuration.component_root);
        let (directory, _) = discovery::find_parent_component(&root, &context.component_path)?
            .ok_or_else(|| decision_error("declared parent contract has no immediate ancestor"))?;
        documents.insert(
            format!("PARENT_CONTRACT.md ({})", parent.path),
            bounded_read(&directory.join("CONTRACT.md"))?,
        );
    }
    let mut retained = BTreeMap::new();
    let mut total = documents.values().map(String::len).sum::<usize>();
    let mut entries = 0;
    for directory in [
        ".kvist/attention",
        ".kvist/continuous-reviews",
        ".kvist/authoring/proposals",
    ] {
        let path = context.component_dir.join(directory);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !is_link_like(&metadata) => {}
            Ok(_) => {
                return Err(decision_error(
                    "decision evidence directory must be a real non-link directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect decision evidence directory",
                    path,
                    source,
                });
            }
        }
        for entry in fs::read_dir(&path).map_err(|source| KvistError::Io {
            operation: "read decision evidence directory",
            path: path.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| KvistError::Io {
                operation: "read decision evidence entry",
                path: path.clone(),
                source,
            })?;
            entries += 1;
            if entries > 4096 {
                return Err(decision_error(
                    "decision evidence exceeds 4096 directory entries",
                ));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if (name.starts_with(&input.task.id) || directory == ".kvist/authoring/proposals")
                && name.ends_with(".md")
            {
                let text = bounded_read(&entry.path())?;
                total += text.len();
                if total > MAX_TOTAL {
                    return Err(decision_error(
                        "decision evidence exceeds 8 MiB; no input was truncated",
                    ));
                }
                retained.insert(format!("{directory}/{name}"), text);
            }
        }
    }
    let tasks = input
        .queue
        .tasks
        .iter()
        .map(|task| {
            serde_json::json!({
                "id":task.id,"title":task.title,"kind":task.kind,"status":task.status,
                "depends_on":task.depends_on,"requirements":task.requirements,
            })
        })
        .collect::<Vec<_>>();
    let conversation_path = context
        .component_dir
        .join(DIRECTORY)
        .join(format!("{}.discussion.json", capture_id(input)?));
    let conversation = read_support::<Conversation>(&conversation_path)?;
    if let Some(conversation) = &conversation {
        validate_conversation(conversation)?;
    }
    if let Some(conversation) = &conversation
        && (conversation.schema_version != 1
            || conversation.capture != capture_id(input)?
            || conversation.task_id != input.task.id)
    {
        return Err(decision_error(
            "discussion input has a mismatched decision identity",
        ));
    }
    let evidence = serde_json::json!({
        "schema_version":1,"decision":input.task,"local_and_allowed_context":documents,
        "queue_graph":tasks,"retained_advisory_evidence":retained,
        "conversation":conversation.map(|conversation| conversation.exchanges),
        "boundary":"Proposal/discussion only; never read peer implementations or claim source/compliance approval.",
    });
    if encode(&evidence)?.len() > MAX_TOTAL {
        return Err(decision_error(
            "decision evidence exceeds 8 MiB; no input was truncated",
        ));
    }
    Ok(evidence)
}

fn ask(
    context: &TaskContext,
    task: &Task,
    phase: &'static str,
    evidence: &serde_json::Value,
) -> Result<String> {
    let configuration = crate::config::load(&context.project_dir)?;
    let redactions = evidence_redactions(&configuration);
    let profile = &configuration.agent.architect;
    let mut session = crate::agent::review::ReviewSession::decision(
        profile,
        &context.component_dir,
        &task.id,
        redactions,
    )?;
    let outcome = encode(evidence).and_then(|evidence| session.request(phase, evidence));
    session.finish(
        if outcome.is_ok() {
            "completed"
        } else {
            "failed"
        },
        outcome.as_ref().err(),
    )?;
    outcome.map(|report| report.report)
}

fn capture_id(input: &Capture) -> Result<String> {
    let mut input = input.clone();
    input.bindings.remove("decision-options");
    input.bindings.remove("decision-discussion");
    Ok(format!(
        "decision-{}",
        digest(encode(&input)?.as_bytes()).trim_start_matches("sha256:")
    ))
}

fn plan_markdown(project: &Path, component: &Path, id: &str) -> Result<PathBuf> {
    Ok(engine::context_for(project, component)?
        .component_dir
        .join(DIRECTORY)
        .join(format!("{id}.md")))
}

fn validate_alternatives(value: &Alternatives) -> Result<()> {
    meaningful(&value.summary, "option summary", 8192)?;
    if value.options.is_empty() || value.options.len() > 32 {
        return Err(decision_error(
            "a decision catalog must contain 1 to 32 substantive alternatives",
        ));
    }
    let mut ids = BTreeSet::new();
    for option in &value.options {
        if option.id.is_empty()
            || option.id.len() > 128
            || !option
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !ids.insert(&option.id)
        {
            return Err(decision_error(
                "option IDs must be distinct bounded ASCII identifiers",
            ));
        }
        meaningful(&option.title, "option title", 1024)?;
        meaningful(&option.consequences, "option consequences", 8192)?;
        meaningful(&option.recommendation, "option recommendation", 8192)?;
    }
    Ok(())
}

fn validate_conversation(value: &Conversation) -> Result<()> {
    if value.schema_version != 1 || value.exchanges.len() > 32 || encode(value)?.len() > 256 * 1024
    {
        return Err(decision_error(
            "discussion must be version 1, at most 32 exchanges and 256 KiB; no history was truncated",
        ));
    }
    for exchange in &value.exchanges {
        meaningful(&exchange.question, "recorded question", 16 * 1024)?;
        meaningful(&exchange.answer, "recorded answer", 65536)?;
    }
    Ok(())
}
fn meaningful(text: &str, field: &str, limit: usize) -> Result<()> {
    if text.trim().is_empty() || text.len() > limit {
        return Err(decision_error(&format!(
            "{field} must contain meaningful text within {limit} bytes"
        )));
    }
    Ok(())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn component_argument(component: &Path) -> String {
    let text = component.to_string_lossy();
    let mut command =
        <Action as clap::Subcommand>::augment_subcommands(clap::Command::new("decision"));
    command.build();
    if text.starts_with('-')
        || command
            .get_subcommands()
            .any(|command| command.get_name() == text)
    {
        quote(&format!("./{text}"))
    } else {
        quote(&text)
    }
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(json_error)
}

fn json_error(error: serde_json::Error) -> KvistError {
    decision_error(&format!("invalid decision JSON: {error}"))
}

fn decision_error(reason: &str) -> KvistError {
    KvistError::AgentEvidenceFailed {
        reason: format!("decision support: {reason}"),
    }
}

fn strict_parse<T: for<'a> Deserialize<'a>>(text: &str) -> Result<T> {
    if text.len() > MAX_FILE as usize {
        return Err(decision_error("decision response/draft exceeds 1 MiB"));
    }
    serde_json::from_str(text).map_err(|error| {
        let preview: String = text
            .chars()
            .take(200)
            .flat_map(|ch| {
                if ch.is_control() {
                    ch.escape_default().collect::<Vec<_>>()
                } else {
                    vec![ch]
                }
            })
            .collect();
        decision_error(&format!(
            "invalid decision JSON: {error} (received {} bytes, preview: `{preview}`)",
            text.len()
        ))
    })
}

fn bounded_read(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect bounded decision file",
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(decision_error("decision input must not be hard-linked"));
        }
    }
    let bytes = read_bounded_regular_file(path, &metadata, MAX_FILE)?;
    String::from_utf8(bytes).map_err(|_| decision_error("decision files must contain UTF-8"))
}

fn read_support<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<Option<T>> {
    read_optional_file(path)?
        .as_deref()
        .map(strict_parse)
        .transpose()
}

fn read_optional_file(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(_) => bounded_read(path).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(KvistError::Io {
            operation: "inspect decision support record",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn save_support<T: Serialize>(
    context: &TaskContext,
    name: &str,
    record: &T,
    markdown: &str,
    previous: Option<&str>,
) -> Result<PathBuf> {
    let bytes = encode(record)?;
    if bytes.len() > MAX_FILE as usize || markdown.len() > MAX_FILE as usize {
        return Err(decision_error(
            "decision support record exceeds 1 MiB; no history was truncated",
        ));
    }
    let now = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(context, "decision-support", &now)?;
    ensure_no_pending(&context.component_dir)?;
    continuous::checked_directory(&context.component_dir.join(".kvist"))?;
    let directory = context.component_dir.join(DIRECTORY);
    continuous::checked_directory(&directory)?;
    if read_optional_file(&directory.join(format!("{name}.json")))?.as_deref() != previous {
        return Err(decision_error(
            "discussion/options changed during inference; repeat after inspecting the current conversation. No history was overwritten",
        ));
    }
    let identity = digest(bytes.as_bytes());
    for (extension, text) in [("json", bytes.as_str()), ("md", markdown)] {
        let history = directory.join(format!(
            "{name}.{}.{}",
            identity.trim_start_matches("sha256:"),
            extension
        ));
        match fs::symlink_metadata(&history) {
            Ok(_) => {
                if bounded_read(&history)? != text {
                    return Err(decision_error(
                        "decision history differs from its immutable identity",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_new_file_atomically(&history, text)?
            }
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect decision history",
                    path: history,
                    source,
                });
            }
        }
        let latest = directory.join(format!("{name}.{extension}"));
        match fs::symlink_metadata(&latest) {
            Ok(_) => {
                bounded_read(&latest)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect decision alias",
                    path: latest,
                    source,
                });
            }
        }
        replace_file_atomically(&latest, text)?;
    }
    lock.release()?;
    Ok(directory.join(format!("{name}.md")))
}
