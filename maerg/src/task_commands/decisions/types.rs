use super::*;

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
    pub documents: BTreeMap<String, String>,
    #[serde(default)]
    pub document_edits: Vec<DocumentEdit>,
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
