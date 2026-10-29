//! Backup-preserving artifact recovery, independent of configuration validity.

mod ai;
mod markdown;
mod queue;
mod storage;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    KvistError, Result, artifacts, component_documents::DocumentKind, config, discovery,
    project_state, task_commands, task_queue,
};
use storage::Snapshot;

/// Explicit recovery choices. Destructive choices are proposals until approved.
#[derive(Debug, Clone)]
pub struct RepairOptions {
    /// Preview only; never create backups or modify artifacts.
    pub dry_run: bool,
    /// Plan comprehensive salvage (enabled by default).
    pub salvage: bool,
    /// Project-relative root/component artifacts to reset to drafts.
    pub reset: Vec<PathBuf>,
    /// Propose fresh drafts and archive project-owned workflow state.
    pub clean_slate: bool,
    /// SHA-256 token returned by the exact proposal to apply.
    pub approve: Option<String>,
    /// Alternate private backup storage, outside archived/reset paths.
    pub backup_dir: Option<PathBuf>,
    /// Component root for replacement configuration, not an implicit config edit.
    pub component_root: Option<PathBuf>,
    /// Native shared-registry profile for a new bounded staged AI proposal.
    pub ai: Option<String>,
    /// Explicit new proposal destination, or a frozen proposal to inspect/apply.
    pub proposal: Option<PathBuf>,
}

impl Default for RepairOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            salvage: true,
            reset: Vec::new(),
            clean_slate: false,
            approve: None,
            backup_dir: None,
            component_root: None,
            ai: None,
            proposal: None,
        }
    }
}

/// A queue's repair outcome, retained for existing report consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueRepair {
    /// Project-relative queue path.
    pub path: String,
    /// Stable outcome such as unchanged, repaired, would-repair or fenced.
    pub outcome: &'static str,
    /// Human-readable reason.
    pub detail: Option<String>,
}

/// An exact proposed mutation. Original content remains in the backup bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Operation {
    /// Project-relative path.
    pub path: String,
    /// Normalization, salvage, reset, or archive.
    pub kind: &'static str,
    /// Exact original object fingerprint, or absent.
    before: Option<Snapshot>,
    /// Replacement digest, or null for a quarantined workflow object.
    pub after_digest: Option<String>,
    /// Explanations of assumptions and removed information.
    pub reasons: Vec<String>,
    /// Whether approval is required.
    pub requires_approval: bool,
    #[serde(skip)]
    contents: Option<String>,
}

/// Recovery results and any still-unapplied proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairReport {
    /// Inspected project root.
    pub project_dir: PathBuf,
    /// Structural state before writes.
    pub state_before: project_state::ProjectState,
    /// Structural state after writes.
    pub state_after: project_state::ProjectState,
    /// Per-queue outcomes.
    pub queues: Vec<QueueRepair>,
    /// Invalid artifacts or unapplied proposals.
    pub remaining: Vec<(String, String)>,
    /// Structurally valid recovery drafts requiring explicit resolution.
    pub blockers: Vec<(String, String)>,
    /// Preview-only invocation.
    pub dry_run: bool,
    /// Exact proposed/applied operations.
    pub plan: Vec<Operation>,
    /// Content-bound token for this plan.
    pub approval: Option<String>,
    /// Retained originals and progress manifest.
    pub backup: Option<PathBuf>,
    /// Explicitly staged/loaded frozen AI proposal, never a live artifact edit.
    pub proposal: Option<PathBuf>,
}

impl RepairReport {
    /// Structural completeness, not acceptance or proof of absent effects.
    pub fn complete(&self) -> bool {
        self.remaining.is_empty()
    }
}

impl std::fmt::Display for RepairReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "repair: {} (state: {} -> {}, dry-run: {})",
            self.project_dir.display(),
            self.state_before,
            self.state_after,
            self.dry_run
        )?;
        for queue in &self.queues {
            writeln!(
                formatter,
                "  {}: {}{}",
                queue.path,
                queue.outcome,
                queue
                    .detail
                    .as_ref()
                    .map(|detail| format!(" - {detail}"))
                    .unwrap_or_default()
            )?;
        }
        for operation in &self.plan {
            writeln!(formatter, "  {}: {}", operation.path, operation.kind)?;
            for reason in &operation.reasons {
                writeln!(formatter, "    {reason}")?;
            }
            if let Some(path) = &self.proposal {
                writeln!(
                    formatter,
                    "frozen AI proposal: {} (review with --proposal PATH; applying never invokes the model)",
                    path.display()
                )?;
            }
        }
        if let Some(backup) = &self.backup {
            writeln!(
                formatter,
                "backup: {} (originals/ and manifest.json)",
                backup.display()
            )?;
        } else if let Some(approval) = &self.approval {
            writeln!(
                formatter,
                "proposal only: rerun the same options with --approve {approval}; --dry-run always prevents writes"
            )?;
        }
        if self.remaining.is_empty() {
            writeln!(
                formatter,
                "result: every root artifact is valid; discovered component artifacts are structurally valid (not accepted or certified)"
            )?;
        } else {
            writeln!(
                formatter,
                "result: {} artifact(s) still need recovery:",
                self.remaining.len()
            )?;
            for (path, status) in &self.remaining {
                writeln!(formatter, "  {path}: {status}")?;
            }
            writeln!(
                formatter,
                "hint: salvage planning is the default; review any pending proposal. For data that cannot be preserved deterministically, stage --ai PROFILE --proposal PATH; --reset PATH resets one artifact and --clean-slate replaces managed drafts and archives workflow state. Destructive changes require the exact --approve token"
            )?;
        }
        for (path, reason) in &self.blockers {
            writeln!(formatter, "  {path}: valid but blocked - {reason}")?;
        }
        Ok(())
    }
}

pub(super) fn failure(reason: impl Into<String>) -> KvistError {
    KvistError::RepairFailed {
        reason: reason.into(),
    }
}

/// Runs safe default repair, retaining a backup before each application.
pub fn repair(project_dir: &Path, dry_run: bool) -> Result<RepairReport> {
    repair_with_options(
        project_dir,
        &RepairOptions {
            dry_run,
            ..RepairOptions::default()
        },
    )
}

/// Plans or applies recovery, never treating drafts as accepted intent.
pub fn repair_with_options(project_dir: &Path, options: &RepairOptions) -> Result<RepairReport> {
    ai::validate_options(options)?;
    let root = storage::root(project_dir)?;
    let backup_storage = options
        .backup_dir
        .as_ref()
        .map(|path| root.join(path))
        .unwrap_or_else(|| root.join(".kvist-recovery"));
    storage::validate_backup_location(&root, &backup_storage)?;
    let selected_root = options.component_root.as_ref().map(|path| {
        if path == Path::new(".") { return Ok(PathBuf::from(".")); }
        let relative = storage::relative(path)?;
        if relative.components().any(|part| matches!(part, std::path::Component::Normal(name) if discovery::is_ignored_directory(name) || name == ".kvist-attempts")) {
            return Err(failure("component root cannot select VCS, build, attempt or backup storage"));
        }
        Ok(relative)
    }).transpose()?;
    if selected_root.is_some()
        && !options.clean_slate
        && !options
            .reset
            .iter()
            .any(|path| storage::relative(path).is_ok_and(|path| path == Path::new("kvist.toml")))
    {
        return Err(failure(
            "--component-root requires --reset kvist.toml or --clean-slate; no implicit configuration overwrite",
        ));
    }
    let (mut specs, ignore_input) = inventory(&root, selected_root.as_deref())?;
    let mut reset = BTreeSet::new();
    for path in &options.reset {
        let relative = storage::relative(path)?;
        let text = relative
            .to_str()
            .ok_or_else(|| failure("reset paths must be UTF-8"))?
            .to_owned();
        template(&text)?;
        if component_names().contains(
            &relative
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
        ) {
            let parent = relative
                .parent()
                .ok_or_else(|| failure("artifact has no parent"))?;
            for name in component_names() {
                specs.insert(
                    parent
                        .join(name)
                        .to_str()
                        .ok_or_else(|| failure("artifact paths must be UTF-8"))?
                        .to_owned(),
                );
            }
        }
        specs.insert(text.clone());
        reset.insert(text);
    }
    let obstructions = obstructing_parents(&root, &specs)?;
    if !options.clean_slate && !obstructions.is_empty() {
        return Err(failure(format!(
            "artifact parents are not real directories: {obstructions:?}; --clean-slate offers an approved intact quarantine and fresh directories without following links"
        )));
    }
    let mut inputs = BTreeMap::new();
    inputs.insert(".gitignore".to_owned(), ignore_input);
    let mut retained_text = 0_usize;
    for path in &specs {
        let mut snapshot = if obstructions
            .iter()
            .any(|parent| Path::new(path).starts_with(parent))
        {
            None
        } else {
            storage::snapshot(&root, Path::new(path))?
        };
        if let Some(snapshot) = &mut snapshot {
            if options.clean_slate
                || (!path.ends_with("TODOS.yaml") && !(options.salvage && path.ends_with(".md")))
            {
                snapshot.text = None;
            }
            retained_text += snapshot.text.as_ref().map(String::len).unwrap_or(0);
            if retained_text > 64 * 1024 * 1024 {
                return Err(failure(
                    "artifact text exceeds the 64 MiB aggregate recovery bound; use clean slate without parsing all artifacts, or recover a smaller project scope",
                ));
            }
        }
        inputs.insert(path.clone(), snapshot);
    }
    let before = verdicts(&root, &specs)?;
    let state_before = structural_state(&before);
    let mut queues = Vec::new();
    let mut notes = BTreeMap::new();
    let mut operations = Vec::new();
    for path in &obstructions {
        let original = storage::snapshot(&root, path)?
            .ok_or_else(|| failure("obstructing parent disappeared"))?;
        let text = path
            .to_str()
            .ok_or_else(|| failure("obstructing paths must be UTF-8"))?;
        inputs.insert(text.to_owned(), Some(original.clone()));
        operations.push(operation(text, "archive", Some(&original), None,
            vec!["quarantine this non-directory/link parent intact, without following its target; create a real directory for fresh component drafts".to_owned()], true));
    }
    let component_dirs = specs
        .iter()
        .filter(|path| path.ends_with("TODOS.yaml"))
        .filter_map(|path| Path::new(path).parent().map(Path::to_path_buf))
        .collect::<BTreeSet<_>>();
    for path in &specs {
        let original = inputs[path].as_ref();
        let is_queue = Path::new(path)
            .file_name()
            .is_some_and(|name| name == "TODOS.yaml");
        if options.clean_slate || reset.contains(path) {
            if is_queue
                && !options.clean_slate
                && has_workflow_state(
                    &root,
                    Path::new(path)
                        .parent()
                        .ok_or_else(|| failure("queue has no component"))?,
                )?
            {
                return Err(failure(format!(
                    "resetting `{path}` would orphan execution evidence; use approved --clean-slate instead; effects remain unknown"
                )));
            }
            let mut contents = template(path)?;
            if path == "kvist.toml"
                && let Some(component_root) = &selected_root
            {
                let value = component_root
                    .to_str()
                    .ok_or_else(|| failure("component root must be UTF-8"))?;
                contents = contents.replace(
                    "component_root = \".\"",
                    &format!("component_root = {}", toml::Value::String(value.to_owned())),
                );
            }
            if path.ends_with(".md") {
                contents.push_str(&format!(
                    "\n{}",
                    crate::markdown_structure::draft(
                        "this artifact was reset to an unreviewed native template"
                    )
                ));
            }
            let mut reasons = vec!["replace all existing content/object with a native draft; original retained intact; intent and queue revisions are unaccepted, not recovered meaning".to_owned()];
            if options.clean_slate {
                reasons.push("clean slate abandons prior workflow authority; existing execution effects remain unknown; inspect source before recreating or replaying tasks".to_owned());
            }
            operations.push(operation(
                path,
                "reset",
                original,
                Some(contents),
                reasons,
                true,
            ));
            continue;
        }
        if !is_queue {
            if options.salvage
                && path.ends_with(".md")
                && before.iter().any(|(invalid, _)| invalid == path)
            {
                if let Some(contents) = original.and_then(|snapshot| snapshot.text.as_deref()) {
                    match markdown::candidate(path, contents) {
                        Ok(candidate) => operations.push(operation(
                            path,
                            "salvage-markdown",
                            original,
                            Some(candidate.contents),
                            candidate.reasons,
                            true,
                        )),
                        Err(error) => {
                            notes.insert(path.clone(), error);
                        }
                    }
                } else {
                    notes.insert(
                        path.clone(),
                        "no bounded UTF-8 regular document to salvage; use --reset PATH".to_owned(),
                    );
                }
            }
            continue;
        }
        let Some(contents) = original.and_then(|snapshot| snapshot.text.as_deref()) else {
            queues.push(QueueRepair {
                path: path.clone(),
                outcome: "unparseable",
                detail: Some(
                    "missing, oversized, non-UTF-8 or non-file queue; use --reset PATH".to_owned(),
                ),
            });
            continue;
        };
        if let Ok(queue) = task_queue::parse(contents) {
            let canonical =
                task_queue::serialize(&queue).map_err(|error| failure(error.to_string()))?;
            queues.push(QueueRepair {
                path: path.clone(),
                outcome: if canonical == contents {
                    "unchanged"
                } else {
                    "non-canonical"
                },
                detail: None,
            });
            continue;
        }
        let candidate = queue::candidate(contents, false)
            .map(|candidate| (candidate, false))
            .or_else(|error| {
                if options.salvage {
                    queue::candidate(contents, true).map(|candidate| (candidate, true))
                } else {
                    Err(error)
                }
            });
        match candidate {
            Ok((candidate, salvage)) => {
                let component = root.join(
                    Path::new(path)
                        .parent()
                        .ok_or_else(|| failure("queue has no component"))?,
                );
                match task_commands::guard_repair_evidence(&component, Some(contents)) {
                    Ok(()) => {
                        operations.push(operation(
                            path,
                            if salvage { "salvage" } else { "normalize" },
                            original,
                            Some(candidate.contents),
                            candidate.reasons,
                            salvage,
                        ));
                        queues.push(QueueRepair {
                            path: path.clone(),
                            outcome: "would-repair",
                            detail: None,
                        });
                    }
                    Err(error) => queues.push(QueueRepair {
                        path: path.clone(),
                        outcome: "fenced",
                        detail: Some(error.to_string()),
                    }),
                }
            }
            Err(error) => queues.push(QueueRepair {
                path: path.clone(),
                outcome: if error.contains("fenced") {
                    "fenced"
                } else {
                    "unparseable"
                },
                detail: Some(format!(
                    "{error}; --salvage or --reset {path} offers recovery"
                )),
            }),
        }
    }
    if options.clean_slate {
        let mut state_dirs = BTreeSet::new();
        for component in component_dirs
            .iter()
            .chain(std::iter::once(&PathBuf::new()))
        {
            for name in [".kvist", ".kvist-attempts"] {
                let path = component.join(name);
                if obstructions.iter().any(|parent| path.starts_with(parent)) {
                    continue;
                }
                if specs.iter().any(|spec| Path::new(spec).starts_with(&path)) {
                    if name == ".kvist" {
                        for filename in [
                            "approved_execution_policy.json",
                            crate::toolchain::TOOLCHAIN_MANIFEST_FILENAME,
                            crate::vendoring::VENDOR_MANIFEST_FILENAME,
                        ] {
                            let metadata_path = path.join(filename);
                            if let Some(snapshot) = storage::snapshot(&root, &metadata_path)? {
                                let text = metadata_path
                                    .to_str()
                                    .ok_or_else(|| failure("metadata paths must be UTF-8"))?;
                                inputs.insert(text.to_owned(), Some(snapshot.clone()));
                                operations.push(operation(text, "archive", Some(&snapshot), None,
                                    vec!["archive runtime metadata inside a component-bearing state directory; preserve adjacent source and intent files".to_owned()], true));
                            }
                        }
                    }
                    continue;
                }
                let text = path
                    .to_str()
                    .ok_or_else(|| failure("state paths must be UTF-8"))?
                    .to_owned();
                if state_dirs
                    .iter()
                    .any(|parent: &String| Path::new(&text).starts_with(parent))
                {
                    continue;
                }
                if let Some(snapshot) = storage::snapshot(&root, &path)? {
                    inputs.insert(text.clone(), Some(snapshot.clone()));
                    operations.push(operation(&text, "archive", Some(&snapshot), None,
                        vec!["archive workflow state and prior approvals intact; execution effects remain unknown; do not blindly replay tasks".to_owned()], true));
                    state_dirs.insert(text);
                }
            }
        }
    }
    let proposal = ai::prepare(&root, options, &inputs, &mut operations)?;
    operations.sort_by(|left, right| left.path.cmp(&right.path));
    let approval = if operations.is_empty() {
        None
    } else {
        let payload = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "project": root, "backup_storage": backup_storage, "inputs": inputs, "operations": operations, "proposal": proposal,
        }))
        .map_err(|error| failure(error.to_string()))?;
        Some(storage::digest(&payload))
    };
    if options.approve.is_some() && options.approve != approval {
        return Err(failure(format!(
            "approval does not match the current recovery plan; inputs or choices changed. Nothing was written. Current token: {}",
            approval.as_deref().unwrap_or("(no pending plan)")
        )));
    }
    let requires_approval = operations
        .iter()
        .any(|operation| operation.requires_approval);
    let apply = !operations.is_empty()
        && !options.dry_run
        && (!requires_approval || options.approve == approval);
    let mut backup = None;
    if apply {
        let mut locks = Vec::new();
        let mut lock_dirs = component_dirs.clone();
        lock_dirs.insert(PathBuf::new());
        for component in &lock_dirs {
            if !obstructions
                .iter()
                .any(|parent| component.starts_with(parent))
                && root.join(component).is_dir()
            {
                locks.push(task_commands::lock_for_repair(
                    &root,
                    &root.join(component),
                )?);
            }
        }
        for (path, expected) in &inputs {
            if obstructions
                .iter()
                .any(|parent| Path::new(path) != parent && Path::new(path).starts_with(parent))
            {
                continue;
            }
            if let Some(proposal) = &proposal {
                ai::verify(proposal)?;
            }
            if storage::snapshot(&root, Path::new(path))?.as_ref() != expected.as_ref() {
                return Err(failure(format!(
                    "`{path}` changed after planning; nothing written, request a new plan"
                )));
            }
        }
        if !options.clean_slate {
            for operation in &operations {
                let component = Path::new(&operation.path)
                    .parent()
                    .ok_or_else(|| failure("artifact has no parent"))?;
                let queue_path = component.join("TODOS.yaml").to_string_lossy().into_owned();
                let contents = inputs
                    .get(&queue_path)
                    .and_then(Option::as_ref)
                    .and_then(|snapshot| snapshot.text.as_deref());
                task_commands::guard_repair_evidence(&root.join(component), contents)?;
            }
        }
        let token = approval
            .as_deref()
            .ok_or_else(|| failure("missing internal plan token"))?;
        let bundle = storage::create_backup(&root, &backup_storage, &operations, token)?;
        let mut applied = Vec::new();
        let result = (|| -> Result<()> {
            for operation in &operations {
                for lock in &locks {
                    task_commands::revalidate_repair_lock(lock)?;
                }
                storage::ensure_parents(&root, Path::new(&operation.path))?;
                storage::apply(&root, &bundle, operation)?;
                applied.push(operation.path.clone());
                storage::manifest(&bundle, token, &operations, &applied, "applying")?;
            }
            storage::manifest(&bundle, token, &operations, &applied, "complete")
        })();
        if let Err(error) = result {
            return Err(failure(format!(
                "{error}; recovery may be partial. Original objects and progress are retained at `{}`; inspect manifest.json before retrying; no task replay was attempted",
                bundle.display()
            )));
        }
        for queue in &mut queues {
            if queue.outcome == "would-repair" {
                queue.outcome = "repaired";
            }
        }
        backup = Some(bundle);
        for lock in locks {
            task_commands::release_repair_lock(lock).map_err(|error| failure(format!(
                "{error}; artifacts were applied and originals retained at `{}`; operation lock release needs inspection", backup.as_ref().map(|path| path.display().to_string()).unwrap_or_default()
            )))?;
        }
    }
    let mut remaining = verdicts(&root, &specs)?;
    for (path, status) in &mut remaining {
        if let Some(note) = notes.get(path) {
            status.push_str(&format!("; recovery: {note}"));
        }
    }
    if !apply {
        for operation in &operations {
            if !remaining.iter().any(|(path, _)| path == &operation.path) {
                remaining.push((
                    operation.path.clone(),
                    "unapplied recovery proposal".to_owned(),
                ));
            }
        }
    }
    let state_after = structural_state(&remaining);
    let blockers = recovery_blockers(&root, &specs, &remaining)?;
    Ok(RepairReport {
        project_dir: root,
        state_before,
        state_after,
        queues,
        remaining,
        blockers,
        dry_run: options.dry_run,
        plan: operations,
        approval,
        backup,
        proposal: proposal.map(|input| input.path),
    })
}

fn recovery_blockers(
    root: &Path,
    specs: &BTreeSet<String>,
    problems: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    let mut blockers = Vec::new();
    for path in specs {
        if problems.iter().any(|(problem, _)| problem == path) {
            continue;
        }
        let Some(text) =
            storage::snapshot(root, Path::new(path))?.and_then(|snapshot| snapshot.text)
        else {
            continue;
        };
        if path.ends_with(".md") {
            blockers.extend(
                crate::markdown_structure::blockers(&text)
                    .into_iter()
                    .map(|reason| (path.clone(), reason)),
            );
        } else if path.ends_with("TODOS.yaml")
            && let Ok(queue) = task_queue::parse(&text)
        {
            for task in queue.tasks {
                if task.status == task_queue::TaskStatus::Blocked
                    && let Some(reason) = task
                        .blocked_reason
                        .filter(|reason| reason.starts_with("Recovery:"))
                {
                    blockers.push((path.clone(), format!("task `{}`: {reason}", task.id)));
                }
            }
        }
    }
    Ok(blockers)
}

fn operation(
    path: &str,
    kind: &'static str,
    original: Option<&Snapshot>,
    contents: Option<String>,
    reasons: Vec<String>,
    requires_approval: bool,
) -> Operation {
    Operation {
        path: path.to_owned(),
        kind,
        before: original.cloned(),
        after_digest: contents
            .as_ref()
            .map(|contents| storage::digest(contents.as_bytes())),
        contents,
        reasons,
        requires_approval,
    }
}

fn structural_state(problems: &[(String, String)]) -> project_state::ProjectState {
    if problems.is_empty() {
        project_state::ProjectState::Current
    } else {
        project_state::ProjectState::Invalid
    }
}

fn verdicts(root: &Path, specs: &BTreeSet<String>) -> Result<Vec<(String, String)>> {
    let mut problems = Vec::new();
    for path in specs {
        if let Err(error) = storage::snapshot(root, Path::new(path)) {
            problems.push((path.clone(), error.to_string()));
            continue;
        }
        let verdict = project_state::inspect_recovery_artifact(root, path)?;
        if !verdict.is_valid() {
            problems.push((path.clone(), verdict.status));
        }
    }
    match project_state::inspect(root) {
        Ok(inspection) => {
            if let Some(reason) = inspection.root_diagnostic {
                problems.push(("project".to_owned(), reason));
            }
            if let Some(reason) = inspection.discovery_error {
                problems.push(("discovery".to_owned(), reason));
            }
        }
        Err(error) => problems.push(("inspection".to_owned(), error.to_string())),
    }
    Ok(problems)
}

fn obstructing_parents(root: &Path, specs: &BTreeSet<String>) -> Result<BTreeSet<PathBuf>> {
    let mut obstructions = BTreeSet::new();
    for path in specs {
        let mut current = PathBuf::new();
        if let Some(parent) = Path::new(path).parent() {
            for part in parent.components() {
                current.push(part);
                match fs::symlink_metadata(root.join(&current)) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                    Ok(_) => {
                        obstructions.insert(current);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                    Err(error) => return Err(failure(format!("inspect artifact parent: {error}"))),
                }
            }
        }
    }
    Ok(obstructions)
}

fn has_workflow_state(root: &Path, component: &Path) -> Result<bool> {
    for name in [".kvist-attempts", ".kvist"] {
        match fs::symlink_metadata(root.join(component).join(name)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(format!("inspect workflow state: {error}"))),
        }
    }
    Ok(false)
}

fn inventory(
    root: &Path,
    selected_root: Option<&Path>,
) -> Result<(BTreeSet<String>, Option<Snapshot>)> {
    let ignore_input = storage::snapshot(root, Path::new(".gitignore"))?;
    let ignore_rules = match &ignore_input {
        Some(snapshot) => snapshot.text.as_deref()
            .ok_or_else(|| failure("project .gitignore must be a bounded UTF-8 regular file to establish safe recovery scope"))?
            .lines().filter_map(discovery::GitIgnoreRule::parse).collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let mut specs = BTreeSet::from(
        [
            "kvist.toml",
            "VISION.md",
            "ARCHITECTURE.md",
            "ROOT_CONTRACT.md",
        ]
        .map(str::to_owned),
    );
    let mut pending = vec![(PathBuf::new(), 0)];
    let mut directories = 0;
    let mut components = BTreeSet::new();
    let configured = config::load(root)
        .ok()
        .map(|configuration| configuration.component_root);
    for component in configured.as_deref().into_iter().chain(selected_root) {
        if component == Path::new(".") {
            continue;
        }
        components.insert(component.to_owned());
        let probe = BTreeSet::from([component.join("TODOS.yaml").to_string_lossy().into_owned()]);
        if obstructing_parents(root, &probe)?.is_empty()
            && fs::symlink_metadata(root.join(component))
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            pending.push((component.to_owned(), 0));
        }
    }
    let mut visited = BTreeSet::new();
    while let Some((relative, depth)) = pending.pop() {
        if !visited.insert(relative.clone()) {
            continue;
        }
        directories += 1;
        if directories > 10000 || depth > 64 {
            return Err(failure(
                "recovery inventory exceeds 10000 directories or 64 levels; use --reset with an explicitly scoped project or adjust the layout; nothing was written",
            ));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(root.join(&relative))
            .map_err(|error| failure(format!("inspect `{}`: {error}", relative.display())))?
        {
            entries.push(entry.map_err(|error| failure(error.to_string()))?);
            if entries.len() > 10000 {
                return Err(failure("recovery directory exceeds 10000 entries"));
            }
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            if component_names().iter().any(|filename| name == *filename) {
                components.insert(relative.clone());
            }
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|error| failure(error.to_string()))?;
            let state_component = name == ".kvist"
                && metadata.is_dir()
                && component_names()
                    .iter()
                    .any(|name| fs::symlink_metadata(entry.path().join(name)).is_ok());
            if metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && !discovery::is_ignored_directory(&name)
                && name != "vendor"
                && name != ".kvist-attempts"
                && !discovery::ignored_by_rules(&ignore_rules, &relative.join(&name), true)
                && (name != ".kvist" || state_component)
                && !component_names().iter().any(|filename| name == *filename)
            {
                pending.push((relative.join(name), depth + 1));
            }
        }
    }
    // Ordinary inspection supplies additional known-layout paths, including
    // namespace components. Recovery's own scan still runs when it cannot.
    if let Ok(inspection) = project_state::inspect(root) {
        for artifact in inspection.artifacts {
            specs.insert(artifact.path);
        }
        if let Some(component_root) = inspection.component_root {
            for component in inspection.components {
                components.insert(component_root.join(component.path));
            }
        }
    }
    if components.is_empty() {
        components.insert(PathBuf::from("src"));
    }
    for component in components {
        for name in component_names() {
            let path = component.join(name);
            specs.insert(
                path.to_str()
                    .ok_or_else(|| failure("component artifact paths must be UTF-8"))?
                    .to_owned(),
            );
        }
    }
    let specs = specs
        .into_iter()
        .map(|path| {
            let relative = storage::relative(Path::new(&path))?;
            relative
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| failure("artifact paths must be UTF-8"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    Ok((specs, ignore_input))
}

fn component_names() -> [&'static str; 5] {
    [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ]
}

fn template(path: &str) -> Result<String> {
    let relative = storage::relative(Path::new(path))?;
    if relative.components().any(|part| matches!(part, std::path::Component::Normal(name) if discovery::is_ignored_directory(name) || name == ".kvist-attempts")) {
        return Err(failure("reset cannot target VCS, build, backup or attempt-history paths"));
    }
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| failure("missing artifact filename"))?;
    if relative.components().count() == 1
        && let Some(artifact) = artifacts::root_artifacts()
            .iter()
            .find(|artifact| artifact.relative_path == name)
    {
        let contents = if name == "kvist.toml" {
            artifact
                .contents
                .replace("component_root = \"src\"", "component_root = \".\"")
        } else {
            artifact.contents.to_owned()
        };
        return Ok(contents);
    }
    if name == "TODOS.yaml" {
        return queue::empty_queue().map_err(failure);
    }
    if name == "IMPL.md" {
        return artifacts::root_artifacts()
            .iter()
            .find(|artifact| artifact.relative_path.ends_with("/IMPL.md"))
            .map(|artifact| artifact.contents.to_owned())
            .ok_or_else(|| failure("missing native implementation-record template"));
    }
    for kind in [
        DocumentKind::Requirements,
        DocumentKind::Contract,
        DocumentKind::Design,
    ] {
        if name == kind.filename() {
            return Ok(kind.template().to_owned());
        }
    }
    Err(failure(format!(
        "`{path}` is not a managed root/component artifact; source files are never reset"
    )))
}
