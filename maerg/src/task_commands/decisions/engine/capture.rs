use super::*;

const DOCUMENTS: [DocumentKind; 3] = [
    DocumentKind::Requirements,
    DocumentKind::Contract,
    DocumentKind::Design,
];
const MAX_ENTRIES: usize = 10_000;
type Snapshot = (
    TaskQueue,
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    Budget,
);

pub(super) fn context_for(project_dir: &Path, component: &Path) -> Result<TaskContext> {
    let project_dir = absolute(project_dir)?;
    safe_path(&project_dir, false)?;
    safe_path(&project_dir.join("kvist.toml"), false)?;
    let config = crate::config::load(&project_dir)?;
    let component_path = context::normalize_component_path(component)?;
    let component_dir = project_dir
        .join(&config.component_root)
        .join(&component_path);
    safe_path(&component_dir, false)?;
    if !fs::symlink_metadata(&component_dir)
        .map_err(|error| fail(&component_dir, error.to_string()))?
        .is_dir()
    {
        return Err(fail(
            &component_dir,
            "component must be an in-project directory",
        ));
    }
    Ok(TaskContext {
        project_dir,
        component_path,
        component_dir,
    })
}

pub(super) fn capture(project_dir: &Path, component: &Path, task: Option<&str>) -> Result<Capture> {
    let context = context_for(project_dir, component)?;
    ensure_no_pending(&context.component_dir)?;
    let lock = TaskLock::task_lock_path(&context)?;
    if exists(&lock)? {
        return Err(fail(
            &lock,
            "component has an operation lock; inspect/unlock its owner before planning",
        ));
    }
    let (queue, files, bindings, mut budget) = snapshot(&context)?;
    guards(&context, &queue)?;
    unknown_journals(&context, &queue, None)?;
    let paused = queue
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                TaskStatus::AwaitingDecision | TaskStatus::Blocked
            )
        })
        .collect::<Vec<_>>();
    let task = match task {
        Some(id) => paused
            .iter()
            .find(|task| task.id == id)
            .copied()
            .ok_or_else(|| {
                fail(
                    &context.component_dir,
                    "selected task is not a known paused task",
                )
            })?,
        None if paused.len() == 1 => paused[0],
        None => {
            return Err(fail(
                &context.component_dir,
                "expected exactly one paused task; specify --task to resolve ambiguity",
            ));
        }
    }
    .clone();
    let mut input = Capture {
        schema_version: 1,
        component: context.component_path.clone(),
        task,
        queue,
        files,
        bindings,
    };
    let id = capture_id(&input)?;
    decision_context(&context, &id, &mut budget, &mut input.bindings)?;
    Ok(input)
}

fn decision_context(
    context: &TaskContext,
    id: &str,
    budget: &mut Budget,
    bindings: &mut BTreeMap<String, String>,
) -> Result<()> {
    for (binding, suffix) in [
        ("decision-options", "options"),
        ("decision-discussion", "discussion"),
    ] {
        let path = context
            .component_dir
            .join(DIRECTORY)
            .join(format!("{id}.{suffix}.json"));
        let identity = if exists(&path)? {
            let bytes = read_bytes(&path)?;
            budget.add(bytes.len(), &path)?;
            digest(&bytes)
        } else {
            budget.add(0, &path)?;
            "absent".to_owned()
        };
        bindings.insert(binding.to_owned(), identity);
    }
    Ok(())
}

pub(super) fn validate_document(name: &str, text: &str) -> Result<()> {
    let kind = DOCUMENTS
        .into_iter()
        .find(|kind| kind.filename() == name)
        .ok_or_else(|| {
            fail(
                Path::new(name),
                "only local REQUIREMENTS.md, CONTRACT.md and DESIGN.md may change",
            )
        })?;
    bounded(text, Path::new(name))?;
    let validation = component_documents::validate(kind, text);
    if !validation.is_valid() || !validation.recovery_blockers.is_empty() {
        return Err(fail(
            Path::new(name),
            format!(
                "intent must pass native structural validation and contain no recovery placeholders: {} {}",
                component_documents::format_diagnostics(&validation.diagnostics),
                validation.recovery_blockers.join(", ")
            ),
        ));
    }
    Ok(())
}

pub(super) fn parent(context: &TaskContext) -> Result<Option<crate::task_queue::ParentContract>> {
    if context.component_path == Path::new(".") {
        return Ok(None);
    }
    let config = crate::config::load(&context.project_dir)?;
    let root = context.project_dir.join(config.component_root);
    let mut relative = context.component_path.parent();
    while let Some(candidate) = relative {
        let candidate = if candidate.as_os_str().is_empty() {
            Path::new(".")
        } else {
            candidate
        };
        let directory = root.join(candidate);
        safe_path(&directory, false)?;
        let mut component = false;
        for name in [
            "REQUIREMENTS.md",
            "CONTRACT.md",
            "DESIGN.md",
            "TODOS.yaml",
            "IMPL.md",
        ] {
            component |= exists(&directory.join(name))?;
        }
        if component {
            let path = directory.join("CONTRACT.md");
            let text = read_text(&path)?;
            validate_document("CONTRACT.md", &text)?;
            return Ok(Some(crate::task_queue::ParentContract {
                path: project_state::relative_parent_contract_path(
                    &context.component_path,
                    candidate,
                ),
                revision: digest(text.as_bytes()),
            }));
        }
        if candidate == Path::new(".") {
            break;
        }
        relative = candidate.parent();
    }
    Ok(None)
}

pub(super) fn guards(context: &TaskContext, queue: &TaskQueue) -> Result<()> {
    for task in &queue.tasks {
        if task.status == TaskStatus::InProgress || task.recovery_state.is_some() {
            return Err(fail(
                &context.component_dir,
                format!(
                    "live or fenced task `{}` requires its ordinary recovery workflow",
                    task.id
                ),
            ));
        }
        ensure_task_attempt_recovered(&context.component_dir, &task.id, task.status, None)?;
    }
    check_ledger(context)
}

fn unknown_journals(
    context: &TaskContext,
    queue: &TaskQueue,
    other: Option<&TaskQueue>,
) -> Result<()> {
    let path = context.component_dir.join(".kvist-attempts");
    if !exists(&path)? {
        return Ok(());
    }
    let mut count = 0;
    for entry in fs::read_dir(&path).map_err(|error| fail(&path, error.to_string()))? {
        count += 1;
        if count > MAX_ENTRIES {
            return Err(fail(
                &path,
                "attempt evidence inventory exceeds entry bound",
            ));
        }
        let entry = entry.map_err(|error| fail(&path, error.to_string()))?;
        let entry = entry.path();
        if entry.extension().and_then(|part| part.to_str()) != Some("jsonl") {
            continue;
        }
        let id = entry
            .file_stem()
            .and_then(|part| part.to_str())
            .ok_or_else(|| fail(&entry, "attempt identity must be UTF-8"))?;
        let known = queue
            .tasks
            .iter()
            .chain(other.into_iter().flat_map(|queue| &queue.tasks))
            .any(|task| task.id == id);
        if !known && !recognizes_retired_journal(&context.component_dir, &entry)? {
            return Err(fail(
                &entry,
                format!(
                    "attempt journal for task `{id}` matches no task in the queue and has no authenticated Withdraw receipt, so it blocks planning; re-introduce the task into TODOS.yaml with the status its journal ended in, or stage a Withdraw plan to retire it, then plan again"
                ),
            ));
        }
    }
    Ok(())
}

fn check_ledger(context: &TaskContext) -> Result<()> {
    let path = context.component_dir.join(".kvist/continuous.json");
    if !exists(&path)? {
        return Ok(());
    }
    let secret = existing_secret()?;
    continuous::decision_preflight(context, &secret)
}

fn snapshot(context: &TaskContext) -> Result<Snapshot> {
    let mut budget = Budget::default();
    let mut files = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    for kind in DOCUMENTS {
        let text = read_text(&context.component_dir.join(kind.filename()))?;
        budget.add(text.len(), &context.component_dir)?;
        validate_document(kind.filename(), &text)?;
        files.insert(kind.filename().to_owned(), text);
    }
    let text = read_text(&context.component_dir.join("TODOS.yaml"))?;
    budget.add(text.len(), &context.component_dir)?;
    let queue = parse(&text)
        .map_err(|error| fail(&context.component_dir.join("TODOS.yaml"), error.to_string()))?;
    files.insert("TODOS.yaml".to_owned(), text);
    for name in [
        "kvist.toml",
        "VISION.md",
        "ARCHITECTURE.md",
        "ROOT_CONTRACT.md",
    ] {
        let path = context.project_dir.join(name);
        let bytes = read_bytes(&path)?;
        budget.add(bytes.len(), &path)?;
        let status = project_state::inspect_recovery_artifact(&context.project_dir, name)?;
        if !status.is_valid() || status.is_blocked() {
            return Err(fail(
                &path,
                "project root constraints must be structurally valid",
            ));
        }
        bindings.insert(format!("root/{name}"), digest(&bytes));
    }
    let config = crate::config::load(&context.project_dir)?;
    let resolved = serde_json::json!({
        "component_root": config.component_root,
        "discovery": {
            "max_depth": config.discovery.max_depth,
            "max_directories": config.discovery.max_directories,
            "max_components": config.discovery.max_components,
            "max_entries_per_directory": config.discovery.max_entries_per_directory,
            "max_relative_path_bytes": config.discovery.max_relative_path_bytes,
        },
        "vcs": format!("{:?}", config.vcs),
        "agent": config.agent,
        "test_policy": config.test_policy,
        "sandbox": config.sandbox,
        "task_run": config.task_run,
    });
    bindings.insert(
        "config/effective".to_owned(),
        digest(encode(&resolved)?.as_bytes()),
    );
    bindings.insert(
        "identity/project".to_owned(),
        digest(context.project_dir.as_os_str().as_encoded_bytes()),
    );
    bindings.insert(
        "identity/component".to_owned(),
        digest(
            absolute(&context.component_dir)?
                .as_os_str()
                .as_encoded_bytes(),
        ),
    );
    let parent = parent(context)?;
    if queue.component.parent_contract != parent {
        return Err(fail(
            &context.component_dir,
            "immediate-parent contract authority changed; separately revalidate it before planning local decisions",
        ));
    }
    bindings.insert(
        "parent/contract".to_owned(),
        digest(encode(&parent)?.as_bytes()),
    );
    for name in [
        "src",
        "tests",
        "IMPL.md",
        ".kvist-attempts",
        ".kvist/continuous.json",
        ".kvist/continuous-reviews",
        ".kvist/authoring/proposals",
        ".kvist/attention",
        ".kvist/reviews",
        ".kvist/evidence",
        "reports",
        "COMPLIANCE.md",
        "SECURITY.md",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        ".gitignore",
    ] {
        tree(
            &context.component_dir,
            Path::new(name),
            0,
            &mut budget,
            &mut bindings,
        )?;
    }
    Ok((queue, files, bindings, budget))
}

#[derive(Default)]
struct Budget {
    bytes: usize,
    entries: usize,
}

impl Budget {
    fn add(&mut self, bytes: usize, path: &Path) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries += 1;
        if self.bytes > MAX_TOTAL || self.entries > MAX_ENTRIES {
            return Err(fail(
                path,
                "capture exceeds 8 MiB or 10000 entries; no input was silently omitted",
            ));
        }
        Ok(())
    }
}

fn tree(
    root: &Path,
    relative: &Path,
    depth: usize,
    budget: &mut Budget,
    bindings: &mut BTreeMap<String, String>,
) -> Result<()> {
    let path = root.join(relative);
    if depth > 64 || relative.as_os_str().len() > 4096 {
        return Err(fail(&path, "capture path exceeds depth/path bound"));
    }
    let key = relative
        .to_str()
        .ok_or_else(|| fail(&path, "capture paths must be UTF-8"))?
        .to_owned();
    if !exists(&path)? {
        bindings.insert(format!("local/{key}"), "absent".to_owned());
        return Ok(());
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| fail(&path, error.to_string()))?;
    if metadata.is_file() {
        let bytes = read_bytes(&path)?;
        budget.add(bytes.len(), &path)?;
        bindings.insert(format!("local/{key}"), digest(&bytes));
    } else {
        budget.add(0, &path)?;
        // A nested component owns its source; do not capture its implementation.
        let mut nested_component = false;
        for name in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "TODOS.yaml"] {
            nested_component |= exists(&path.join(name))?;
        }
        if nested_component {
            bindings.insert(format!("local/{key}"), "component-boundary".to_owned());
            return Ok(());
        }
        bindings.insert(format!("local/{key}"), "directory".to_owned());
        let mut entries = Vec::new();
        for entry in fs::read_dir(&path).map_err(|error| fail(&path, error.to_string()))? {
            let entry = entry.map_err(|error| fail(&path, error.to_string()))?;
            if entries.len() >= MAX_ENTRIES {
                return Err(fail(&path, "capture directory exceeds entry bound"));
            }
            entries.push(entry.file_name());
        }
        entries.sort();
        for name in entries {
            tree(root, &relative.join(name), depth + 1, budget, bindings)?;
        }
    }
    Ok(())
}

pub(super) fn verify(
    context: &TaskContext,
    input: &Capture,
    outputs: &BTreeMap<String, String>,
    recovery: bool,
) -> Result<()> {
    let (queue, files, mut bindings, mut budget) = snapshot(context)?;
    // Recovery keeps the original decision workspace even after its own queue
    // and documents have reached known post-state bytes.
    decision_context(context, &capture_id(input)?, &mut budget, &mut bindings)?;
    if bindings != input.bindings {
        return Err(fail(
            &context.component_dir,
            "source, evidence, decision context, root, parent or configuration changed; regenerate the plan",
        ));
    }
    if files.len() != input.files.len()
        || files.iter().any(|(name, contents)| {
            input.files.get(name) != Some(contents)
                && !(recovery && outputs.get(name) == Some(contents))
        })
    {
        return Err(fail(
            &context.component_dir,
            "controlled inputs changed outside the exact before/after transaction; preserve edits and regenerate the plan",
        ));
    }
    guards(context, &queue)?;
    guards(context, &input.queue)?;
    unknown_journals(context, &queue, Some(&input.queue))?;
    Ok(())
}
