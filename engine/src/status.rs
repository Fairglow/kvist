//! Deterministic, read-only project status rendering.

use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::ValueEnum;

use crate::{
    project_state::{
        ComponentInspection, ComponentState, ProjectInspection, ProjectState, RevalidationCause,
    },
    task_queue::StalenessCauseKind,
};

/// Presentation format for the versioned status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFormat {
    /// Stable human-readable text intended for terminals and simple scripts.
    Text,
    /// Stable compact JSON intended for structured automation.
    Json,
    /// High-level human-friendly project overview with progress and next tasks.
    Overview,
}

/// Renders a completed project inspection without performing any filesystem I/O.
pub fn render(
    inspection: &ProjectInspection,
    format: StatusFormat,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
) -> String {
    match format {
        StatusFormat::Text => render_text(inspection, only_documents, only_impls, unfinished),
        StatusFormat::Json => render_json(inspection, only_documents, only_impls, unfinished),
        StatusFormat::Overview => render_overview(inspection),
    }
}

fn render_text(
    inspection: &ProjectInspection,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
) -> String {
    let mut output = format!(
        "status-format-version: 1\nproject: {}\nproject-state: {}",
        escape_text(&inspection.project_dir.to_string_lossy()),
        inspection.state.name()
    );
    match &inspection.component_root {
        Some(component_root) => {
            output.push_str("\ncomponent-root: ");
            output.push_str(&escape_text(&component_root.to_string_lossy()));
        }
        None => output.push_str("\ncomponent-root: unavailable"),
    }
    if let Some(error) = &inspection.discovery_error {
        output.push_str("\ndiscovery-error: ");
        output.push_str(&escape_text(error));
    }
    for artifact in &inspection.artifacts {
        output.push_str("\nroot-artifact: ");
        output.push_str(&escape_text(&artifact.path));
        output.push_str(": ");
        output.push_str(&escape_text(&artifact.status));
    }
    if let Some(diagnostic) = &inspection.root_diagnostic {
        output.push_str("\nroot-diagnostic: ");
        output.push_str(&escape_text(diagnostic));
    }
    if inspection.state != ProjectState::Current {
        output.push_str("\nNext Step: ");
        output.push_str(&project_action_line(inspection));
    }
    for component in &inspection.components {
        if unfinished && component.state == ComponentState::Current {
            continue;
        }
        output.push_str("\ncomponent: ");
        output.push_str(&escape_text(&component.path.to_string_lossy()));
        output.push_str(" state: ");
        output.push_str(component.state.name());
        for artifact in &component.artifacts {
            if only_documents
                && !matches!(
                    artifact.path,
                    "REQUIREMENTS.md" | "CONTRACT.md" | "DESIGN.md"
                )
            {
                continue;
            }
            if only_impls && artifact.path != "IMPL.md" {
                continue;
            }
            output.push_str("\n  ");
            output.push_str(artifact.path);
            output.push_str(": ");
            output.push_str(artifact.state.name());
        }
        if !only_impls {
            if component.revalidation_causes.is_empty() {
                output.push_str("\n  revalidation-causes: []");
            } else {
                for cause in &component.revalidation_causes {
                    output.push_str("\n  cause: ");
                    output.push_str(staleness_kind_name(cause.kind));
                    output.push(' ');
                    output.push_str(&escape_text(&cause.path));
                    output.push_str(" expected ");
                    output.push_str(&escape_text(&cause.expected_revision));
                    output.push_str(" observed ");
                    output.push_str(&escape_text(&cause.observed_revision));
                }
            }
        }
        if component.state == ComponentState::Stale {
            output.push_str(&format!(
                "\n  Next Step: Component intent documents have changed. Run 'kvist component accept {}' after review to record their revisions.",
                escape_text(&component.path.to_string_lossy())
            ));
        } else if component.state == ComponentState::Blocked {
            let component_dir = component_directory(inspection, &component.path);
            let (blocked, decisions) =
                guidance_entries_for(&component_dir, &component.path.to_string_lossy());
            output.push_str(&format!(
                "\n  Next Step: {} blocked task(s) need resolution; 'kvist help task-states' explains states and transitions.",
                blocked.len()
            ));
            if let Some(details) = render_text_entries(&blocked, &decisions) {
                output.push_str(&details);
            }
        } else if component.state == ComponentState::Invalid {
            output.push_str(
                "\n  Next Step: The component contains invalid or malformed artifacts. Run 'kvist repair' to canonicalize a parseable queue, 'kvist doctor' for artifact-level detail, then re-run 'kvist status'.",
            );
        } else if component.state == ComponentState::Missing {
            output.push_str(&format!(
                "\n  Next Step: The component is missing required adjacent Kvist artifacts. Run 'kvist component new {}' to create its intent-document templates.",
                escape_text(&component.path.to_string_lossy())
            ));
        } else if component.state == ComponentState::UnsupportedVersion {
            output.push_str(
                "\n  Next Step: An artifact uses an unsupported schema version; upgrade the kvist binary ('kvist help concepts' explains the artifact set).",
            );
        }
    }
    output
}

/// The concrete next action for a non-current project state, naming the
/// repair command when the only defect is a canonicalizable TODO queue.
fn project_action_line(inspection: &ProjectInspection) -> String {
    match inspection.state {
        ProjectState::Uninitialized => {
            "Run 'kvist init' to create the Phase 1 root artifacts.".to_owned()
        }
        ProjectState::Partial => "Create the missing root artifacts listed above, then re-run 'kvist doctor' to verify them.".to_owned(),
        ProjectState::Invalid => {
            let repairable = inspection.artifacts.iter().any(|artifact| {
                artifact.path.ends_with("TODOS.yaml")
                    && artifact
                        .status
                        .contains("must be lexically sorted and duplicate-free")
            });
            if repairable {
                "Run 'kvist repair' to canonicalize the affected TODO queues (it sorts and deduplicates their set-like lists), then re-run 'kvist status'.".to_owned()
            } else {
                "Repair the artifacts listed above; 'kvist doctor' shows the full inspection.".to_owned()
            }
        }
        ProjectState::UnsupportedVersion => {
            "Upgrade or migrate the artifacts listed above to the supported versions, then re-run 'kvist status'.".to_owned()
        }
        ProjectState::Current => String::new(),
    }
}

/// A blocked or awaiting-decision task rendered for status guidance.
struct GuidanceEntry {
    id: String,
    reason: String,
    next: String,
}

/// Resolves a component-root-relative component path against the project.
fn component_directory(inspection: &ProjectInspection, component_path: &Path) -> PathBuf {
    match &inspection.component_root {
        Some(root) => {
            if root == Path::new(".") {
                inspection.project_dir.join(component_path)
            } else {
                inspection.project_dir.join(root).join(component_path)
            }
        }
        None => inspection.project_dir.join(component_path),
    }
}

/// Reads and parses a component's queue best-effort; a missing or malformed
/// queue yields no entries rather than an error (status stays read-only).
fn guidance_entries_for(
    component_dir: &Path,
    component_path: &str,
) -> (Vec<GuidanceEntry>, Vec<GuidanceEntry>) {
    let Ok(content) = fs::read_to_string(component_dir.join("TODOS.yaml")) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(queue) = crate::task_queue::parse(&content) else {
        return (Vec::new(), Vec::new());
    };
    guidance_entries(&queue, component_path)
}

/// Collects the guidance entries for a parsed queue: each blocked task with
/// the exact command that re-runs it (dependency chain completed) or reopens
/// it (chain incomplete), then each awaiting-decision task with a pointer to
/// the task-states help topic, because resuming one is a human decision.
fn guidance_entries(
    queue: &crate::task_queue::TaskQueue,
    component_path: &str,
) -> (Vec<GuidanceEntry>, Vec<GuidanceEntry>) {
    let mut blocked = Vec::new();
    for task in queue
        .tasks
        .iter()
        .filter(|t| t.status == crate::task_queue::TaskStatus::Blocked)
    {
        let next = if crate::task_queue::dependencies_completed(task, &queue.tasks) {
            format!("kvist task run {component_path} {}", task.id)
        } else {
            let n = crate::task_queue::incomplete_dependency_count(task, &queue.tasks);
            let unit = if n == 1 { "task" } else { "tasks" };
            format!(
                "kvist task transition {component_path} {} pending ({} dependency {unit} incomplete)",
                task.id, n
            )
        };
        blocked.push(GuidanceEntry {
            id: task.id.clone(),
            reason: truncate_reason(task.blocked_reason.as_deref().unwrap_or(""), 96),
            next,
        });
    }
    let mut decisions = Vec::new();
    for task in queue
        .tasks
        .iter()
        .filter(|t| t.status == crate::task_queue::TaskStatus::AwaitingDecision)
    {
        decisions.push(GuidanceEntry {
            id: task.id.clone(),
            reason: truncate_reason(task.blocked_reason.as_deref().unwrap_or(""), 96),
            next: "human decision needed; 'kvist help task-states' explains resuming".to_owned(),
        });
    }
    (blocked, decisions)
}

/// Renders the text-report entries for blocked and decision tasks.
fn render_text_entries(blocked: &[GuidanceEntry], decisions: &[GuidanceEntry]) -> Option<String> {
    if blocked.is_empty() && decisions.is_empty() {
        return None;
    }
    let mut text = String::new();
    for entry in blocked {
        text.push_str(&format!(
            "\n  blocked: {}\n    reason: {}\n    next: {}",
            entry.id, entry.reason, entry.next
        ));
    }
    for entry in decisions {
        text.push_str(&format!(
            "\n  decision: {}\n    reason: {}\n    next: {}",
            entry.id, entry.reason, entry.next
        ));
    }
    Some(text)
}

/// Renders the overview entries for blocked and decision tasks under the box.
fn render_overview_entries(
    blocked: &[GuidanceEntry],
    decisions: &[GuidanceEntry],
) -> Option<String> {
    if blocked.is_empty() && decisions.is_empty() {
        return None;
    }
    let mut text = String::new();
    for entry in blocked {
        text.push_str(&format!("│    Blocked:   {}\n", entry.id));
        text.push_str(&format!("│               reason: {}\n", entry.reason));
        text.push_str(&format!("│               next:   {}\n", entry.next));
    }
    for entry in decisions {
        text.push_str(&format!("│    Decisions: {}\n", entry.id));
        text.push_str(&format!("│               reason: {}\n", entry.reason));
        text.push_str(&format!("│               next:   {}\n", entry.next));
    }
    Some(text)
}

/// Truncates a recorded reason to its first line, bounded to `max` characters
/// at a word boundary, marked with an ellipsis when cut.
fn truncate_reason(value: &str, max: usize) -> String {
    let first_line = value.lines().next().unwrap_or("").trim();
    if first_line.chars().count() <= max {
        return first_line.to_owned();
    }
    let cut: String = first_line.chars().take(max).collect();
    let end = cut.rfind(char::is_whitespace).unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end())
}

fn render_json(
    inspection: &ProjectInspection,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
) -> String {
    let mut output = String::from("{\"format_version\":1,\"project_path\":");
    json_string(&mut output, &inspection.project_dir.to_string_lossy());
    output.push_str(",\"project_state\":");
    json_string(&mut output, inspection.state.name());
    output.push_str(",\"component_root\":");
    match &inspection.component_root {
        Some(component_root) => json_string(&mut output, &component_root.to_string_lossy()),
        None => output.push_str("null"),
    }
    output.push_str(",\"root_artifacts\":[");
    let mut rendered_any_root_artifact = false;
    for artifact in &inspection.artifacts {
        if rendered_any_root_artifact {
            output.push(',');
        }
        output.push_str("{\"path\":");
        json_string(&mut output, &artifact.path);
        output.push_str(",\"status\":");
        json_string(&mut output, &artifact.status);
        output.push('}');
        rendered_any_root_artifact = true;
    }
    output.push_str("],\"root_diagnostic\":");
    match &inspection.root_diagnostic {
        Some(diagnostic) => json_string(&mut output, diagnostic),
        None => output.push_str("null"),
    }
    output.push_str(",\"guidance\":");
    json_string(&mut output, &inspection.guidance);
    output.push_str(",\"components\":[");
    let mut rendered_any = false;
    for component in &inspection.components {
        if unfinished && component.state == ComponentState::Current {
            continue;
        }
        if rendered_any {
            output.push(',');
        }
        append_component_json(&mut output, component, only_documents, only_impls);
        rendered_any = true;
    }
    output.push_str("],\"discovery_error\":");
    match &inspection.discovery_error {
        Some(error) => json_string(&mut output, error),
        None => output.push_str("null"),
    }
    output.push('}');
    output
}

fn append_component_json(
    output: &mut String,
    component: &ComponentInspection,
    only_documents: bool,
    only_impls: bool,
) {
    output.push_str("{\"path\":");
    json_string(output, &component.path.to_string_lossy());
    output.push_str(",\"state\":");
    json_string(output, component.state.name());
    output.push_str(",\"artifacts\":[");
    let mut rendered_any_artifact = false;
    for artifact in &component.artifacts {
        if only_documents
            && !matches!(
                artifact.path,
                "REQUIREMENTS.md" | "CONTRACT.md" | "DESIGN.md"
            )
        {
            continue;
        }
        if only_impls && artifact.path != "IMPL.md" {
            continue;
        }
        if rendered_any_artifact {
            output.push(',');
        }
        output.push_str("{\"path\":");
        json_string(output, artifact.path);
        output.push_str(",\"state\":");
        json_string(output, artifact.state.name());
        output.push('}');
        rendered_any_artifact = true;
    }
    output.push_str("],\"revalidation_causes\":[");
    if !only_impls {
        for (index, cause) in component.revalidation_causes.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            append_cause_json(output, cause);
        }
    }
    output.push_str("]}");
}

fn append_cause_json(output: &mut String, cause: &RevalidationCause) {
    output.push_str("{\"kind\":");
    json_string(output, staleness_kind_name(cause.kind));
    output.push_str(",\"path\":");
    json_string(output, &cause.path);
    output.push_str(",\"expected_revision\":");
    json_string(output, &cause.expected_revision);
    output.push_str(",\"observed_revision\":");
    json_string(output, &cause.observed_revision);
    output.push('}');
}

fn json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\0'..='\u{1f}' => {
                use std::fmt::Write;

                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            _ => output.push(character),
        }
    }
    output.push('"');
}

fn escape_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\0'..='\u{1f}' | '\u{7f}' => {
                use std::fmt::Write;

                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn staleness_kind_name(kind: StalenessCauseKind) -> &'static str {
    match kind {
        StalenessCauseKind::ComponentRequirementsRevisionChanged => {
            "component-requirements-revision-changed"
        }
        StalenessCauseKind::ComponentContractRevisionChanged => {
            "component-contract-revision-changed"
        }
        StalenessCauseKind::ComponentDesignRevisionChanged => "component-design-revision-changed",
        StalenessCauseKind::ParentContractRevisionChanged => "parent-contract-revision-changed",
    }
}

/// Renders a human-oriented overview of the project, including component states,
/// compact document summaries, task progress, and next tasks.
pub fn render_overview(inspection: &ProjectInspection) -> String {
    let mut output = String::new();
    output.push_str("╭── Project Status ───────────────────────────────────────────────\n");
    output.push_str(&format!(
        "│  Project:  {} ({})\n",
        inspection.project_dir.display(),
        inspection.state.name()
    ));

    let mut total_tasks = 0;
    let mut total_completed = 0;
    let mut total_in_progress = 0;
    let mut total_pending = 0;
    let mut total_blocked = 0;
    let mut total_awaiting = 0;

    struct CompSummary {
        path: String,
        state: &'static str,
        docs_summary: String,
        tasks_summary: Option<String>,
        next_task: Option<(String, String)>,
        stale_details: Option<String>,
        guidance: Option<String>,
        blocked_details: Option<String>,
    }

    let mut comp_summaries = Vec::new();

    for component in &inspection.components {
        let comp_path_str = component.path.to_string_lossy().into_owned();
        let state_name = component.state.name();

        let total_artifacts = component.artifacts.len();
        let valid_artifacts = component
            .artifacts
            .iter()
            .filter(|a| a.state.name() == "valid")
            .count();

        let docs_summary = if total_artifacts > 0 && valid_artifacts == total_artifacts {
            format!("all valid ({valid_artifacts}/{total_artifacts})")
        } else {
            let invalid_or_missing: Vec<String> = component
                .artifacts
                .iter()
                .filter(|a| a.state.name() != "valid")
                .map(|a| format!("{}: {}", a.path, a.state.name()))
                .collect();
            if invalid_or_missing.is_empty() {
                "none".to_owned()
            } else {
                format!(
                    "{valid_artifacts}/{total_artifacts} valid (issues: {})",
                    invalid_or_missing.join(", ")
                )
            }
        };

        let component_dir = match &inspection.component_root {
            Some(root) => {
                if root == Path::new(".") {
                    inspection.project_dir.join(&component.path)
                } else {
                    inspection.project_dir.join(root).join(&component.path)
                }
            }
            None => inspection.project_dir.join(&component.path),
        };

        let (tasks_summary, next_task, blocked_entries, decision_entries) = if let Ok(content) =
            fs::read_to_string(component_dir.join("TODOS.yaml"))
        {
            if let Ok(queue) = crate::task_queue::parse(&content) {
                let comp_total = queue.tasks.len();
                let comp_completed = queue
                    .tasks
                    .iter()
                    .filter(|t| t.status == crate::task_queue::TaskStatus::Completed)
                    .count();
                let comp_in_progress = queue
                    .tasks
                    .iter()
                    .filter(|t| t.status == crate::task_queue::TaskStatus::InProgress)
                    .count();
                let comp_pending = queue
                    .tasks
                    .iter()
                    .filter(|t| t.status == crate::task_queue::TaskStatus::Pending)
                    .count();
                let comp_blocked = queue
                    .tasks
                    .iter()
                    .filter(|t| t.status == crate::task_queue::TaskStatus::Blocked)
                    .count();
                let comp_awaiting = queue
                    .tasks
                    .iter()
                    .filter(|t| t.status == crate::task_queue::TaskStatus::AwaitingDecision)
                    .count();

                total_tasks += comp_total;
                total_completed += comp_completed;
                total_in_progress += comp_in_progress;
                total_pending += comp_pending;
                total_blocked += comp_blocked;
                total_awaiting += comp_awaiting;

                let pct = (comp_completed * 100)
                    .checked_div(comp_total)
                    .unwrap_or(100);
                let mut detail_parts = Vec::new();
                if comp_in_progress > 0 {
                    detail_parts.push(format!("{comp_in_progress} in-progress"));
                }
                if comp_pending > 0 {
                    detail_parts.push(format!("{comp_pending} pending"));
                }
                if comp_blocked > 0 {
                    detail_parts.push(format!("{comp_blocked} blocked"));
                }
                if comp_awaiting > 0 {
                    detail_parts.push(format!("{comp_awaiting} awaiting-decision"));
                }

                let summary = if detail_parts.is_empty() {
                    format!("{comp_completed}/{comp_total} completed ({pct}%)")
                } else {
                    format!(
                        "{comp_completed}/{comp_total} completed ({pct}%) [{}]",
                        detail_parts.join(", ")
                    )
                };

                let next = crate::task_queue::next_ready_task_id(&queue.tasks)
                    .and_then(|id| queue.tasks.iter().find(|t| t.id == id))
                    .map(|t| (t.id.clone(), t.title.clone()));

                let (blocked_entries, decision_entries) = guidance_entries(&queue, &comp_path_str);

                (Some(summary), next, blocked_entries, decision_entries)
            } else {
                (None, None, Vec::new(), Vec::new())
            }
        } else {
            (None, None, Vec::new(), Vec::new())
        };

        // For stale components, show exactly what changed (expected vs
        // observed revision per document) plus a concrete diff command, so
        // the user can review before re-accepting.
        let stale_details = matches!(component.state, ComponentState::Stale)
            .then(|| {
                let mut lines = Vec::new();
                for cause in &component.revalidation_causes {
                    lines.push(format!("│    Changed:   {}\n", cause.path));
                    lines.push(format!(
                        "              expected  {}\n",
                        short_revision(&cause.expected_revision)
                    ));
                    lines.push(format!(
                        "              observed  {}\n",
                        short_revision(&cause.observed_revision)
                    ));
                    let diff_path = normalize_relative_path(&{
                        let comp_rel = match &inspection.component_root {
                            Some(root) if root != Path::new(".") => root.join(&component.path),
                            _ => component.path.clone(),
                        };
                        comp_rel.join(&cause.path)
                    })
                    .to_string_lossy()
                    .into_owned();
                    lines.push(format!(
                        "              (under Git: git diff HEAD -- {})\n",
                        diff_path
                    ));
                }
                lines.join("")
            })
            .filter(|details| !details.is_empty());

        let guidance = match component.state {
            ComponentState::Stale => Some(format!(
                "Review the changed documents above, then run 'kvist component accept {comp_path_str}'"
            )),
            ComponentState::Blocked => Some(
                "Resolve the blocked tasks below; 'kvist help task-states' explains each state and its resolution."
                    .to_owned(),
            ),
            ComponentState::Invalid => Some(format!(
                "Run 'kvist repair' to canonicalize a parseable queue; 'kvist doctor' shows artifact-level detail for {comp_path_str}."
            )),
            ComponentState::Missing => Some(format!(
                "Run 'kvist component new {comp_path_str}' to create missing templates."
            )),
            ComponentState::UnsupportedVersion => Some(format!(
                "Upgrade or adapt the unsupported artifact versions in {comp_path_str}."
            )),
            ComponentState::Current => None,
        };

        let blocked_details = render_overview_entries(&blocked_entries, &decision_entries);

        comp_summaries.push(CompSummary {
            path: comp_path_str,
            state: state_name,
            docs_summary,
            tasks_summary,
            next_task,
            stale_details,
            guidance,
            blocked_details,
        });
    }

    let overall_pct = (total_completed * 100)
        .checked_div(total_tasks)
        .unwrap_or(100);
    let mut overall_details = Vec::new();
    if total_in_progress > 0 {
        overall_details.push(format!("{total_in_progress} in-progress"));
    }
    if total_pending > 0 {
        overall_details.push(format!("{total_pending} pending"));
    }
    if total_blocked > 0 {
        overall_details.push(format!("{total_blocked} blocked"));
    }
    if total_awaiting > 0 {
        overall_details.push(format!("{total_awaiting} awaiting-decision"));
    }
    let details_suffix = if overall_details.is_empty() {
        String::new()
    } else {
        format!(" [{}]", overall_details.join(", "))
    };
    output.push_str(&format!(
        "│  Progress: {} components · {}/{} tasks completed ({}%){}\n",
        inspection.components.len(),
        total_completed,
        total_tasks,
        overall_pct,
        details_suffix
    ));

    // A non-current project state is never shown bare: name the offending
    // artifacts with their reasons and the concrete unblocking action.
    if inspection.state != ProjectState::Current {
        output.push_str("│\n");
        output.push_str("│  Project issues:\n");
        if let Some(diagnostic) = &inspection.root_diagnostic {
            output.push_str(&format!("│    diagnostic: {}\n", diagnostic));
        }
        for artifact in inspection
            .artifacts
            .iter()
            .filter(|artifact| !artifact.is_valid())
        {
            output.push_str(&format!("│    {}: {}\n", artifact.path, artifact.status));
        }
        if let Some(error) = &inspection.discovery_error {
            output.push_str(&format!("│    discovery-error: {}\n", error));
        }
        output.push_str(&format!(
            "│    Action:    {}\n",
            project_action_line(inspection)
        ));
    }

    for comp in comp_summaries {
        output.push_str("│\n");
        output.push_str(&format!("│  Component: {}\n", comp.path));
        output.push_str(&format!("│    State:     {}\n", comp.state));
        output.push_str(&format!("│    Documents: {}\n", comp.docs_summary));
        if let Some(tasks) = comp.tasks_summary {
            output.push_str(&format!("│    Tasks:     {}\n", tasks));
        }
        if let Some((next_id, next_title)) = comp.next_task {
            output.push_str(&format!(
                "│    Next task: {} (\"{}\")\n",
                next_id, next_title
            ));
        }
        if let Some(details) = comp.stale_details {
            output.push_str(&details);
        }
        if let Some(hint) = comp.guidance {
            output.push_str(&format!("│    Action:    {}\n", hint));
        }
        if let Some(details) = comp.blocked_details {
            output.push_str(&details);
        }
    }

    output.push_str("╰─────────────────────────────────────────────────────────────────");
    output
}

/// Shortens a `sha256:<64 hex>` revision for overview display.
fn short_revision(revision: &str) -> String {
    match revision.strip_prefix("sha256:") {
        Some(hex) if hex.len() > 12 => format!("sha256:{}…", &hex[..12]),
        _ => revision.to_owned(),
    }
}

/// Collapses `.` and `..` segments in a relative path for display.
fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_revision_truncates_long_sha256_revisions() {
        assert_eq!(
            short_revision("sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcd"),
            "sha256:1234567890ab…"
        );
        assert_eq!(short_revision("sha256:abc"), "sha256:abc");
        assert_eq!(short_revision(""), "");
    }

    #[test]
    fn normalize_relative_path_collapses_dot_segments() {
        assert_eq!(
            normalize_relative_path(Path::new("src/ordinary/../../CONTRACT.md")),
            PathBuf::from("CONTRACT.md")
        );
        assert_eq!(
            normalize_relative_path(Path::new("src/engine/REQUIREMENTS.md")),
            PathBuf::from("src/engine/REQUIREMENTS.md")
        );
        assert_eq!(
            normalize_relative_path(Path::new("..")),
            PathBuf::from("..")
        );
    }
}
