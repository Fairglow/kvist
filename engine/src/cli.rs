//! Command-line contract and dispatch for Kvist.

use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    KvistError, Result, component_documents, config, context, convert, discovery, import, init,
    project_state, prompt_input, reverse_discovery, status, task_commands, task_queue::TaskStatus,
    toolchain, tree, vendor_command, wizard,
};

/// Kvist's top-level command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "kvist",
    version,
    about = "Spec-driven architecture workflow for human-directed AI development",
    long_about = "\
Kvist manages filesystem-native requirements, contracts, designs, task queues, \
and compliance evidence.\n\
\
Typical flow:\n\
  1. kvist shell                start the interactive workspace shell (recommended)\n\
  2. kvist status               where does the project stand, and what is next?\n\
  3. kvist task run             execute the next ready task with a supervised agent\n\
  4. kvist task log / finalize  inspect the run and record your decision\n\
  5. kvist component accept     accept changed intent documents when they go stale\n\
  6. kvist tree / doctor        browse the component tree / check project health\n\
\nCommands work from the directory you are standing in: the project root is \
found by walking upward to the nearest kvist.toml, and component commands act \
on the component containing the current directory unless you name one \
explicitly. Run `kvist <command> --help` for details."
)]
pub struct Cli {
    /// Output structured JSON instead of plain text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Command to execute. Omit it inside a project for a status overview,
    /// outside a project for this help.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Commands that form Kvist's public CLI contract.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the interactive workspace shell (the recommended way to work).
    Shell {
        /// Project directory; defaults to the nearest Kvist project at or
        /// above the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        path: Option<PathBuf>,
    },
    /// Initialize a project with Kvist's root artifacts.
    Init {
        /// Project directory; defaults to the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        path: Option<PathBuf>,
    },
    /// Convert an existing Rust project into a Kvist-managed component.
    Convert {
        /// Existing Rust project directory.
        #[arg(value_name = "PROJECT_DIR")]
        project_dir: PathBuf,
    },
    /// Populate and enforce offline vendored dependencies so Rust builds and
    /// tests run with the sandbox network denied (ADR-0011).
    Vendor {
        /// Project directory containing `Cargo.lock`; defaults to the nearest
        /// Kvist project at or above the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        project_dir: Option<PathBuf>,
        /// Use this directory as the vendored registry instead of the default
        /// `<project>/.kvist/vendored`.
        #[arg(long, value_name = "PATH")]
        vendored_dir: Option<PathBuf>,
    },
    /// Provision the project's pinned Rust toolchain on the host, outside the
    /// sandbox (ADR-0012).
    Toolchain {
        /// Project directory containing `rust-toolchain.toml` when pinned;
        /// defaults to the nearest Kvist project at or above the current
        /// directory.
        #[arg(value_name = "PROJECT_DIR")]
        project_dir: Option<PathBuf>,
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
    Tree {
        /// Project directory; defaults to the nearest Kvist project at or
        /// above the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        path: Option<PathBuf>,
    },
    /// Inspect root artifacts without changing the project.
    Doctor {
        /// Project directory; defaults to the nearest Kvist project at or
        /// above the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        path: Option<PathBuf>,
    },
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
        /// Run the agent on the host, bypassing the Bubblewrap sandbox. Interactive
        /// work is sandboxed (protected) by default; with this flag agent-runner
        /// runs with host privileges and is single-turn unless --multi-turn allows
        /// more turns. Never the default.
        #[arg(long)]
        allow_host_execution: bool,
        /// Allow the agent to work across multiple model turns. Meaningful only with
        /// --allow-host-execution (host execution is single-turn by default);
        /// sandboxed interactive work is multi-turn by default.
        #[arg(long)]
        multi_turn: bool,
    },
    /// Answer "where do I stand?": project and component states, task
    /// progress, and the exact next action for anything that needs it.
    /// Defaults to the human-friendly report; `--format text|json` gives
    /// the stable report forms for scripts.
    Status {
        /// Project directory; defaults to the nearest Kvist project at or
        /// above the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        path: Option<PathBuf>,
        /// Report representation: human overview (default), stable text, or JSON.
        #[arg(long, value_enum, default_value_t = status::StatusFormat::Overview)]
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
    /// Manage AI agent model profiles and role assignments.
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
    /// Apply one staged authoring effect inside the effect sandbox.
    ///
    /// Internal effect-applier entry point, invoked by the supervised effect
    /// loop from inside the sandbox against a read-only staged-intent mount.
    /// It is hidden from help because it is not part of the interactive CLI.
    #[command(hide = true)]
    AuthoringApply {
        /// Component directory the effect destination is relative to.
        #[arg(long, value_name = "COMPONENT_DIR")]
        component: PathBuf,
        /// Read-only staged intent file to apply.
        #[arg(long, value_name = "INTENT_FILE")]
        intent_file: PathBuf,
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

/// Component intent-document operations.
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
        /// Component directory containing the documents; defaults to the
        /// component containing the current directory.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: Option<PathBuf>,
    },
    /// Accept the component's intent documents: structurally validate them and
    /// record their revisions in TODOS.yaml as the new baseline.
    Accept {
        /// Component directory containing the intent documents; defaults to
        /// the component containing the current directory.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: Option<PathBuf>,
        /// Create a local Git commit containing exactly the accepted changes.
        #[arg(long)]
        commit: bool,
        /// Explicit commit message when --commit is used.
        #[arg(long)]
        message: Option<String>,
    },
    /// (Re)perform the isolated index commit for a pending acceptance.
    ///
    /// Use this when `component accept --commit` recorded the acceptance but
    /// the Git commit could not be created; the pending state is reported in
    /// that failure message.
    Commit {
        /// Unique identifier of the pending acceptance state.
        #[arg(value_name = "ACCEPTANCE_ID")]
        acceptance_id: String,
    },
}

/// Explicit human disposition for a task attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum FinalizeDispositionArgument {
    /// Accepted attempt; the `Default` placeholder for parsed forms.
    #[default]
    Accept,
    Block,
}

/// Role assignment operations for model profiles.
#[derive(Debug, Subcommand)]
pub enum AgentRoleCommand {
    /// List current role assignments and available model profiles.
    List,
    /// Assign a model profile to a role.
    Set {
        /// Role to assign: developer, architect, or security-reviewer.
        #[arg(value_name = "ROLE")]
        role: String,
        /// Name of the model profile to assign.
        #[arg(value_name = "MODEL_NAME")]
        model: String,
        /// Optional thinking effort for this role: none, minimal, low, medium, high, xhigh, max.
        #[arg(long, value_name = "EFFORT")]
        thinking_effort: Option<String>,
        /// Update global user configuration instead of project configuration.
        #[arg(long)]
        global: bool,
    },
    /// Clear role assignment(s).
    Clear {
        /// Role to clear: developer, architect, or security-reviewer.
        #[arg(value_name = "ROLE", required_unless_present = "all")]
        role: Option<String>,
        /// Clear assignments for all roles.
        #[arg(long)]
        all: bool,
        /// Update global user configuration instead of project configuration.
        #[arg(long)]
        global: bool,
    },
}

/// Agent setup, profile management, and role assignment operations.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Discover, qualify, and save a new model profile (interactive wizard).
    Setup {
        /// Save a profile even when mandatory live qualification fails.
        #[arg(long)]
        force: bool,
    },
    /// List configured model profiles and their role assignments.
    List,
    /// Remove configured model profile(s).
    Remove {
        /// Name of the model profile to remove.
        #[arg(value_name = "MODEL_NAME", required_unless_present = "all")]
        name: Option<String>,
        /// Remove all configured model profiles and reset agent configuration.
        #[arg(long)]
        all: bool,
        /// Remove from global user configuration instead of project configuration.
        #[arg(long)]
        global: bool,
    },
    /// Live-verify that configured model profiles are reachable and working.
    Check {
        /// Check profiles in the global user configuration instead of the project configuration.
        #[arg(long)]
        global: bool,
    },
    /// Manage role bindings to model profiles (list, set, clear).
    Role {
        #[command(subcommand)]
        command: Option<AgentRoleCommand>,
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
        /// Component directory; defaults to the component containing the
        /// current directory.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: Option<PathBuf>,
    },
    /// Persist one legal task status transition and audit attempt.
    ///
    /// Form: `kvist task transition [COMPONENT_DIR] TASK_ID STATUS` where
    /// COMPONENT_DIR defaults to the component containing the current
    /// directory.
    Transition {
        /// Optional component directory, then TASK_ID and STATUS.
        #[arg(value_name = "ARG", num_args = 0..=3)]
        args: Vec<String>,
        /// Resolved component directory (filled during normalization).
        #[arg(skip)]
        component_dir: Option<PathBuf>,
        /// Task ID (the first trailing positional argument).
        #[arg(skip)]
        task_id: String,
        /// Requested durable task status.
        #[arg(skip)]
        status: TaskStatusArgument,
        /// Required nonblank blocker explanation only when STATUS is `blocked`.
        #[arg(long)]
        reason: Option<String>,
        #[doc(hidden)]
        #[arg(skip)]
        _unparsed: std::marker::PhantomData<()>,
    },
    /// Run an external AI agent to execute a task, tracking progress and token usage.
    Run {
        /// Component directory; defaults to the component containing the
        /// current directory.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: Option<PathBuf>,
        /// Queue-local task identifier; when omitted, the next ready task is
        /// suggested and run only after an interactive confirmation (a
        /// non-interactive context fails instead of auto-executing).
        #[arg(value_name = "TASK_ID")]
        task_id: Option<String>,
        /// Optional flag to stream agent stdout and stderr directly to the console.
        #[arg(long)]
        stream: bool,
    },
    /// View the raw execution log of an agent task attempt.
    ///
    /// Form: `kvist task log [COMPONENT_DIR] TASK_ID` where COMPONENT_DIR
    /// defaults to the component containing the current directory.
    Log {
        /// Optional component directory, then TASK_ID.
        #[arg(value_name = "ARG", num_args = 0..=2)]
        args: Vec<String>,
        /// Resolved component directory (filled during normalization).
        #[arg(skip)]
        component_dir: Option<PathBuf>,
        /// Queue-local task identifier.
        #[arg(skip)]
        task_id: String,
        #[doc(hidden)]
        #[arg(skip)]
        _unparsed: std::marker::PhantomData<()>,
    },
    /// Replay an agent execution session from a structured JSONL journal file.
    Replay {
        /// Path to the session JSONL journal file.
        #[arg(value_name = "SESSION_JSONL")]
        session_file: PathBuf,
        /// Maximum turn to step through (optional).
        #[arg(long)]
        max_turns: Option<usize>,
    },
    /// Approve the current test-command policy.
    ApprovePolicy {
        /// Project directory; defaults to the nearest Kvist project at or
        /// above the current directory.
        #[arg(value_name = "PROJECT_DIR")]
        path: Option<PathBuf>,
    },
    /// Unlock a locked component directory.
    Unlock {
        /// Component directory; defaults to the component containing the
        /// current directory.
        #[arg(value_name = "COMPONENT_DIR")]
        component_dir: Option<PathBuf>,
        /// Force unlocking without prompting for confirmation.
        #[arg(long)]
        force: bool,
    },
    /// Reconcile one fenced attempt only when durable host evidence proves no execution began.
    ///
    /// Form: `kvist task recover [COMPONENT_DIR] TASK_ID ATTEMPT_ID
    /// --disposition execution-did-not-start` where COMPONENT_DIR defaults to
    /// the component containing the current directory.
    Recover {
        /// Optional component directory, then TASK_ID and ATTEMPT_ID.
        #[arg(value_name = "ARG", num_args = 0..=3)]
        args: Vec<String>,
        /// Resolved component directory (filled during normalization).
        #[arg(skip)]
        component_dir: Option<PathBuf>,
        /// Queue-local task identifier.
        #[arg(skip)]
        task_id: String,
        /// Stable identity of the fenced attempt.
        #[arg(skip)]
        attempt_id: String,
        /// Explicit human disposition for the exact fenced attempt.
        #[arg(long, value_enum)]
        disposition: RecoveryDispositionArgument,
        #[doc(hidden)]
        #[arg(skip)]
        _unparsed: std::marker::PhantomData<()>,
    },
    /// Finalize a completed task attempt with explicit human disposition.
    ///
    /// Form: `kvist task finalize [COMPONENT_DIR] TASK_ID ATTEMPT_ID
    /// accept|block` where COMPONENT_DIR defaults to the component containing
    /// the current directory.
    Finalize {
        /// Optional component directory, then TASK_ID, ATTEMPT_ID, and
        /// DISPOSITION.
        #[arg(value_name = "ARG", num_args = 0..=4)]
        args: Vec<String>,
        /// Resolved component directory (filled during normalization).
        #[arg(skip)]
        component_dir: Option<PathBuf>,
        /// Queue-local task identifier.
        #[arg(skip)]
        task_id: String,
        /// Stable identity of the completed attempt.
        #[arg(skip)]
        attempt_id: String,
        /// Explicit human disposition for the exact completed attempt.
        #[arg(skip)]
        disposition: FinalizeDispositionArgument,
        /// Create a local Git commit containing exactly the accepted changes.
        #[arg(long)]
        commit: bool,
        /// Required nonblank blocker explanation only when DISPOSITION is `block`.
        #[arg(long)]
        reason: Option<String>,
        #[doc(hidden)]
        #[arg(skip)]
        _unparsed: std::marker::PhantomData<()>,
    },
}

/// The limited recovery disposition supported by the pre-spawn recovery tier.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RecoveryDispositionArgument {
    ExecutionDidNotStart,
}

/// Command-line spelling of a queue task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum TaskStatusArgument {
    /// Initial queue state; the `Default` placeholder for parsed forms.
    #[default]
    Pending,
    InProgress,
    Blocked,
    AwaitingDecision,
    Completed,
}

impl From<TaskStatusArgument> for TaskStatus {
    fn from(status: TaskStatusArgument) -> Self {
        match status {
            TaskStatusArgument::Pending => Self::Pending,
            TaskStatusArgument::InProgress => Self::InProgress,
            TaskStatusArgument::Blocked => Self::Blocked,
            TaskStatusArgument::AwaitingDecision => Self::AwaitingDecision,
            TaskStatusArgument::Completed => Self::Completed,
        }
    }
}

/// Successful command output written by the binary at the process boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput(String);

impl CommandOutput {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn none() -> Self {
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
pub fn execute(command: Option<Command>, json: bool) -> Result<CommandOutput> {
    let Some(command) = command else {
        return bare_invocation(json);
    };
    // The in-sandbox effect applier is internal plumbing, not a user-facing
    // command: dispatch it before any presentation handling so it always
    // returns the applier's bounded result verbatim.
    if let Command::AuthoringApply {
        component,
        intent_file,
    } = &command
    {
        return crate::authoring::apply::apply_intent(component, intent_file)
            .map(CommandOutput::message);
    }
    let command = normalize_task_positionals(command)?;
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
            Command::Vendor {
                project_dir,
                vendored_dir,
            } => {
                let project = context::ProjectContext::resolve(project_dir.as_deref())?;
                vendor_command::vendor_project(
                    &project.project_dir,
                    vendor_command::VendorOptions {
                        vendored_dir,
                        populate: true,
                    },
                )
                .map(|report| {
                    let mut project_dir_json = String::new();
                    let mut vendored_dir_json = String::new();
                    let mut message_json = String::new();
                    json_string_escape(&mut project_dir_json, &project.project_dir.to_string_lossy());
                json_string_escape(&mut vendored_dir_json, report.vendored_dir());
                json_string_escape(&mut message_json, &report.to_string());
                CommandOutput::message(format!(
                    r#"{{"status":"success","command":"vendor","project_dir":{project_dir_json},"vendored_dir":{vendored_dir_json},"message":{message_json}}}"#
                ))
            })
        },
        Command::Toolchain { project_dir } => {
            let project = context::ProjectContext::resolve(project_dir.as_deref())?;
            toolchain::ensure_toolchain(&project.project_dir, "toolchain").map(|manifest| {
                let mut project_dir_json = String::new();
                let mut channel_json = String::new();
                let mut toolchain_root_json = String::new();
                json_string_escape(&mut project_dir_json, &project.project_dir.to_string_lossy());
                json_string_escape(&mut channel_json, &manifest.channel);
                json_string_escape(&mut toolchain_root_json, &manifest.toolchain_root);
                CommandOutput::message(format!(
                    r#"{{"status":"success","command":"toolchain","project_dir":{project_dir_json},"channel":{channel_json},"toolchain_root":{toolchain_root_json}}}"#
                ))
            })
        }
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
                multi_turn,
            } => {
                let resolved_prompt = prompt_input::resolve(prompt, file.as_deref(), editor)?;

                // Interactive custom prompt work runs in the standalone
                // agent-runner shell. Kvist authors the prompt and preselects the
                // model/effort; agent-runner owns the interactive transcript and the
                // execution scope, and its stdio is inherited so the shell is the
                // view. By default the sandbox confines and multiplies the work; the
                // one-shot host path below only applies when there is no interactive
                // terminal or host execution was not acknowledged.
                if delegate_interactive_prompt(
                    &resolved_prompt,
                    &role,
                    model.as_deref(),
                    reasoning_effort.map(Into::into),
                    allow_host_execution,
                    multi_turn,
                )? {
                    return Ok(CommandOutput::none());
                }

                if !allow_host_execution {
                    return Err(agent_runtime::Error::HostExecutionNotAcknowledged.into());
                }
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
                command: AgentCommand::Role { command },
            } => {
                let (current_dir, fallback_global) = agent_scope()?;
                match command.unwrap_or(AgentRoleCommand::List) {
                    AgentRoleCommand::List => {
                        let text = wizard::list_roles(&current_dir)?;
                        let mut text_json = String::new();
                        json_string_escape(&mut text_json, &text);
                        Ok(CommandOutput::message(format!(
                            r#"{{"status":"success","command":"agent-role-list","output":{text_json}}}"#
                        )))
                    }
                    AgentRoleCommand::Set {
                        role,
                        model,
                        thinking_effort,
                        global,
                    } => {
                        let global = global || fallback_global;
                        let config_path = agent_config_path(&current_dir, global)?;
                        let effort = match thinking_effort {
                            Some(s) => Some(agent_runtime::ReasoningEffort::parse_effort(&s).ok_or_else(|| {
                                KvistError::AgentSetupFailed {
                                    reason: format!("invalid reasoning effort `{s}`: expected none, minimal, low, medium, high, xhigh, or max"),
                                }
                            })?),
                            None => None,
                        };
                        let msg = wizard::set_role_model_with_effort(&config_path, &current_dir, !global, &role, &model, effort)?;
                        let mut msg_json = String::new();
                        json_string_escape(&mut msg_json, &msg);
                        Ok(CommandOutput::message(format!(
                            r#"{{"status":"success","command":"agent-role-set","message":{msg_json}}}"#
                        )))
                    }
                    AgentRoleCommand::Clear { role, all, global } => {
                        let global = global || fallback_global;
                        let config_path = agent_config_path(&current_dir, global)?;
                        let msg = wizard::clear_role(&config_path, &current_dir, !global, role.as_deref(), all)?;
                        let mut msg_json = String::new();
                        json_string_escape(&mut msg_json, &msg);
                        Ok(CommandOutput::message(format!(
                            r#"{{"status":"success","command":"agent-role-clear","message":{msg_json}}}"#
                        )))
                    }
                }
            }
            Command::Agent {
                command: AgentCommand::Setup { force },
            } => {
                let (current_dir, _) = agent_scope()?;
                let mut reader = crate::interruptible_stdin::interruptible_reader();
                let mut writer = std::io::BufWriter::new(std::io::stderr());
                match wizard::run_wizard_with_force(
                    &mut reader,
                    &mut writer,
                    &current_dir,
                    force,
                ) {
                    Ok(()) => Ok(CommandOutput::message(
                        r#"{"status":"success","command":"agent-setup","message":"agent setup wizard complete"}"#.to_owned()
                    )),
                    Err(KvistError::AgentSetupCancelled) => Ok(CommandOutput::message(
                        r#"{"status":"cancelled","command":"agent-setup","message":"agent setup cancelled"}"#.to_owned()
                    )),
                    Err(source) => Err(source),
                }
            },
            Command::Agent {
                command: AgentCommand::List,
            } => {
                let (current_dir, _) = agent_scope()?;
                let text = wizard::list_models(&current_dir)?;
                let mut text_json = String::new();
                json_string_escape(&mut text_json, &text);
                Ok(CommandOutput::message(format!(
                    r#"{{"status":"success","command":"agent-list","output":{text_json}}}"#
                )))
            }
            Command::Agent {
                command: AgentCommand::Remove { name, all, global },
            } => {
                let (current_dir, fallback_global) = agent_scope()?;
                let global = global || fallback_global;
                let config_path = agent_config_path(&current_dir, global)?;
                let msg = if all {
                    wizard::remove_all_models(&config_path, &current_dir, !global)?
                } else if let Some(model_name) = name {
                    wizard::remove_model(&config_path, &current_dir, !global, &model_name)?
                } else {
                    return Err(KvistError::AgentSetupFailed {
                        reason: "specify a model name or use --all to clear all agent configuration"
                            .to_owned(),
                    });
                };
                let mut msg_json = String::new();
                json_string_escape(&mut msg_json, &msg);
                Ok(CommandOutput::message(format!(
                    r#"{{"status":"success","command":"agent-remove","message":{msg_json}}}"#
                )))
            },
            Command::Agent { command: AgentCommand::Check { global } } => {
                let (current_dir, fallback_global) = agent_scope()?;
                let global = global || fallback_global;
                let mut reader = crate::interruptible_stdin::interruptible_reader();
                let mut writer = std::io::BufWriter::new(std::io::stderr());
                match wizard::check_agents(&mut reader, &mut writer, &current_dir, global) {
                    Ok(()) => Ok(CommandOutput::message(
                        r#"{"status":"success","command":"agent-check","message":"agent check complete"}"#.to_owned()
                    )),
                    Err(KvistError::AgentSetupCancelled) => Ok(CommandOutput::message(
                        r#"{"status":"cancelled","command":"agent-check","message":"agent check cancelled"}"#.to_owned()
                    )),
                    Err(source) => Err(source),
                }
            },
            Command::Shell { .. } => Err(KvistError::SandboxUnavailable {
                runner: "shell".to_owned(),
                reason: "interactive shell is not supported in JSON mode".to_owned(),
            }),
            Command::Init { path } => {
                let dir = path.unwrap_or_else(|| PathBuf::from("."));
                let outcome = init::initialize(&dir)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"init\",\"project_path\":\"{}\",\"message\":\"{}\"}}",
                    dir.to_string_lossy().replace('\\', "\\\\"),
                    outcome
                        .to_string()
                        .replace('\n', "\\n")
                        .replace('"', "\\\"")
                )))
            }
            Command::Tree { path } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                let configuration = config::load(&project.project_dir)?;
                let discovery = discovery::discover_with_limits(
                    &project.project_dir.join(&configuration.component_root),
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
            Command::Doctor { path } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                let inspection = project_state::inspect(&project.project_dir)?;
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
                let project = context::ProjectContext::resolve(path.as_deref())?;
                let inspection = project_state::inspect(&project.project_dir)?;
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
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let ready_task = task_commands::next(&project.project_dir, &component)?;
                let ready_task = if ready_task == "no ready task" {
                    "null".to_owned()
                } else {
                    format!("\"{ready_task}\"")
                };
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-next\",\"component_dir\":\"{}\",\"ready_task_id\":{ready_task}}}",
                    component.to_string_lossy().replace('\\', "\\\\"),
                )))
            }
            Command::Task {
                command: TaskCommand::Transition {
                    component_dir,
                    task_id,
                    status,
                    reason,
                    ..
                },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let message = task_commands::transition(
                    &project.project_dir,
                    &component,
                    &task_id,
                    status.into(),
                    reason.as_deref(),
                )?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-transition\",\"component_dir\":\"{}\",\"task_id\":\"{}\",\"target_status\":\"{}\",\"message\":\"{}\"}}",
                    component.to_string_lossy().replace('\\', "\\\\"),
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
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let message = task_commands::run_task_or_item(
                    &project.project_dir,
                    &component,
                    task_id.as_deref(),
                    stream,
                )?;
                let task_id_field = match task_id.as_deref() {
                    Some(id) => format!("\"{id}\""),
                    None => "null".to_owned(),
                };
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-run\",\"component_dir\":\"{}\",\"task_id\":{task_id_field},\"message\":\"{}\"}}",
                    component.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command: TaskCommand::Log {
                    component_dir,
                    task_id,
                    ..
                },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let log_content = task_commands::task_log(&project.project_dir, &component, &task_id)?;
                let mut escaped_log = String::new();
                json_string_escape(&mut escaped_log, &log_content);
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-log\",\"component_dir\":\"{}\",\"task_id\":\"{}\",\"log_content\":{escaped_log}}}",
                    component.to_string_lossy().replace('\\', "\\\\"),
                    task_id,
                )))
            }
            Command::Task {
                command: TaskCommand::Replay {
                    session_file,
                    max_turns,
                },
            } => {
                let replay_content =
                    task_commands::replay_task_session(&session_file, max_turns)?;
                let mut escaped_content = String::new();
                json_string_escape(&mut escaped_content, &replay_content);
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-replay\",\"session_file\":\"{}\",\"replay\":{escaped_content}}}",
                    session_file.to_string_lossy().replace('\\', "\\\\"),
                )))
            }
            Command::Task {
                command: TaskCommand::ApprovePolicy { path },
            } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                let message = task_commands::approve_policy(&project.project_dir)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-approve-policy\",\"policy_path\":\"{}\",\"message\":\"{}\"}}",
                    project.project_dir.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command: TaskCommand::Unlock {
                    component_dir,
                    force,
                },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let message = task_commands::unlock(&project.project_dir, &component, force)?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-unlock\",\"component_dir\":\"{}\",\"message\":\"{}\"}}",
                    component.to_string_lossy().replace('\\', "\\\\"),
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command:
                    TaskCommand::Recover {
                        component_dir,
                        task_id,
                        attempt_id,
                        disposition,
                        ..
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let message = task_commands::recover(
                    &project.project_dir,
                    &component,
                    &task_id,
                    &attempt_id,
                    disposition,
                )?;
                Ok(CommandOutput::message(format!(
                    "{{\"status\":\"success\",\"command\":\"task-recover\",\"component_dir\":\"{}\",\"task_id\":\"{}\",\"attempt_id\":\"{}\",\"message\":\"{}\"}}",
                    component.to_string_lossy().replace('\\', "\\\\"),
                    task_id,
                    attempt_id,
                    message.replace('\n', "\\n").replace('"', "\\\"")
                )))
            }
            Command::Task {
                command:
                    TaskCommand::Finalize {
                        component_dir,
                        task_id,
                        attempt_id,
                        disposition,
                        commit,
                        reason,
                        ..
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let response = task_commands::finalize(
                    &project.project_dir,
                    &component,
                    &task_id,
                    &attempt_id,
                    disposition,
                    commit,
                    reason.as_deref(),
                    true,
                )?;
                Ok(CommandOutput::message(response))
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
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let component_path = project.component_path(&component)?;
                let validations = validate_component_documents(&component_path)?;
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
                    "component_dir": component,
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
                command:
                    ComponentCommand::Accept {
                        component_dir,
                        commit,
                        message,
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let response =
                    task_commands::accept(&project.project_dir, &component, commit, message.as_deref(), true)?;
                Ok(CommandOutput::message(response))
            }
            Command::Component {
                command: ComponentCommand::Commit { acceptance_id },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let response = task_commands::commit_accepted(&project.project_dir, &acceptance_id, true)?;
                Ok(CommandOutput::message(response))
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
            // Dispatched above before presentation handling; retained so the
            // match stays total and the JSON branch never sees it.
            Command::AuthoringApply {
                component,
                intent_file,
            } => crate::authoring::apply::apply_intent(&component, &intent_file)
                .map(CommandOutput::message),
        }
    } else {
        match command {
            Command::Convert { project_dir } => convert::convert(&project_dir)
                .map(|outcome| CommandOutput::message(outcome.to_string())),
            Command::Vendor {
                project_dir,
                vendored_dir,
            } => {
                let project = context::ProjectContext::resolve(project_dir.as_deref())?;
                vendor_command::vendor_project(
                    &project.project_dir,
                    vendor_command::VendorOptions {
                        vendored_dir,
                        populate: true,
                    },
                )
                .map(|report| CommandOutput::message(report.to_string()))
            }
            Command::Toolchain { project_dir } => {
                let project = context::ProjectContext::resolve(project_dir.as_deref())?;
                toolchain::ensure_toolchain(&project.project_dir, "toolchain").map(|manifest| {
                    CommandOutput::message(format!(
                        "Toolchain ensured: channel `{}`, root `{}`",
                        manifest.channel, manifest.toolchain_root
                    ))
                })
            }
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
                multi_turn,
            } => {
                let resolved_prompt = prompt_input::resolve(prompt, file.as_deref(), editor)?;

                // `kvist shell` is itself interactive, so prompt work runs in the
                // standalone agent-runner shell. By default the sandbox confines and
                // multiplies the work; the one-shot host path below only applies when
                // there is no interactive terminal or host execution was not
                // acknowledged.
                if delegate_interactive_prompt(
                    &resolved_prompt,
                    &role,
                    model.as_deref(),
                    reasoning_effort.map(Into::into),
                    allow_host_execution,
                    multi_turn,
                )? {
                    return Ok(CommandOutput::none());
                }

                if !allow_host_execution {
                    return Err(agent_runtime::Error::HostExecutionNotAcknowledged.into());
                }
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
                command: AgentCommand::Role { command },
            } => {
                let (current_dir, fallback_global) = agent_scope()?;
                match command.unwrap_or(AgentRoleCommand::List) {
                    AgentRoleCommand::List => {
                        wizard::list_roles(&current_dir).map(CommandOutput::message)
                    }
                    AgentRoleCommand::Set {
                        role,
                        model,
                        thinking_effort,
                        global,
                    } => {
                        let global = global || fallback_global;
                        let config_path = agent_config_path(&current_dir, global)?;
                        let effort = match thinking_effort {
                            Some(s) => Some(agent_runtime::ReasoningEffort::parse_effort(&s).ok_or_else(|| {
                                KvistError::AgentSetupFailed {
                                    reason: format!("invalid reasoning effort `{s}`: expected none, minimal, low, medium, high, xhigh, or max"),
                                }
                            })?),
                            None => None,
                        };
                        wizard::set_role_model_with_effort(
                            &config_path,
                            &current_dir,
                            !global,
                            &role,
                            &model,
                            effort,
                        )
                        .map(CommandOutput::message)
                    }
                    AgentRoleCommand::Clear { role, all, global } => {
                        let global = global || fallback_global;
                        let config_path = agent_config_path(&current_dir, global)?;
                        wizard::clear_role(
                            &config_path,
                            &current_dir,
                            !global,
                            role.as_deref(),
                            all,
                        )
                        .map(CommandOutput::message)
                    }
                }
            }
            Command::Agent {
                command: AgentCommand::Setup { force },
            } => {
                let (current_dir, _) = agent_scope()?;
                let mut reader = crate::interruptible_stdin::interruptible_reader();
                let mut writer = std::io::BufWriter::new(std::io::stdout());
                match wizard::run_wizard_with_force(&mut reader, &mut writer, &current_dir, force) {
                    Ok(()) => Ok(CommandOutput::message(
                        "agent setup wizard complete".to_owned(),
                    )),
                    Err(KvistError::AgentSetupCancelled) => {
                        Ok(CommandOutput::message("agent setup cancelled".to_owned()))
                    }
                    Err(source) => Err(source),
                }
            }
            Command::Agent {
                command: AgentCommand::List,
            } => {
                let (current_dir, _) = agent_scope()?;
                wizard::list_models(&current_dir).map(CommandOutput::message)
            }
            Command::Agent {
                command: AgentCommand::Remove { name, all, global },
            } => {
                let (current_dir, fallback_global) = agent_scope()?;
                let global = global || fallback_global;
                let config_path = agent_config_path(&current_dir, global)?;
                if all {
                    wizard::remove_all_models(&config_path, &current_dir, !global)
                        .map(CommandOutput::message)
                } else if let Some(model_name) = name {
                    wizard::remove_model(&config_path, &current_dir, !global, &model_name)
                        .map(CommandOutput::message)
                } else {
                    Err(KvistError::AgentSetupFailed {
                        reason:
                            "specify a model name or use --all to clear all agent configuration"
                                .to_owned(),
                    })
                }
            }
            Command::Agent {
                command: AgentCommand::Check { global },
            } => {
                let (current_dir, fallback_global) = agent_scope()?;
                let global = global || fallback_global;
                let mut reader = crate::interruptible_stdin::interruptible_reader();
                let mut writer = std::io::BufWriter::new(std::io::stdout());
                match wizard::check_agents(&mut reader, &mut writer, &current_dir, global) {
                    Ok(()) => Ok(CommandOutput::message("agent check complete".to_owned())),
                    Err(KvistError::AgentSetupCancelled) => {
                        Ok(CommandOutput::message("agent check cancelled".to_owned()))
                    }
                    Err(source) => Err(source),
                }
            }
            Command::Shell { path } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                crate::shell::run_shell(&project.project_dir)?;
                Ok(CommandOutput::none())
            }
            Command::Init { path } => {
                let dir = path.unwrap_or_else(|| PathBuf::from("."));
                init::initialize(&dir).map(|outcome| CommandOutput::message(outcome.to_string()))
            }
            Command::Tree { path } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                tree::render_project(&project.project_dir).map(CommandOutput::message)
            }
            Command::Doctor { path } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                project_state::inspect(&project.project_dir)
                    .map(|inspection| CommandOutput::message(inspection.to_string()))
            }
            Command::Status {
                path,
                format,
                only_documents,
                only_impls,
                unfinished,
            } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                project_state::inspect(&project.project_dir).map(|inspection| {
                    CommandOutput::message(status::render(
                        &inspection,
                        format,
                        only_documents,
                        only_impls,
                        unfinished,
                    ))
                })
            }
            Command::Task {
                command: TaskCommand::Next { component_dir },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::next(&project.project_dir, &component).map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Transition {
                        component_dir,
                        task_id,
                        status,
                        reason,
                        ..
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::transition(
                    &project.project_dir,
                    &component,
                    &task_id,
                    status.into(),
                    reason.as_deref(),
                )
                .map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Run {
                        component_dir,
                        task_id,
                        stream,
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::run_task_or_item(
                    &project.project_dir,
                    &component,
                    task_id.as_deref(),
                    stream,
                )
                .map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Log {
                        component_dir,
                        task_id,
                        ..
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::task_log(&project.project_dir, &component, &task_id)
                    .map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Replay {
                        session_file,
                        max_turns,
                    },
            } => task_commands::replay_task_session(&session_file, max_turns)
                .map(CommandOutput::message),
            Command::Task {
                command: TaskCommand::ApprovePolicy { path },
            } => {
                let project = context::ProjectContext::resolve(path.as_deref())?;
                task_commands::approve_policy(&project.project_dir).map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Unlock {
                        component_dir,
                        force,
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::unlock(&project.project_dir, &component, force)
                    .map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Recover {
                        component_dir,
                        task_id,
                        attempt_id,
                        disposition,
                        ..
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::recover(
                    &project.project_dir,
                    &component,
                    &task_id,
                    &attempt_id,
                    disposition,
                )
                .map(CommandOutput::message)
            }
            Command::Task {
                command:
                    TaskCommand::Finalize {
                        component_dir,
                        task_id,
                        attempt_id,
                        disposition,
                        commit,
                        reason,
                        ..
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                task_commands::finalize(
                    &project.project_dir,
                    &component,
                    &task_id,
                    &attempt_id,
                    disposition,
                    commit,
                    reason.as_deref(),
                    false,
                )
                .map(CommandOutput::message)
            }
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
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let component_path = project.component_path(&component)?;
                let validations = validate_component_documents(&component_path)?;
                if validations
                    .iter()
                    .all(|(_, validation)| validation.is_valid())
                {
                    Ok(CommandOutput::message(format!(
                        "valid component documents: {}",
                        component.display()
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
                        path: component,
                        diagnostics,
                    })
                }
            }
            Command::Component {
                command:
                    ComponentCommand::Accept {
                        component_dir,
                        commit,
                        message,
                    },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                let component = project.resolve_component(component_dir.as_deref())?;
                let response = task_commands::accept(
                    &project.project_dir,
                    &component,
                    commit,
                    message.as_deref(),
                    false,
                )?;
                Ok(CommandOutput::message(response))
            }
            Command::Component {
                command: ComponentCommand::Commit { acceptance_id },
            } => {
                let project = context::ProjectContext::resolve(None)?;
                task_commands::commit_accepted(&project.project_dir, &acceptance_id, false)
                    .map(CommandOutput::message)
            }
            Command::Completions { shell } => {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                let mut buffer = Vec::new();
                let generator: clap_complete::Shell = shell.into();
                clap_complete::generate(generator, &mut cmd, "kvist", &mut buffer);
                let script = String::from_utf8(buffer).expect("valid UTF-8 completion script");
                Ok(CommandOutput::message(script))
            }
            // Dispatched above before presentation handling; retained so the
            // match stays total and the plain branch never sees it.
            Command::AuthoringApply {
                component,
                intent_file,
            } => crate::authoring::apply::apply_intent(&component, &intent_file)
                .map(CommandOutput::message),
        }
    }
}

/// Splits a task subcommand's positional arguments into an optional leading
/// COMPONENT_DIR and the exact required tail, rejecting any other arity.
fn split_component_and_tail(
    args: Vec<String>,
    tail_len: usize,
) -> Result<(Option<PathBuf>, Vec<String>)> {
    match args.len() {
        n if n == tail_len => Ok((None, args)),
        n if n == tail_len + 1 => Ok((Some(PathBuf::from(&args[0])), args[1..].to_vec())),
        n => Err(usage_error(format!(
            "expected {} positional argument(s) after an optional COMPONENT_DIR, got {n}",
            tail_len
        ))),
    }
}

/// Builds a parser-level usage error (exit status 2) from a message.
fn usage_error(message: String) -> KvistError {
    KvistError::ArgumentParsing(clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        message,
    ))
}

/// Parses a task-status literal, rejecting anything outside the closed set.
fn status_argument_from(value: String) -> Result<TaskStatusArgument> {
    TaskStatusArgument::from_str(&value, true).map_err(|_| {
        usage_error(format!(
            "invalid task status `{value}`; expected pending, in-progress, blocked, awaiting-decision, or completed"
        ))
    })
}

/// Parses a finalize-disposition literal, rejecting anything outside the set.
fn disposition_argument_from(value: String) -> Result<FinalizeDispositionArgument> {
    FinalizeDispositionArgument::from_str(&value, true).map_err(|_| {
        usage_error(format!(
            "invalid disposition `{value}`; expected accept or block"
        ))
    })
}

/// Task subcommands that mix an optional COMPONENT_DIR with required trailing
/// arguments (transition, log, recover, finalize) cannot be declared
/// declaratively: clap forbids a non-required positional before a required
/// one. They therefore parse into a raw argument vector and this function
/// completes the parse immediately after argument parsing, keeping the
/// dispatch surface uniform.
fn normalize_task_positionals(command: Command) -> Result<Command> {
    let Command::Task { command: task } = command else {
        return Ok(command);
    };
    let task = match task {
        TaskCommand::Transition { args, reason, .. } => {
            let (component_dir, tail) = split_component_and_tail(args, 2)?;
            let status = status_argument_from(tail[1].clone())?;
            TaskCommand::Transition {
                args: Vec::new(),
                component_dir,
                task_id: tail[0].clone(),
                status,
                reason,
                _unparsed: Default::default(),
            }
        }
        TaskCommand::Log { args, .. } => {
            let (component_dir, tail) = split_component_and_tail(args, 1)?;
            TaskCommand::Log {
                args: Vec::new(),
                component_dir,
                task_id: tail[0].clone(),
                _unparsed: Default::default(),
            }
        }
        TaskCommand::Recover {
            args, disposition, ..
        } => {
            let (component_dir, tail) = split_component_and_tail(args, 2)?;
            TaskCommand::Recover {
                args: Vec::new(),
                component_dir,
                task_id: tail[0].clone(),
                attempt_id: tail[1].clone(),
                disposition,
                _unparsed: Default::default(),
            }
        }
        TaskCommand::Finalize {
            args,
            commit,
            reason,
            ..
        } => {
            let (component_dir, tail) = split_component_and_tail(args, 3)?;
            let disposition = disposition_argument_from(tail[2].clone())?;
            TaskCommand::Finalize {
                args: Vec::new(),
                component_dir,
                task_id: tail[0].clone(),
                attempt_id: tail[1].clone(),
                disposition,
                commit,
                reason,
                _unparsed: Default::default(),
            }
        }
        other => other,
    };
    Ok(Command::Task { command: task })
}

/// Handles `kvist` with no command: inside a project it answers "where do I
/// stand and what is next?" with the same overview as `kvist status` plus a
/// help pointer; outside a project it prints the guided help.
fn bare_invocation(json: bool) -> Result<CommandOutput> {
    use clap::CommandFactory;

    match context::ProjectContext::resolve(None) {
        Ok(project) => {
            let inspection = project_state::inspect(&project.project_dir)?;
            if json {
                let report =
                    status::render(&inspection, status::StatusFormat::Json, false, false, false);
                Ok(CommandOutput::message(format!(
                    r#"{{"status":"success","command":"status","report":{report}}}"#,
                )))
            } else {
                let mut text = status::render(
                    &inspection,
                    status::StatusFormat::Overview,
                    false,
                    false,
                    false,
                );
                text.push_str("\n\ntip: run `kvist help` for all commands");
                Ok(CommandOutput::message(text))
            }
        }
        Err(_) => {
            if json {
                Ok(CommandOutput::message(
                    r#"{"status":"error","reason":"not-in-project","hint":"run `kvist init <DIR>` to create a project, or pass an explicit PROJECT_DIR"}"#.to_owned(),
                ))
            } else {
                Cli::command()
                    .print_long_help()
                    .map_err(|source| KvistError::Io {
                        operation: "render command-line help",
                        path: PathBuf::new(),
                        source,
                    })?;
                Ok(CommandOutput::none())
            }
        }
    }
}

fn task_status_name(status: crate::task_queue::TaskStatus) -> &'static str {
    match status {
        crate::task_queue::TaskStatus::Pending => "pending",
        crate::task_queue::TaskStatus::InProgress => "in-progress",
        crate::task_queue::TaskStatus::Blocked => "blocked",
        crate::task_queue::TaskStatus::AwaitingDecision => "awaiting-decision",
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

/// Resolve the standalone `agent-runner` executable: an explicit path via the
/// `KVIST_AGENT_RUNNER` environment variable, otherwise the first `agent-runner`
/// found on `PATH`. Returns an actionable message when it is not installed, so a
/// missing agent fails closed rather than falling back to unconstrained host
/// execution.
fn resolve_agent_runner() -> std::result::Result<PathBuf, String> {
    let explicit = std::env::var_os("KVIST_AGENT_RUNNER");
    let path = std::env::var_os("PATH");
    resolve_agent_runner_from(explicit, path)
}

/// Pure resolution used by [`resolve_agent_runner`]. An explicit executable path
/// wins; otherwise the `PATH` directories are scanned for an `agent-runner` file.
/// Kept free of direct I/O so it is unit-testable without touching the process
/// environment.
fn resolve_agent_runner_from(
    explicit: Option<std::ffi::OsString>,
    path: Option<std::ffi::OsString>,
) -> std::result::Result<PathBuf, String> {
    if let Some(path) = explicit {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "KVIST_AGENT_RUNNER points at `{}` which is not an executable file",
            candidate.display()
        ));
    }
    let Some(paths) = path else {
        return Err("PATH is not set; cannot locate the agent-runner executable".to_owned());
    };
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("agent-runner");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(
        "the `agent-runner` executable was not found on PATH; build it with \
         `cargo build -p agent-runner` and add its output to PATH, or set \
         KVIST_AGENT_RUNNER to its path"
            .to_owned(),
    )
}

/// Resolve a role-name argument to its configured role profile, mirroring the
/// selection the one-shot host path performs.
fn resolve_role_config<'a>(
    config: &'a crate::config::ProjectConfig,
    role_name: &str,
) -> Result<&'a crate::config::RoleConfig> {
    let profile = match role_name {
        "developer" => &config.agent.developer,
        "architect" => &config.agent.architect,
        "security-reviewer" | "security_reviewer" => &config.agent.security_reviewer,
        _ => {
            return Err(KvistError::AgentSetupFailed {
                reason: format!("unknown role profile: {role_name}"),
            });
        }
    };
    Ok(profile)
}

/// The autonomous turn cap the engine grants when a user opts into multi-turn
/// host execution via `kvist prompt --allow-host-execution --multi-turn`. It
/// mirrors the sandbox's own default cap: sandboxed interactive work is
/// multi-turn by default, while host work is single-turn unless this opts in.
const HOST_AUTONOMOUS_CAP: u32 = 50;

/// Launch the standalone `agent-runner` shell for an interactive, sandboxed
/// custom prompt. Kvist supplies the authored prompt and the preselected model
/// and thinking effort, and inherits the child's stdio so the terminal UI is the
/// transcript; the child's exit status is surfaced.
///
/// Returns `Ok(true)` when delegation happened, `Ok(false)` when there is no
/// interactive terminal (so the caller can fall back to the one-shot host path),
/// and `Err` on a recoverable failure.
fn delegate_interactive_prompt(
    prompt: &str,
    role_name: &str,
    model: Option<&str>,
    effort: Option<agent_runtime::ReasoningEffort>,
    allow_host_execution: bool,
    multi_turn: bool,
) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }

    let binary = resolve_agent_runner().map_err(|reason| KvistError::AgentSetupFailed {
        reason: format!("could not start agent-runner: {reason}"),
    })?;

    let current_dir = std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current project directory",
        path: PathBuf::from("."),
        source,
    })?;
    let config = crate::config::load(&current_dir)?;
    let role_config = resolve_role_config(&config, role_name)?;
    // Preselect the model the role is configured to use; an explicit `--model`
    // override wins. The id must exist in agent-runner's own configuration.
    let model = model.or(Some(role_config.profile.as_str()));
    let effort = effort.or(role_config.thinking_effort);

    let mut command = std::process::Command::new(&binary);
    command
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .current_dir(&current_dir);
    if let Some(model) = model {
        command.arg("--model").arg(model);
    }
    if let Some(effort) = effort {
        command.arg("--effort").arg(effort.as_str());
    }
    // Bypassing the Bubblewrap sandbox runs the agent with host privileges, so it
    // is single-turn by default; --multi-turn lifts that cap only in the
    // elevated (host) case, where sandboxed multi-turn is already the default.
    if allow_host_execution {
        command.arg("--allow-host-execution");
        if multi_turn {
            command
                .arg("--host-turns")
                .arg(HOST_AUTONOMOUS_CAP.to_string());
        }
    }
    // The prompt is positional; agent-runner prefills and auto-starts it.
    command.arg(prompt);

    let status = command.status().map_err(|source| KvistError::Io {
        operation: "run agent-runner",
        path: binary,
        source,
    })?;
    if !status.success() {
        return Err(KvistError::AgentSetupFailed {
            reason: format!("agent-runner exited with status {status}"),
        });
    }
    Ok(true)
}

/// Resolves the configuration scope for an agent command.
///
/// Agent configuration exists per project and per user. Inside a project the
/// project configuration is the scope; outside a project the commands target
/// the global user configuration, exactly as if `--global` had been passed,
/// so configuring agents never requires a project that may not exist yet.
/// Returns `(directory, effective_global)`: the directory whose `kvist.toml`
/// holds the project configuration when not global, and a usable base
/// directory otherwise.
fn agent_scope() -> Result<(PathBuf, bool)> {
    match context::ProjectContext::resolve(None) {
        Ok(project) => Ok((project.project_dir, false)),
        Err(KvistError::NotInProject { .. }) => Ok((global_user_config_dir()?, true)),
        Err(error) => Err(error),
    }
}

/// The directory containing the global user configuration file.
fn global_user_config_dir() -> Result<PathBuf> {
    config::global_user_config_path()
        .and_then(|path| path.parent().map(PathBuf::from))
        .ok_or_else(|| KvistError::AgentSetupFailed {
            reason: "cannot resolve user configuration directory".to_owned(),
        })
}

/// The configuration file an agent command operates on for one scope.
fn agent_config_path(current_dir: &Path, global: bool) -> Result<PathBuf> {
    if global {
        config::global_user_config_path().ok_or_else(|| KvistError::AgentSetupFailed {
            reason: "cannot resolve user configuration directory".to_owned(),
        })
    } else {
        Ok(current_dir.join("kvist.toml"))
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

    /// Parses and completes the positional normalization a dispatched command
    /// goes through.
    fn normalized(cli: Cli) -> Command {
        normalize_task_positionals(cli.command.expect("a command was parsed"))
            .expect("positional normalization succeeds")
    }

    #[test]
    fn parses_shell_with_the_current_directory_by_default() {
        let cli = Cli::try_parse_from(["kvist", "shell"]).expect("valid shell command");

        let Some(Command::Shell { path: project }) = cli.command else {
            panic!("expected shell command");
        };

        assert_eq!(project, None);
    }

    #[test]
    fn parses_init_with_the_current_directory_by_default() {
        let cli = Cli::try_parse_from(["kvist", "init"]).expect("valid init command");

        let Some(Command::Init { path: project }) = cli.command else {
            panic!("expected init command");
        };

        assert_eq!(project, None);
    }

    #[test]
    fn parses_tree_with_an_explicit_project_directory() {
        let cli =
            Cli::try_parse_from(["kvist", "tree", "projects/demo"]).expect("valid tree command");

        let Some(Command::Tree { path: project }) = cli.command else {
            panic!("expected tree command");
        };

        assert_eq!(project, Some(PathBuf::from("projects/demo")));
    }

    #[test]
    fn prompt_command_parses_multi_turn_flag() {
        let cli = Cli::try_parse_from(["kvist", "prompt", "refactor this", "--multi-turn"])
            .expect("valid prompt command");
        let Some(Command::Prompt { multi_turn, .. }) = cli.command else {
            panic!("expected prompt command");
        };
        assert!(multi_turn, "--multi-turn opts into an autonomous loop");
    }

    #[test]
    fn prompt_command_defaults_to_a_single_model_turn() {
        let cli = Cli::try_parse_from(["kvist", "prompt", "explain this"]).expect("valid prompt");
        let Some(Command::Prompt { multi_turn, .. }) = cli.command else {
            panic!("expected prompt command");
        };
        assert!(!multi_turn, "a prompt defaults to a single model turn");
    }

    #[test]
    fn resolve_agent_runner_prefers_explicit_file_path() {
        // An explicit `KVIST_AGENT_RUNNER` wins when it names an executable file,
        // even when a different `agent-runner` also sits on `PATH`.
        let dir = tempfile::TempDir::new().expect("temp dir");
        let explicit = dir.path().join("explicit-agent-runner");
        std::fs::write(&explicit, "").expect("seed explicit executable");
        let on_path = dir.path().join("path-agent-runner");
        std::fs::write(&on_path, "").expect("seed path executable");
        let path_value = on_path.to_string_lossy().into_owned();
        let resolved = resolve_agent_runner_from(
            Some(std::ffi::OsString::from(explicit.clone())),
            Some(std::ffi::OsString::from(path_value)),
        )
        .expect("the explicit path should resolve");
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_agent_runner_rejects_a_non_file_explicit_path() {
        // An explicit path that is not an executable file fails closed rather
        // than falling back to the host path.
        let dir = tempfile::TempDir::new().expect("temp dir");
        let not_a_file = dir.path().join("is-a-directory");
        std::fs::create_dir_all(&not_a_file).expect("create directory");
        let message = resolve_agent_runner_from(Some(not_a_file.into_os_string()), None)
            .expect_err("a directory is not an executable file");
        assert!(
            message.contains("not an executable file"),
            "expected a not-an-executable-file message, got: {message}"
        );
    }

    #[test]
    fn resolve_agent_runner_scans_path_for_the_binary() {
        // Without an explicit path, the first `agent-runner` file found on `PATH`
        // wins.
        let dir = tempfile::TempDir::new().expect("temp dir");
        let found = dir.path().join("agent-runner");
        std::fs::write(&found, "").expect("seed executable");
        let path_value = dir.path().to_string_lossy().into_owned();
        let resolved = resolve_agent_runner_from(None, Some(std::ffi::OsString::from(path_value)))
            .expect("the PATH binary should resolve");
        assert_eq!(resolved, found);
    }

    #[test]
    fn resolve_agent_runner_fails_closed_when_absent_from_path() {
        // Empty PATH yields the actionable not-found message rather than a panic.
        let message = resolve_agent_runner_from(None, Some(std::ffi::OsString::new()))
            .expect_err("a binary missing from PATH should fail closed");
        assert!(
            message.contains("was not found on PATH"),
            "expected a not-found message, got: {message}"
        );
    }

    #[test]
    fn parses_doctor_with_the_current_directory_by_default() {
        let cli = Cli::try_parse_from(["kvist", "doctor"]).expect("valid doctor command");

        let Some(Command::Doctor { path: project }) = cli.command else {
            panic!("expected doctor command");
        };

        assert_eq!(project, None);
    }

    #[test]
    fn parses_component_creation() {
        let cli = Cli::try_parse_from(["kvist", "component", "new", "src/network"])
            .expect("valid component creation command");

        let Some(Command::Component {
            command: ComponentCommand::New { component_dir },
        }) = cli.command
        else {
            panic!("expected component new command");
        };

        assert_eq!(component_dir, PathBuf::from("src/network"));
    }

    #[test]
    fn parses_component_validation() {
        let cli = Cli::try_parse_from(["kvist", "component", "validate", "src/network"])
            .expect("valid component validation command");

        let Some(Command::Component {
            command: ComponentCommand::Validate { component_dir },
        }) = cli.command
        else {
            panic!("expected component validate command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src/network")));
    }

    #[test]
    fn parses_convert_command() {
        let cli = Cli::try_parse_from(["kvist", "convert", "/path/to/project"])
            .expect("valid convert command");

        let Some(Command::Convert { project_dir }) = cli.command else {
            panic!("expected convert command");
        };

        assert_eq!(project_dir, PathBuf::from("/path/to/project"));
    }

    #[test]
    fn parses_task_next_command() {
        let cli =
            Cli::try_parse_from(["kvist", "task", "next", "src"]).expect("valid task next command");

        let Some(Command::Task {
            command: TaskCommand::Next { component_dir },
        }) = cli.command
        else {
            panic!("expected task next command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
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
                    ..
                },
        } = normalized(cli)
        else {
            panic!("expected task transition command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
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
                    ..
                },
        } = normalized(cli)
        else {
            panic!("expected task transition command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
        assert_eq!(task_id, "task-1");
        assert_eq!(status, TaskStatusArgument::Blocked);
        assert_eq!(reason, Some("waiting on PR".to_string()));
    }

    #[test]
    fn parses_task_run_command() {
        let cli = Cli::try_parse_from(["kvist", "task", "run", "src", "task-1"])
            .expect("valid task run command");

        let Some(Command::Task {
            command:
                TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
        }) = cli.command
        else {
            panic!("expected task run command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
        assert_eq!(task_id, Some("task-1".to_owned()));
        assert!(!stream);
    }

    #[test]
    fn parses_task_run_command_with_omitted_task_id() {
        let cli = Cli::try_parse_from(["kvist", "task", "run", "src"])
            .expect("valid task run command without a task id");

        let Some(Command::Task {
            command:
                TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
        }) = cli.command
        else {
            panic!("expected task run command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
        assert_eq!(task_id, None);
        assert!(!stream);
    }

    #[test]
    fn parses_task_run_command_with_task_id() {
        let cli = Cli::try_parse_from(["kvist", "task", "run", "src", "task-1", "--stream"])
            .expect("valid task run command with task id");

        let Some(Command::Task {
            command:
                TaskCommand::Run {
                    component_dir,
                    task_id,
                    stream,
                },
        }) = cli.command
        else {
            panic!("expected task run command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
        assert_eq!(task_id, Some("task-1".to_owned()));
        assert!(stream);
    }

    #[test]
    fn parses_task_recover_command() {
        let cli = Cli::try_parse_from([
            "kvist",
            "task",
            "recover",
            "engine",
            "task-1",
            "attempt-0001",
            "--disposition",
            "execution-did-not-start",
        ])
        .expect("valid recovery command");

        let Command::Task {
            command:
                TaskCommand::Recover {
                    component_dir,
                    task_id,
                    attempt_id,
                    disposition,
                    ..
                },
        } = normalized(cli)
        else {
            panic!("expected task recover command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("engine")));
        assert_eq!(task_id, "task-1");
        assert_eq!(attempt_id, "attempt-0001");
        assert!(matches!(
            disposition,
            RecoveryDispositionArgument::ExecutionDidNotStart
        ));
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
                    ..
                },
        } = normalized(cli)
        else {
            panic!("expected task log command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
        assert_eq!(task_id, "task-1".to_string());
    }

    #[test]
    fn parses_task_replay_command() {
        let cli = Cli::try_parse_from([
            "kvist",
            "task",
            "replay",
            ".kvist/runs/session.jsonl",
            "--max-turns",
            "3",
        ])
        .expect("valid task replay command");

        let Some(Command::Task {
            command:
                TaskCommand::Replay {
                    session_file,
                    max_turns,
                },
        }) = cli.command
        else {
            panic!("expected task replay command");
        };

        assert_eq!(session_file, PathBuf::from(".kvist/runs/session.jsonl"));
        assert_eq!(max_turns, Some(3));
    }

    #[test]
    fn parses_task_approve_policy_command() {
        let cli = Cli::try_parse_from(["kvist", "task", "approve-policy", "/path/to/project"])
            .expect("valid task approve-policy command");

        let Some(Command::Task {
            command: TaskCommand::ApprovePolicy { path },
        }) = cli.command
        else {
            panic!("expected task approve-policy command");
        };

        assert_eq!(path, Some(PathBuf::from("/path/to/project")));
    }

    #[test]
    fn parses_task_unlock_command() {
        let cli = Cli::try_parse_from(["kvist", "task", "unlock", "src", "--force"])
            .expect("valid task unlock command");

        let Some(Command::Task {
            command:
                TaskCommand::Unlock {
                    component_dir,
                    force,
                },
        }) = cli.command
        else {
            panic!("expected task unlock command");
        };

        assert_eq!(component_dir, Some(PathBuf::from("src")));
        assert!(force);
    }

    #[test]
    fn help_exits_successfully() {
        let error = Cli::try_parse_from(["kvist", "--help"]).expect_err("help exits successfully");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        assert_eq!(KvistError::from(error).exit_code(), 0);
    }

    #[test]
    fn task_transition_rejects_an_invalid_status_with_a_usage_error() {
        let parsed = Cli::try_parse_from(["kvist", "task", "transition", "write-tests", "flying"])
            .expect("parses raw arguments");
        let error = normalize_task_positionals(parsed.command.expect("command"))
            .expect_err("invalid status must fail");
        assert!(
            error.to_string().contains("invalid task status `flying`"),
            "got: {error}"
        );
    }

    #[test]
    fn task_finalize_rejects_a_missing_disposition_with_a_usage_error() {
        let parsed =
            Cli::try_parse_from(["kvist", "task", "finalize", "write-tests", "attempt-0001"])
                .expect("parses raw arguments");
        let error = normalize_task_positionals(parsed.command.expect("command"))
            .expect_err("missing disposition must fail");
        assert!(
            error
                .to_string()
                .contains("expected 3 positional argument(s)"),
            "got: {error}"
        );
    }

    #[test]
    fn help_lists_the_complete_phase_one_command_surface() {
        let error = Cli::try_parse_from(["kvist", "--help"]).expect_err("help exits successfully");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let help = error.to_string();
        for expected in [
            "shell",
            "init",
            "convert",
            "reverse-discover",
            "import",
            "vendor",
            "toolchain",
            "tree",
            "doctor",
            "status",
            "task",
            "component",
            "agent",
            "prompt",
            "completions",
        ] {
            assert!(
                help.contains(expected),
                "help should mention {expected} command"
            );
        }
        assert!(
            !help.contains("overview"),
            "overview is merged into status and must not appear as a command"
        );
        assert!(
            !help.contains("vcs"),
            "vcs commit-accepted moved under component commit"
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

        let Some(Command::Completions { shell }) = cli.command else {
            panic!("expected completions command");
        };

        assert_eq!(shell, SupportedShell::Bash);
    }

    #[test]
    fn generates_bash_completions() {
        let outcome = execute(
            Some(Command::Completions {
                shell: SupportedShell::Bash,
            }),
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
            Some(Command::Init {
                path: Some(project.path().to_path_buf()),
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
