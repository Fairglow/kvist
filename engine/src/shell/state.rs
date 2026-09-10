//! Dynamic completion state derived from the active project.
//!
//! The shell's tab-completion offers two kinds of values. Static values
//! (verbs, flags, enum literals) come from the clap-derived command tree.
//! Dynamic values (component paths, task IDs, attempt IDs, model profile
//! names, and the active Git branch) come from the active project's durable
//! state. This module snapshots those dynamic sets so the completion engine
//! can resolve them in memory without touching the filesystem per keystroke.
//!
//! The snapshot is best-effort: each source contributes independently, so a
//! missing configuration, an unparsable queue, or an absent Git repository
//! degrades that one source to empty rather than aborting the whole shell.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::{config, project_state, task_queue};

/// A dynamic value domain the completion engine can resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueDomain {
    /// A component path relative to the component root; `.` is the root.
    Component,
    /// A queue-local task ID within one component.
    Task,
    /// An execution attempt ID for one task.
    Attempt,
    /// A model profile name from the agent configuration.
    Model,
    /// The active Git branch of the project repository.
    Branch,
}

/// Task and attempt data for one component.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentScope {
    /// The component's tasks in durable queue order.
    pub tasks: Vec<task_queue::Task>,
    /// Attempt IDs grouped by task ID, in lexical order.
    pub attempts: BTreeMap<String, Vec<String>>,
}

/// A best-effort snapshot of the dynamic value sets offered by completion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DynamicState {
    /// Component paths relative to the component root; `.` is the root.
    components: Vec<String>,
    /// Task and attempt scopes keyed by component path.
    scopes: BTreeMap<String, ComponentScope>,
    /// Model profile names across all roles, sorted and de-duplicated.
    models: Vec<String>,
    /// The active Git branch, when the project is inside a Git repository.
    branch: Option<String>,
}

impl DynamicState {
    /// Builds a snapshot directly from its parts.
    ///
    /// This is the injection point for tests and any caller that already holds
    /// the value sets; [`DynamicState::load`] is the filesystem-backed wrapper.
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub fn new(
        components: Vec<String>,
        scopes: BTreeMap<String, ComponentScope>,
        models: Vec<String>,
        branch: Option<String>,
    ) -> Self {
        Self {
            components,
            scopes,
            models,
            branch,
        }
    }

    /// Builds a snapshot for `project_dir`.
    ///
    /// Never fails: an unreadable or absent source contributes nothing.
    pub fn load(project_dir: &Path) -> Self {
        let mut state = Self::default();
        state.load_components_and_scopes(project_dir);
        state.load_models(project_dir);
        state.branch = git_branch(project_dir);
        state
    }

    /// Every component path offered for a `COMPONENT_DIR` value.
    pub fn components(&self) -> &[String] {
        &self.components
    }

    /// Task IDs for one component path, in queue order.
    pub fn task_ids_for(&self, component: &str) -> Vec<String> {
        self.scopes
            .get(component)
            .map(|scope| scope.tasks.iter().map(|task| task.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Finds a task by ID across every component, returning its component path.
    pub fn find_task(&self, task_id: &str) -> Option<(&str, &task_queue::Task)> {
        for (component, scope) in &self.scopes {
            if let Some(task) = scope.tasks.iter().find(|task| task.id == task_id) {
                return Some((component.as_str(), task));
            }
        }
        None
    }

    /// Attempt IDs for one task within one component path.
    pub fn attempts_for(&self, component: &str, task: &str) -> &[String] {
        self.scopes
            .get(component)
            .and_then(|scope| scope.attempts.get(task))
            .map(|attempts| attempts.as_slice())
            .unwrap_or(&[])
    }

    /// Every task ID across every component, in component-then-queue order.
    pub fn all_task_ids(&self) -> Vec<String> {
        let mut tasks = Vec::new();
        for component in &self.components {
            tasks.extend(self.task_ids_for(component));
        }
        tasks
    }

    /// Model profile names, sorted and de-duplicated.
    pub fn models(&self) -> &[String] {
        &self.models
    }

    /// The active Git branch, when known.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    fn load_components_and_scopes(&mut self, project_dir: &Path) {
        let Ok(inspection) = project_state::inspect(project_dir) else {
            return;
        };
        let Some(component_root) = inspection.component_root.clone() else {
            return;
        };

        for component in &inspection.components {
            let key = component_key(&component.path);
            let component_dir = project_dir.join(&component_root).join(&component.path);
            let scope = read_component_scope(&component_dir);
            self.components.push(key.clone());
            self.scopes.insert(key, scope);
        }
    }

    fn load_models(&mut self, project_dir: &Path) {
        let Ok(cfg) = config::load(project_dir) else {
            return;
        };
        let mut names: Vec<String> = Vec::new();
        for profile in [
            &cfg.agent.architect,
            &cfg.agent.developer,
            &cfg.agent.security_reviewer,
        ] {
            for model in &profile.models {
                names.push(model.name.clone());
            }
        }
        names.sort();
        names.dedup();
        self.models = names;
    }
}

/// Maps a component's root-relative path to its completion spelling.
/// The root component (empty relative path) is spelled `.`.
fn component_key(relative_path: &Path) -> String {
    if relative_path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        relative_path.to_string_lossy().into_owned()
    }
}

/// Reads one component's task queue and attempt journal into a scope.
fn read_component_scope(component_dir: &Path) -> ComponentScope {
    let mut scope = ComponentScope::default();
    if let Some(queue) = read_queue(component_dir) {
        scope.tasks = queue.tasks;
    }
    scope.attempts = read_attempts(component_dir);
    scope
}

/// Parses a component's `TODOS.yaml`, returning `None` when absent or invalid.
fn read_queue(component_dir: &Path) -> Option<task_queue::TaskQueue> {
    let path = component_dir.join("TODOS.yaml");
    let contents = fs::read_to_string(path).ok()?;
    task_queue::parse(&contents).ok()
}

/// Reads `.kvist-attempts/*.jsonl` into attempt IDs grouped by task.
fn read_attempts(component_dir: &Path) -> BTreeMap<String, Vec<String>> {
    let attempts_dir = component_dir.join(".kvist-attempts");
    let Ok(entries) = fs::read_dir(&attempts_dir) else {
        return BTreeMap::new();
    };

    let mut attempts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(task_id) = file_name.strip_suffix(".jsonl") else {
            continue;
        };

        let ids = read_attempt_ids(&path);
        if !ids.is_empty() {
            attempts.insert(task_id.to_owned(), ids);
        }
    }
    attempts
}

/// Extracts the ordered, unique `attempt_id` values from one JSONL journal.
fn read_attempt_ids(path: &Path) -> Vec<String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return Vec::new(),
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut ids = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(id) = value.get("attempt_id").and_then(Value::as_str)
            && seen.insert(id.to_owned())
        {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    ids
}

/// Resolves the active Git branch via `git rev-parse --abbrev-ref HEAD`.
fn git_branch(project_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if branch.is_empty() || branch == "HEAD" {
        return None;
    }
    Some(branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    /// A valid, minimal `TODOS.yaml` with two tasks.
    const QUEUE: &str = r#"schema_version: 1
component:
  requirements_revision: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  contract_revision: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  design_revision: sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
  parent_contract: null
  revalidation:
    state: current
    checked_at: 2026-08-13T20:19:53Z
    stale_since: null
    causes: []
tasks:
  - id: write-tests
    title: Write tests
    description: Define tests.
    context: Context.
    purpose: Purpose.
    expected_outcome: Outcome.
    kind: test
    status: pending
    depends_on: []
    requirements: []
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: null
    blocked_reason: null
  - id: implement-code
    title: Implement
    description: Implement.
    context: Context.
    purpose: Purpose.
    expected_outcome: Outcome.
    kind: implementation
    status: pending
    depends_on:
      - write-tests
    requirements: []
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: null
    blocked_reason: null
"#;

    #[test]
    fn empty_state_for_an_absent_project() {
        let dir = tempfile::tempdir().unwrap();
        let state = DynamicState::load(dir.path());
        assert!(state.components().is_empty());
        assert!(state.models().is_empty());
        assert!(state.branch().is_none());
        assert!(state.all_task_ids().is_empty());
    }

    fn task_ids(tasks: &[task_queue::Task]) -> Vec<&str> {
        tasks.iter().map(|task| task.id.as_str()).collect()
    }

    #[test]
    fn reads_component_tasks_and_attempts() {
        let dir = tempfile::tempdir().unwrap();
        // A bare directory with a queue is not a discovered component (the
        // project must be Current), so exercise the scope reader directly.
        let component = dir.path().join("comp");
        write(&component.join("TODOS.yaml"), QUEUE);
        let attempts = component.join(".kvist-attempts");
        write(
            &attempts.join("write-tests.jsonl"),
            "{\"phase\":\"spawn\",\"task_id\":\"write-tests\",\"attempt_id\":\"attempt-0002\"}\n\
             {\"phase\":\"complete\",\"task_id\":\"write-tests\",\"attempt_id\":\"attempt-0001\"}\n\
             {\"phase\":\"spawn\",\"task_id\":\"write-tests\",\"attempt_id\":\"attempt-0002\"}\n\
             not-json\n",
        );
        write(
            &attempts.join("implement-code.jsonl"),
            "{\"attempt_id\":\"attempt-0001\"}\n",
        );
        // A non-jsonl file must be ignored.
        write(&attempts.join("notes.txt"), "ignored\n");

        let scope = read_component_scope(&component);
        assert_eq!(
            task_ids(&scope.tasks),
            vec!["write-tests", "implement-code"]
        );
        assert_eq!(
            scope.attempts.get("write-tests"),
            Some(&vec!["attempt-0001".to_owned(), "attempt-0002".to_owned()])
        );
        assert_eq!(
            scope.attempts.get("implement-code"),
            Some(&vec!["attempt-0001".to_owned()])
        );
        assert_eq!(scope.attempts.len(), 2);
    }

    #[test]
    fn tolerates_a_missing_or_invalid_queue() {
        let dir = tempfile::tempdir().unwrap();
        let component = dir.path().join("comp");
        fs::create_dir_all(&component).unwrap();
        assert!(read_component_scope(&component).tasks.is_empty());

        write(&component.join("TODOS.yaml"), "schema_version: [\n");
        assert!(read_component_scope(&component).tasks.is_empty());
    }

    #[test]
    fn component_key_maps_root_to_dot() {
        assert_eq!(component_key(Path::new(".")), ".");
        assert_eq!(component_key(Path::new("")), ".");
        assert_eq!(component_key(Path::new("engine")), "engine");
        assert_eq!(component_key(Path::new("engine/src")), "engine/src");
    }

    #[test]
    fn git_branch_returns_none_outside_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert!(git_branch(dir.path()).is_none());
    }
}
