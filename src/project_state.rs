//! Read-only classification of Kvist root artifacts.
//!
//! Phase 1 deliberately does not repair or migrate projects.  This module is
//! the single inspection surface used by `init` and `doctor`, so the decision
//! to refuse a project is based on the same checks shown to its owner.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    KvistError, Result,
    artifacts::{
        CONFIGURATION_VERSION, IMPLEMENTATION_RECORD_HEADING, IMPLEMENTATION_RECORD_VERSION,
        IMPLEMENTATION_RECORD_VERSION_MARKER, ROOT_CONTRACT_VERSION,
        ROOT_IMPLEMENTATION_RECORD_PATH, SPECIFICATION_VERSION, TODO_QUEUE_VERSION,
    },
    config,
    discovery::{self, ComponentArtifact},
    filesystem::is_link_like,
    specification::{self, SpecificationDiagnosticKind},
    task_queue::{self, StalenessCause, StalenessCauseKind, TaskQueue, TaskQueueError, TaskStatus},
    vcs::{self, VcsInspection},
};

/// Required root artifacts, in stable diagnostic order.
pub const REQUIRED_ROOT_ARTIFACT_PATHS: [&str; 5] = [
    "kvist.toml",
    "ROOT_CONTRACT.md",
    "src/SPEC.md",
    "src/TODOS.yaml",
    ROOT_IMPLEMENTATION_RECORD_PATH,
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
        kind: ArtifactKind::ImplementationRecord,
    },
];

/// Maximum supported size for root contract, TODO queue, and implementation records.
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
    ImplementationRecord,
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
    /// Configured component root when the root project is current.
    pub component_root: Option<PathBuf>,
    /// Per-component inspection results in lexical path order.
    pub components: Vec<ComponentInspection>,
    /// Discovery failure captured without changing root-artifact classification.
    pub discovery_error: Option<String>,
}

/// The inspected state of one discovered component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentState {
    /// All component artifacts and workflow evidence are valid and ready.
    Current,
    /// An artifact uses a well-formed version this binary cannot inspect.
    UnsupportedVersion,
    /// An artifact or required parent contract cannot be validated.
    Invalid,
    /// One or more adjacent component artifacts are absent.
    Missing,
    /// The recorded or freshly derived specification evidence is stale.
    Stale,
    /// A current queue contains one or more explicitly blocked tasks.
    Blocked,
}

impl ComponentState {
    /// Stable lowercase name used by text and JSON status reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::UnsupportedVersion => "unsupported-version",
            Self::Invalid => "invalid",
            Self::Missing => "missing",
            Self::Stale => "stale",
            Self::Blocked => "blocked",
        }
    }
}

/// The inspected state of one adjacent component artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentArtifactState {
    /// A supported, valid artifact was read and parsed.
    Valid,
    /// No artifact exists at the expected path.
    Missing,
    /// An artifact has an invalid type, encoding, or content.
    Invalid,
    /// An artifact declares a supported-shape but unsupported version.
    UnsupportedVersion,
}

impl ComponentArtifactState {
    /// Stable lowercase name used by text and JSON status reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::UnsupportedVersion => "unsupported-version",
        }
    }
}

/// Status inspection for one adjacent component artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentArtifactInspection {
    /// Filename adjacent to the component directory.
    pub path: &'static str,
    /// Validated artifact state.
    pub state: ComponentArtifactState,
}

/// An attributable recorded or derived specification revision mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidationCause {
    /// The changed local or immediate-parent specification.
    pub kind: StalenessCauseKind,
    /// Path relative to the component context.
    pub path: String,
    /// Revision recorded when the task queue was reviewed.
    pub expected_revision: String,
    /// Revision observed by this inspection.
    pub observed_revision: String,
}

/// Complete read-only inspection for one discovered component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInspection {
    /// Component path relative to the configured component root.
    pub path: PathBuf,
    /// Aggregate state with stable precedence.
    pub state: ComponentState,
    /// Adjacent artifact states in specification, queue, implementation-record order.
    pub artifacts: Vec<ComponentArtifactInspection>,
    /// Recorded and freshly derived evidence explaining stale state.
    pub revalidation_causes: Vec<RevalidationCause>,
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
            component_root: None,
            components: Vec::new(),
            discovery_error: None,
        });
    }

    let artifacts = ARTIFACTS
        .iter()
        .map(|artifact| inspect_artifact(project_dir, *artifact))
        .collect::<Result<Vec<_>>>()?;
    let state = classify(&artifacts);
    let (component_root, components, discovery_error) = inspect_components(project_dir, state)?;
    Ok(ProjectInspection {
        project_dir: project_dir.to_path_buf(),
        state,
        artifacts,
        root_diagnostic: None,
        vcs: inspect_vcs(project_dir, state),
        guidance: guidance(state).to_owned(),
        component_root,
        components,
        discovery_error,
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
        component_root: None,
        components: Vec::new(),
        discovery_error: None,
    }
}

fn inspect_components(
    project_dir: &Path,
    project_state: ProjectState,
) -> Result<(Option<PathBuf>, Vec<ComponentInspection>, Option<String>)> {
    if project_state != ProjectState::Current {
        return Ok((None, Vec::new(), None));
    }

    let config = match config::load(project_dir) {
        Ok(config) => config,
        Err(error) => return Ok((None, Vec::new(), Some(error.to_string()))),
    };
    let component_root = config.component_root.clone();
    let discovery =
        match discovery::discover_with_limits(&project_dir.join(&component_root), config.discovery)
        {
            Ok(discovery) => discovery,
            Err(error) => return Ok((Some(component_root), Vec::new(), Some(error.to_string()))),
        };

    let components = discovery
        .components
        .iter()
        .map(|component| inspect_component(&project_dir.join(&component_root), component))
        .collect::<Result<Vec<_>>>()?;
    Ok((Some(component_root), components, None))
}

fn inspect_component(
    component_root: &Path,
    component: &discovery::Component,
) -> Result<ComponentInspection> {
    let component_dir = component_root.join(&component.relative_path);
    let specification_path = component_dir.join(ComponentArtifact::Specification.filename());
    let queue_path = component_dir.join(ComponentArtifact::TaskQueue.filename());
    let implementation_record_path =
        component_dir.join(ComponentArtifact::ImplementationRecord.filename());

    let (specification_state, specification_contents) =
        inspect_component_specification(&specification_path)?;
    let (queue_state, queue) = inspect_component_queue(&queue_path)?;
    let implementation_record_state =
        inspect_component_implementation_record(&implementation_record_path)?;
    let artifacts = vec![
        ComponentArtifactInspection {
            path: ComponentArtifact::Specification.filename(),
            state: specification_state,
        },
        ComponentArtifactInspection {
            path: ComponentArtifact::TaskQueue.filename(),
            state: queue_state,
        },
        ComponentArtifactInspection {
            path: ComponentArtifact::ImplementationRecord.filename(),
            state: implementation_record_state,
        },
    ];

    let mut revalidation_causes = Vec::new();
    let mut context_is_invalid = false;
    let mut queue_is_stale = false;
    let mut has_blocked_task = false;
    if let Some(queue) = queue {
        queue_is_stale = matches!(
            queue.component.revalidation.state,
            task_queue::RevalidationState::Stale
        );
        revalidation_causes.extend(
            queue
                .component
                .revalidation
                .causes
                .iter()
                .map(revalidation_cause),
        );

        if let Some(specification_contents) = specification_contents.as_deref() {
            let observed_revision = sha256_revision(specification_contents);
            add_revision_cause(
                &mut revalidation_causes,
                StalenessCauseKind::ComponentSpecificationRevisionChanged,
                "SPEC.md",
                &queue.component.specification_revision,
                observed_revision,
            );
        }

        let is_component_root = is_component_root(&component.relative_path);
        match (&queue.component.parent_specification, is_component_root) {
            (None, true) => {}
            (Some(_), true) | (None, false) => context_is_invalid = true,
            (Some(parent), false) => {
                let parent_specification = component_dir
                    .join("..")
                    .join(ComponentArtifact::Specification.filename());
                let (state, contents) = inspect_component_specification(&parent_specification)?;
                if state != ComponentArtifactState::Valid {
                    context_is_invalid = true;
                } else if let Some(contents) = contents {
                    add_revision_cause(
                        &mut revalidation_causes,
                        StalenessCauseKind::ParentSpecificationRevisionChanged,
                        "../SPEC.md",
                        &parent.revision,
                        sha256_revision(&contents),
                    );
                }
            }
        }
        has_blocked_task = queue
            .tasks
            .iter()
            .any(|task| task.status == TaskStatus::Blocked);
    }

    let state = if artifacts
        .iter()
        .any(|artifact| artifact.state == ComponentArtifactState::UnsupportedVersion)
    {
        ComponentState::UnsupportedVersion
    } else if artifacts
        .iter()
        .any(|artifact| artifact.state == ComponentArtifactState::Invalid)
        || context_is_invalid
    {
        ComponentState::Invalid
    } else if artifacts
        .iter()
        .any(|artifact| artifact.state == ComponentArtifactState::Missing)
    {
        ComponentState::Missing
    } else if queue_is_stale || !revalidation_causes.is_empty() {
        ComponentState::Stale
    } else if has_blocked_task {
        ComponentState::Blocked
    } else {
        ComponentState::Current
    };

    Ok(ComponentInspection {
        path: if is_component_root(&component.relative_path) {
            PathBuf::from(".")
        } else {
            component.relative_path.clone()
        },
        state,
        artifacts,
        revalidation_causes,
    })
}

fn is_component_root(path: &Path) -> bool {
    path.as_os_str().is_empty() || path == Path::new(".")
}

fn revalidation_cause(cause: &StalenessCause) -> RevalidationCause {
    RevalidationCause {
        kind: cause.kind,
        path: cause.path.clone(),
        expected_revision: cause.expected_revision.clone(),
        observed_revision: cause.observed_revision.clone(),
    }
}

fn add_revision_cause(
    causes: &mut Vec<RevalidationCause>,
    kind: StalenessCauseKind,
    path: &str,
    expected_revision: &str,
    observed_revision: String,
) {
    if expected_revision == observed_revision {
        return;
    }
    let cause = RevalidationCause {
        kind,
        path: path.to_owned(),
        expected_revision: expected_revision.to_owned(),
        observed_revision,
    };
    if !causes.contains(&cause) {
        causes.push(cause);
    }
}

fn sha256_revision(contents: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(contents.as_bytes()))
}

fn inspect_component_specification(
    path: &Path,
) -> Result<(ComponentArtifactState, Option<String>)> {
    let Some(contents) = read_component_text_artifact(path)? else {
        return Ok((component_path_state(path)?, None));
    };
    let validation = specification::validate(&contents);
    if validation.is_valid() {
        Ok((ComponentArtifactState::Valid, Some(contents)))
    } else if validation.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            SpecificationDiagnosticKind::UnsupportedTemplateVersion { .. }
        )
    }) {
        Ok((ComponentArtifactState::UnsupportedVersion, None))
    } else {
        Ok((ComponentArtifactState::Invalid, None))
    }
}

fn inspect_component_queue(path: &Path) -> Result<(ComponentArtifactState, Option<TaskQueue>)> {
    let Some(contents) = read_component_text_artifact(path)? else {
        return Ok((component_path_state(path)?, None));
    };
    match task_queue::parse(&contents) {
        Ok(queue) => Ok((ComponentArtifactState::Valid, Some(queue))),
        Err(TaskQueueError::UnsupportedVersion { .. }) => {
            Ok((ComponentArtifactState::UnsupportedVersion, None))
        }
        Err(_) => Ok((ComponentArtifactState::Invalid, None)),
    }
}

fn inspect_component_implementation_record(path: &Path) -> Result<ComponentArtifactState> {
    let Some(contents) = read_component_text_artifact(path)? else {
        return component_path_state(path);
    };
    Ok(markdown_component_state(
        &contents,
        IMPLEMENTATION_RECORD_VERSION_MARKER,
        IMPLEMENTATION_RECORD_VERSION,
        IMPLEMENTATION_RECORD_HEADING,
    ))
}

fn read_component_text_artifact(path: &Path) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect component artifact",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES {
        return Ok(None);
    }
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(source) if source.kind() == io::ErrorKind::InvalidData => Ok(None),
        Err(source) => Err(KvistError::Io {
            operation: "read component artifact",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn component_path_state(path: &Path) -> Result<ComponentArtifactState> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ComponentArtifactState::Missing);
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect component artifact",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if is_link_like(&metadata)
        || !metadata.file_type().is_file()
        || metadata.len() > MAX_ROOT_TEXT_ARTIFACT_BYTES
    {
        return Ok(ComponentArtifactState::Invalid);
    }
    match fs::read_to_string(path) {
        Ok(_) => Ok(ComponentArtifactState::Invalid),
        Err(source) if source.kind() == io::ErrorKind::InvalidData => {
            Ok(ComponentArtifactState::Invalid)
        }
        Err(source) => Err(KvistError::Io {
            operation: "read component artifact",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn markdown_component_state(
    contents: &str,
    marker_name: &str,
    supported_version: u32,
    required_heading: &str,
) -> ComponentArtifactState {
    let expected_prefix = format!("<!-- {marker_name}: ");
    let first_line = contents.lines().next().unwrap_or_default();
    let Some(version_text) = first_line
        .strip_prefix(&expected_prefix)
        .and_then(|value| value.strip_suffix(" -->"))
    else {
        return ComponentArtifactState::Invalid;
    };
    let Ok(version) = version_text.parse::<u32>() else {
        return ComponentArtifactState::Invalid;
    };
    if version == 0 {
        return ComponentArtifactState::Invalid;
    }
    if version != supported_version {
        return ComponentArtifactState::UnsupportedVersion;
    }
    if !contents.lines().any(|line| line == required_heading) {
        return ComponentArtifactState::Invalid;
    }
    ComponentArtifactState::Valid
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
            ComponentArtifact::ImplementationRecord,
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
        ArtifactKind::ImplementationRecord => validate_markdown_version(
            artifact.path,
            &path,
            IMPLEMENTATION_RECORD_VERSION_MARKER,
            IMPLEMENTATION_RECORD_VERSION,
            IMPLEMENTATION_RECORD_HEADING,
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
    match task_queue::parse(&contents) {
        Ok(_) => Ok(valid_status(
            relative_path,
            format!("valid (TODO queue version {TODO_QUEUE_VERSION})"),
        )),
        Err(TaskQueueError::UnsupportedVersion { found, .. }) => Ok(unsupported_status(
            relative_path,
            format!(
                "unsupported version {found} (supported TODO queue version {TODO_QUEUE_VERSION})"
            ),
        )),
        Err(error) => Ok(invalid_status(relative_path, format!("invalid ({error})"))),
    }
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
        assert_eq!(IMPLEMENTATION_RECORD_VERSION, 1);
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
