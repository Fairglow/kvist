//! Markdown status projection; native status formats remain in `status`.

use std::fmt::Write;
use std::path::Path;

use crate::presentation::{fence, literal};
use crate::project_state::{ComponentState, ProjectInspection, ProjectState};

/// Presents inspection and bounded queue facts as Markdown.
pub fn render(
    inspection: &ProjectInspection,
    only_documents: bool,
    only_impls: bool,
    unfinished: bool,
) -> String {
    let mut output = format!(
        "# Project Status\n\n**Project:** {}  \n**State:** {}\n\n",
        literal(&inspection.project_dir.display().to_string()),
        inspection.state.name()
    );
    output.push_str("## Project documents\n\n| Artifact | State |\n| --- | --- |\n");
    for artifact in &inspection.artifacts {
        let _ = writeln!(
            output,
            "| {} | {} |",
            literal(&artifact.path),
            literal(&artifact.status)
        );
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
    if !inspection.vcs.artifacts.is_empty() {
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
        let directory = inspection
            .project_dir
            .join(
                inspection
                    .component_root
                    .as_deref()
                    .unwrap_or(Path::new(".")),
            )
            .join(&component.path);
        match crate::file_io::read_text_bounded(&directory.join("TODOS.yaml"), 1_048_576)
            .map_err(|error| error.to_string())
            .and_then(|text| crate::task_queue::parse(&text).map_err(|error| error.to_string()))
        {
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
                    let _ = write!(
                        output,
                        "\n**Next ready:** {}  \n**Next command:** `kvist task run {} {}`\n",
                        literal(&next),
                        crate::task_commands::decisions::component_argument(&component.path),
                        next
                    );
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
