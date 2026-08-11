//! Command-line contract and dispatch for Kvist.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{KvistError, Result, init};

/// Kvist's top-level command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "kvist",
    version,
    about = "Spec-driven architecture workflow for human-directed AI development",
    long_about = "Kvist manages filesystem-native component specifications, task queues, and compliance documentation."
)]
pub struct Cli {
    /// Command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Commands that form Kvist's initial public CLI contract.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a project with Kvist's root artifacts.
    Init(ProjectDirectory),
    /// Render the component tree for a Kvist project.
    Tree(ProjectDirectory),
    /// Create or validate a component specification.
    Spec {
        /// Specification operation to execute.
        #[command(subcommand)]
        command: SpecCommand,
    },
}

/// An explicit project directory argument shared by project-scoped commands.
#[derive(Debug, Args)]
pub struct ProjectDirectory {
    /// Project directory; defaults to the current working directory.
    #[arg(value_name = "PROJECT_DIR", default_value = ".")]
    pub path: PathBuf,
}

/// Specification-specific commands.
#[derive(Debug, Subcommand)]
pub enum SpecCommand {
    /// Create a layered SPEC.md in a component directory.
    New {
        /// Component directory that will contain the generated SPEC.md.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
    },
    /// Validate a layered SPEC.md file.
    Validate {
        /// Path to the SPEC.md file to validate.
        #[arg(value_name = "SPEC_FILE")]
        spec_file: PathBuf,
    },
}

/// Successful command output written by the binary at the process boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput(String);

impl CommandOutput {
    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for CommandOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Executes a parsed command.
///
/// This dispatch layer deliberately contains no process handling; callers can
/// test command behavior and choose how errors are presented.
pub fn execute(command: Command) -> Result<CommandOutput> {
    let (command, next_step) = match command {
        Command::Init(project) => {
            return init::initialize(&project.path)
                .map(|outcome| CommandOutput::message(outcome.to_string()));
        }
        Command::Tree(_) => (
            "tree",
            "component discovery and rendering will be implemented in Phase 1 task P1-05",
        ),
        Command::Spec {
            command: SpecCommand::New { .. },
        } => (
            "spec new",
            "specification generation will be implemented in Phase 1 task P1-07",
        ),
        Command::Spec {
            command: SpecCommand::Validate { .. },
        } => (
            "spec validate",
            "specification validation will be implemented in Phase 1 task P1-06",
        ),
    };

    Err(KvistError::CommandUnavailable { command, next_step })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn parses_init_with_the_current_directory_by_default() {
        let cli = Cli::try_parse_from(["kvist", "init"]).expect("valid init command");

        let Command::Init(project) = cli.command else {
            panic!("expected init command");
        };

        assert_eq!(project.path, PathBuf::from("."));
    }

    #[test]
    fn parses_tree_with_an_explicit_project_directory() {
        let cli =
            Cli::try_parse_from(["kvist", "tree", "projects/demo"]).expect("valid tree command");

        let Command::Tree(project) = cli.command else {
            panic!("expected tree command");
        };

        assert_eq!(project.path, PathBuf::from("projects/demo"));
    }

    #[test]
    fn parses_specification_creation() {
        let cli = Cli::try_parse_from(["kvist", "spec", "new", "src/network"])
            .expect("valid specification creation command");

        let Command::Spec {
            command: SpecCommand::New { component_dir },
        } = cli.command
        else {
            panic!("expected spec new command");
        };

        assert_eq!(component_dir, PathBuf::from("src/network"));
    }

    #[test]
    fn parses_specification_validation() {
        let cli = Cli::try_parse_from(["kvist", "spec", "validate", "src/SPEC.md"])
            .expect("valid specification validation command");

        let Command::Spec {
            command: SpecCommand::Validate { spec_file },
        } = cli.command
        else {
            panic!("expected spec validate command");
        };

        assert_eq!(spec_file, PathBuf::from("src/SPEC.md"));
    }

    #[test]
    fn help_exits_successfully() {
        let error = Cli::try_parse_from(["kvist", "--help"]).expect_err("help exits successfully");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        assert_eq!(KvistError::from(error).exit_code(), 0);
    }

    #[test]
    fn help_lists_the_complete_phase_one_command_surface() {
        let error = Cli::try_parse_from(["kvist", "--help"]).expect_err("help exits successfully");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let help = error.to_string();
        assert!(help.contains("init"));
        assert!(help.contains("tree"));
        assert!(help.contains("spec"));
    }

    #[test]
    fn unknown_commands_are_rejected_by_the_parser() {
        let error = Cli::try_parse_from(["kvist", "unknown"]).expect_err("invalid command");

        assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn unavailable_command_errors_explain_the_next_implementation_step() {
        let error = execute(Command::Tree(ProjectDirectory {
            path: PathBuf::from("."),
        }))
        .expect_err("tree is not implemented yet");

        assert_eq!(
            error.to_string(),
            "`tree` is not available yet; component discovery and rendering will be implemented in Phase 1 task P1-05"
        );
    }
}
