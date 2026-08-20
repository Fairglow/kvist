//! Command-line contract and dispatch for Kvist.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::{
    KvistError, Result, init, project_state, specification, status, task_commands,
    task_queue::TaskStatus, tree,
};

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
    /// Inspect root artifacts without changing the project.
    Doctor(ProjectDirectory),
    /// Render a versioned project and component status report.
    Status {
        /// Project directory; defaults to the current working directory.
        #[arg(value_name = "PROJECT_DIR", default_value = ".")]
        path: PathBuf,
        /// Stable report representation for scripts and tools.
        #[arg(long, value_enum, default_value_t = status::StatusFormat::Text)]
        format: status::StatusFormat,
    },
    /// Select or transition component tasks.
    Task {
        /// Task operation to execute from the current project root.
        #[command(subcommand)]
        command: TaskCommand,
    },
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
    /// Revalidate a component specification, resetting the stale state back to Current.
    Accept {
        /// Component-root-relative component directory; `.` selects the root component.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
    },
}

/// Task selection and state-transition operations.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Print the first ready task without changing durable state.
    Next {
        /// Component-root-relative component directory; `.` selects the root component.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
    },
    /// Persist one legal task status transition and audit attempt.
    Transition {
        /// Component-root-relative component directory; `.` selects the root component.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
        /// Queue-local task identifier.
        #[arg(value_name = "TASK_ID")]
        task_id: String,
        /// Requested durable task status.
        #[arg(value_name = "STATUS")]
        status: TaskStatusArgument,
        /// Required nonblank blocker explanation only when STATUS is `blocked`.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Run an external AI agent to execute a task, tracking progress and token usage.
    Run {
        /// Component-root-relative component directory; `.` selects the root component.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
        /// Optional queue-local task identifier; if omitted, automatically selects the next ready task.
        #[arg(value_name = "TASK_ID")]
        task_id: Option<String>,
        /// Optional flag to stream agent stdout and stderr directly to the console.
        #[arg(long)]
        stream: bool,
    },
    /// View the raw execution log of an agent task attempt.
    Log {
        /// Component-root-relative component directory; `.` selects the root component.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
        /// Queue-local task identifier.
        #[arg(value_name = "TASK_ID")]
        task_id: String,
    },
    /// Approve the current test-command policy.
    ApprovePolicy {
        /// Project directory; defaults to the current working directory.
        #[arg(value_name = "PROJECT_DIR", default_value = ".")]
        path: PathBuf,
    },
}

/// Command-line spelling of a queue task status.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TaskStatusArgument {
    Pending,
    InProgress,
    Blocked,
    Completed,
}

impl From<TaskStatusArgument> for TaskStatus {
    fn from(status: TaskStatusArgument) -> Self {
        match status {
            TaskStatusArgument::Pending => Self::Pending,
            TaskStatusArgument::InProgress => Self::InProgress,
            TaskStatusArgument::Blocked => Self::Blocked,
            TaskStatusArgument::Completed => Self::Completed,
        }
    }
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
    match command {
        Command::Init(project) => init::initialize(&project.path)
            .map(|outcome| CommandOutput::message(outcome.to_string())),
        Command::Tree(project) => tree::render_project(&project.path).map(CommandOutput::message),
        Command::Doctor(project) => project_state::inspect(&project.path)
            .map(|inspection| CommandOutput::message(inspection.to_string())),
        Command::Status { path, format } => project_state::inspect(&path)
            .map(|inspection| CommandOutput::message(status::render(&inspection, format))),
        Command::Task {
            command: TaskCommand::Next { component_dir },
        } => task_commands::next(&component_dir).map(CommandOutput::message),
        Command::Task {
            command:
                TaskCommand::Transition {
                    component_dir,
                    task_id,
                    status,
                    reason,
                },
        } => task_commands::transition(&component_dir, &task_id, status.into(), reason.as_deref())
            .map(CommandOutput::message),
        Command::Task {
            command:
                TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
        } => task_commands::run_task(&component_dir, task_id.as_deref(), stream)
            .map(CommandOutput::message),
        Command::Task {
            command:
                TaskCommand::Log {
                    component_dir,
                    task_id,
                },
        } => task_commands::task_log(&component_dir, &task_id).map(CommandOutput::message),
        Command::Task {
            command: TaskCommand::ApprovePolicy { path },
        } => task_commands::approve_policy(&path).map(CommandOutput::message),
        Command::Spec {
            command: SpecCommand::New { component_dir },
        } => specification::create(&component_dir).map(|generated| {
            CommandOutput::message(format!(
                "created specification at {}",
                generated.path.display()
            ))
        }),
        Command::Spec {
            command: SpecCommand::Validate { spec_file },
        } => {
            let validation = specification::validate_file(&spec_file)?;
            if validation.is_valid() {
                Ok(CommandOutput::message(format!(
                    "valid specification: {}",
                    spec_file.display()
                )))
            } else {
                Err(KvistError::SpecificationValidationFailed {
                    path: spec_file,
                    diagnostics: specification::format_diagnostics(&validation.diagnostics),
                })
            }
        }
        Command::Spec {
            command: SpecCommand::Accept { component_dir },
        } => task_commands::accept(&component_dir).map(CommandOutput::message),
    }
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
    fn parses_doctor_with_the_current_directory_by_default() {
        let cli = Cli::try_parse_from(["kvist", "doctor"]).expect("valid doctor command");

        let Command::Doctor(project) = cli.command else {
            panic!("expected doctor command");
        };

        assert_eq!(project.path, PathBuf::from("."));
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
        assert!(help.contains("doctor"));
        assert!(help.contains("spec"));
    }

    #[test]
    fn unknown_commands_are_rejected_by_the_parser() {
        let error = Cli::try_parse_from(["kvist", "unknown"]).expect_err("invalid command");

        assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    }
}
