//! Markdown status projection; native status formats remain in `status`.

use std::fmt::Write;
use std::path::Path;

use crate::presentation::{fence, literal};
use crate::project_state::{
    ComponentArtifactState, ComponentInspection, ComponentState, ProjectInspection, ProjectState,
};
use crate::task_queue::{TaskQueue, TaskStatus};

/// Presents inspection and bounded queue facts as Markdown.
pub fn render(
    inspection: &ProjectInspection,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
) -> String {
    render_details(inspection, only_documents, only_impls, unfinished, true)
}

pub(super) fn render_component(
    inspection: &ProjectInspection,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
) -> String {
    render_details(inspection, only_documents, only_impls, unfinished, false)
}

fn render_context(
    inspection: &ProjectInspection,
    project_details: bool,
    vcs_details: bool,
) -> String {
    let mut output = format!(
        "# Project Status\n\n**Project:** {}  \n**State:** {}\n\n",
        literal(&inspection.project_dir.display().to_string()),
        inspection.state.name()
    );
    if project_details
        || inspection
            .artifacts
            .iter()
            .any(|artifact| !artifact.is_valid())
    {
        output.push_str("## Project documents\n\n| Artifact | State |\n| --- | --- |\n");
        for artifact in &inspection.artifacts {
            if project_details || !artifact.is_valid() {
                let _ = writeln!(
                    output,
                    "| {} | {} |",
                    literal(&artifact.path),
                    literal(&artifact.status)
                );
            }
        }
    }
    for diagnostic in [&inspection.root_diagnostic, &inspection.discovery_error]
        .into_iter()
        .flatten()
    {
        let _ = write!(output, "\n**Diagnostic:** {}\n", literal(diagnostic));
    }
    if inspection.state != ProjectState::Current {
        let _ = write!(
            output,
            "\n**Next Step:** {}\n",
            crate::status::project_action_line(inspection)
        );
        let _ = writeln!(
            output,
            "\n**Guidance:** {}",
            crate::presentation::sanitize(&inspection.guidance)
        );
    }
    let _ = write!(output, "\n**VCS:** {}\n", literal(&inspection.vcs.summary));
    if let Some(diagnostic) = &inspection.vcs.diagnostic {
        let _ = writeln!(output, "\n**VCS diagnostic:** {}", literal(diagnostic));
    }
    if vcs_details && !inspection.vcs.artifacts.is_empty() {
        output.push_str("\n| VCS artifact | Tracking |\n| --- | --- |\n");
        for artifact in &inspection.vcs.artifacts {
            let _ = writeln!(
                output,
                "| {} | {} |",
                literal(&artifact.path.display().to_string()),
                artifact.state.description()
            );
        }
    }
    if let Some(toolchain) = &inspection.toolchain {
        let _ = write!(
            output,
            "\n**Toolchain:** {} (available: {:?})\n",
            literal(toolchain.channel.as_deref().unwrap_or("not pinned")),
            toolchain.available
        );
        if let Some(diagnostic) = &toolchain.diagnostic {
            let _ = writeln!(
                output,
                "\n**Toolchain diagnostic:** {}",
                literal(diagnostic)
            );
        }
    }
    output
}

fn render_details(
    inspection: &ProjectInspection,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
    project_details: bool,
) -> String {
    let mut output = render_context(inspection, project_details, true);
    for component in &inspection.components {
        if unfinished && component.state == ComponentState::Current {
            continue;
        }
        let path = if component.path.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            component.path.display().to_string()
        };
        let _ = write!(
            output,
            "\n## Component: {}\n\n**State:** {}\n\n| Artifact | State |\n| --- | --- |\n",
            literal(&path),
            component.state.name()
        );
        for artifact in &component.artifacts {
            if only_impls && artifact.path != "IMPL.md"
                || only_documents
                    && !matches!(
                        artifact.path,
                        "REQUIREMENTS.md" | "CONTRACT.md" | "DESIGN.md"
                    )
            {
                continue;
            }
            let _ = writeln!(
                output,
                "| {} | {} |",
                literal(artifact.path),
                artifact.state.name()
            );
        }
        if !only_impls {
            for cause in &component.revalidation_causes {
                let _ = write!(
                    output,
                    "\n### Changed document: {}\n\n| Revision | Digest |\n| --- | --- |\n| expected | {} |\n| observed | {} |\n",
                    literal(&cause.path),
                    literal(&cause.expected_revision),
                    literal(&cause.observed_revision)
                );
                let path = inspection
                    .component_root
                    .as_deref()
                    .unwrap_or(Path::new("."))
                    .join(&component.path)
                    .join(&cause.path);
                let path = path
                    .components()
                    .filter(|part| *part != std::path::Component::CurDir)
                    .collect::<std::path::PathBuf>();
                let argument = path.to_string_lossy();
                let argument = if argument
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
                {
                    argument.into_owned()
                } else {
                    crate::task_commands::decisions::component_argument(&path)
                };
                let _ = write!(
                    output,
                    "\n**Review under Git:**\n\n{}\n",
                    fence(&format!("git diff HEAD -- {argument}"), "sh")
                );
            }
        }
        match read_queue(inspection, component) {
            Ok(queue) => {
                let done = queue
                    .tasks
                    .iter()
                    .filter(|task| task.status == crate::task_queue::TaskStatus::Completed)
                    .count();
                let _ = write!(
                    output,
                    "\n**Task progress:** {done}/{} completed\n",
                    queue.tasks.len()
                );
                for status in [
                    crate::task_queue::TaskStatus::Blocked,
                    crate::task_queue::TaskStatus::AwaitingDecision,
                    crate::task_queue::TaskStatus::InProgress,
                ] {
                    let count = queue
                        .tasks
                        .iter()
                        .filter(|task| task.status == status)
                        .count();
                    if count > 0 {
                        let _ =
                            writeln!(output, "\n- {count} {}", super::bark::status_label(status));
                    }
                }
                if let Some(next) = crate::task_queue::next_ready_task_id(&queue.tasks) {
                    let _ = writeln!(output, "\n**Next ready:** {}", literal(&next));
                    if inspection.state == ProjectState::Current
                        && component.state == ComponentState::Current
                    {
                        let _ = writeln!(
                            output,
                            "\n**Next command:** `kvist task run {} {}`",
                            crate::task_commands::decisions::component_argument(&component.path),
                            next
                        );
                    } else {
                        output.push_str("\n**Execution unavailable:** resolve the project/component state before running queued work.\n");
                    }
                } else {
                    output.push_str("\n**Next ready:** no ready task; inspect blockers/dependencies or review completed work.\n");
                }
                for task in &queue.tasks {
                    if let Some(reason) = &task.blocked_reason {
                        let _ = write!(
                            output,
                            "\n### {}: {}\n\n{}\n\n",
                            super::bark::status_label(task.status),
                            literal(&task.id),
                            fence(reason, "text")
                        );
                        if let Some(next) = super::bark::next_command_for(task, &queue.tasks, &path)
                        {
                            let _ = writeln!(output, "**Next:**\n\n{}", fence(&next, "sh"));
                        }
                    }
                }
            }
            Err(error) => {
                let _ = write!(
                    output,
                    "\n**Task progress unavailable:** {}\n",
                    literal(&error)
                );
            }
        }
        if component.state != ComponentState::Current {
            let command = match component.state {
                ComponentState::Stale => format!(
                    "kvist component accept {}",
                    crate::task_commands::decisions::component_argument(&component.path)
                ),
                ComponentState::Blocked => "kvist help task-states".to_owned(),
                _ => "kvist doctor; kvist repair --dry-run".to_owned(),
            };
            let _ = write!(
                output,
                "\n**Next Step:** review the reported state, then `{command}`.\n"
            );
        }
    }
    output
}

pub(super) fn render_summary(inspection: &ProjectInspection, unfinished: bool) -> String {
    let mut output = render_context(inspection, false, false);
    output.push_str(
        "\n## Components\n\n| Component | State | Artifacts | Tasks | Next action |\n| --- | --- | --- | --- | --- |\n",
    );
    let mut count = 0;
    for component in &inspection.components {
        if unfinished && component.state == ComponentState::Current {
            continue;
        }
        count += 1;
        let valid = component
            .artifacts
            .iter()
            .filter(|artifact| artifact.state == ComponentArtifactState::Valid)
            .count();
        let argument = crate::task_commands::decisions::component_argument(&component.path);
        let mut next = format!("kvist status {argument}");
        let progress = match read_queue(inspection, component) {
            Ok(queue) => {
                let completed = queue
                    .tasks
                    .iter()
                    .filter(|task| task.status == TaskStatus::Completed)
                    .count();
                let mut progress = format!("{completed}/{} completed", queue.tasks.len());
                for status in [
                    TaskStatus::InProgress,
                    TaskStatus::Blocked,
                    TaskStatus::AwaitingDecision,
                ] {
                    let count = queue
                        .tasks
                        .iter()
                        .filter(|task| task.status == status)
                        .count();
                    if count > 0 {
                        let _ = write!(progress, "; {count} {}", super::bark::status_label(status));
                    }
                }
                if inspection.state == ProjectState::Current
                    && component.state == ComponentState::Current
                    && let Some(task) = crate::task_queue::next_ready_task_id(&queue.tasks)
                {
                    next = format!("kvist task run {argument} {task}");
                }
                progress
            }
            Err(error) => format!("unavailable: {error}"),
        };
        let _ = writeln!(
            output,
            "| {} | {} | {valid}/{} valid | {} | {} |",
            literal(&component.path.display().to_string()),
            component.state.name(),
            component.artifacts.len(),
            literal(&progress),
            literal(&next),
        );
    }
    if count == 0 {
        output.push_str("\nNo components match this report's filters or project discovery.\n");
    }
    output.push_str("\nUse `kvist status COMPONENT` for component details or `kvist status --details` for all component details.\n");
    output
}

fn read_queue(
    inspection: &ProjectInspection,
    component: &ComponentInspection,
) -> Result<TaskQueue, String> {
    let directory = inspection
        .project_dir
        .join(
            inspection
                .component_root
                .as_deref()
                .unwrap_or(Path::new(".")),
        )
        .join(&component.path);
    crate::file_io::read_text_bounded(&directory.join("TODOS.yaml"), 1_048_576)
        .map_err(|error| error.to_string())
        .and_then(|text| crate::task_queue::parse(&text).map_err(|error| error.to_string()))
}
