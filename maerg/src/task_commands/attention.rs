//! Human-facing presentation only; never task disposition or approval authority.

use super::*;

const DIRECTORY: &str = ".kvist/attention";
const MAX_BRIEF_BYTES: usize = 256 * 1024;

pub(super) fn resolution(component: &Path, task: Option<&str>, decision: bool) -> String {
    let component = component.to_string_lossy();
    let component = if component.starts_with('-') {
        format!("./{component}")
    } else {
        component.into_owned()
    };
    let component = argument(&component);
    let resume = task.map_or_else(
        || format!("kvist task run {component}"),
        |task| format!("kvist task run {component} {}", argument(task)),
    );
    let unblock = task.filter(|_| !decision).map_or_else(String::new, |task| {
        format!(
            "\nFor a task durably marked blocked, after resolving its cause:\n\n    kvist task transition {component} {} pending\n\n",
            argument(task)
        )
    });
    format!(
        "## Choose or approve\n\n\
         Record the selected approach and rationale in the affected component's \
         DESIGN.md and any changed REQUIREMENTS.md, CONTRACT.md or TODOS.yaml. \
         Review the changes, including required follow-up tasks. If intent changed, \
         review and accept it with:\n\n    kvist component accept {component}\n\n\
         This accepts intent only, not implementation or a reviewer's claims. \
         A recommendation is not human acceptance.\n\n\
         ## Ask for clarification\n\n\
         Keep work paused. Ask your development assistant: \
         \"Explain the cited evidence, separate confirmed facts from assumptions, \
         and compare the consequences of each alternative before changing anything.\" \
         Kvist has no dedicated decision-response command yet; a chat answer alone \
         does not update approved intent.\n\n\
         ## Reject or redirect\n\n\
         Keep work paused and record why the proposal is rejected and the replacement \
         instructions in the affected intent/task definition. Ask the assistant to \
         propose a revised plan, not to mark the task completed or weaken obligations \
         merely to close a finding.\n\n\
         ## Resume after resolution\n\n\
         Run these commands from the project root only after reviewing retained \
         effects and resolving the issue. Unresolved attempts/fences still require \
         explicit recovery/disposition; do not delete evidence or replay unknown effects.\
         {unblock}\n\
         Explicitly retry the paused task (bare continuous run does not select \
         awaiting-decision tasks), or retry a run/checkpoint without rerunning completed tasks:\n\n\
             {resume}\n\n\
         Changed execution policy requires `kvist task approve-policy` first. \
         This brief changes no task state, approval or recovery fence.\n"
    )
}

fn argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn decision(
    context: &TaskContext,
    task: &str,
    attempt: &str,
    explanation: &str,
    evidence: &Path,
    redactions: &[String],
) -> Result<PathBuf> {
    persist(
        context,
        Some(task),
        &format!(
            "# Decision required\n\n\
             Task: `{task}`. Attempt: `{attempt}`. Snapshot: awaiting-decision.\n\n\
             This is advisory evidence, not human acceptance. It describes the \
             retained attempt, not a guarantee of current source or task state.\n\n\
             ## Agent explanation (advisory)\n\n\
             The retained explanation may be output-limited; consult the raw \
             Markdown proposal/review companions for full evidence.\n\n{explanation}\n\n\
             ## Evidence\n\nRaw proposal/review evidence: {}\n\n{}",
            evidence.display(),
            resolution(&context.component_path, Some(task), true)
        ),
        redactions,
    )
}

pub(super) fn failure(
    project_dir: &Path,
    component_path: &Path,
    task: Option<&str>,
    error: KvistError,
    explanation: Option<&str>,
) -> KvistError {
    // No queue/ledger mutation or model call: even preflight failures can be explained.
    let (component, config) = match context::normalize_component_path(component_path).and_then(
        |component| {
            let config = crate::config::load(project_dir)?;
            Ok((component, config))
        },
    ) {
        Ok(resolved) => resolved,
        Err(recording) => {
            return KvistError::TaskRunFailed {
                task_id: task.unwrap_or("continuous-run").to_owned(),
                reason: format!(
                    "{error}\nMarkdown brief unavailable: safe component/configuration resolution failed: {recording}"
                ),
            };
        }
    };
    let context = TaskContext {
        project_dir: project_dir.to_path_buf(),
        component_dir: project_dir.join(&config.component_root).join(&component),
        component_path: component,
    };
    let redactions = evidence_redactions(&config);
    let mut cause = error.to_string();
    for value in redactions.iter().filter(|value| !value.is_empty()) {
        cause = cause.replace(value, "[REDACTED]");
    }
    if cause.len() > MAX_BRIEF_BYTES / 2 {
        truncate_utf8(&mut cause, MAX_BRIEF_BYTES / 2);
        cause.push_str(
            "\n\n[Diagnostic excerpt truncated; consult retained logs and attempt evidence.]",
        );
    }
    let agent = explanation.filter(|text| !text.trim().is_empty()).map_or_else(
        || "No usable agent explanation is available for this stop.".to_owned(),
        |text| format!("## Agent explanation (advisory)\n\n{text}\n\nThis text predates the stop; it is not independently verified."),
    );
    let queued = match require_real_directory_chain(&context.project_dir, &context.component_dir)
        .and_then(|()| read_queue(&context.component_dir))
    {
        Ok(queue) => queue
            .tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Blocked | TaskStatus::AwaitingDecision
                )
            })
            .map(|task| {
                format!(
                    "- `{}` ({}): {}\n",
                    task.id,
                    status_name(task.status),
                    task.blocked_reason
                        .as_deref()
                        .unwrap_or("No reason recorded.")
                )
            })
            .collect::<String>(),
        Err(recording) => format!("Current queue inspection is unavailable: {recording}"),
    };
    let text = format!(
        "# Work stopped\n\n\
         Scope: {}. This is a historical stop diagnostic; inspect current task state \
         before acting. No completion or approval is implied.\n\n\
         ## Situation - Host diagnostic\n\n{cause}\n\n\
         {agent}\n\n\
         ## Queued decisions and blockers\n\n{queued}\n\n\
         ## Design implications\n\n\
         No new design conclusion is inferred from this error. Existing requirements \
         and approved limits remain binding.\n\n\
         ## Alternatives\n\n\
         Resolve the observed environment/verification failure, or request a reviewed \
         change to intent or policy if the current plan cannot proceed. \
         Leave work paused if the evidence is insufficient.\n\n\
         ## Recommendation and rationale\n\n\
         Inspect the original cause and retained logs/attempts before retrying. \
         Lifecycle instructions and diagnostics are host-written; any available \
         agent explanation is labelled above. No extra inference or automatic replay was attempted.\n\n{}",
        task.unwrap_or("continuous component run"),
        resolution(&context.component_path, task, false)
    );
    let reason = match persist(&context, task, &text, &redactions) {
        Ok(path) => format!(
            "{cause}\n\n## Human attention\n\nHuman-readable summary: {}\nOpen this Markdown file for the explanation and resolution steps.",
            path.display()
        ),
        Err(recording) => format!(
            "{cause}\nMarkdown brief persistence also failed: {recording}. \
             Retained effects and recovery evidence were not cleared."
        ),
    };
    KvistError::TaskRunFailed {
        task_id: task.unwrap_or("continuous-run").to_owned(),
        reason,
    }
}

fn persist(
    context: &TaskContext,
    task: Option<&str>,
    text: &str,
    redactions: &[String],
) -> Result<PathBuf> {
    let text = redact(text, redactions)?;
    // Check every ancestor, not just the leaf, before creating any evidence.
    require_real_directory_chain(&context.project_dir, &context.component_dir)?;
    continuous::checked_directory(&context.component_dir.join(".kvist"))?;
    let directory = context
        .component_dir
        .join(DIRECTORY)
        .components()
        .collect::<PathBuf>();
    continuous::checked_directory(&directory)?;
    let task = task.unwrap_or("run");
    let stem = if !task.is_empty()
        && task.len() <= 128
        && task
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        task.to_owned()
    } else {
        format!(
            "task-{}",
            digest(task.as_bytes()).trim_start_matches("sha256:")
        )
    };
    let hash = digest(text.as_bytes());
    let history = directory.join(format!("{stem}.{}.md", hash.trim_start_matches("sha256:")));
    match fs::symlink_metadata(&history) {
        Ok(metadata) => {
            let existing = read_bounded_regular_file(&history, &metadata, MAX_BRIEF_BYTES as u64)?;
            if existing != text.as_bytes() {
                return Err(journal_error(
                    &history,
                    "historical attention brief differs from its content identity",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            write_new_file_atomically(&history, &text)?;
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect historical attention brief",
                path: history,
                source,
            });
        }
    }
    let latest = directory.join(format!("{stem}.md"));
    match fs::symlink_metadata(&latest) {
        Ok(metadata) => {
            if is_link_like(&metadata) || !metadata.is_file() {
                return Err(journal_error(
                    &latest,
                    "attention alias must be a regular non-link file",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect attention alias",
                path: latest,
                source,
            });
        }
    }
    replace_file_atomically(&latest, &text)?;
    Ok(latest)
}

pub(super) fn redact(text: &str, redactions: &[String]) -> Result<String> {
    let mut text = text.to_owned();
    for value in redactions.iter().filter(|value| !value.is_empty()) {
        text = text.replace(value, "[REDACTED]");
    }
    if text.len() > MAX_BRIEF_BYTES {
        return Err(KvistError::AgentEvidenceFailed {
            reason: "Markdown attention brief exceeds 256 KiB; no brief was silently truncated"
                .into(),
        });
    }
    Ok(text)
}

pub(super) fn review(
    context: &TaskContext,
    task: &str,
    attempt: &str,
    source_digest: &str,
    observed: Option<&crate::agent::review::ReviewReport>,
    report: &crate::agent::review::ReviewReport,
) -> String {
    format!(
        "# Independent review report\n\nTask: `{task}`. Attempt: `{attempt}`.\n\
         Source evidence digest: `{source_digest}`.\n\n\
         This is agent-written advisory evidence, not human acceptance or \
         verified facts. Inspect current source/evidence freshness before acting.\n\n\
         ## Source-only observation\n\n{}\n\n\
         ## Comparison or audit\n\n{}\n\n\
         Raw machine evidence: {task}.{attempt}.json\n\n{}",
        observed.map_or(
            "Not a separate observation phase for this audit.",
            |report| report.report.as_str()
        ),
        report.report,
        resolution(&context.component_path, Some(task), true),
    )
}

pub(super) fn review_index(context: &TaskContext, reports: &[String]) -> Result<String> {
    let mut links = String::new();
    for path in reports {
        let markdown = Path::new(path).with_extension("md").display().to_string();
        let full = context
            .component_dir
            .join(".kvist/continuous-reviews")
            .join(&markdown);
        match fs::symlink_metadata(&full) {
            Ok(metadata) if metadata.is_file() && !is_link_like(&metadata) => {
                links.push_str(&format!("- [{markdown}]({markdown})\n"));
            }
            Ok(_) => {
                return Err(journal_error(
                    &full,
                    "review index requires regular non-link reports",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                links.push_str(&format!("- `{path}`: no Markdown companion is available for this historical task; inspect retained evidence.\n"));
            }
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect Markdown review index entry",
                    path: full,
                    source,
                });
            }
        }
    }
    Ok(format!(
        "# Component execution awaiting approval\n\n\
         Component: `{}`. This is a snapshot, not human acceptance.\n\n\
         All queued execution is complete. Review the changes, IMPL.md and the \
         independent agent reports before deciding whether further work is required.\n\n\
         ## Independent reports\n\n{links}\n\n\
         ## Human review\n\n\
         Resolve discrepancies and evidence gaps explicitly. If clarification or \
         changes are needed, record the follow-up in component intent/TODOS before \
         scheduling more work. `component accept` accepts intent only; there is no \
         component implementation-approval command yet. Checkpoint commits do not \
         certify completion, compliance or acceptance.\n\n\
         Raw state evidence: [component.json](component.json).\n",
        context.component_path.display()
    ))
}

fn require_real_directory_chain(root: &Path, target: &Path) -> Result<()> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| journal_error(target, "attention target must remain inside the project"))?;
    let mut current = root.to_path_buf();
    for part in std::iter::once(None).chain(relative.components().map(Some)) {
        match part {
            None | Some(Component::CurDir) => {}
            Some(Component::Normal(name)) => current.push(name),
            Some(_) => return Err(journal_error(target, "invalid attention directory path")),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|source| KvistError::Io {
            operation: "inspect attention directory ancestor",
            path: current.clone(),
            source,
        })?;
        if is_link_like(&metadata) || !metadata.is_dir() {
            return Err(journal_error(
                &current,
                "attention ancestor must be a real directory",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, TaskContext) {
        let directory = tempfile::tempdir().unwrap();
        let context = TaskContext {
            project_dir: directory.path().to_path_buf(),
            component_dir: directory.path().join("child"),
            component_path: PathBuf::from("child"),
        };
        fs::create_dir(&context.component_dir).unwrap();
        (directory, context)
    }

    #[test]
    fn briefs_are_redacted_bounded_and_keep_content_addressed_history() {
        let (_directory, context) = fixture();
        let redactions = vec!["private-value".into()];
        let first = persist(
            &context,
            Some("task"),
            "# First\nprivate-value",
            &redactions,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&first).unwrap(), "# First\n[REDACTED]");
        persist(
            &context,
            Some("task"),
            "# First\nprivate-value",
            &redactions,
        )
        .unwrap();
        assert_eq!(fs::read_dir(first.parent().unwrap()).unwrap().count(), 2);
        persist(&context, Some("task"), "# Second", &redactions).unwrap();
        assert_eq!(fs::read_dir(first.parent().unwrap()).unwrap().count(), 3);
        assert_eq!(fs::read_to_string(&first).unwrap(), "# Second");
        assert!(
            persist(
                &context,
                Some("task"),
                &"x".repeat(MAX_BRIEF_BYTES + 1),
                &[]
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&first).unwrap(), "# Second");
    }

    #[test]
    fn unsafe_task_names_cannot_escape_and_commands_quote_component_names() {
        let (_directory, mut context) = fixture();
        let path = persist(&context, Some("../escape"), "# Stop", &[]).unwrap();
        assert_eq!(
            path.parent().unwrap(),
            context.component_dir.join(DIRECTORY)
        );
        context.component_path = PathBuf::from("-child's name");
        let instructions = resolution(&context.component_path, Some("task"), true);
        assert!(instructions.contains("kvist task run './-child'\\''s name' 'task'"));
        assert!(!instructions.contains("task transition"));
    }

    #[test]
    #[cfg(unix)]
    fn linked_evidence_directory_alias_and_ancestor_are_refused() {
        use std::os::unix::fs::symlink;
        let (directory, context) = fixture();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(context.component_dir.join(".kvist")).unwrap();
        symlink(&outside, context.component_dir.join(DIRECTORY)).unwrap();
        assert!(persist(&context, Some("task"), "# Stop", &[]).is_err());
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        fs::remove_file(context.component_dir.join(DIRECTORY)).unwrap();
        fs::create_dir(context.component_dir.join(DIRECTORY)).unwrap();
        let target = outside.join("original.md");
        fs::write(&target, "preserve").unwrap();
        symlink(
            &target,
            context.component_dir.join(DIRECTORY).join("task.md"),
        )
        .unwrap();
        assert!(persist(&context, Some("task"), "# Stop", &[]).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "preserve");
        let linked = TaskContext {
            project_dir: directory.path().to_path_buf(),
            component_dir: directory.path().join("alias"),
            component_path: PathBuf::from("alias"),
        };
        symlink(&outside, &linked.component_dir).unwrap();
        assert!(persist(&linked, Some("task"), "# Stop", &[]).is_err());
        assert!(!outside.join(".kvist").exists());
    }
}
