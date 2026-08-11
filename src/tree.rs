//! Deterministic, non-interactive rendering of a Kvist component tree.

use std::path::Path;

use crate::{
    Result, config,
    discovery::{self, ArtifactStatus, Component, ComponentArtifact, InvalidArtifactKind},
};

/// Discovers and renders the component tree for a project root.
pub fn render_project(project_root: &Path) -> Result<String> {
    let config = config::load(project_root)?;
    let discovery = discovery::discover(&project_root.join(&config.component_root))?;

    Ok(render(&config.component_root, &discovery))
}

/// Renders a previously discovered component tree as stable ASCII text.
pub fn render(component_root: &Path, discovery: &discovery::Discovery) -> String {
    let mut output = format!("component root: {}\n", component_root.display());
    for component in &discovery.components {
        let depth = component
            .relative_path
            .components()
            .filter(|component| matches!(component, std::path::Component::Normal(_)))
            .count();
        output.push_str(&"  ".repeat(depth));
        output.push_str(&component.relative_path.display().to_string());
        output.push_str(" [");
        output.push_str(&component_diagnostic(component));
        output.push_str("]\n");
    }
    output.pop();
    output
}

fn component_diagnostic(component: &Component) -> String {
    let diagnostics = [
        ComponentArtifact::Specification,
        ComponentArtifact::TaskQueue,
        ComponentArtifact::Documentation,
    ]
    .into_iter()
    .filter_map(|artifact| match component.artifact_status(artifact) {
        ArtifactStatus::Present => None,
        ArtifactStatus::Missing => Some(format!("missing {}", artifact.filename())),
        ArtifactStatus::Invalid(kind) => Some(format!(
            "{} is {}",
            artifact.filename(),
            invalid_artifact_description(kind)
        )),
    })
    .collect::<Vec<_>>();

    if diagnostics.is_empty() {
        "complete".to_owned()
    } else if diagnostics
        .iter()
        .any(|diagnostic| !diagnostic.starts_with("missing "))
    {
        format!("invalid: {}", diagnostics.join("; "))
    } else {
        format!("incomplete: {}", diagnostics.join("; "))
    }
}

const fn invalid_artifact_description(kind: InvalidArtifactKind) -> &'static str {
    match kind {
        InvalidArtifactKind::Directory => "a directory",
        InvalidArtifactKind::SymbolicLink => "a symbolic link",
        InvalidArtifactKind::Other => "an unsupported filesystem object",
    }
}
