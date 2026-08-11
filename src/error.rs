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
    /// A command is part of the stable CLI contract but is not implemented in
    /// the current development phase.
    #[error("`{command}` is not available yet; {next_step}")]
    CommandUnavailable {
        /// Command name as shown to the user.
        command: &'static str,
        /// Specific next action that explains the command's status.
        next_step: &'static str,
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
