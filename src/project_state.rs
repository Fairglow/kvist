//! Read-only classification of Kvist root artifacts.
//!
//! Phase 1 deliberately does not repair or migrate projects.  This module is
//! the single inspection surface used by `init` and `doctor`, so the decision
//! to refuse a project is based on the same checks shown to its owner.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use serde_yaml::Value;

use crate::{
    KvistError, Result,
    artifacts::{
        CONFIGURATION_VERSION, DOCUMENTATION_VERSION, ROOT_CONTRACT_VERSION, SPECIFICATION_VERSION,
        TODO_QUEUE_VERSION,
    },
    config,
    discovery::{self, ComponentArtifact},
    filesystem::is_link_like,
    specification::{self, SpecificationDiagnosticKind},
    vcs::{self, VcsInspection},
};

/// Required root artifacts, in stable diagnostic order.
pub const REQUIRED_ROOT_ARTIFACT_PATHS: [&str; 5] = [
    "kvist.toml",
    "ROOT_CONTRACT.md",
    "src/SPEC.md",
    "src/TODOS.yaml",
    "src/DOCS.md",
];

const ARTIFACTS: [Artifact; 5] = [
    Artifact {
        path: REQUIRED_ROOT_ARTIFACT_PATHS[0],
        kind: ArtifactKind::Configuration,
    },
    Artifact {
        path: REQUIRED_ROOT_ARTIFACT_PATHS[1],
        kind: ArtifactKind::RootContract,
    },
    Artifact {
        path: REQUIRED_ROOT_ARTIFACT_PATHS[2],
        kind: ArtifactKind::Specification,
    },
    Artifact {
        path: REQUIRED_ROOT_ARTIFACT_PATHS[3],
        kind: ArtifactKind::TodoQueue,
    },
    Artifact {
        path: REQUIRED_ROOT_ARTIFACT_PATHS[4],
        kind: ArtifactKind::Documentation,
    },
];

/// Maximum supported size for root contract, TODO queue, and documentation.
pub const MAX_ROOT_TEXT_ARTIFACT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct Artifact {
    path: &'static str,
    kind: ArtifactKind,
}

#[derive(Debug, Clone, Copy)]
enum ArtifactKind {
    Configuration,
    RootContract,
    Specification,
    TodoQueue,
    Documentation,
}

/// The safe Phase 1 state of a project root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectState {
    /// No Kvist root artifact exists.
    Uninitialized,
    /// Every root artifact is a current, valid regular file.
    Current,
    /// Some, but not all, root artifacts exist and those present are valid.
    Partial,
    /// An artifact has an invalid type or content.
    Invalid,
    /// An artifact declares a well-formed version this binary does not support.
    UnsupportedVersion,
}

impl ProjectState {
    /// Stable lowercase name used by `doctor`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Current => "current",
            Self::Partial => "partial",
            Self::Invalid => "invalid",
            Self::UnsupportedVersion => "unsupported-version",
        }
    }
}

impl fmt::Display for ProjectState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// The inspection result for one required root artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactStatus {
    /// Artifact path relative to the project root.
    pub path: &'static str,
    /// Human-readable status suitable for `doctor`.
    pub status: String,
    /// Whether this artifact exists at the expected location.
    pub exists: bool,
    class: ArtifactClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactClass {
    Missing,
    Valid,
    Invalid,
    UnsupportedVersion,
}

/// A complete, read-only project inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInspection {
    /// Inspected project directory.
    pub project_dir: PathBuf,
    /// Classification derived from all five required root artifacts.
    pub state: ProjectState,
    /// Per-artifact validation results in stable artifact order.
    pub artifacts: Vec<ArtifactStatus>,
    /// Project-root diagnostic when inspection could not enumerate artifacts.
    pub root_diagnostic: Option<String>,
    /// Read-only durable-artifact tracking inspection.
    pub vcs: VcsInspection,
    /// Action the owner can take without Kvist rewriting project content.
    pub guidance: String,
}

impl fmt::Display for ProjectInspection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "project: {}\nstate: {}",
            self.project_dir.display(),
            self.state
        )?;
        if let Some(diagnostic) = &self.root_diagnostic {
            writeln!(formatter, "diagnostic: {diagnostic}")?;
        }
        for artifact in &self.artifacts {
            writeln!(formatter, "{}: {}", artifact.path, artifact.status)?;
        }
        writeln!(formatter, "vcs: {}", self.vcs.summary)?;
        if let Some(repository_root) = &self.vcs.repository_root {
            writeln!(formatter, "vcs repository: {}", repository_root.display())?;
        }
        for artifact in &self.vcs.artifacts {
            writeln!(
                formatter,
                "vcs {}: {}",
                artifact.path.display(),
                artifact.state.description()
            )?;
        }
        if let Some(diagnostic) = &self.vcs.diagnostic {
            writeln!(formatter, "vcs diagnostic: {diagnostic}")?;
        }
        write!(formatter, "guidance: {}", self.guidance)
    }
}

/// Inspects a project without creating, modifying, or following root artifact
/// symbolic links.
pub fn inspect(project_dir: &Path) -> Result<ProjectInspection> {
    let root_exists = match fs::symlink_metadata(project_dir) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if is_link_like(&metadata) || !file_type.is_dir() {
                return Ok(invalid_root_inspection(project_dir));
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect project directory",
                path: project_dir.to_path_buf(),
                source,
            });
        }
    };

    if !root_exists {
        return Ok(ProjectInspection {
            project_dir: project_dir.to_path_buf(),
            state: ProjectState::Uninitialized,
            artifacts: ARTIFACTS
                .iter()
                .map(|artifact| missing_status(artifact.path))
                .collect(),
            root_diagnostic: None,
            vcs: VcsInspection::not_checked(
                "root artifacts do not exist yet; initialize and validate the project first",
            ),
            guidance: "run `kvist init` to create the Phase 1 root artifacts".to_owned(),
        });
    }

    let artifacts = ARTIFACTS
        .iter()
        .map(|artifact| inspect_artifact(project_dir, *artifact))
        .collect::<Result<Vec<_>>>()?;
    let state = classify(&artifacts);
    Ok(ProjectInspection {
        project_dir: project_dir.to_path_buf(),
        state,
        artifacts,
        root_diagnostic: None,
        vcs: inspect_vcs(project_dir, state),
        guidance: guidance(state).to_owned(),
    })
}

fn invalid_root_inspection(project_dir: &Path) -> ProjectInspection {
    ProjectInspection {
        project_dir: project_dir.to_path_buf(),
        state: ProjectState::Invalid,
        artifacts: Vec::new(),
        root_diagnostic: Some(
            "project path must be a real directory, not a file or link-like path".to_owned(),
        ),
        vcs: VcsInspection::not_checked(
            "project root is not a real directory, so durable artifact paths cannot be inspected",
        ),
        guidance: guidance(ProjectState::Invalid).to_owned(),
    }
}

fn inspect_vcs(project_dir: &Path, state: ProjectState) -> VcsInspection {
    if state != ProjectState::Current {
        return VcsInspection::not_checked(
            "root artifact state is not current; repair it before checking durable-artifact tracking",
        );
    }

    let config = match config::load(project_dir) {
        Ok(config) => config,
        Err(error) => {
            return VcsInspection::not_checked(format!(
                "cannot load current project configuration for VCS inspection: {error}"
            ));
        }
    };
    let discovery = match discovery::discover_with_limits(
        &project_dir.join(&config.component_root),
        config.discovery,
    ) {
        Ok(discovery) => discovery,
        Err(error) => {
            return VcsInspection::not_checked(format!(
                "cannot discover component artifacts for VCS inspection: {error}"
            ));
        }
    };

    let mut required_paths = REQUIRED_ROOT_ARTIFACT_PATHS
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    for component in discovery.components {
        let component_dir = if component.relative_path == Path::new(".") {
            config.component_root.clone()
        } else {
            config.component_root.join(component.relative_path)
        };
        for artifact in [
            ComponentArtifact::Specification,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::Documentation,
        ] {
            required_paths.push(component_dir.join(artifact.filename()));
        }
    }

    vcs::inspect(project_dir, config.vcs, required_paths)
}

fn inspect_artifact(project_dir: &Path, artifact: Artifact) -> Result<ArtifactStatus> {
    let path = project_dir.join(artifact.path);
    if let Some(parent_problem) = parent_problem(project_dir, artifact.path)? {
        return Ok(ArtifactStatus {
            path: artifact.path,
            status: parent_problem,
            exists: false,
            class: ArtifactClass::Invalid,
        });
    }

    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(missing_status(artifact.path));
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect root artifact",
                path,
                source,
            });
        }
    };
    let file_type = metadata.file_type();
    if is_link_like(&metadata) {
        return Ok(invalid_status(artifact.path, "invalid (link-like path)"));
    }
    if !file_type.is_file() {
        return Ok(invalid_status(
            artifact.path,
            "invalid (must be a regular file)",
        ));
    }

    validate_artifact(project_dir, artifact)
}

fn parent_problem(project_dir: &Path, relative_path: &str) -> Result<Option<String>> {
    let Some(parent) = Path::new(relative_path).parent() else {
        return Ok(None);
    };
    if parent.as_os_str().is_empty() {
        return Ok(None);
    }
    let path = project_dir.join(parent);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if is_link_like(&metadata) => Ok(Some(format!(
            "invalid (parent `{}` is link-like)",
            parent.display()
        ))),
        Ok(metadata) if !metadata.file_type().is_dir() => Ok(Some(format!(
            "invalid (parent `{}` is not a directory)",
            parent.display()
        ))),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(KvistError::Io {
            operation: "inspect root artifact parent",
            path,
            source,
        }),
    }
}

fn validate_artifact(project_dir: &Path, artifact: Artifact) -> Result<ArtifactStatus> {
    let path = project_dir.join(artifact.path);
    match artifact.kind {
        ArtifactKind::Configuration => match config::load(project_dir) {
            Ok(_) => Ok(valid_status(
                artifact.path,
                format!("valid (configuration version {CONFIGURATION_VERSION})"),
            )),
            Err(KvistError::UnsupportedProjectConfigurationVersion { version, .. }) => {
                Ok(unsupported_status(
                    artifact.path,
                    format!(
                        "unsupported version {version} (supported configuration version {CONFIGURATION_VERSION})"
                    ),
                ))
            }
            Err(error) => Ok(invalid_status(artifact.path, format!("invalid ({error})"))),
        },
        ArtifactKind::RootContract => validate_markdown_version(
            artifact.path,
            &path,
            "kvist-root-contract-version",
            ROOT_CONTRACT_VERSION,
            "# Kvist Root Contract",
        ),
        ArtifactKind::Specification => {
            let validation = match specification::validate_file(&path) {
                Ok(validation) => validation,
                Err(KvistError::SpecificationTooLarge { .. }) => {
                    return Ok(invalid_status(
                        artifact.path,
                        format!(
                            "invalid (exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte root artifact limit)"
                        ),
                    ));
                }
                Err(
                    KvistError::SpecificationIsSymlink { .. }
                    | KvistError::SpecificationNotFile { .. },
                ) => {
                    return Ok(invalid_status(
                        artifact.path,
                        "invalid (must be a regular non-symbolic-link file)",
                    ));
                }
                Err(KvistError::Io { source, .. })
                    if source.kind() == io::ErrorKind::InvalidData =>
                {
                    return Ok(invalid_status(
                        artifact.path,
                        "invalid (must be valid UTF-8)",
                    ));
                }
                Err(error) => return Err(error),
            };
            if validation.is_valid() {
                Ok(valid_status(
                    artifact.path,
                    format!("valid (specification version {SPECIFICATION_VERSION})"),
                ))
            } else if let Some(SpecificationDiagnosticKind::UnsupportedTemplateVersion {
                found,
                ..
            }) =
                validation
                    .diagnostics
                    .iter()
                    .find_map(|diagnostic| match &diagnostic.kind {
                        kind @ SpecificationDiagnosticKind::UnsupportedTemplateVersion {
                            ..
                        } => Some(kind),
                        _ => None,
                    })
            {
                Ok(unsupported_status(
                    artifact.path,
                    format!(
                        "unsupported version {found} (supported specification version {SPECIFICATION_VERSION})"
                    ),
                ))
            } else {
                Ok(invalid_status(
                    artifact.path,
                    format!(
                        "invalid ({})",
                        specification::format_diagnostics(&validation.diagnostics)
                    ),
                ))
            }
        }
        ArtifactKind::TodoQueue => validate_todo_queue(artifact.path, &path),
        ArtifactKind::Documentation => validate_markdown_version(
            artifact.path,
            &path,
            "kvist-documentation-version",
            DOCUMENTATION_VERSION,
            "# Root Component Compliance Documentation",
        ),
    }
}

fn validate_markdown_version(
    relative_path: &'static str,
    path: &Path,
    marker_name: &str,
    supported_version: u32,
    required_heading: &str,
) -> Result<ArtifactStatus> {
    let contents = match read_root_text_artifact(relative_path, path)? {
        Ok(contents) => contents,
        Err(status) => return Ok(status),
    };
    let expected_prefix = format!("<!-- {marker_name}: ");
    let first_line = contents.lines().next().unwrap_or_default();
    let Some(version_text) = first_line
        .strip_prefix(&expected_prefix)
        .and_then(|value| value.strip_suffix(" -->"))
    else {
        return Ok(invalid_status(
            relative_path,
            format!("invalid (line 1 must be `{expected_prefix}<positive integer> -->`)"),
        ));
    };
    let Ok(version) = version_text.parse::<u32>() else {
        return Ok(invalid_status(
            relative_path,
            "invalid (version must be a positive integer)",
        ));
    };
    if version == 0 {
        return Ok(invalid_status(
            relative_path,
            "invalid (version must be a positive integer)",
        ));
    }
    if version != supported_version {
        return Ok(unsupported_status(
            relative_path,
            format!("unsupported version {version} (supported version {supported_version})"),
        ));
    }
    if !contents.lines().any(|line| line == required_heading) {
        return Ok(invalid_status(
            relative_path,
            format!("invalid (missing required heading `{required_heading}`)"),
        ));
    }
    Ok(valid_status(
        relative_path,
        format!("valid (version {supported_version})"),
    ))
}

fn validate_todo_queue(relative_path: &'static str, path: &Path) -> Result<ArtifactStatus> {
    let contents = match read_root_text_artifact(relative_path, path)? {
        Ok(contents) => contents,
        Err(status) => return Ok(status),
    };
    let value: Value = match serde_yaml::from_str(&contents) {
        Ok(value) => value,
        Err(error) => {
            return Ok(invalid_status(
                relative_path,
                format!("invalid (YAML parse error: {error})"),
            ));
        }
    };
    let Some(root) = value.as_mapping() else {
        return Ok(invalid_status(
            relative_path,
            "invalid (root value must be a mapping)",
        ));
    };
    let version = root
        .get(Value::String("schema_version".to_owned()))
        .and_then(Value::as_u64);
    let Some(version) = version else {
        return Ok(invalid_status(
            relative_path,
            "invalid (`schema_version` must be a positive integer)",
        ));
    };
    if version == 0 {
        return Ok(invalid_status(
            relative_path,
            "invalid (`schema_version` must be a positive integer)",
        ));
    }
    if version != u64::from(TODO_QUEUE_VERSION) {
        return Ok(unsupported_status(
            relative_path,
            format!(
                "unsupported version {version} (supported TODO queue version {TODO_QUEUE_VERSION})"
            ),
        ));
    }
    let Some(tasks) = root
        .get(Value::String("tasks".to_owned()))
        .and_then(Value::as_sequence)
    else {
        return Ok(invalid_status(
            relative_path,
            "invalid (`tasks` must be a non-empty sequence)",
        ));
    };
    if tasks.is_empty() || tasks.iter().any(|task| !valid_task(task)) {
        return Ok(invalid_status(
            relative_path,
            "invalid (each task requires non-empty string `id`, `status`, and `description` fields)",
        ));
    }
    Ok(valid_status(
        relative_path,
        format!("valid (TODO queue version {TODO_QUEUE_VERSION})"),
    ))
}

fn read_root_text_artifact(
    relative_path: &'static str,
    path: &Path,
) -> Result<std::result::Result<String, ArtifactStatus>> {
    let metadata = fs::metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect root artifact",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
        return Ok(Err(invalid_status(
            relative_path,
            format!(
                "invalid (exceeds the {MAX_ROOT_TEXT_ARTIFACT_BYTES}-byte root artifact limit)"
            ),
        )));
    }

    match fs::read_to_string(path) {
        Ok(contents) => Ok(Ok(contents)),
        Err(source) if source.kind() == io::ErrorKind::InvalidData => Ok(Err(invalid_status(
            relative_path,
            "invalid (must be valid UTF-8)",
        ))),
        Err(source) => Err(KvistError::Io {
            operation: "read root artifact",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn valid_task(task: &Value) -> bool {
    let Some(task) = task.as_mapping() else {
        return false;
    };
    ["id", "status", "description"].iter().all(|key| {
        task.get(Value::String((*key).to_owned()))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn classify(artifacts: &[ArtifactStatus]) -> ProjectState {
    if artifacts
        .iter()
        .any(|artifact| artifact.class == ArtifactClass::UnsupportedVersion)
    {
        ProjectState::UnsupportedVersion
    } else if artifacts
        .iter()
        .any(|artifact| artifact.class == ArtifactClass::Invalid)
    {
        ProjectState::Invalid
    } else if artifacts
        .iter()
        .all(|artifact| artifact.class == ArtifactClass::Missing)
    {
        ProjectState::Uninitialized
    } else if artifacts
        .iter()
        .all(|artifact| artifact.class == ArtifactClass::Valid)
    {
        ProjectState::Current
    } else {
        ProjectState::Partial
    }
}

fn guidance(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Uninitialized => "run `kvist init` to create the Phase 1 root artifacts",
        ProjectState::Current => "project is ready for Phase 1 read-only commands",
        ProjectState::Partial | ProjectState::Invalid | ProjectState::UnsupportedVersion => {
            "Phase 1 never repairs or migrates artifacts automatically; preserve user content, inspect these diagnostics, and repair or migrate explicitly. A future explicit migration/repair command must define every rewrite."
        }
    }
}

fn missing_status(path: &'static str) -> ArtifactStatus {
    ArtifactStatus {
        path,
        status: "missing".to_owned(),
        exists: false,
        class: ArtifactClass::Missing,
    }
}

fn valid_status(path: &'static str, status: String) -> ArtifactStatus {
    ArtifactStatus {
        path,
        status,
        exists: true,
        class: ArtifactClass::Valid,
    }
}

fn invalid_status(path: &'static str, status: impl Into<String>) -> ArtifactStatus {
    ArtifactStatus {
        path,
        status: status.into(),
        exists: true,
        class: ArtifactClass::Invalid,
    }
}

fn unsupported_status(path: &'static str, status: String) -> ArtifactStatus {
    ArtifactStatus {
        path,
        status,
        exists: true,
        class: ArtifactClass::UnsupportedVersion,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::{artifacts::root_artifacts, init::initialize};

    use super::*;

    #[test]
    fn independent_version_domains_are_declared() {
        assert_eq!(CONFIGURATION_VERSION, 1);
        assert_eq!(ROOT_CONTRACT_VERSION, 1);
        assert_eq!(SPECIFICATION_VERSION, 1);
        assert_eq!(TODO_QUEUE_VERSION, 1);
        assert_eq!(DOCUMENTATION_VERSION, 1);
    }

    #[test]
    fn classifies_every_project_state() {
        let uninitialized = TempDir::new().expect("workspace");
        assert_eq!(
            inspect(uninitialized.path()).expect("inspect").state,
            ProjectState::Uninitialized
        );

        let current = TempDir::new().expect("workspace");
        initialize(current.path()).expect("initialize");
        assert_eq!(
            inspect(current.path()).expect("inspect").state,
            ProjectState::Current
        );

        let partial = TempDir::new().expect("workspace");
        fs::write(
            partial.path().join("kvist.toml"),
            root_artifacts()[0].contents,
        )
        .expect("write");
        assert_eq!(
            inspect(partial.path()).expect("inspect").state,
            ProjectState::Partial
        );

        let invalid = TempDir::new().expect("workspace");
        initialize(invalid.path()).expect("initialize");
        fs::write(invalid.path().join("ROOT_CONTRACT.md"), "not a contract").expect("write");
        assert_eq!(
            inspect(invalid.path()).expect("inspect").state,
            ProjectState::Invalid
        );

        let unsupported = TempDir::new().expect("workspace");
        initialize(unsupported.path()).expect("initialize");
        fs::write(
            unsupported.path().join("src/TODOS.yaml"),
            "schema_version: 99\ntasks: []\n",
        )
        .expect("write");
        assert_eq!(
            inspect(unsupported.path()).expect("inspect").state,
            ProjectState::UnsupportedVersion
        );
    }
}
