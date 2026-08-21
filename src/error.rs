use std::{
    io::{self, Write},
    path::PathBuf,
};

use thiserror::Error;

/// Errors that Kvist can report independently of its presentation layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KvistError {
    /// The required external isolation boundary is absent or did not attest its capability.
    #[error(
        "sandbox runner `{runner}` is unavailable or cannot provide required isolation: {reason}"
    )]
    SandboxUnavailable { runner: String, reason: String },
    /// A command-line syntax or help error produced by the parser.
    #[error(transparent)]
    ArgumentParsing(#[from] clap::Error),
    /// The requested project location is not a real directory.
    #[error("project directory `{path}` must be a directory")]
    ProjectPathNotDirectory {
        /// Path supplied to `kvist init`.
        path: PathBuf,
    },
    /// Initialization would follow a project-root symbolic link.
    #[error(
        "refusing to initialize through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    ProjectPathIsSymlink {
        /// Symbolic-link path supplied to `kvist init`.
        path: PathBuf,
    },
    /// An artifact parent is not a real directory.
    #[error("artifact parent `{path}` must be a directory")]
    ArtifactParentNotDirectory {
        /// Invalid parent path.
        path: PathBuf,
    },
    /// Initialization would follow an artifact-directory symbolic link.
    #[error(
        "refusing to write artifacts through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    ArtifactParentIsSymlink {
        /// Symbolic-link parent path.
        path: PathBuf,
    },
    /// An existing artifact path is not a regular file.
    #[error("artifact path `{path}` must be a regular file")]
    ArtifactPathNotFile {
        /// Invalid artifact path.
        path: PathBuf,
    },
    /// Existing Kvist artifacts require an explicit user decision.
    #[error(
        "cannot initialize `{project_dir}` because Kvist artifacts already exist: {artifacts:?}; \
         inspect or remove them explicitly before retrying"
    )]
    ExistingArtifacts {
        /// Project root that contains the conflict.
        project_dir: PathBuf,
        /// Existing generated artifact paths.
        artifacts: Vec<PathBuf>,
    },
    /// A project is not safe for `init` to modify.
    #[error(
        "cannot initialize `{project_dir}` because its Kvist project state is {state}; \
         run `kvist doctor {project_dir}` and repair or migrate it explicitly"
    )]
    ProjectStateNotInitializable {
        /// Project root that was inspected.
        project_dir: PathBuf,
        /// State reported by the read-only inspector.
        state: String,
    },
    /// The component root does not exist.
    #[error("component root `{path}` does not exist")]
    ComponentRootNotFound {
        /// Missing component-root path.
        path: PathBuf,
    },
    /// The component root is not a real directory.
    #[error("component root `{path}` must be a directory")]
    ComponentRootNotDirectory {
        /// Invalid component-root path.
        path: PathBuf,
    },
    /// Discovery would follow a symbolic-link component root.
    #[error(
        "refusing to discover components through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    ComponentRootIsSymlink {
        /// Symbolic-link component-root path.
        path: PathBuf,
    },
    /// Discovery encountered a link-like descendant.
    #[error("refusing to inspect link-like component path `{path}`")]
    ComponentDiscoveryLinkLikePath {
        /// Link-like path that would otherwise be inspected or traversed.
        path: PathBuf,
    },
    /// Discovery reached the configured traversal bound.
    #[error(
        "component discovery reached its maximum depth of {max_depth} at `{path}`; \
         reduce nesting or make the traversal bound configurable"
    )]
    ComponentDiscoveryDepthExceeded {
        /// Directory at the traversal boundary.
        path: PathBuf,
        /// Maximum allowed depth below the component root.
        max_depth: usize,
    },
    /// Discovery has inspected too many directories.
    #[error(
        "component discovery exceeded its maximum of {max_directories} scanned directories at `{path}`"
    )]
    ComponentDiscoveryDirectoryLimitExceeded {
        /// Directory that would exceed the limit.
        path: PathBuf,
        /// Configured directory limit.
        max_directories: usize,
    },
    /// Discovery has recognized too many components.
    #[error(
        "component discovery exceeded its maximum of {max_components} recognized components at `{path}`"
    )]
    ComponentDiscoveryComponentLimitExceeded {
        /// Component that would exceed the limit.
        path: PathBuf,
        /// Configured component limit.
        max_components: usize,
    },
    /// A directory contains too many entries to inspect safely.
    #[error(
        "component discovery exceeded its maximum of {max_entries} entries in directory `{path}`"
    )]
    ComponentDiscoveryEntriesPerDirectoryExceeded {
        /// Directory whose entry count exceeds the limit.
        path: PathBuf,
        /// Configured entry limit.
        max_entries: usize,
    },
    /// A relative component path is too long to represent within the policy.
    #[error(
        "component discovery exceeded its maximum relative path length of {max_bytes} encoded bytes at `{path}`"
    )]
    ComponentDiscoveryRelativePathTooLong {
        /// Path whose relative representation is too long.
        path: PathBuf,
        /// Configured encoded-byte limit.
        max_bytes: usize,
    },
    /// A recognized component appears below an ordinary directory.
    #[error(
        "component `{path}` is below ordinary directory `{intermediate}`; every intermediate directory must be a component"
    )]
    ComponentDiscoveryHierarchyViolation {
        /// Relative path of the recognized descendant component.
        path: PathBuf,
        /// Relative path of the first ordinary intermediate directory.
        intermediate: PathBuf,
    },
    /// The project root does not exist or cannot be used as a directory.
    #[error("project root `{path}` must be a directory")]
    ProjectRootNotDirectory {
        /// Invalid project-root path.
        path: PathBuf,
    },
    /// Loading configuration would follow a symbolic-link project root.
    #[error(
        "refusing to load project configuration through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    ProjectRootIsSymlink {
        /// Symbolic-link project-root path.
        path: PathBuf,
    },
    /// The required project-local configuration file is absent.
    #[error("Kvist project configuration `{path}` does not exist; run `kvist init` first")]
    ProjectConfigurationMissing {
        /// Missing configuration path.
        path: PathBuf,
    },
    /// The project-local configuration is not a regular file.
    #[error("Kvist project configuration `{path}` must be a regular file")]
    ProjectConfigurationNotFile {
        /// Invalid configuration path.
        path: PathBuf,
    },
    /// Loading configuration would follow a symbolic link.
    #[error(
        "refusing to load project configuration through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    ProjectConfigurationIsSymlink {
        /// Symbolic-link configuration path.
        path: PathBuf,
    },
    /// The configuration exceeds the bounded parsing limit.
    #[error("Kvist project configuration `{path}` exceeds the {max_bytes}-byte limit")]
    ProjectConfigurationTooLarge {
        /// Oversized configuration path.
        path: PathBuf,
        /// Maximum permitted configuration size.
        max_bytes: u64,
    },
    /// The configuration cannot be parsed or violates its schema.
    #[error("invalid Kvist project configuration `{path}`: {reason}")]
    InvalidProjectConfiguration {
        /// Invalid configuration path.
        path: PathBuf,
        /// Actionable schema or parsing diagnostic.
        reason: String,
    },
    /// The configuration schema version is unsupported.
    #[error(
        "unsupported Kvist project configuration version {version} in `{path}`; \
         this binary supports version {supported_version}"
    )]
    UnsupportedProjectConfigurationVersion {
        /// Configuration path.
        path: PathBuf,
        /// Version read from the configuration.
        version: i64,
        /// Version supported by this binary.
        supported_version: i64,
    },
    /// A component target is not a real directory.
    #[error("component directory `{path}` must be a directory")]
    ComponentDirectoryNotDirectory {
        /// Invalid component-directory path.
        path: PathBuf,
    },
    /// Specification generation would follow a component-directory symlink.
    #[error(
        "refusing to create a specification through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    ComponentDirectoryIsSymlink {
        /// Symbolic-link component-directory path.
        path: PathBuf,
    },
    /// A specification already exists and must never be overwritten implicitly.
    #[error(
        "specification `{path}` already exists; edit it or remove it explicitly before retrying"
    )]
    SpecificationAlreadyExists {
        /// Existing specification path.
        path: PathBuf,
    },
    /// The checked-in generation template no longer satisfies its own contract.
    #[error("generated specification template is invalid: {diagnostics}")]
    GeneratedSpecificationInvalid {
        /// Rendered validation diagnostics.
        diagnostics: String,
    },
    /// A specification failed validation.
    #[error("specification `{path}` is invalid:\n{diagnostics}")]
    SpecificationValidationFailed {
        /// Invalid specification path.
        path: PathBuf,
        /// Line-aware validation diagnostics.
        diagnostics: String,
    },
    /// A specification path is not a regular file.
    #[error("specification `{path}` must be a regular file")]
    SpecificationNotFile {
        /// Invalid specification path.
        path: PathBuf,
    },
    /// Validation would follow a symbolic-link specification.
    #[error(
        "refusing to validate specification through link-like path `{path}` (symbolic link or Windows reparse point)"
    )]
    SpecificationIsSymlink {
        /// Symbolic-link specification path.
        path: PathBuf,
    },
    /// The specification exceeds the bounded parsing limit.
    #[error("specification `{path}` exceeds the {max_bytes}-byte limit")]
    SpecificationTooLarge {
        /// Oversized specification path.
        path: PathBuf,
        /// Maximum permitted specification size.
        max_bytes: u64,
    },
    /// A task command requires a normal component-root-relative path.
    #[error(
        "component directory `{path}` must be `.` or a normal path relative to the configured component root"
    )]
    TaskComponentPathInvalid {
        /// User-supplied component path.
        path: PathBuf,
    },
    /// A task command can only run from a complete current project.
    #[error(
        "cannot run task command because project `{project_dir}` is not current (state: {state})"
    )]
    TaskProjectNotCurrent {
        /// Current project root.
        project_dir: PathBuf,
        /// Inspected project state.
        state: String,
    },
    /// The requested directory is not a discovered current component.
    #[error("cannot run task command because component `{component}` is {state}")]
    TaskComponentNotCurrent {
        /// Component-root-relative component path.
        component: PathBuf,
        /// Inspected component state or absence.
        state: String,
    },
    /// Task mutation and selection require every durable artifact to be tracked.
    #[error(
        "cannot run task command because durable artifacts are not completely VCS tracked: {summary}"
    )]
    TaskVcsNotCurrent {
        /// VCS inspection summary.
        summary: String,
    },
    /// A task queue unexpectedly changed after component revalidation.
    #[error("cannot use TODO queue `{path}` after revalidation: {reason}")]
    TaskQueueUnavailable {
        /// Queue path.
        path: PathBuf,
        /// Read or validation failure.
        reason: String,
    },
    /// Another writer or an explicitly retained stale lock owns the component.
    #[error(
        "cannot transition task `{task_id}` because component lock `{path}` already exists; inspect the owner and remove it explicitly only when no writer is active"
    )]
    TaskLockExists {
        /// Existing lock path.
        path: PathBuf,
        /// Target task.
        task_id: String,
    },
    /// A requested task is not in the selected component queue.
    #[error("task `{task_id}` does not exist in component `{component}`")]
    TaskNotFound {
        /// Component-root-relative component path.
        component: PathBuf,
        /// Requested task ID.
        task_id: String,
    },
    /// A task is not eligible to start work.
    #[error("task `{task_id}` is not ready: {reason}")]
    TaskNotReady {
        /// Requested task ID.
        task_id: String,
        /// Blocking condition.
        reason: String,
    },
    /// A task requires verification but the test-command policy has changed or is not approved.
    #[error(
        "unapproved test-command policy has changed or has not been approved. Run `kvist task approve-policy` to approve it. Current hash: {current_hash}, expected: {expected_hash:?}"
    )]
    UnapprovedTestPolicy {
        /// Canonical hash of the current test-command policy.
        current_hash: String,
        /// Canonical hash of the expected/previously approved test-command policy.
        expected_hash: Option<String>,
    },
    /// A test command is missing for a component requiring verification.
    #[error(
        "missing test-command policy for component `{component}`. Please define a command for it in `kvist.toml` under `[test_policy]` and approve it"
    )]
    MissingTestCommand {
        /// Component path requiring verification.
        component: String,
    },
    /// A state-machine transition or reason violates the task command contract.
    #[error("cannot transition task `{task_id}` from {from} to {to}: {reason}")]
    TaskTransitionInvalid {
        /// Target task.
        task_id: String,
        /// Existing status.
        from: String,
        /// Requested status.
        to: String,
        /// Contract violation.
        reason: String,
    },
    /// The system clock could not provide a UTC timestamp for a transition.
    #[error("cannot determine the current UTC timestamp for a task transition: {source}")]
    TaskClock {
        /// Underlying clock error.
        #[source]
        source: std::time::SystemTimeError,
    },
    /// A filesystem operation failed.
    #[error("cannot {operation} `{path}`: {source}")]
    Io {
        /// Filesystem operation being attempted.
        operation: &'static str,
        /// Path associated with the failure.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
}

/// Result type used by Kvist's domain and command layers.
pub type Result<T> = std::result::Result<T, KvistError>;

impl KvistError {
    /// Returns the process exit status associated with this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::ArgumentParsing(error) => error.exit_code() as u8,
            _ => 1,
        }
    }

    /// Writes this error using the parser's output policy when applicable.
    pub fn print(&self) -> io::Result<()> {
        match self {
            Self::ArgumentParsing(error) => error.print(),
            _ => {
                let mut stderr = io::stderr().lock();
                writeln!(stderr, "error: {self}")
            }
        }
    }
}
