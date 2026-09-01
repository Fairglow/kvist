//! Command-line contract and dispatch for Kvist.

use std::{io::IsTerminal, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::{
    KvistError, Result, component_documents, config, convert, discovery, import, init,
    project_state, prompt_input, reverse_discovery, status, task_commands, task_queue::TaskStatus,
    tree, wizard,
};

/// Kvist's top-level command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "kvist",
    version,
    about = "Spec-driven architecture workflow for human-directed AI development",
    long_about = "Kvist manages filesystem-native requirements, contracts, designs, task queues, and compliance evidence."
)]
pub struct Cli {
    /// Output structured JSON instead of plain text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Commands that form Kvist's public CLI contract.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a project with Kvist's root artifacts.
    Init(ProjectDirectory),
    /// Convert an existing Rust project into a Kvist-managed component.
    Convert {
        /// Existing Rust project directory.
        #[arg(value_name = "PROJECT_DIR")]
        project_dir: PathBuf,
    },
    /// Import Kvist artifacts from a Git repository.
    Import {
        /// Git repository URL to clone.
        #[arg(value_name = "REPO_URL")]
        repo_url: String,
        /// Git branch to use; defaults to the main branch.
        #[arg(long, default_value = "main")]
        branch: String,
        /// Component directory inside the cloned repository; if omitted, the root is used.
        #[arg(long)]
        component: Option<PathBuf>,
        /// Local destination directory; defaults to the current directory.
        #[arg(value_name = "DEST_DIR", default_value = ".")]
        dest_dir: PathBuf,
    },
    /// Render the component tree for a Kvist project.
    Tree(ProjectDirectory),
    /// Inspect root artifacts without changing the project.
    Doctor(ProjectDirectory),
    /// Reverse-discover and generate Kvist artifacts from an existing implementation.
    ReverseDiscover {
        /// Path to the existing implementation directory.
        #[arg(value_name = "PATH")]
        path: PathBuf,
    },
    /// Execute a custom prompt under supervision.
    Prompt {
        /// Custom prompt text. Conflicts with --file and --editor.
        #[arg(value_name = "PROMPT", conflicts_with_all = ["file", "editor"])]
        prompt: Option<String>,
        /// Read the prompt from a UTF-8 file; use `-` for standard input.
        #[arg(short, long, value_name = "PROMPT_FILE", conflicts_with_all = ["prompt", "editor"])]
        file: Option<PathBuf>,
        /// Author the prompt in VISUAL, EDITOR, or vi.
        #[arg(long, conflicts_with_all = ["prompt", "file"])]
        editor: bool,
        /// The role profile context to use (developer, architect, security-reviewer).
        #[arg(long, default_value = "developer")]
        role: String,
        /// Select one configured model from the chosen role for this prompt.
        #[arg(long, value_name = "NAME")]
        model: Option<String>,
        /// Apply a supported reasoning effort through the selected command template.
        #[arg(long, value_enum)]
        reasoning_effort: Option<ReasoningEffortArgument>,
        /// Idle timeout in seconds before restarting the command if no new output.
        #[arg(long, default_value_t = 900)]
        idle_timeout: u64,
        /// Enable repetitive loop detection in the streaming output.
        #[arg(long)]
        detect_loops: bool,
        /// Maximum number of automatic restarts allowed.
        #[arg(long, default_value_t = 3)]
        max_restarts: u32,
        /// Acknowledge that this command runs the provider with your host permissions.
        #[arg(long)]
        allow_host_execution: bool,
    },
    /// Render a versioned project and component status report.
    Status {
        /// Project directory; defaults to the current working directory.
        #[arg(value_name = "PROJECT_DIR", default_value = ".")]
        path: PathBuf,
        /// Stable report representation for scripts and tools.
        #[arg(long, value_enum, default_value_t = status::StatusFormat::Text)]
        format: status::StatusFormat,
        /// Show only requirements, contract, and design artifacts plus revalidation details.
        #[arg(long)]
        only_documents: bool,
        /// Filter report to show only implementation artifacts (IMPL.md).
        #[arg(long)]
        only_impls: bool,
        /// Filter report to show only blocked, stale, or incomplete components (omitting finished ones).
        #[arg(long)]
        unfinished: bool,
    },
    /// Select or transition component tasks.
    Task {
        /// Task operation to execute from the current project root.
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Create, validate, or accept component intent documents.
    Component {
        /// Component-document operation to execute.
        #[command(subcommand)]
        command: ComponentCommand,
    },
    /// Manage AI agent profiles, models, and configurations.
    Agent {
        /// Agent setup/management operation to execute.
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Generate shell completion scripts on stdout.
    Completions {
        /// Target shell for completion.
        #[arg(value_name = "SHELL", value_enum)]
        shell: SupportedShell,
    },
}

/// Supported shells for shell completion generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub enum SupportedShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl From<SupportedShell> for clap_complete::Shell {
    fn from(shell: SupportedShell) -> Self {
        match shell {
            SupportedShell::Bash => Self::Bash,
            SupportedShell::Zsh => Self::Zsh,
            SupportedShell::Fish => Self::Fish,
            SupportedShell::Powershell => Self::PowerShell,
        }
    }
}

/// An explicit project directory argument shared by project-scoped commands.
#[derive(Debug, Args)]
pub struct ProjectDirectory {
    /// Project directory; defaults to the current working directory.
    #[arg(value_name = "PROJECT_DIR", default_value = ".")]
    pub path: PathBuf,
}

/// Component-document operations.
#[derive(Debug, Subcommand)]
pub enum ComponentCommand {
    /// Create REQUIREMENTS.md, CONTRACT.md, and DESIGN.md templates.
    New {
        /// Component directory that will contain the generated documents.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
    },
    /// Validate a component's three intent documents.
    Validate {
        /// Component directory containing the documents.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
    },
    /// Record reviewed document and immediate-parent contract revisions.
    Accept {
        /// Component directory containing the intent documents to revalidate.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
    },
}

/// Agent setup operations.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Launch the interactive setup wizard to configure and test new models.
    Setup {
        /// Save a profile even when mandatory live qualification fails.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReasoningEffortArgument {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl From<ReasoningEffortArgument> for agent_runtime::ReasoningEffort {
    fn from(value: ReasoningEffortArgument) -> Self {
        match value {
            ReasoningEffortArgument::None => Self::None,
            ReasoningEffortArgument::Minimal => Self::Minimal,
            ReasoningEffortArgument::Low => Self::Low,
            ReasoningEffortArgument::Medium => Self::Medium,
            ReasoningEffortArgument::High => Self::High,
            ReasoningEffortArgument::Xhigh => Self::Xhigh,
            ReasoningEffortArgument::Max => Self::Max,
        }
    }
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
    /// Unlock a locked component directory.
    Unlock {
        /// Component-root-relative component directory; `.` selects the root component.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: PathBuf,
        /// Force unlocking without prompting for confirmation.
        #[arg(long)]
        force: bool,
    },
}

/// Command-line spelling of a queue task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
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

    fn none() -> Self {
        Self(String::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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
pub fn execute(command: Command, json: bool) -> Result<CommandOutput> {
    if json {
        match command {
            Command::Convert { project_dir } => convert::convert(&project_dir).map(|outcome| {
                let mut project_dir_json = String::new();
                let mut message_json = String::new();
                json_string_escape(&mut project_dir_json, &project_dir.to_string_lossy());
                json_string_escape(&mut message_json, &outcome.to_string());
                CommandOutput::message(format!(
                    r#"{{"status":"success","command":"convert","project_dir":{project_dir_json},"message":{message_json}}}"#
                ))
            }),
            Command::Import {
                repo_url,
                branch,
                component,
                dest_dir,
            } => {
                let outcome = import::import(&repo_url, &branch, component.as_deref(), &dest_dir)?;
                let mut dest_dir_json = String::new();
                let mut message_json = String::new();
                json_string_escape(&mut dest_dir_json, &dest_dir.to_string_lossy());
                json_string_escape(&mut message_json, &outcome.to_string());
                Ok(CommandOutput::message(format!(
                    r#"{{"status":"success","command":"import","dest_dir":{dest_dir_json},"message":{message_json}}}"#
                )))
            },
            Command::ReverseDiscover { path } => {
                let outcome = reverse_discovery::reverse_discover(&path)?;
                let mut path_json = String::new();
                let mut message_json = String::new();
                json_string_escape(&mut path_json, &path.to_string_lossy());
                json_string_escape(&mut message_json, &outcome.to_string());
                Ok(CommandOutput::message(format!(
                    r#"{{"status":"success","command":"reverse-discover","path":{path_json},"message":{message_json}}}"#
                )))
            },
            Command::Prompt {
                prompt,
                file,
                editor,
                role,
                model,
                reasoning_effort,
                idle_timeout,
                detect_loops,
                max_restarts,
                allow_host_execution,
            } => {
                if !allow_host_execution {
                    return Err(agent_runtime::Error::HostExecutionNotAcknowledged.into());
                }
                let resolved_prompt = prompt_input::resolve(prompt, file.as_deref(), editor)?;
                let content = execute_prompt(
                    &resolved_prompt,
                    PromptExecutionOptions {
                        role: &role,
                        model: model.as_deref(),
                        reasoning_effort: reasoning_effort.map(Into::into),
                        idle_timeout,
                        detect_loops,
                        max_restarts,
                        capture: true,
                    },
                )?
                .unwrap_or_default();
                let mut content_json = String::new();
                json_string_escape(&mut content_json, &content);
                Ok(CommandOutput::message(format!(
                    r#"{{"content":{content_json}}}"#
                )))
            },
            Command::Agent {
                command: AgentCommand::Setup { force },
            } => {
                let current_dir = std::env::current_dir().map_err(|source| KvistError::Io {
                    operation: "determine current project directory",
                    path: PathBuf::from("."),
                    source,
                })?;
                let mut reader = std::io::BufReader::new(std::io::stdin());
                let mut writer = std::io::BufWriter::new(std::io::stderr());
                wizard::run_wizard_with_force(
                    &mut reader,
                    &mut writer,
                    &current_dir,
                    force,
                )?;
                Ok(CommandOutput::message(
                    r#"{"status":"success","command":"agent-setup","message":"agent setup wizard complete"}"#.to_owned()
                ))
            },
            Command::Init(project) => {
                let outcome = init::initialize(&project.path)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"init\",\"project_path\":\"{}\",\"message\":\"{}\"}}",
                    project.path.to_string_lossy().replace('\\', "\\\\"),
                    outcome
                        .to_string()
                        .replace('\n', "\\n")
                        .replace('"', "\\\"")
                )))
            }
            Command::Tree(project) => {
                let configuration = config::load(&project.path)?;
                let discovery = discovery::discover_with_limits(
                    &project.path.join(&configuration.component_root),
                    configuration.discovery,
                )?;
                let components = discovery
                    .components
                    .iter()
                    .map(|component| {
                        serde_json::json!({
                            "path": component.relative_path.to_string_lossy(),
                            "state": format!("{:?}", component.status()),
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(CommandOutput::message(
                    serde_json::json!({
                        "status": "success",
                        "command": "tree",
                        "component_root": configuration.component_root.to_string_lossy(),
                        "components": components,
                    })
                    .to_string(),
                ))
            }
            Command::Doctor(project) => {
                let inspection = project_state::inspect(&project.path)?;
                let status_json =
                    status::render(&inspection, status::StatusFormat::Json, false, false, false);
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"doctor\",\"inspection\":{status_json}}}"
                )))
            }
            Command::Status {
                path,
                format: _,
                only_documents,
                only_impls,
                unfinished,
            } => {
                let inspection = project_state::inspect(&path)?;
                Ok(CommandOutput::message(status::render(
                    &inspection,
                    status::StatusFormat::Json,
                    only_documents,
                    only_impls,
                    unfinished,
                )))
            }
            Command::Task {
                command: TaskCommand::Next { component_dir },
            } => {
                let ready_task = task_commands::next(&component_dir)?;
                let ready_task = if ready_task == "no ready task" {
                    "null".to_owned()
                } else {
                    format!("\"{ready_task}\"")
                };
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-next\",\"component_dir\":\"{}\",\"ready_task_id\":{ready_task}}}",
                    component_dir.to_string_lossy().replace('\\', "\\\\"),
                )))
            }
            Command::Task {
                command: TaskCommand::Transition {
                    component_dir,
                    task_id,
                    status,
                    reason,
                },
            } => {
                let message = task_commands::transition(
                    &component_dir,
                    &task_id,
                    status.into(),
                    reason.as_deref(),
                )?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-transition\",\"component_dir\":\"{}\",\"task_id\":\"{}\",\"target_status\":\"{}\",\"message\":\"{}\"}}",
                    component_dir.to_string_lossy().replace('\\', "\\\\"),
                    task_id,
                    task_status_name(status.into()),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command: TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
            } => {
                let message = task_commands::run_task(&component_dir, task_id.as_deref(), stream)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-run\",\"component_dir\":\"{}\",\"message\":\"{}\"}}",
                    component_dir.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command: TaskCommand::Log {
                    component_dir,
                    task_id,
                },
            } => {
                let log_content = task_commands::task_log(&component_dir, &task_id)?;
                let mut escaped_log = String::new();
                json_string_escape(&mut escaped_log, &log_content);
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-log\",\"component_dir\":\"{}\",\"task_id\":\"{}\",\"log_content\":{escaped_log}}}",
                    component_dir.to_string_lossy().replace('\\', "\\\\"),
                    task_id,
                )))
            }
            Command::Task {
                command: TaskCommand::ApprovePolicy { path },
            } => {
                let message = task_commands::approve_policy(&path)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-approve-policy\",\"policy_path\":\"{}\",\"message\":\"{}\"}}",
                    path.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command: TaskCommand::Unlock {
                    component_dir,
                    force,
                },
            } => {
                let message = task_commands::unlock(&component_dir, force)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-unlock\",\"component_dir\":\"{}\",\"message\":\"{}\"}}",
                    component_dir.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Component {
                command: ComponentCommand::New { component_dir },
            } => {
                let generated = component_documents::create(&component_dir)?;
                let paths = generated
                    .paths
                    .iter()
                    .map(|path| {
                        let mut encoded = String::new();
                        json_string_escape(&mut encoded, &path.to_string_lossy());
                        encoded
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"component-new\",\"document_paths\":[{paths}],\"message\":\"created component document templates\"}}"
                )))
            }
            Command::Component {
                command: ComponentCommand::Validate { component_dir },
            } => {
                let validations = validate_component_documents(&component_dir)?;
                let mut diagnostics = Vec::new();
                for (path, validation) in &validations {
                    for diagnostic in &validation.diagnostics {
                        let message = component_documents::format_diagnostics(
                            std::slice::from_ref(diagnostic),
                        );
                        diagnostics.push(serde_json::json!({
                            "document": path,
                            "kind": format!("{:?}", diagnostic.kind),
                            "line": diagnostic.line,
                            "column": diagnostic.column,
                            "message": message,
                        }));
                    }
                }
                let valid = validations
                    .iter()
                    .all(|(_, validation)| validation.is_valid());
                let response = serde_json::json!({
                    "status": if valid { "success" } else { "error" },
                    "command": "component-validate",
                    "valid": valid,
                    "component_dir": component_dir,
                    "diagnostics": diagnostics,
                })
                .to_string();
                if valid {
                    Ok(CommandOutput::message(response))
                } else {
                    Err(KvistError::JsonCommandFailure { output: response })
                }
            }
            Command::Component {
                command: ComponentCommand::Accept { component_dir },
            } => {
                let message = task_commands::accept(&component_dir)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"component-accept\",\"component_dir\":\"{}\",\"message\":\"{}\"}}",
                    component_dir.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Completions { shell } => {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                let mut buffer = Vec::new();
                let generator: clap_complete::Shell = shell.into();
                clap_complete::generate(generator, &mut cmd, "kvist", &mut buffer);
                let script = String::from_utf8(buffer).expect("valid UTF-8 completion script");
                let mut escaped_script = String::new();
                json_string_escape(&mut escaped_script, &script);
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"completions\",\"shell\":\"{shell:?}\",\"script\":{escaped_script}}}"
                )))
            }
        }
    } else {
        match command {
            Command::Convert { project_dir } => convert::convert(&project_dir)
                .map(|outcome| CommandOutput::message(outcome.to_string())),
            Command::Import {
                repo_url,
                branch,
                component,
                dest_dir,
            } => import::import(&repo_url, &branch, component.as_deref(), &dest_dir)
                .map(|outcome| CommandOutput::message(outcome.to_string())),
            Command::ReverseDiscover { path } => reverse_discovery::reverse_discover(&path)
                .map(|outcome| CommandOutput::message(outcome.to_string())),
            Command::Prompt {
                prompt,
                file,
                editor,
                role,
                model,
                reasoning_effort,
                idle_timeout,
                detect_loops,
                max_restarts,
                allow_host_execution,
            } => {
                if !allow_host_execution {
                    return Err(agent_runtime::Error::HostExecutionNotAcknowledged.into());
                }
                let resolved_prompt = prompt_input::resolve(prompt, file.as_deref(), editor)?;
                execute_prompt(
                    &resolved_prompt,
                    PromptExecutionOptions {
                        role: &role,
                        model: model.as_deref(),
                        reasoning_effort: reasoning_effort.map(Into::into),
                        idle_timeout,
                        detect_loops,
                        max_restarts,
                        capture: false,
                    },
                )?;
                Ok(CommandOutput::none())
            }
            Command::Agent {
                command: AgentCommand::Setup { force },
            } => {
                let current_dir = std::env::current_dir().map_err(|source| KvistError::Io {
                    operation: "determine current project directory",
                    path: PathBuf::from("."),
                    source,
                })?;
                let mut reader = std::io::BufReader::new(std::io::stdin());
                let mut writer = std::io::BufWriter::new(std::io::stdout());
                wizard::run_wizard_with_force(&mut reader, &mut writer, &current_dir, force)?;
                Ok(CommandOutput::message(
                    "agent setup wizard complete".to_owned(),
                ))
            }
            Command::Init(project) => init::initialize(&project.path)
                .map(|outcome| CommandOutput::message(outcome.to_string())),
            Command::Tree(project) => {
                tree::render_project(&project.path).map(CommandOutput::message)
            }
            Command::Doctor(project) => project_state::inspect(&project.path)
                .map(|inspection| CommandOutput::message(inspection.to_string())),
            Command::Status {
                path,
                format,
                only_documents,
                only_impls,
                unfinished,
            } => project_state::inspect(&path).map(|inspection| {
                CommandOutput::message(status::render(
                    &inspection,
                    format,
                    only_documents,
                    only_impls,
                    unfinished,
                ))
            }),
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
            } => task_commands::transition(
                &component_dir,
                &task_id,
                status.into(),
                reason.as_deref(),
            )
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
            Command::Task {
                command:
                    TaskCommand::Unlock {
                        component_dir,
                        force,
                    },
            } => task_commands::unlock(&component_dir, force).map(CommandOutput::message),
            Command::Component {
                command: ComponentCommand::New { component_dir },
            } => component_documents::create(&component_dir).map(|generated| {
                CommandOutput::message(format!(
                    "created component documents at {}",
                    generated
                        .paths
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }),
            Command::Component {
                command: ComponentCommand::Validate { component_dir },
            } => {
                let validations = validate_component_documents(&component_dir)?;
                if validations
                    .iter()
                    .all(|(_, validation)| validation.is_valid())
                {
                    Ok(CommandOutput::message(format!(
                        "valid component documents: {}",
                        component_dir.display()
                    )))
                } else {
                    let diagnostics = validations
                        .iter()
                        .filter(|(_, validation)| !validation.is_valid())
                        .map(|(path, validation)| {
                            format!(
                                "{}:\n{}",
                                path.display(),
                                component_documents::format_diagnostics(&validation.diagnostics)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Err(KvistError::ComponentDocumentValidationFailed {
                        path: component_dir,
                        diagnostics,
                    })
                }
            }
            Command::Component {
                command: ComponentCommand::Accept { component_dir },
            } => task_commands::accept(&component_dir).map(CommandOutput::message),
            Command::Completions { shell } => {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                let mut buffer = Vec::new();
                let generator: clap_complete::Shell = shell.into();
                clap_complete::generate(generator, &mut cmd, "kvist", &mut buffer);
                let script = String::from_utf8(buffer).expect("valid UTF-8 completion script");
                Ok(CommandOutput::message(script))
            }
        }
    }
}

fn task_status_name(status: crate::task_queue::TaskStatus) -> &'static str {
    match status {
        crate::task_queue::TaskStatus::Pending => "pending",
        crate::task_queue::TaskStatus::InProgress => "in-progress",
        crate::task_queue::TaskStatus::Blocked => "blocked",
        crate::task_queue::TaskStatus::Completed => "completed",
    }
}

fn json_string_escape(output: &mut String, value: &str) {
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

struct PromptExecutionOptions<'a> {
    role: &'a str,
    model: Option<&'a str>,
    reasoning_effort: Option<agent_runtime::ReasoningEffort>,
    idle_timeout: u64,
    detect_loops: bool,
    max_restarts: u32,
    capture: bool,
}

fn execute_prompt(prompt: &str, options: PromptExecutionOptions<'_>) -> Result<Option<String>> {
    let current_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let config = crate::config::load(&current_dir)?;

    let (profile, role) = match options.role {
        "developer" => (&config.agent.developer, crate::config::Role::Developer),
        "architect" => (&config.agent.architect, crate::config::Role::Architect),
        "security-reviewer" | "security_reviewer" => (
            &config.agent.security_reviewer,
            crate::config::Role::SecurityReviewer,
        ),
        _ => {
            return Err(KvistError::ImportFailed {
                reason: format!("unknown role profile: {}", options.role),
            });
        }
    };

    let policy = agent_runtime::SupervisionPolicy {
        idle_timeout: std::time::Duration::from_secs(options.idle_timeout),
        attempt_timeout: None,
        detect_loops: options.detect_loops,
        max_retries: options.max_restarts,
        max_output_bytes: profile.max_output_bytes,
    };
    let command_for_attempt = |context: &agent_runtime::AttemptContext| {
        let prompt = match context.retry_notice() {
            Some(notice) => format!("{prompt}\n\n{notice}"),
            None => prompt.to_owned(),
        };
        let (program, arguments) = crate::agent::get_effective_command_with_options(
            profile,
            role,
            options.model,
            options.reasoning_effort,
            &prompt,
            &[],
            &current_dir,
        )
        .map_err(|error| agent_runtime::Error::InvalidCommandTemplate {
            reason: error.to_string(),
        })?;
        Ok(agent_runtime::CommandSpec::new(program, arguments).in_directory(current_dir.clone()))
    };

    if options.capture {
        let report = agent_runtime::run_supervised_capture(&policy, command_for_attempt)?;
        Ok(Some(String::from_utf8_lossy(&report.stdout).into_owned()))
    } else {
        if std::io::stderr().is_terminal() {
            eprintln!("Prompt:\n{prompt}\n\nResponse:");
        }
        agent_runtime::run_supervised(&policy, command_for_attempt)?;
        Ok(None)
    }
}

fn validate_component_documents(
    component_dir: &std::path::Path,
) -> Result<Vec<(PathBuf, component_documents::DocumentValidation)>> {
    [
        component_documents::DocumentKind::Requirements,
        component_documents::DocumentKind::Contract,
        component_documents::DocumentKind::Design,
    ]
    .into_iter()
    .map(|kind| {
        let path = component_dir.join(kind.filename());
        let validation = component_documents::validate_file(kind, &path)?;
        Ok((path, validation))
    })
    .collect()
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
    fn parses_component_creation() {
        let cli = Cli::try_parse_from(["kvist", "component", "new", "src/network"])
            .expect("valid component creation command");

        let Command::Component {
            command: ComponentCommand::New { component_dir },
        } = cli.command
        else {
            panic!("expected component new command");
        };

        assert_eq!(component_dir, PathBuf::from("src/network"));
    }

    #[test]
    fn parses_component_validation() {
        let cli = Cli::try_parse_from(["kvist", "component", "validate", "src/network"])
            .expect("valid component validation command");

        let Command::Component {
            command: ComponentCommand::Validate { component_dir },
        } = cli.command
        else {
            panic!("expected component validate command");
        };

        assert_eq!(component_dir, PathBuf::from("src/network"));
    }

    #[test]
    fn parses_convert_command() {
        let cli = Cli::try_parse_from(["kvist", "convert", "/path/to/project"])
            .expect("valid convert command");

        let Command::Convert { project_dir } = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(project_dir, PathBuf::from("/path/to/project"));
    }

    #[test]
    fn parses_task_next_command() {
        let cli =
            Cli::try_parse_from(["kvist", "task", "next", "src"]).expect("valid task next command");

        let Command::Task {
            command: TaskCommand::Next { component_dir },
        } = cli.command
        else {
            panic!("expected task next command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
    }

    #[test]
    fn parses_task_transition_command() {
        let cli = Cli::try_parse_from([
            "kvist",
            "task",
            "transition",
            "src",
            "task-1",
            "in-progress",
        ])
        .expect("valid task transition command");

        let Command::Task {
            command:
                TaskCommand::Transition {
                    component_dir,
                    task_id,
                    status,
                    reason,
                },
        } = cli.command
        else {
            panic!("expected task transition command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
        assert_eq!(task_id, "task-1");
        assert_eq!(status, TaskStatusArgument::InProgress);
        assert_eq!(reason, None);
    }

    #[test]
    fn parses_task_transition_command_with_reason() {
        let cli = Cli::try_parse_from([
            "kvist",
            "task",
            "transition",
            "src",
            "task-1",
            "blocked",
            "--reason",
            "waiting on PR",
        ])
        .expect("valid task transition with reason command");

        let Command::Task {
            command:
                TaskCommand::Transition {
                    component_dir,
                    task_id,
                    status,
                    reason,
                },
        } = cli.command
        else {
            panic!("expected task transition command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
        assert_eq!(task_id, "task-1");
        assert_eq!(status, TaskStatusArgument::Blocked);
        assert_eq!(reason, Some("waiting on PR".to_string()));
    }

    #[test]
    fn parses_task_run_command() {
        let cli =
            Cli::try_parse_from(["kvist", "task", "run", "src"]).expect("valid task run command");

        let Command::Task {
            command:
                TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
        } = cli.command
        else {
            panic!("expected task run command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
        assert_eq!(task_id, None);
        assert!(!stream);
    }

    #[test]
    fn parses_task_run_command_with_task_id() {
        let cli = Cli::try_parse_from(["kvist", "task", "run", "src", "task-1", "--stream"])
            .expect("valid task run command with task id");

        let Command::Task {
            command:
                TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
        } = cli.command
        else {
            panic!("expected task run command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
        assert_eq!(task_id, Some("task-1".to_string()));
        assert!(stream);
    }

    #[test]
    fn parses_task_log_command() {
        let cli = Cli::try_parse_from(["kvist", "task", "log", "src", "task-1"])
            .expect("valid task log command");

        let Command::Task {
            command:
                TaskCommand::Log {
                    component_dir,
                    task_id,
                },
        } = cli.command
        else {
            panic!("expected task log command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
        assert_eq!(task_id, "task-1".to_string());
    }

    #[test]
    fn parses_task_approve_policy_command() {
        let cli = Cli::try_parse_from(["kvist", "task", "approve-policy", "/path/to/project"])
            .expect("valid task approve-policy command");

        let Command::Task {
            command: TaskCommand::ApprovePolicy { path },
        } = cli.command
        else {
            panic!("expected task approve-policy command");
        };

        assert_eq!(path, PathBuf::from("/path/to/project"));
    }

    #[test]
    fn parses_task_unlock_command() {
        let cli = Cli::try_parse_from(["kvist", "task", "unlock", "src", "--force"])
            .expect("valid task unlock command");

        let Command::Task {
            command:
                TaskCommand::Unlock {
                    component_dir,
                    force,
                },
        } = cli.command
        else {
            panic!("expected task unlock command");
        };

        assert_eq!(component_dir, PathBuf::from("src"));
        assert!(force);
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
        assert!(
            help.contains("convert"),
            "help should mention convert command"
        );
        assert!(help.contains("init"), "help should mention init command");
        assert!(help.contains("tree"), "help should mention tree command");
        assert!(
            help.contains("doctor"),
            "help should mention doctor command"
        );
        assert!(
            help.contains("status"),
            "help should mention status command"
        );
        assert!(help.contains("spec"), "help should mention spec command");
        assert!(help.contains("task"), "help should mention task command");
        assert!(
            help.contains("completions"),
            "help should mention completions command"
        );
    }

    #[test]
    fn unknown_commands_are_rejected_by_the_parser() {
        let error = Cli::try_parse_from(["kvist", "unknown"]).expect_err("invalid command");

        assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn parses_completions() {
        let cli = Cli::try_parse_from(["kvist", "completions", "bash"])
            .expect("valid completions command");

        let Command::Completions { shell } = cli.command else {
            panic!("expected completions command");
        };

        assert_eq!(shell, SupportedShell::Bash);
    }

    #[test]
    fn generates_bash_completions() {
        let outcome = execute(
            Command::Completions {
                shell: SupportedShell::Bash,
            },
            false,
        )
        .expect("generates completions");

        let output = outcome.to_string();
        assert!(output.contains("_kvist") || output.contains("kvist"));
    }

    #[test]
    fn generates_json_output() {
        let project = tempfile::TempDir::new().expect("create project");
        let outcome = execute(
            Command::Init(ProjectDirectory {
                path: project.path().to_path_buf(),
            }),
            true,
        )
        .expect("generates JSON output");

        let output = outcome.to_string();
        assert!(output.contains(r#""command":"init""#));
        let expected_path = project.path().to_string_lossy().replace('\\', "\\\\");
        assert!(output.contains(&format!(r#""project_path":"{expected_path}""#)));
    }

    #[test]
    fn task_status_argument_from_impl_works() {
        let pending: crate::task_queue::TaskStatus = TaskStatusArgument::Pending.into();
        let in_progress: crate::task_queue::TaskStatus = TaskStatusArgument::InProgress.into();
        let blocked: crate::task_queue::TaskStatus = TaskStatusArgument::Blocked.into();
        let completed: crate::task_queue::TaskStatus = TaskStatusArgument::Completed.into();

        assert_eq!(pending, crate::task_queue::TaskStatus::Pending);
        assert_eq!(in_progress, crate::task_queue::TaskStatus::InProgress);
        assert_eq!(blocked, crate::task_queue::TaskStatus::Blocked);
        assert_eq!(completed, crate::task_queue::TaskStatus::Completed);
    }

    #[test]
    fn json_string_escape_handles_special_characters() {
        let mut output = String::new();
        json_string_escape(
            &mut output,
            r#"He said "Hello\nWorld"\t#);
        assert_eq!(output, r#""He said \"Hello\nWorld\"\t"#,
        );
    }
}
