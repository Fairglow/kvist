//! Dependency-driven execution and durable, unaccepted item checkpoints.

use super::*;
use crate::config::{CommitFrequency, TaskRunConfig};
use checkpoint::{PreparedCommit, Snapshot};

const LEDGER_VERSION: u32 = 1;
const MAX_LEDGER_BYTES: u64 = 192 * 1024 * 1024;
const LEDGER_PATH: &str = ".kvist/continuous.json";
const REVIEW_PATH: &str = ".kvist/continuous-reviews";
const SUFFIXES: [&str; 4] = [
    "write-tests",
    "implement-code",
    "security-audit",
    "compliance-review",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Running,
    Paused,
    AwaitingApproval,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u32,
    component: PathBuf,
    policy: TaskRunConfig,
    policy_identity: String,
    definition_digest: String,
    phase: Phase,
    active_item: Option<String>,
    completed_items: BTreeSet<String>,
    baseline: Snapshot,
    expected_head: Option<String>,
    expected_effects: String,
    pending: Option<PreparedCommit>,
    authentication_tag: String,
}

pub(super) fn run(
    project_dir: &Path,
    component_path: &Path,
    stream: bool,
    allow_control: bool,
) -> Result<String> {
    run_inner(project_dir, component_path, stream, allow_control)
        .map_err(|error| attention::failure(project_dir, component_path, None, error, None, None))
}

fn run_inner(
    project_dir: &Path,
    component_path: &Path,
    stream: bool,
    allow_control: bool,
) -> Result<String> {
    sav::install_handler();
    let context = validate_context(project_dir, component_path)?;
    let config = crate::config::load(project_dir)?;
    check_policy_approved(project_dir, &config)?;
    let (approval, secret) = load_authenticated_execution_approval(project_dir, &config)?;
    let at = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let lock = TaskLock::for_context(&context, "continuous-run", &at)?;
    let result = run_locked(
        &context,
        &config,
        &approval,
        &secret,
        &lock,
        stream,
        allow_control,
    );
    match (result, lock.release()) {
        (Ok(output), Ok(())) => Ok(output),
        (Ok(_), Err(error)) | (Err(error), _) => Err(error),
    }
}

fn run_locked(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
    approval: &ExecutionApproval,
    secret: &[u8],
    lock: &TaskLock,
    stream: bool,
    allow_control: bool,
) -> Result<String> {
    ensure_component_attempts_recovered(&context.component_dir)?;
    let queue = read_queue(&context.component_dir)?;
    let groups = if config.task_run.commit == CommitFrequency::Item {
        Some(item_groups(&queue.tasks)?)
    } else {
        None
    };
    let definition = definition_digest(&queue)?;
    let retained = load(context, secret)?;
    let retained = retained.filter(|ledger| {
        !(ledger.phase == Phase::AwaitingApproval
            && ledger.pending.is_none()
            && ledger.definition_digest != definition
            && queue
                .tasks
                .iter()
                .any(|task| task.status != TaskStatus::Completed))
    });
    let mut ledger = match retained {
        Some(mut ledger) => {
            if ledger.component != context.component_path || ledger.policy != config.task_run {
                return Err(error(
                    context,
                    "retained continuous run has a different component or commit policy; finish or explicitly review its retained checkpoint before changing policy",
                ));
            }
            if ledger.definition_digest != definition {
                if ledger.phase != Phase::Paused || ledger.pending.is_some() {
                    return Err(error(
                        context,
                        "task definitions changed during continuous work; review retained effects and resolve the paused item before continuing",
                    ));
                }
                ledger.definition_digest = definition;
                ledger.active_item = None;
                ledger.completed_items.retain(|item| {
                    groups
                        .as_ref()
                        .and_then(|groups| groups.get(item))
                        .is_some_and(|ids| {
                            ids.iter().all(|id| {
                                task_by_id(&queue.tasks, id)
                                    .is_some_and(|task| task.status == TaskStatus::Completed)
                            })
                        })
                });
            }
            if ledger.policy_identity != approval.approval_digest {
                if ledger.pending.is_some() {
                    return Err(error(
                        context,
                        "execution policy changed with a pending checkpoint; review and finish the retained checkpoint under its original policy",
                    ));
                }
                ledger.policy_identity = approval.approval_digest.clone();
            }
            // Orphaned checkpoint: HEAD advanced past the checkpoint's parent
            // (external commits) while it was retained, so `finish` would
            // reject because HEAD is neither the expected parent nor the
            // commit. Recover it before the effect-digest check so the run
            // continues instead of wedging. No task is replayed.
            recover_orphaned_checkpoint(context, &mut ledger, secret)?;
            if ledger.phase != Phase::Paused && ledger.expected_effects != effect_digest(context)? {
                return Err(error(
                    context,
                    "continuous-run files changed outside recorded execution; inspect retained effects before continuing; no task was replayed",
                ));
            }
            // Bounded re-baseline: when HEAD has advanced past the ledger's
            // recorded expected_head (commits were made outside this run) and
            // there is no pending checkpoint, a stale worktree baseline would
            // otherwise diff against pre-HEAD state and mint no-op changes for
            // every path that was committed in the interim. If the worktree is
            // provably clean relative to the new HEAD (no orphaned uncommitted
            // work, no staged overlap), refresh the baseline and expected_head
            // so the ledger self-heals. When the worktree is dirty the stale
            // baseline is left intact: the checkpoint's no-op tolerance then
            // carries the genuine, still-uncommitted delta to publication.
            // ensure_clean returns the HEAD only when the worktree and index
            // are clean relative to it; a dirty worktree yields Err and the
            // stale baseline is left intact for the no-op tolerance to carry.
            if ledger.pending.is_none()
                && let Some(expected) = ledger.expected_head.clone()
                && let Ok(worktree) = checkpoint::capture(context)
                && let Ok(head) = checkpoint::ensure_clean(context, &worktree)
                && head != expected
            {
                ledger.baseline = worktree;
                ledger.expected_head = Some(head);
            }
            ledger
        }
        None => {
            let baseline = checkpoint::capture(context)?;
            let head = if config.task_run.commit == CommitFrequency::Manual {
                None
            } else {
                Some(checkpoint::ensure_clean(context, &baseline)?)
            };
            Ledger {
                version: LEDGER_VERSION,
                component: context.component_path.clone(),
                policy: config.task_run.clone(),
                policy_identity: approval.approval_digest.clone(),
                definition_digest: definition,
                phase: Phase::Running,
                active_item: None,
                completed_items: groups
                    .as_ref()
                    .map(|groups| {
                        groups
                            .iter()
                            .filter(|(_, ids)| {
                                ids.iter().all(|id| {
                                    task_by_id(&queue.tasks, id)
                                        .is_some_and(|task| task.status == TaskStatus::Completed)
                                })
                            })
                            .map(|(item, _)| item.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                baseline,
                expected_head: head,
                expected_effects: effect_digest(context)?,
                pending: None,
                authentication_tag: String::new(),
            }
        }
    };
    save(context, &mut ledger, secret)?;
    if sav::take_interrupted() {
        ledger.phase = Phase::Paused;
        save(context, &mut ledger, secret)?;
        return Err(error(
            context,
            "continuous run interrupted before checkpoint recovery; retained work was not replayed",
        ));
    }
    if ledger.pending.is_some() {
        finish_checkpoint(context, &mut ledger, secret, lock)?;
    }
    loop {
        lock.revalidate()?;
        if sav::take_interrupted() {
            ledger.phase = Phase::Paused;
            save(context, &mut ledger, secret)?;
            return Err(error(
                context,
                "continuous run interrupted at a safe boundary; completed tasks were retained; rerun the same command after inspection",
            ));
        }
        let queue = read_queue(&context.component_dir)?;
        if definition_digest(&queue)? != ledger.definition_digest {
            return Err(error(
                context,
                "task definitions changed while the run was active; no later task was started",
            ));
        }
        if let Some(item) = ledger.active_item.as_ref()
            && let Some(groups) = groups.as_ref()
        {
            let ids = groups
                .get(item)
                .ok_or_else(|| error(context, "active item was removed from the queue"))?;
            if ids.iter().all(|id| {
                task_by_id(&queue.tasks, id)
                    .is_some_and(|task| task.status == TaskStatus::Completed)
            }) {
                if queue
                    .tasks
                    .iter()
                    .all(|task| task.status == TaskStatus::Completed)
                {
                    record_final_state(context, &queue, config)?;
                    ledger.expected_effects = effect_digest(context)?;
                    save(context, &mut ledger, secret)?;
                }
                checkpoint_item(context, &mut ledger, secret, lock)?;
                continue;
            }
        }
        if queue
            .tasks
            .iter()
            .all(|task| task.status == TaskStatus::Completed)
        {
            if ledger.phase != Phase::AwaitingApproval {
                record_final_state(context, &queue, config)?;
                if ledger.policy.commit == CommitFrequency::Component {
                    create_checkpoint(context, &mut ledger, secret, lock, "component")?;
                }
                ledger.phase = Phase::AwaitingApproval;
                ledger.expected_effects = effect_digest(context)?;
                save(context, &mut ledger, secret)?;
            }
            return Ok(format!(
                "Component `{}` execution is complete and awaiting human approval; no work was accepted automatically.\nReview IMPL.md, the Markdown review index at {}, and the resulting changes. `component accept` accepts intent only, not implementation.\nRun ledger: {}",
                context.component_path.display(),
                context
                    .component_dir
                    .join(REVIEW_PATH)
                    .join("component.md")
                    .display(),
                context.component_dir.join(LEDGER_PATH).display(),
            ));
        }
        let selected = queue.tasks.iter().find(|task| {
            task_is_ready(task, &queue.tasks)
                && ledger.active_item.as_ref().is_none_or(|item| {
                    groups.as_ref().is_some_and(|groups| {
                        groups.get(item).is_some_and(|ids| ids.contains(&task.id))
                    })
                })
        });
        let Some(task) = selected else {
            ledger.phase = Phase::Paused;
            save(context, &mut ledger, secret)?;
            let waiting = queue
                .tasks
                .iter()
                .filter(|task| task.status != TaskStatus::Completed)
                .map(|task| {
                    format!(
                        "{}: {}{}",
                        task.id,
                        status_name(task.status),
                        task.blocked_reason
                            .as_ref()
                            .map(|reason| format!(" ({reason})"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Err(error(
                context,
                &format!(
                    "no dependency-ready work can advance the active item; resolve the decision/blocker before continuing:\n{waiting}\nInspect `kvist task next {}` and retained attempt evidence; no task was unblocked automatically.",
                    context.component_path.display()
                ),
            ));
        };
        if let Some(groups) = groups.as_ref()
            && ledger.active_item.is_none()
        {
            let item = groups
                .iter()
                .find(|(_, ids)| ids.contains(&task.id))
                .map(|(item, _)| item.clone())
                .ok_or_else(|| error(context, "ready task has no lifecycle item"))?;
            ledger.active_item = Some(item);
        }
        ledger.phase = Phase::Running;
        ledger.expected_effects = effect_digest(context)?;
        save(context, &mut ledger, secret)?;
        eprintln!(
            "Running `{}` in `{}`...",
            task.id,
            context.component_path.display()
        );
        let outcome = run_task_under_lock(
            &context.project_dir,
            &context.component_path,
            &task.id,
            stream,
            allow_control,
            true,
            Some(lock),
        );
        ledger.expected_effects = effect_digest(context)?;
        let after = read_queue(&context.component_dir)?;
        let status = task_by_id(&after.tasks, &task.id)
            .ok_or_else(|| error(context, "executed task disappeared"))?
            .status;
        ledger.phase = if outcome.is_ok() && status == TaskStatus::Completed {
            Phase::Running
        } else {
            Phase::Paused
        };
        save(context, &mut ledger, secret)?;
        let output = outcome?;
        eprintln!("{output}");
        if status != TaskStatus::Completed {
            return Ok(format!(
                "Continuous run paused at `{}` ({}) for human attention; later tasks were not started.\n{output}",
                task.id,
                status_name(status)
            ));
        }
    }
}

fn checkpoint_item(
    context: &TaskContext,
    ledger: &mut Ledger,
    secret: &[u8],
    lock: &TaskLock,
) -> Result<()> {
    let item = ledger
        .active_item
        .clone()
        .ok_or_else(|| error(context, "no active checkpoint item"))?;
    create_checkpoint(context, ledger, secret, lock, &item)?;
    ledger.completed_items.insert(item);
    ledger.active_item = None;
    save(context, ledger, secret)
}

fn create_checkpoint(
    context: &TaskContext,
    ledger: &mut Ledger,
    secret: &[u8],
    lock: &TaskLock,
    item: &str,
) -> Result<()> {
    let after = checkpoint::capture(context)?;
    if after != ledger.baseline {
        let head = ledger
            .expected_head
            .as_deref()
            .ok_or_else(|| error(context, "automatic checkpoint has no recorded HEAD"))?;
        let message = format!(
            "checkpoint({}): complete {item} execution (unaccepted)",
            context.component_path.display()
        );
        eprintln!(
            "Preparing local unaccepted checkpoint for `{item}`; existing Git signing policy remains in effect."
        );
        ledger.pending = Some(checkpoint::prepare(
            context,
            head,
            &ledger.baseline,
            &after,
            &message,
        )?);
        save(context, ledger, secret)?;
        finish_checkpoint(context, ledger, secret, lock)?;
    }
    Ok(())
}

fn finish_checkpoint(
    context: &TaskContext,
    ledger: &mut Ledger,
    secret: &[u8],
    lock: &TaskLock,
) -> Result<()> {
    lock.revalidate()?;
    let prepared = ledger
        .pending
        .as_ref()
        .ok_or_else(|| error(context, "no prepared checkpoint"))?;
    // A prepared checkpoint whose commit is not an ancestor of HEAD was never
    // published: HEAD advanced past its parent (external commits) while the
    // checkpoint was retained. `finish` would reject because HEAD is neither
    // the expected parent nor the commit. When the worktree still matches the
    // checkpoint's after-snapshot, re-prepare it against the current HEAD so
    // the retained delta is published and the run continues. No task is
    // replayed; only the unaccepted checkpoint is re-based.
    let (head, _branch) = checkpoint::head_branch(context)?;
    if head != prepared.expected_head && head != prepared.commit_id {
        eprintln!(
            "Retained checkpoint was orphaned by external commits; re-preparing it against the current HEAD."
        );
        let reprepared = checkpoint::reprepare(
            context,
            prepared,
            &format!(
                "checkpoint({}): re-prepare orphaned {} execution (unaccepted)",
                context.component_path.display(),
                ledger.active_item.as_deref().unwrap_or("component")
            ),
        )?;
        ledger.pending = Some(reprepared);
        save(context, ledger, secret)?;
    }
    let prepared = ledger
        .pending
        .as_ref()
        .ok_or_else(|| error(context, "no prepared checkpoint"))?;
    let commit = checkpoint::finish(context, prepared)?;
    eprintln!("Local unaccepted checkpoint: {commit}");
    ledger.expected_head = Some(commit);
    ledger.baseline = checkpoint::capture(context)?;
    let head = checkpoint::ensure_clean(context, &ledger.baseline)?;
    if ledger.expected_head.as_deref() != Some(&head) {
        return Err(error(
            context,
            "HEAD changed immediately after checkpoint publication; inspect the retained checkpoint",
        ));
    }
    ledger.expected_effects = effect_digest(context)?;
    ledger.pending = None;
    save(context, ledger, secret)
}

pub(super) fn decision_preflight(context: &TaskContext, secret: &[u8]) -> Result<()> {
    if let Some(mut ledger) = load(context, secret)?
        && (ledger.pending.is_some()
            || !matches!(ledger.phase, Phase::Paused | Phase::AwaitingApproval))
    {
        // A retained checkpoint may be orphaned by external commits; recover
        // it so the decision command sees a clean, paused ledger.
        recover_orphaned_checkpoint(context, &mut ledger, secret)?;
        if ledger.pending.is_some()
            || !matches!(ledger.phase, Phase::Paused | Phase::AwaitingApproval)
        {
            return Err(error(
                context,
                "decision changes require a paused continuous run with no pending checkpoint; finish or inspect retained work first",
            ));
        }
    }
    Ok(())
}

/// Recover an orphaned pending checkpoint so the run can resume. A prepared
/// checkpoint whose commit is not an ancestor of HEAD was never published:
/// HEAD advanced past its parent (external commits) while it was retained, so
/// `finish` would reject because HEAD is neither the expected parent nor the
/// commit. When the worktree still matches the checkpoint's after-snapshot,
/// re-prepare it against the current HEAD so the retained delta is published.
/// When the worktree has moved on (later tasks ran and were not checkpointed),
/// the retained delta is superseded; discard the checkpoint, pause the run,
/// and re-baseline. Either way the ledger is re-baselined so the run
/// continues. No task is replayed.
fn recover_orphaned_checkpoint(
    context: &TaskContext,
    ledger: &mut Ledger,
    secret: &[u8],
) -> Result<()> {
    let Some(prepared) = ledger.pending.as_ref() else {
        return Ok(());
    };
    let Ok((head, branch)) = checkpoint::head_branch(context) else {
        return Ok(());
    };
    if head == prepared.expected_head || head == prepared.commit_id {
        return Ok(());
    }
    if branch != prepared.branch {
        return Err(error(
            context,
            "branch changed while the checkpoint was retained; switch back to the recorded branch before resuming",
        ));
    }
    if checkpoint::capture(context)? == prepared.after {
        eprintln!(
            "Retained checkpoint was orphaned by external commits; re-preparing it against the current HEAD."
        );
        ledger.pending = Some(checkpoint::reprepare(
            context,
            prepared,
            &format!(
                "checkpoint({}): re-prepare orphaned {} execution (unaccepted)",
                context.component_path.display(),
                ledger.active_item.as_deref().unwrap_or("component")
            ),
        )?);
        save(context, ledger, secret)?;
    } else {
        eprintln!(
            "Retained checkpoint was superseded by later work; discarding the orphaned checkpoint and re-baselining."
        );
        ledger.pending = None;
        ledger.phase = Phase::Paused;
        ledger.baseline = checkpoint::capture(context)?;
        ledger.expected_effects = effect_digest(context)?;
        save(context, ledger, secret)?;
    }
    Ok(())
}

fn item_groups(tasks: &[Task]) -> Result<BTreeMap<String, Vec<String>>> {
    let stems = tasks
        .iter()
        .filter_map(|task| item_stem(&task.id))
        .collect::<BTreeSet<_>>();
    let test_stems = tasks
        .iter()
        .filter(|task| task.kind == TaskKind::Test)
        .filter_map(|task| item_stem(&task.id))
        .collect::<BTreeSet<_>>();
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for task in tasks {
        let enclosing_test = if task.kind == TaskKind::Implementation {
            test_stems
                .iter()
                .filter(|stem| {
                    !stem.is_empty()
                        && (task.id.as_str() == stem.as_str()
                            || task.id.starts_with(&format!("{stem}-")))
                })
                .max_by_key(|stem| stem.len())
                .cloned()
        } else {
            None
        };
        let stem = enclosing_test.or_else(|| item_stem(&task.id)).or_else(|| {
            stems
                .iter()
                .filter(|stem| {
                    !stem.is_empty()
                        && (task.id.as_str() == stem.as_str()
                            || task.id.starts_with(&format!("{stem}-")))
                })
                .max_by_key(|stem| stem.len())
                .cloned()
        });
        if stem.is_none() && task.status == TaskStatus::Completed {
            continue;
        }
        let stem = stem.ok_or_else(|| KvistError::TaskRunFailed {
            task_id: task.id.clone(),
            reason: "cannot infer a complete lifecycle item; use a shared ID prefix ending in write-tests, implement-code, security-audit, compliance-review, or set task_run.commit to component/manual and reapprove policy".into(),
        })?;
        groups.entry(stem).or_default().push(task.id.clone());
    }
    for (item, ids) in &groups {
        let members = ids
            .iter()
            .filter_map(|id| task_by_id(tasks, id))
            .collect::<Vec<_>>();
        if members
            .iter()
            .all(|task| task.status == TaskStatus::Completed)
        {
            continue;
        }
        let positions = tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| ids.contains(&task.id))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if let (Some(first), Some(last)) = (positions.first(), positions.last())
            && tasks[*first..=*last]
                .iter()
                .any(|task| task.status != TaskStatus::Completed && !ids.contains(&task.id))
        {
            return Err(KvistError::TaskRunFailed { task_id: item.clone(),
                reason: "active item tasks are interleaved with another item; use component/manual commits rather than mixing item changes".into() });
        }
    }
    Ok(groups)
}

fn item_stem(id: &str) -> Option<String> {
    SUFFIXES.iter().find_map(|suffix| {
        if id == *suffix {
            Some(String::new())
        } else {
            id.strip_suffix(suffix)
                .and_then(|prefix| prefix.strip_suffix('-'))
                .map(str::to_owned)
        }
    })
}

fn definition_digest(queue: &TaskQueue) -> Result<String> {
    let definitions = queue
        .tasks
        .iter()
        .map(|task| {
            (
                &task.id,
                &task.title,
                &task.description,
                &task.context,
                &task.purpose,
                &task.expected_outcome,
                task.kind,
                &task.depends_on,
                &task.requirements,
            )
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&(
        &queue.component.requirements_revision,
        &queue.component.contract_revision,
        &queue.component.design_revision,
        &queue.component.parent_contract,
        definitions,
    ))
    .map(|bytes| digest(&bytes))
    .map_err(|error| KvistError::AgentEvidenceFailed {
        reason: format!("cannot record task definitions: {error}"),
    })
}

fn effect_digest(context: &TaskContext) -> Result<String> {
    let mut hashes = Vec::new();
    for path in ["src", "tests", "IMPL.md", REVIEW_PATH] {
        hashes.push((path, digest_path_tree(&context.component_dir.join(path))?));
    }
    serde_json::to_vec(&hashes)
        .map(|bytes| digest(&bytes))
        .map_err(|err| error(context, &format!("cannot record continuous effects: {err}")))
}

fn tag(ledger: &Ledger, secret: &[u8]) -> Result<String> {
    let mut value =
        serde_json::to_value(ledger).map_err(|error| KvistError::AgentEvidenceFailed {
            reason: error.to_string(),
        })?;
    value["authentication_tag"] = Value::String(String::new());
    let bytes = serde_json::to_vec(&value).map_err(|error| KvistError::AgentEvidenceFailed {
        reason: error.to_string(),
    })?;
    Ok(format!("hmac-sha256:{}", hmac_sha256(secret, &bytes)))
}

fn tag_struct_order(ledger: &Ledger, secret: &[u8]) -> Result<String> {
    let encoded =
        serde_json::to_string(ledger).map_err(|error| KvistError::AgentEvidenceFailed {
            reason: error.to_string(),
        })?;
    let current_field = format!(
        "\"authentication_tag\":{}",
        serde_json::to_string(&ledger.authentication_tag).map_err(|error| {
            KvistError::AgentEvidenceFailed {
                reason: error.to_string(),
            }
        })?
    );
    let cleared = match encoded.rsplit_once(&current_field) {
        Some((before, after)) => format!("{before}\"authentication_tag\":\"\"{after}"),
        None => encoded,
    };
    Ok(format!(
        "hmac-sha256:{}",
        hmac_sha256(secret, cleared.as_bytes())
    ))
}

fn load(context: &TaskContext, secret: &[u8]) -> Result<Option<Ledger>> {
    let path = context.component_dir.join(LEDGER_PATH);
    checked_directory(&context.component_dir.join(".kvist"))?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect continuous ledger",
                path,
                source,
            });
        }
    };
    let bytes = read_bounded_regular_file(&path, &metadata, MAX_LEDGER_BYTES)?;
    let ledger: Ledger = serde_json::from_slice(&bytes)
        .map_err(|err| error(context, &format!("invalid continuous ledger: {err}")))?;
    if ledger.version != LEDGER_VERSION
        || (tag(&ledger, secret)? != ledger.authentication_tag
            && tag_struct_order(&ledger, secret)? != ledger.authentication_tag)
    {
        return Err(error(
            context,
            "continuous ledger version/authentication failed; inspect retained state, do not delete recovery evidence",
        ));
    }
    Ok(Some(ledger))
}

fn save(context: &TaskContext, ledger: &mut Ledger, secret: &[u8]) -> Result<()> {
    checked_directory(&context.component_dir.join(".kvist"))?;
    ledger.authentication_tag = tag(ledger, secret)?;
    let text = serde_json::to_string_pretty(ledger)
        .map_err(|err| error(context, &format!("cannot encode continuous ledger: {err}")))?;
    if text.len() as u64 > MAX_LEDGER_BYTES {
        return Err(error(
            context,
            "continuous ledger exceeds the bounded file size; split the component",
        ));
    }
    replace_file_atomically(&context.component_dir.join(LEDGER_PATH), &text)?;
    sync_directory(&context.component_dir.join(".kvist"))
}

pub(super) fn checked_directory(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => sync_directory(
            path.parent()
                .ok_or_else(|| journal_error(path, "missing directory parent"))?,
        )?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(KvistError::Io {
                operation: "create continuous evidence directory",
                path: path.to_path_buf(),
                source,
            });
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect continuous evidence directory",
        path: path.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.is_dir() {
        return Err(journal_error(
            path,
            "continuous evidence directory must be a real, non-link directory",
        ));
    }
    Ok(())
}

fn error(context: &TaskContext, reason: &str) -> KvistError {
    journal_error(&context.component_dir.join(LEDGER_PATH), reason)
}

fn record_final_state(
    context: &TaskContext,
    queue: &TaskQueue,
    config: &crate::config::ProjectConfig,
) -> Result<()> {
    checked_directory(&context.component_dir.join(".kvist"))?;
    checked_directory(&context.component_dir.join(REVIEW_PATH))?;
    let reports = queue
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.kind,
                TaskKind::SecurityAudit | TaskKind::ComplianceReview
            )
        })
        .map(|task| format!("{}.json", task.id))
        .collect::<Vec<_>>();
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version":1,
        "state":"awaiting-approval",
        "human_acceptance":false,
        "task_definition_digest":definition_digest(queue)?,
        "queue_digest":current_queue_digest(&context.component_dir)?,
        "source_evidence_digest":digest(source_evidence(context)?.as_bytes()),
        "review_report_paths":reports,
        "notice":"Execution and checkpoint commits are not human acceptance. Inspect observed evidence and resolve any discrepancies before approval."
    })).map_err(|err| error(context, &err.to_string()))?;
    replace_file_atomically(
        &context
            .component_dir
            .join(REVIEW_PATH)
            .join("component.json"),
        &text,
    )?;
    let markdown = attention::redact(
        &attention::review_index(context, &reports)?,
        &evidence_redactions(config),
    )?;
    replace_file_atomically(
        &context.component_dir.join(REVIEW_PATH).join("component.md"),
        &markdown,
    )?;
    sync_directory(&context.component_dir.join(REVIEW_PATH))
}

pub(super) struct ReviewOptions<'a> {
    pub(super) attempt_id: &'a str,
    pub(super) stream: bool,
    pub(super) allow_control: bool,
}

pub(super) fn execute_review(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
    task: &Task,
    profile: &crate::config::AgentProfile,
    role: crate::config::Role,
    options: ReviewOptions<'_>,
) -> Result<crate::agent::AgentRunResult> {
    let mut session = crate::agent::review::ReviewSession::new(
        profile,
        role,
        &context.component_dir,
        &task.id,
        options.stream,
        options.allow_control,
        evidence_redactions(config),
    )?;
    let attempt_id = options.attempt_id;
    let outcome = (|| {
        let source = source_evidence(context)?;
        let source_digest = digest(source.as_bytes());
        let (observed, mut report) = if task.kind == TaskKind::SecurityAudit {
            (None, session.request("source-only-security", source)?)
        } else {
            let mut observed = session.request("source-only-observation", source)?;
            sanitize_report(context, config, &mut observed)?;
            checked_directory(&context.component_dir.join(".kvist"))?;
            checked_directory(&context.component_dir.join(REVIEW_PATH))?;
            let observation_path = context
                .component_dir
                .join(REVIEW_PATH)
                .join(format!("{}.{attempt_id}.observation.json", task.id));
            let observation = serde_json::to_string_pretty(&serde_json::json!({
                "schema_version":1, "source_digest":source_digest,
                "phase":"source-only-observation", "observed":observed,
                "human_acceptance":false,
            }))
            .map_err(|err| error(context, &err.to_string()))?;
            write_new_file_atomically(&observation_path, &observation)?;
            let observed_impl = format!(
                "<!-- kvist-implementation-record-version: 1 -->\n# Component Implementation Record\n\n## Observed public contract\n\n{}\n\n## Observed requirements and constraints\n\nOnly source/test evidence was supplied. This is observation, not acceptance.\n\n## Observed internal design and failure paths\n\nSource evidence digest: {source_digest}\n\nSee the independent report for observations and evidence limitations.\n",
                observed.report
            );
            replace_file_atomically(&context.component_dir.join("IMPL.md"), &observed_impl)?;
            sync_directory(&context.component_dir.join(REVIEW_PATH))?;
            sync_directory(&context.component_dir)?;
            let mut intent = BTreeMap::new();
            let mut budget = ScopeHashBudget::default();
            for name in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md"] {
                let path = context.component_dir.join(name);
                let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
                    operation: "inspect comparison intent",
                    path: path.clone(),
                    source,
                })?;
                let bytes = read_bounded_scope_file(&path, &metadata, &mut budget)?;
                intent.insert(
                    name,
                    String::from_utf8(bytes).map_err(|err| {
                        error(context, &format!("comparison intent is not UTF-8: {err}"))
                    })?,
                );
            }
            let root_path = context.project_dir.join("ROOT_CONTRACT.md");
            let root_metadata =
                fs::symlink_metadata(&root_path).map_err(|source| KvistError::Io {
                    operation: "inspect comparison global constraints",
                    path: root_path.clone(),
                    source,
                })?;
            let root = read_bounded_scope_file(&root_path, &root_metadata, &mut budget)?;
            intent.insert(
                "ROOT_CONTRACT.md",
                String::from_utf8(root).map_err(|err| error(context, &err.to_string()))?,
            );
            let queue = read_queue(&context.component_dir)?;
            if queue.component.parent_contract.is_some() {
                let (parent, _) = discovery::find_parent_component_dir(
                    &context.project_dir.join(&config.component_root),
                    &context.component_path,
                )?;
                let path = parent.join("CONTRACT.md");
                let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
                    operation: "inspect comparison parent contract",
                    path: path.clone(),
                    source,
                })?;
                let bytes = read_bounded_scope_file(&path, &metadata, &mut budget)?;
                intent.insert(
                    "PARENT_CONTRACT.md",
                    String::from_utf8(bytes).map_err(|err| error(context, &err.to_string()))?,
                );
            }
            let evidence = serde_json::to_string(&serde_json::json!({
                "intent":intent, "observed":observed_impl,
                "test_evidence": verification_evidence(context)?,
            }))
            .map_err(|err| error(context, &err.to_string()))?;
            let report = session.request("independent-comparison", evidence).map_err(|failure| {
            error(context, &format!(
                "{failure}; fresh source-only observations are retained at {} and IMPL.md",
                observation_path.display()
            ))
        })?;
            (Some(observed), report)
        };
        sanitize_report(context, config, &mut report)?;
        if digest(source_evidence(context)?.as_bytes()) != source_digest {
            return Err(error(
                context,
                "source/test evidence changed during independent review; retained reports are not current and work must be reviewed again",
            ));
        }
        checked_directory(&context.component_dir.join(".kvist"))?;
        checked_directory(&context.component_dir.join(REVIEW_PATH))?;
        let log_path = context
            .component_dir
            .join(REVIEW_PATH)
            .join(format!("{}.json", task.id));
        let encoded = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version":1, "task_id":task.id, "attempt_id":attempt_id, "source_digest":source_digest,
        "observed":observed, "comparison_or_audit":report,
        "human_acceptance":false,
    })).map_err(|err| error(context, &err.to_string()))?;
        if encoded.len() as u64 > MAX_SCOPE_FILE_BYTES {
            return Err(error(
                context,
                "independent review evidence exceeds bounded file size",
            ));
        }
        write_new_file_atomically(
            &context
                .component_dir
                .join(REVIEW_PATH)
                .join(format!("{}.{attempt_id}.json", task.id)),
            &encoded,
        )?;
        replace_file_atomically(&log_path, &encoded)?;
        let markdown = attention::redact(
            &attention::review(
                context,
                &task.id,
                attempt_id,
                &source_digest,
                observed.as_ref(),
                &report,
            ),
            &evidence_redactions(config),
        )?;
        write_new_file_atomically(
            &context
                .component_dir
                .join(REVIEW_PATH)
                .join(format!("{}.{attempt_id}.md", task.id)),
            &markdown,
        )?;
        replace_file_atomically(&log_path.with_extension("md"), &markdown)?;
        sync_directory(&context.component_dir.join(REVIEW_PATH))?;
        let redacted = redact_bounded(
            format!(
                "{}\n\n{}",
                observed
                    .as_ref()
                    .map_or("", |report| report.report.as_str()),
                report.report
            ),
            &evidence_redactions(config),
            65536,
        );
        let decision_required = report.decision_required
            || observed
                .as_ref()
                .is_some_and(|report| report.decision_required);
        Ok(crate::agent::AgentRunResult {
            success: !decision_required,
            surfaced_decision: decision_required,
            tokens_input: None,
            tokens_output: None,
            log_path: session.log_path(),
            stdout: redacted,
            stderr: String::new(),
            timed_out: false,
            output_limit_exceeded: false,
        })
    })();
    let disposition = match &outcome {
        Ok(result) if result.surfaced_decision => "awaiting-decision",
        Ok(_) => "completed",
        Err(_) => "failed",
    };
    let finished = session.finish(disposition, outcome.as_ref().err());
    match (outcome, finished) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(other)) => Err(self::error(
            context,
            &format!("{error}; retaining independent review diagnostics also failed: {other}",),
        )),
    }
}

fn sanitize_report(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
    report: &mut crate::agent::review::ReviewReport,
) -> Result<()> {
    for value in evidence_redactions(config)
        .iter()
        .filter(|value| !value.is_empty())
    {
        report.report = report.report.replace(value, "[REDACTED]");
    }
    if report.report.len() > 65_536 {
        return Err(error(
            context,
            "redacted review report exceeds its evidence bound; no report was silently truncated",
        ));
    }
    Ok(())
}

fn source_evidence(context: &TaskContext) -> Result<String> {
    let mut budget = ScopeHashBudget::default();
    let mut pending = vec![
        (context.component_dir.join("src"), 0),
        (context.component_dir.join("tests"), 0),
    ];
    let mut sources = BTreeMap::new();
    while let Some((path, depth)) = pending.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect review source",
                    path,
                    source,
                });
            }
        };
        if is_link_like(&metadata) || depth > MAX_SCOPE_DEPTH {
            return Err(journal_error(
                &path,
                "review sources must be bounded and non-link",
            ));
        }
        if metadata.is_dir() {
            for (_, child) in read_scope_entries(&path, &mut budget)? {
                pending.push((child, depth + 1));
            }
        } else {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| journal_error(&path, "review path is not UTF-8"))?;
            if [
                "REQUIREMENTS.md",
                "CONTRACT.md",
                "DESIGN.md",
                "TODOS.yaml",
                "IMPL.md",
                "ROOT_CONTRACT.md",
                "ARCHITECTURE.md",
                "VISION.md",
            ]
            .contains(&name)
            {
                continue;
            }
            let bytes = read_bounded_scope_file(&path, &metadata, &mut budget)?;
            let hash = digest(&bytes);
            let length = bytes.len();
            let content = match String::from_utf8(bytes) {
                Ok(text) => serde_json::json!({"text":text,"digest":hash}),
                Err(_) => {
                    serde_json::json!({"encoding":"binary","bytes":length,"digest":hash,"content_supplied":false})
                }
            };
            let relative = path
                .strip_prefix(&context.component_dir)
                .map_err(|err| error(context, &err.to_string()))?;
            sources.insert(relative.to_path_buf(), content);
        }
    }
    let evidence = serde_json::to_string(&serde_json::json!({
        "source_and_tests":sources, "test_evidence":verification_evidence(context)?,
    }))
    .map_err(|err| error(context, &err.to_string()))?;
    if evidence.len() as u64 > MAX_SCOPE_TOTAL_BYTES {
        return Err(error(
            context,
            "encoded review evidence exceeds 8 MiB; split the component or reduce fixtures; nothing was truncated",
        ));
    }
    Ok(evidence)
}

fn verification_evidence(context: &TaskContext) -> Result<Vec<Value>> {
    let queue = read_queue(&context.component_dir)?;
    let mut evidence = BTreeMap::<String, (usize, Value)>::new();
    let current_scopes = serde_json::json!({
        "src":digest_path_tree(&context.component_dir.join("src"))?,
        "tests":digest_path_tree(&context.component_dir.join("tests"))?,
    });
    for task in queue.tasks.iter().filter(|task| {
        task.kind == TaskKind::Implementation && task.status == TaskStatus::Completed
    }) {
        let path = existing_attempt_path(&context.component_dir, &task.id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect verification observation",
                    path,
                    source,
                });
            }
        };
        let bytes = read_bounded_regular_file(&path, &metadata, MAX_ATTEMPT_JOURNAL_BYTES)?;
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let event: Value = serde_json::from_slice(line)
                .map_err(|err| error(context, &format!("invalid verification journal: {err}")))?;
            if event["phase"] == "verification-finished" {
                let key = serde_json::to_string(&event["command"])
                    .map_err(|err| error(context, &err.to_string()))?;
                let record = evidence.entry(key).or_insert((0, Value::Null));
                record.0 += 1;
                record.1 = serde_json::json!({
                    "result":event["result"], "command":event["command"],
                    "stdout":event["stdout"], "stderr":event["stderr"],
                    "source_scopes":event["source_scopes"],
                    "matches_current_source_scopes": event.get("source_scopes").map(|scopes| scopes == &current_scopes),
                });
            }
        }
    }
    Ok(evidence.into_values().map(|(count, verification)| serde_json::json!({
        "selection":"last recorded verification per command in declared task order",
        "recorded_verifications":count,
        "verification":verification,
        "older_output":"retained in canonical attempt journals, not repeated in model context"
    })).collect())
}

#[cfg(test)]
mod tests;
