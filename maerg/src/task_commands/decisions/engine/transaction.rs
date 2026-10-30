use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    plan: Plan,
    completed: BTreeSet<String>,
    lock_digest: String,
    authentication_tag: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    plan: Plan,
    applied_at: Timestamp,
    source_derived_evidence_stale: bool,
    follow_up: String,
    execution_accepted: bool,
    compliance_certified: bool,
    model_findings_accepted: bool,
    git_published: bool,
    authentication_tag: String,
}

pub(super) fn prepare(
    project_dir: &Path,
    input: Capture,
    draft: Draft,
    selection: Selection,
    review: IntentReview,
) -> Result<Plan> {
    let context = context_for(project_dir, &input.component)?;
    let fresh = capture(project_dir, &input.component, Some(&input.task.id))?;
    if encode(&fresh)? != encode(&input)? {
        return Err(fail(
            &context.component_dir,
            "captured inputs are stale or inconsistent; capture again",
        ));
    }
    if review.report.trim().is_empty() {
        return Err(fail(
            &context.component_dir,
            "a nonblank advisory review or explicit review exception is required",
        ));
    }
    for text in [&review.report, &selection.guidance] {
        bounded(text, Path::new("decision input"))?;
    }
    if selection.option.as_ref().is_some_and(|option| {
        option.trim().is_empty() || option.len() > 256 || option.chars().any(char::is_control)
    }) {
        return Err(fail(
            &context.component_dir,
            "selected option ID must be nonblank, bounded and free of control characters",
        ));
    }
    let at = now()?;
    let disposition = draft.disposition;
    let changes = tasks::outputs(&context, &input, draft, &at)?;
    let mut plan = Plan {
        schema_version: 1,
        id: String::new(),
        approval: String::new(),
        input,
        selection,
        rationale: changes.rationale,
        review,
        outputs: changes.outputs,
        retired: changes.retired,
        retirement_reasons: changes.retirement_reasons,
        disposition,
        created_at: at,
        authentication_tag: String::new(),
    };
    let (id, approval) = identity(&plan)?;
    plan.id = id;
    plan.approval = approval;
    let secret = secret(&context)?;
    plan.authentication_tag = authenticated(&plan, &secret)?;
    let json = encode(&plan)?;
    bounded(&json, Path::new("plan.json"))?;
    let markdown = preview(&plan)?;
    bounded(&markdown, Path::new("plan.md"))?;
    let total = json
        .len()
        .saturating_add(markdown.len())
        .saturating_add(plan.input.files.values().map(String::len).sum::<usize>())
        .saturating_add(plan.outputs.values().map(String::len).sum::<usize>());
    if total > MAX_TOTAL {
        return Err(fail(
            &context.component_dir,
            "plan and preview exceed the 8 MiB aggregate limit",
        ));
    }
    let fresh = capture(
        project_dir,
        &plan.input.component,
        Some(&plan.input.task.id),
    )?;
    if encode(&fresh)? != encode(&plan.input)? {
        return Err(fail(
            &context.component_dir,
            "inputs changed before plan persistence; regenerate",
        ));
    }
    let directory = context.component_dir.join(DIRECTORY);
    storage::directory(&directory)?;
    retain(&directory.join(format!("{}.md", plan.id)), &markdown)?;
    retain(&directory.join(format!("{}.json", plan.id)), &json)?;
    Ok(plan)
}

fn retain(path: &Path, text: &str) -> Result<()> {
    if exists(path)? {
        if read_text(path)? != text {
            return Err(fail(
                path,
                "content-addressed decision history already exists with different bytes; never clobber it",
            ));
        }
        return Ok(());
    }
    write_new(path, text)
}

fn identity(plan: &Plan) -> Result<(String, String)> {
    let mut payload = plan.clone();
    payload.id.clear();
    payload.approval.clear();
    payload.authentication_tag.clear();
    let value = serde_json::to_value(payload)
        .map_err(|error| fail(Path::new(DIRECTORY), error.to_string()))?;
    let json = encode(&value)?;
    let id = digest(format!("kvist decision plan v1\n{json}").as_bytes())
        .trim_start_matches("sha256:")
        .to_owned();
    let approval = digest(format!("kvist decision human approval v1\n{json}").as_bytes());
    Ok((id, approval))
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_plan(plan: &Plan, secret: &[u8], context: &TaskContext, id: &str) -> Result<()> {
    let (expected_id, approval) = identity(plan)?;
    if plan.schema_version != 1
        || plan.input.schema_version != 1
        || !valid_id(id)
        || !equal(&plan.id, id)
        || !equal(&plan.id, &expected_id)
        || !equal(&plan.approval, &approval)
        || !equal(&plan.authentication_tag, &authenticated(plan, secret)?)
        || plan.input.component != context.component_path
        || plan.input.bindings.get("identity/project")
            != Some(&digest(context.project_dir.as_os_str().as_encoded_bytes()))
        || plan.input.bindings.get("identity/component")
            != Some(&digest(
                absolute(&context.component_dir)?
                    .as_os_str()
                    .as_encoded_bytes(),
            ))
    {
        return Err(fail(
            &context.component_dir.join(DIRECTORY),
            "plan identity, approval or authentication failed; edited tokens never authorize changed content",
        ));
    }
    if plan
        .input
        .files
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from(["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "TODOS.yaml"])
        || !plan.outputs.contains_key("TODOS.yaml")
    {
        return Err(fail(
            &context.component_dir,
            "plan contains an invalid controlled-file scope",
        ));
    }
    for (name, text) in &plan.outputs {
        if name == "TODOS.yaml" {
            parse(text).map_err(|error| fail(Path::new(name), error.to_string()))?;
        } else {
            capture::validate_document(name, text)?;
        }
    }
    Ok(())
}

pub(super) fn load_plan(project_dir: &Path, component: &Path, id: &str) -> Result<Plan> {
    let context = context_for(project_dir, component)?;
    let secret = secret(&context)?;
    load(&context, id, &secret)
}

fn load(context: &TaskContext, id: &str, secret: &[u8]) -> Result<Plan> {
    if !valid_id(id) {
        return Err(fail(
            &context.component_dir,
            "plan ID must be exactly 64 lowercase hexadecimal characters",
        ));
    }
    let directory = context.component_dir.join(DIRECTORY);
    let plan: Plan = decode(&directory.join(format!("{id}.json")))?;
    validate_plan(&plan, secret, context, id)?;
    let markdown = read_text(&directory.join(format!("{id}.md")))?;
    if markdown != preview(&plan)? {
        return Err(fail(
            &directory,
            "plan preview is missing or changed; do not approve an edited preview",
        ));
    }
    Ok(plan)
}

fn journal(context: &TaskContext, secret: &[u8]) -> Result<Option<Journal>> {
    let path = context.component_dir.join(DIRECTORY).join("pending.json");
    if !exists(&path)? {
        return Ok(None);
    }
    let journal: Journal = decode(&path)?;
    if journal.schema_version != 1
        || !equal(
            &journal.authentication_tag,
            &authenticated(&journal, secret)?,
        )
        || journal
            .completed
            .iter()
            .any(|name| !journal.plan.outputs.contains_key(name))
    {
        return Err(fail(
            &path,
            "pending transaction authentication/progress failed; preserve recovery evidence",
        ));
    }
    validate_plan(&journal.plan, secret, context, &journal.plan.id)?;
    Ok(Some(journal))
}

fn save_journal(
    context: &TaskContext,
    journal: &mut Journal,
    secret: &[u8],
    new: bool,
) -> Result<()> {
    let path = context.component_dir.join(DIRECTORY).join("pending.json");
    if !new {
        let current: Journal = decode(&path)?;
        if !equal(
            &current.authentication_tag,
            &authenticated(&current, secret)?,
        ) || !equal(&current.authentication_tag, &journal.authentication_tag)
        {
            return Err(fail(
                &path,
                "pending progress changed outside this authenticated transaction",
            ));
        }
    }
    journal.authentication_tag = authenticated(journal, secret)?;
    let text = encode(journal)?;
    if new {
        write_new(&path, &text)
    } else {
        replace(&path, &text)
    }
}

pub(super) fn apply(
    project_dir: &Path,
    component: &Path,
    id: &str,
    approval: &str,
    fail_after: Option<usize>,
) -> Result<Plan> {
    let context = context_for(project_dir, component)?;
    let secret = secret(&context)?;
    let plan = load(&context, id, &secret)?;
    if !equal(approval, &plan.approval) {
        return Err(fail(
            &context.component_dir,
            "exact content-bound human approval is required",
        ));
    }
    let pending = journal(&context, &secret)?;
    if pending
        .as_ref()
        .is_some_and(|journal| encode(&journal.plan).ok() != encode(&plan).ok())
    {
        return Err(fail(
            &context.component_dir,
            "another decision transaction is pending; only its identical approved plan may recover",
        ));
    }
    let at = now()?;
    let lock = if let Some(journal) = &pending {
        TaskLock::recover_for_context(&context, "decision-apply", &at, &journal.lock_digest)?
    } else {
        TaskLock::for_context(&context, "decision-apply", &at)?
    };
    let result = apply_locked(&context, &plan, pending, &secret, &lock, fail_after);
    match (result, lock.release()) {
        (Ok(()), Ok(())) => Ok(plan),
        (Err(error), _) | (Ok(()), Err(error)) => Err(error),
    }
}

fn apply_locked(
    context: &TaskContext,
    plan: &Plan,
    pending: Option<Journal>,
    secret: &[u8],
    lock: &TaskLock,
    fail_after: Option<usize>,
) -> Result<()> {
    let directory = context.component_dir.join(DIRECTORY);
    let receipt_path = directory.join(format!("{}.applied.json", plan.id));
    if exists(&receipt_path)? {
        let receipt: Receipt = decode(&receipt_path)?;
        if receipt.schema_version != 1
            || !equal(
                &receipt.authentication_tag,
                &authenticated(&receipt, secret)?,
            )
            || encode(&receipt.plan)? != encode(plan)?
            || !receipt.source_derived_evidence_stale
            || receipt.execution_accepted
            || receipt.compliance_certified
            || receipt.model_findings_accepted
            || receipt.git_published
        {
            return Err(fail(
                &receipt_path,
                "applied receipt authentication/identity failed",
            ));
        }
        if pending.is_some() {
            capture::verify(context, &plan.input, &plan.outputs, true)?;
            require_outputs(context, plan)?;
            lock.revalidate()?;
            clear_pending(context, &plan.id, secret)?;
        }
        return Ok(());
    }
    capture::verify(context, &plan.input, &plan.outputs, pending.is_some())?;
    let mut journal = match pending {
        Some(journal) => journal,
        None => {
            let mut journal = Journal {
                schema_version: 1,
                plan: plan.clone(),
                completed: BTreeSet::new(),
                lock_digest: lock.contents_digest(),
                authentication_tag: String::new(),
            };
            lock.revalidate()?;
            save_journal(context, &mut journal, secret, true)?;
            journal
        }
    };
    journal.lock_digest = lock.contents_digest();
    lock.revalidate()?;
    save_journal(context, &mut journal, secret, false)?;
    let mut writes = 0;
    for (name, after) in &plan.outputs {
        lock.revalidate()?;
        capture::verify(context, &plan.input, &plan.outputs, true)?;
        let path = context.component_dir.join(name);
        let current = read_text(&path)?;
        if current != *after {
            if journal.completed.contains(name) {
                return Err(fail(
                    &path,
                    "a recorded completed write was externally reverted; recovery refuses to clobber it",
                ));
            }
            if plan.input.files.get(name) != Some(&current) {
                return Err(fail(
                    &path,
                    "file is neither the exact before nor after bytes",
                ));
            }
            lock.revalidate()?;
            replace_controlled(&path, &current, after, lock)?;
            writes += 1;
            if fail_after == Some(writes) {
                return Err(fail(
                    &path,
                    "injected interruption after durable file replacement",
                ));
            }
        }
        journal.completed.insert(name.clone());
        lock.revalidate()?;
        save_journal(context, &mut journal, secret, false)?;
    }
    capture::verify(context, &plan.input, &plan.outputs, true)?;
    require_outputs(context, plan)?;
    lock.revalidate()?;
    let mut receipt = Receipt {
        schema_version: 1,
        plan: plan.clone(),
        applied_at: now()?,
        source_derived_evidence_stale: true,
        follow_up: "Run the ordinary test, implementation, independent security audit and clean-slate observation/source-blind compliance review lifecycle for changed intent; retained IMPL.md and reports were not rewritten and do not certify the new intent.".to_owned(),
        execution_accepted: false,
        compliance_certified: false,
        model_findings_accepted: false,
        git_published: false,
        authentication_tag: String::new(),
    };
    receipt.authentication_tag = authenticated(&receipt, secret)?;
    write_new(&receipt_path, &encode(&receipt)?)?;
    if fail_after == Some(plan.outputs.len() + 1) {
        return Err(fail(
            &receipt_path,
            "injected interruption after durable applied receipt",
        ));
    }
    lock.revalidate()?;
    clear_pending(context, &plan.id, secret)
}

fn clear_pending(context: &TaskContext, id: &str, secret: &[u8]) -> Result<()> {
    let retained = journal(context, secret)?.ok_or_else(|| {
        fail(
            &context.component_dir,
            "pending decision journal disappeared before durable cleanup",
        )
    })?;
    if retained.plan.id != id {
        return Err(fail(
            &context.component_dir,
            "pending decision identity changed before cleanup",
        ));
    }
    remove(&context.component_dir.join(DIRECTORY).join("pending.json"))
}

fn require_outputs(context: &TaskContext, plan: &Plan) -> Result<()> {
    for (name, expected) in &plan.outputs {
        if read_text(&context.component_dir.join(name))? != *expected {
            return Err(fail(
                &context.component_dir.join(name),
                "applied receipt requires exact final output bytes",
            ));
        }
    }
    Ok(())
}

pub(super) fn recognizes_retired_journal(component: &Path, journal: &Path) -> Result<bool> {
    let component = absolute(component)?;
    let journal = absolute(journal)?;
    safe_path(&component, false)?;
    safe_path(&journal, false)?;
    if journal.parent() != Some(component.join(".kvist-attempts").as_path())
        || journal.extension().and_then(|part| part.to_str()) != Some("jsonl")
    {
        return Ok(false);
    }
    let id = journal
        .file_stem()
        .and_then(|part| part.to_str())
        .ok_or_else(|| fail(&journal, "retired journal identity must be UTF-8"))?;
    let directory = component.join(DIRECTORY);
    if !exists(&directory)? {
        return Ok(false);
    }
    let mut receipts = Vec::new();
    let mut entries = 0;
    for entry in fs::read_dir(&directory).map_err(|error| fail(&directory, error.to_string()))? {
        entries += 1;
        let entry = entry.map_err(|error| fail(&directory, error.to_string()))?;
        if entries > 10_000 {
            return Err(fail(
                &directory,
                "retired receipt inventory exceeds entry bound",
            ));
        }
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|name| name.ends_with(".applied.json"))
        {
            receipts.push(entry.path());
        }
    }
    if receipts.is_empty() {
        return Ok(false);
    }
    receipts.sort();
    let secret = existing_secret()?;
    let observed = digest(&read_bytes(&journal)?);
    let key = format!("local/.kvist-attempts/{id}.jsonl");
    let mut total = 0_usize;
    for path in receipts {
        let text = read_text(&path)?;
        total = total.saturating_add(text.len());
        if total > MAX_TOTAL {
            return Err(fail(&directory, "retired receipt inventory exceeds 8 MiB"));
        }
        let receipt: Receipt = serde_json::from_str(&text)
            .map_err(|error| fail(&path, format!("invalid applied receipt: {error}")))?;
        let plan = &receipt.plan;
        let (expected_id, approval) = identity(plan)?;
        if receipt.schema_version != 1
            || plan.schema_version != 1
            || plan.input.bindings.get("identity/component")
                != Some(&digest(component.as_os_str().as_encoded_bytes()))
            || !equal(
                &receipt.authentication_tag,
                &authenticated(&receipt, &secret)?,
            )
            || !equal(&plan.authentication_tag, &authenticated(plan, &secret)?)
            || !equal(&plan.id, &expected_id)
            || !equal(&plan.approval, &approval)
            || path.file_name().and_then(|name| name.to_str())
                != Some(format!("{}.applied.json", plan.id).as_str())
        {
            return Err(fail(
                &path,
                "retired task receipt authentication/identity failed",
            ));
        }
        if let Some(task) = plan.retired.iter().find(|task| task.id == id) {
            if plan.disposition != Disposition::Withdraw
                || plan
                    .input
                    .queue
                    .tasks
                    .iter()
                    .find(|original| original.id == id)
                    != Some(task)
                || plan
                    .retirement_reasons
                    .get(id)
                    .is_none_or(|reason| reason.trim().is_empty())
                || plan.input.bindings.get(&key) != Some(&observed)
                || task.recovery_state.is_some()
                || matches!(task.status, TaskStatus::Completed | TaskStatus::InProgress)
            {
                return Err(fail(
                    &journal,
                    "retired journal differs from its authenticated full archived task and exact evidence digest",
                ));
            }
            return Ok(true);
        }
    }
    Ok(false)
}

fn code_block(text: &str) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!(
        "{fence}\n{text}{}{fence}\n",
        if text.ends_with('\n') { "" } else { "\n" }
    )
}

fn preview(plan: &Plan) -> Result<String> {
    let mut text = format!(
        "# Deterministic decision plan\n\nPlan: `{}`\n\nComponent: `{}`\n\nPaused task: `{}`\n\nDisposition: `{:?}`\n\nExact approval: `{}`\n\nThis approves only these local intent and scheduling changes, not execution, compliance, model findings or Git publication. Application is a durable, recoverable multi-file transaction, not one atomic filesystem operation.\n\n## Selected option and human guidance\n\n{}\n## Rationale\n\n{}\n## Advisory review or explicit exception\n\nException: `{}`\n\n{}\n",
        plan.id,
        plan.input.component.display(),
        plan.input.task.id,
        plan.disposition,
        plan.approval,
        code_block(&encode(&plan.selection)?),
        code_block(&plan.rationale),
        plan.review.exception,
        code_block(&plan.review.report),
    );
    let command = format!(
        "kvist decision {} apply {} --approve {}",
        component_argument(&plan.input.component),
        plan.id,
        plan.approval,
    );
    text.push_str("\n## Apply exactly this plan\n\n");
    text.push_str(&code_block(&command));
    for (name, before) in &plan.input.files {
        let after = plan.outputs.get(name).unwrap_or(before);
        text.push_str(&format!(
            "\n## {name}\n\n### Exact before\n\n{}\n### Exact after\n\n{}\n",
            code_block(before),
            code_block(after)
        ));
    }
    text.push_str(&format!(
        "\n## Retired task history\n\n{}\n### Exact retirement reasons\n\n{}\n",
        code_block(&encode(&plan.retired)?),
        code_block(&encode(&plan.retirement_reasons)?)
    ));
    text.push_str("\n## Evidence and follow-up\n\nRetained IMPL.md and source-derived reports remain historical observations; new intent needs fresh tests, implementation, independent audit, clean-slate observation and source-blind comparison. No source was removed and no task was marked completed by this decision.\n\n### Frozen input bindings\n\n");
    text.push_str(&code_block(&encode(&plan.input.bindings)?));
    Ok(text)
}
