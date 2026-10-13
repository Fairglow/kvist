//! Guided help: a short tour of the core concepts plus closed-set topics.
//!
//! Both the CLI (`kvist help [TOPIC]`) and the shell's `help [TOPIC]` builtin
//! render through [`render`], so the content cannot drift between the two
//! surfaces. Every topic ends with a pointer to the deeper documentation.

use clap::ValueEnum;

/// A closed set of help topics, tab-completable on the CLI and in the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum HelpTopic {
    /// Core concepts and the artifact set.
    Concepts,
    /// Component and task lifecycle with the exact commands.
    Lifecycle,
    /// Task states, legal transitions, and the command that achieves each.
    TaskStates,
}

impl HelpTopic {
    /// Every topic name in definition order (completion and help lists).
    pub const fn names() -> &'static [&'static str] {
        &["concepts", "lifecycle", "task-states"]
    }

    /// The command-line spelling of this topic.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Concepts => "concepts",
            Self::Lifecycle => "lifecycle",
            Self::TaskStates => "task-states",
        }
    }
}

/// Renders the guided tour (no topic) or a single topic.
pub fn render(topic: Option<HelpTopic>) -> String {
    match topic {
        None => tour(),
        Some(HelpTopic::Concepts) => concepts(),
        Some(HelpTopic::Lifecycle) => lifecycle(),
        Some(HelpTopic::TaskStates) => task_states(),
    }
}

/// Resolves a topic name, returning the rendered text and whether it was an
/// unknown topic (for the shell's usage-error handling).
pub fn render_named(name: &str) -> (String, bool) {
    match HelpTopic::from_str(name, true) {
        Ok(topic) => (render(Some(topic)), false),
        Err(_) => (
            format!(
                "unknown help topic `{name}`; available: {}",
                HelpTopic::names().join(", ")
            ),
            true,
        ),
    }
}

fn tour() -> String {
    "\
Kvist — concepts and workflow in 30 seconds

Core concepts (kvist help concepts):
  project    a directory with kvist.toml + VISION.md, ARCHITECTURE.md, ROOT_CONTRACT.md
  component  a directory owning REQUIREMENTS.md, CONTRACT.md, DESIGN.md, TODOS.yaml, IMPL.md
  intent     the three documents above; `kvist component accept` records their baseline
  tasks      durable queue entries in TODOS.yaml with status and dependencies

Component lifecycle (kvist help lifecycle):
  component new -> component validate -> component accept -> task run -> task finalize
  intent changed after accept = STALE -> review the diff -> component accept

Topics:
  kvist help concepts      core concepts and the artifact set
  kvist help lifecycle     component and task lifecycle with the exact commands
  kvist help task-states   task states, legal transitions, and how each is achieved

More: kvist --help (command reference) · GUIDE.md · docs/command-set.md
"
    .to_owned()
}

fn concepts() -> String {
    "\
Kvist core concepts

  project      a directory with kvist.toml + the root artifacts (VISION.md,
               ARCHITECTURE.md, ROOT_CONTRACT.md). The root is found by walking
               up from the current directory, like git.
  component    a directory owning its five artifacts: REQUIREMENTS.md (outcomes
               and acceptance criteria), CONTRACT.md (consumer-visible
               interface), DESIGN.md (private structure), TODOS.yaml (the
               durable task queue), IMPL.md (observed-behavior record).
  intent       REQUIREMENTS/CONTRACT/DESIGN describe what MUST hold before code
               changes. `kvist component accept [DIR]` records their revisions
               as the baseline; later edits make the component STALE until it
               is reviewed and re-accepted.
  tasks        entries in TODOS.yaml: id, kind (test, implementation,
               security-audit, compliance-review), status, depends_on, and
               requirement references. A task is ready when pending with every
               (transitive) dependency completed: `kvist task next [DIR]`.
  supervision  `kvist task run [DIR] [TASK]` executes a task with a supervised
               agent under the sandbox and approved test policy; results are
               verified and human finalization records the disposition
               (`kvist task finalize [DIR] TASK ATTEMPT accept|block`).

More: GUIDE.md · docs/command-set.md §2 · a component's CONTRACT.md
"
    .to_owned()
}

fn lifecycle() -> String {
    "\
Kvist component lifecycle

  1. kvist component new [DIR]    scaffold REQUIREMENTS/CONTRACT/DESIGN
  2. kvist component validate     structural check of the three intent docs
  3. kvist component accept [DIR] record accepted revisions (the baseline)
  4. kvist task run [DIR] [TASK]  work the queue (see `kvist help task-states`)
  5. kvist task finalize [DIR] TASK ATTEMPT accept|block   human disposition

  Intent changed after accept -> the component is STALE:
  review the diff (git diff HEAD -- <DOC>), then `kvist component accept [DIR]`.

  `kvist status` shows the exact action for every non-current state, and
  `kvist help task-states` explains the task states that status reports.

More: GUIDE.md §1-6 · docs/command-set.md
"
    .to_owned()
}

fn task_states() -> String {
    "\
Kvist task states (TODOS.yaml `status`)

  pending            defined; no attempt active
  in-progress        one authorized attempt active
  blocked            cannot progress until its recorded blocked_reason is resolved
  awaiting-decision  controlled pause pending a human decision (not a failure)
  completed          outcome achieved; terminal

Legal transitions
  pending            -> in-progress | blocked
  in-progress        -> pending | blocked | completed | awaiting-decision
  blocked            -> pending | in-progress
  awaiting-decision  -> pending | in-progress
  completed          -> (none; terminal)

How to achieve them
  pending -> in-progress      kvist task run [DIR] [TASK]  (ready: deps completed)
  in-progress -> completed    automatic after test verification, or
                              kvist task finalize [DIR] TASK ATTEMPT accept
  in-progress -> blocked      automatic when verification fails, or
                              kvist task transition [DIR] TASK blocked --reason \"...\"
  in-progress -> awaiting-decision
                              kvist task transition [DIR] TASK awaiting-decision --reason \"...\"
  blocked -> pending          fix the blocker (read blocked_reason in `kvist status`
                              or the shell `tasks` builtin; e.g. add [[test_policy.commands]]
                              to kvist.toml and run `kvist task approve-policy`), then
                              kvist task transition [DIR] TASK pending
  blocked -> in-progress      kvist task run [DIR] TASK  (dependency chain completed)
  awaiting-decision -> ...    decide, update intent, re-accept, then
                              kvist task transition [DIR] TASK in-progress
  in-progress -> pending      kvist task transition [DIR] TASK pending  (reset)

More: GUIDE.md §7 · docs/command-set.md §4.2
"
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_names_are_the_closed_set() {
        assert_eq!(
            HelpTopic::names(),
            &["concepts", "lifecycle", "task-states"]
        );
        for name in HelpTopic::names() {
            let (text, failed) = render_named(name);
            assert!(!failed, "topic `{name}` must resolve");
            assert!(!text.is_empty());
        }
    }

    #[test]
    fn every_topic_points_at_deeper_documentation() {
        for topic in [
            Option::<HelpTopic>::None,
            Some(HelpTopic::Concepts),
            Some(HelpTopic::Lifecycle),
            Some(HelpTopic::TaskStates),
        ] {
            let text = render(topic);
            assert!(
                text.contains("GUIDE.md") || text.contains("docs/command-set.md"),
                "topic {topic:?} must point at the deeper documentation"
            );
        }
    }

    #[test]
    fn unknown_topic_is_reported_with_the_closed_set() {
        let (text, failed) = render_named("bogus");
        assert!(failed);
        for name in HelpTopic::names() {
            assert!(text.contains(name), "error missing topic `{name}`");
        }
    }
}
