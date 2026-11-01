use super::*;

/// Required-component command surface for discussion and exact application.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum Action {
    /// List short response numbers and their exact command expansions.
    Actions,
    /// Invoke a displayed response exactly; its number authorizes its labelled effect.
    Do {
        #[arg(value_name = "RESPONSE_NUMBER", value_parser = clap::value_parser!(u32).range(1..))]
        number: u32,
    },
    /// Inspect the pause, retained evidence, and response instructions.
    Show {
        #[arg(value_name = "TASK_ID")]
        task_id: Option<String>,
    },
    /// Ask a bounded agent to prepare structured advisory alternatives.
    Options {
        #[arg(long, value_name = "TASK_ID")]
        task: Option<String>,
    },
    /// Discuss a decision without changing controlled artifacts.
    Discuss {
        message: String,
        #[arg(long, value_name = "TASK_ID")]
        task: Option<String>,
    },
    /// Stage all proposed intent/task effects, without applying them.
    #[command(
        long_about = "Stage a complete proposal without applying it. Manual/model draft is strict native JSON version 1:\n{\"schema_version\":1,\"rationale\":\"Your chosen direction and reasons\",\"documents\":{},\"tasks\":[],\"disposition\":\"resolve\"}\nDocument keys: REQUIREMENTS.md, CONTRACT.md, DESIGN.md (complete changed text only).\nOptional document_edits: [{\"path\":\"DESIGN.md\",\"before\":\"unique exact excerpt\",\"after\":\"replacement\"}]. Prefer these for large unchanged files; no fuzzy matching or overlap with full replacements.\nTask edits: {\"action\":\"add\"|\"update\",\"task\":{id,title,description,context,purpose,expected_outcome,kind,depends_on,requirements}}, or {\"action\":\"retire\",\"id\":\"...\",\"reason\":\"...\"}.\nNo status, timestamp, acceptance, recovery or whole-queue fields. Withdrawal must explicitly retire the entire affected uncompleted dependant closure. Inspect the generated Markdown before exact approval. No implementation approval or Git publication is implied."
    )]
    Plan {
        #[arg(long, value_name = "TASK_ID")]
        task: Option<String>,
        #[arg(long, conflicts_with = "import")]
        option: Option<String>,
        /// Require these exact catalog bytes rather than selecting a changed option.
        #[arg(long, value_name = "SHA256", requires = "option")]
        catalog: Option<String>,
        #[arg(long)]
        guidance: Option<String>,
        #[arg(long, value_name = "PATH")]
        import: Option<PathBuf>,
        #[arg(long, value_name = "REASON")]
        review_exception: Option<String>,
    },
    /// Apply one authenticated plan; approval must match its exact SHA-256.
    Apply {
        plan_id: String,
        #[arg(long, value_name = "SHA256")]
        approve: String,
        #[arg(long = "continue")]
        continue_run: bool,
    },
}

/// The authored fields of one task; the host owns lifecycle and acceptance fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDefinition {
    pub id: String,
    pub title: String,
    pub description: String,
    pub context: String,
    pub purpose: String,
    pub expected_outcome: String,
    pub kind: TaskKind,
    pub depends_on: Vec<String>,
    pub requirements: Vec<String>,
}

/// Typed queue operations accepted by the decision planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TaskEdit {
    Add { task: TaskDefinition },
    Update { task: TaskDefinition },
    Retire { id: String, reason: String },
}

/// Resolving a pause is not withdrawing its work or claiming completion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    Resolve,
    Withdraw,
}

/// Strict agent/manual input; never raw queue metadata or filesystem authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub schema_version: u32,
    pub rationale: String,
    #[serde(default)]
    pub documents: BTreeMap<String, String>,
    #[serde(default)]
    pub document_edits: Vec<DocumentEdit>,
    #[serde(default)]
    pub tasks: Vec<TaskEdit>,
    pub disposition: Disposition,
}

/// A literal uniquely anchored change, materialized by the host before approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentEdit {
    pub path: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    pub(super) option: Option<String>,
    pub(super) guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IntentReview {
    pub(super) report: String,
    pub(super) exception: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capture {
    pub(super) schema_version: u32,
    pub(super) component: PathBuf,
    pub(super) task: Task,
    pub(super) queue: TaskQueue,
    pub(super) files: BTreeMap<String, String>,
    pub(super) bindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub(super) schema_version: u32,
    pub(super) id: String,
    pub(super) approval: String,
    pub(super) input: Capture,
    pub(super) selection: Selection,
    pub(super) rationale: String,
    pub(super) review: IntentReview,
    pub(super) outputs: BTreeMap<String, String>,
    pub(super) retired: Vec<Task>,
    pub(super) retirement_reasons: BTreeMap<String, String>,
    pub(super) disposition: Disposition,
    pub(super) created_at: Timestamp,
    pub(super) authentication_tag: String,
}
