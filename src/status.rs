//! Deterministic, read-only project status rendering.

use clap::ValueEnum;

use crate::{
    project_state::{ComponentInspection, ProjectInspection, RevalidationCause},
    task_queue::StalenessCauseKind,
};

/// Presentation format for the versioned status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFormat {
    /// Stable human-readable text intended for terminals and simple scripts.
    Text,
    /// Stable compact JSON intended for structured automation.
    Json,
}

/// Renders a completed project inspection without performing any filesystem I/O.
pub fn render(inspection: &ProjectInspection, format: StatusFormat) -> String {
    match format {
        StatusFormat::Text => render_text(inspection),
        StatusFormat::Json => render_json(inspection),
    }
}

fn render_text(inspection: &ProjectInspection) -> String {
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
        output.push_str("\ncomponent: ");
        output.push_str(&escape_text(&component.path.to_string_lossy()));
        output.push_str(" state: ");
        output.push_str(component.state.name());
        for artifact in &component.artifacts {
            output.push_str("\n  ");
            output.push_str(artifact.path);
            output.push_str(": ");
            output.push_str(artifact.state.name());
        }
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
    output
}

fn render_json(inspection: &ProjectInspection) -> String {
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
    for (index, component) in inspection.components.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        append_component_json(&mut output, component);
    }
    output.push_str("],\"discovery_error\":");
    match &inspection.discovery_error {
        Some(error) => json_string(&mut output, error),
        None => output.push_str("null"),
    }
    output.push('}');
    output
}

fn append_component_json(output: &mut String, component: &ComponentInspection) {
    output.push_str("{\"path\":");
    json_string(output, &component.path.to_string_lossy());
    output.push_str(",\"state\":");
    json_string(output, component.state.name());
    output.push_str(",\"artifacts\":[");
    for (index, artifact) in component.artifacts.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str("{\"path\":");
        json_string(output, artifact.path);
        output.push_str(",\"state\":");
        json_string(output, artifact.state.name());
        output.push('}');
    }
    output.push_str("],\"revalidation_causes\":[");
    for (index, cause) in component.revalidation_causes.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        append_cause_json(output, cause);
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
        StalenessCauseKind::ComponentSpecificationRevisionChanged => {
            "component-specification-revision-changed"
        }
        StalenessCauseKind::ParentSpecificationRevisionChanged => {
            "parent-specification-revision-changed"
        }
    }
}
