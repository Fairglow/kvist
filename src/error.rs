use std::{
    io::{self, Write},
    path::PathBuf,
};

use thiserror::Error;

/// Errors that Kvist can report independently of its presentation layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KvistError {
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
    #[error("refusing to initialize through symbolic link `{path}`")]
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
    #[error("refusing to write artifacts through symbolic link `{path}`")]
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
    #[error("refusing to discover components through symbolic link `{path}`")]
    ComponentRootIsSymlink {
        /// Symbolic-link component-root path.
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
    /// The project root does not exist or cannot be used as a directory.
    #[error("project root `{path}` must be a directory")]
    ProjectRootNotDirectory {
        /// Invalid project-root path.
        path: PathBuf,
    },
    /// Loading configuration would follow a symbolic-link project root.
    #[error("refusing to load project configuration through symbolic link `{path}`")]
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
    #[error("refusing to load project configuration through symbolic link `{path}`")]
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
    #[error("refusing to create a specification through symbolic link `{path}`")]
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
    #[error("refusing to validate specification through symbolic link `{path}`")]
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
