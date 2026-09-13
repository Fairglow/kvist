//! Deterministic, read-only project status rendering.

use std::{fs, path::Path};

use clap::ValueEnum;

use crate::{
    project_state::{ComponentInspection, ComponentState, ProjectInspection, RevalidationCause},
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
            output.push_str(&format!(
                "\n  Next Step: Component tasks are blocked or need manual intervention. Resolve the blocked tasks or manual gates in {}/TODOS.yaml.",
                escape_text(&component.path.to_string_lossy())
            ));
        } else if component.state == ComponentState::Invalid {
            output.push_str(&format!(
                "\n  Next Step: The component contains invalid or malformed artifacts. Run 'kvist component validate {}' and check the reported Markdown or YAML document.",
                escape_text(&component.path.to_string_lossy())
            ));
        } else if component.state == ComponentState::Missing {
            output.push_str(&format!(
                "\n  Next Step: The component is missing required adjacent Kvist artifacts. Run 'kvist component new {}' to create its intent-document templates.",
                escape_text(&component.path.to_string_lossy())
            ));
        }
    }
    output
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

    struct CompSummary {
        path: String,
        state: &'static str,
        docs_summary: String,
        tasks_summary: Option<String>,
        next_task: Option<(String, String)>,
        guidance: Option<String>,
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

        let (tasks_summary, next_task) = if let Ok(content) =
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

                total_tasks += comp_total;
                total_completed += comp_completed;
                total_in_progress += comp_in_progress;
                total_pending += comp_pending;
                total_blocked += comp_blocked;

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

                let summary = if detail_parts.is_empty() {
                    format!("{comp_completed}/{comp_total} completed ({pct}%)")
                } else {
                    format!(
                        "{comp_completed}/{comp_total} completed ({pct}%) [{}]",
                        detail_parts.join(", ")
                    )
                };

                let next = queue
                    .tasks
                    .iter()
                    .find(|t| {
                        t.status == crate::task_queue::TaskStatus::Pending
                            && t.depends_on.iter().all(|dep| {
                                queue.tasks.iter().any(|other| {
                                    &other.id == dep
                                        && other.status == crate::task_queue::TaskStatus::Completed
                                })
                            })
                    })
                    .map(|t| (t.id.clone(), t.title.clone()));

                (Some(summary), next)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let guidance = match component.state {
            ComponentState::Stale => Some(format!(
                "Run 'kvist component accept {comp_path_str}' after review."
            )),
            ComponentState::Blocked => Some(format!(
                "Resolve blocked tasks in {comp_path_str}/TODOS.yaml."
            )),
            ComponentState::Invalid => Some(format!(
                "Run 'kvist component validate {comp_path_str}' to inspect invalid artifacts."
            )),
            ComponentState::Missing => Some(format!(
                "Run 'kvist component new {comp_path_str}' to create missing templates."
            )),
            ComponentState::UnsupportedVersion => Some(format!(
                "Upgrade or adapt the unsupported artifact versions in {comp_path_str}."
            )),
            ComponentState::Current => None,
        };

        comp_summaries.push(CompSummary {
            path: comp_path_str,
            state: state_name,
            docs_summary,
            tasks_summary,
            next_task,
            guidance,
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
        if let Some(hint) = comp.guidance {
            output.push_str(&format!("│    Action:    {}\n", hint));
        }
    }

    output.push_str("╰─────────────────────────────────────────────────────────────────");
    output
}
