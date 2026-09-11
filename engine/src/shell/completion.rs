//! The tab-completion engine.
//!
//! Clap's derived command surface is the single source of truth for the
//! *static* part of completion (verbs, flags, enum literals); the
//! [`CommandNode`] tree is built once from the same `clap::Command` the parser
//! uses, so completions can never drift from the parseable surface. Dynamic
//! values (component paths, task IDs, attempt IDs, model names, the active
//! branch) are resolved in memory from a [`DynamicState`] snapshot.
//!
//! The engine is a pure function of `(line, cursor)` and is fully testable
//! without a terminal. The [`reedline::Completer`] implementation at the
//! bottom is a thin adapter.

use std::sync::{Arc, Mutex};

use reedline::{Completer, Span, Suggestion};

use super::state::{DynamicState, ValueDomain};
use super::tree::{CommandNode, FlagSpec, PositionalSpec};

/// A completion candidate and the byte span of the token it replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The text to insert.
    pub value: String,
    /// Optional one-line description shown beside the candidate.
    pub description: Option<String>,
    /// Byte span in the line of the token being replaced.
    pub span: (usize, usize),
    /// Whether a trailing space is appended on selection.
    pub append_whitespace: bool,
}

/// Resolves one token being typed into its dynamic value domain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Scope {
    /// The component path already typed, if any.
    component: Option<String>,
    /// The task ID already typed, if any.
    task: Option<String>,
}

/// The tab-completion engine for the Kvist shell.
///
/// The static tree is fixed for the session; the dynamic snapshot is shared
/// behind an `Arc<Mutex>` so the shell can refresh it after each command while
/// the line editor keeps its own copy of the completer.
pub struct KvistCompleter {
    root: CommandNode,
    state: Arc<Mutex<DynamicState>>,
}

impl KvistCompleter {
    /// Builds a completer over a static command tree and a shared snapshot.
    pub fn new(root: CommandNode, state: Arc<Mutex<DynamicState>>) -> Self {
        Self { root, state }
    }

    /// Replaces the dynamic snapshot the completer resolves against.
    #[allow(dead_code)]
    pub fn refresh(&self, state: DynamicState) {
        if let Ok(mut guard) = self.state.lock() {
            *guard = state;
        }
    }

    /// Resolves completions against this completer's current shared state.
    #[allow(dead_code)]
    pub fn complete(&self, line: &str, pos: usize) -> Vec<Candidate> {
        let Ok(guard) = self.state.lock() else {
            return Vec::new();
        };
        Self::resolve(&self.root, &guard, line, pos)
    }

    /// Resolves completions for the text under the cursor.
    ///
    /// `line` is the full buffer and `pos` the cursor's byte offset. Pure and
    /// deterministic: it performs no I/O and reads no shared state, so it is
    /// the unit under test.
    pub fn resolve(
        root: &CommandNode,
        state: &DynamicState,
        line: &str,
        pos: usize,
    ) -> Vec<Candidate> {
        let pos = pos.min(line.len());
        let region = &line[..pos];
        let tokens = tokenize(region);

        // The token under the cursor is the last one unless the cursor sits on
        // trailing whitespace (in which case a fresh, empty token is starting).
        let (resolved, active_prefix, active_start) = match tokens.last() {
            Some(last) if last.end == pos => {
                let active = last.text.clone();
                let start = last.start;
                (&tokens[..tokens.len() - 1], active, start)
            }
            _ => (tokens.as_slice(), String::new(), pos),
        };

        let mut scope = Scope::default();
        let cursor = walk(root, resolved, &mut scope);
        let node = cursor.node;

        let span = (active_start, pos);
        if active_prefix.starts_with('-') {
            complete_flag(node, &active_prefix, state, span)
        } else if let Some(flag) = cursor.pending_value_flag() {
            // The previous token was a value-taking option; complete its value.
            complete_option_value(flag, &active_prefix, state, span)
        } else if let Some(positional) = node.positionals.get(cursor.positionals_consumed()) {
            complete_positional(positional, &scope, &active_prefix, state, span)
        } else {
            complete_subcommands(node, &active_prefix, span)
        }
    }
}

impl Completer for KvistCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let candidates = match self.state.lock() {
            Ok(guard) => Self::resolve(&self.root, &guard, line, pos),
            // A poisoned lock means a completer panicked while holding it;
            // degrade to no completions rather than abort the editor.
            Err(_) => Vec::new(),
        };
        candidates
            .into_iter()
            .map(|c| Suggestion {
                value: c.value,
                description: c.description,
                style: None,
                extra: None,
                span: Span::new(c.span.0, c.span.1),
                append_whitespace: c.append_whitespace,
            })
            .collect()
    }
}

/// Walks the command tree over the resolved (complete) tokens, returning the
/// node under the cursor and recording any typed component/task for scoping.
fn walk<'a>(root: &'a CommandNode, resolved: &[Token], scope: &mut Scope) -> Cursor<'a> {
    let mut node: &'a CommandNode = root;
    let mut positionals_consumed = 0usize;
    let mut pending_value: Option<String> = None;

    for token in resolved {
        let text = &token.text;
        if let Some(sub) = node.find_subcommand(text) {
            node = sub;
            positionals_consumed = 0;
            *scope = Scope::default();
            pending_value = None;
            continue;
        }

        if let Some(flag) = parse_flag(node, text) {
            // A value-taking option leaves the next bare token as its value,
            // unless the value was supplied inline (`--opt=value`).
            if flag.takes_value && flag.inline_value.is_none() {
                pending_value = flag.long;
            }
            continue;
        }

        // A bare value: either the value of a pending option or the next
        // positional.
        if let Some(long) = pending_value.take() {
            apply_option_value(node, &long, text, scope);
            continue;
        }
        if let Some(positional) = node.positionals.get(positionals_consumed) {
            positionals_consumed += 1;
            apply_positional_value(positional, text, scope);
        }
    }

    Cursor {
        node,
        positionals_consumed,
        pending_value,
    }
}

/// The state of the traversal at the cursor.
struct Cursor<'a> {
    node: &'a CommandNode,
    positionals_consumed: usize,
    pending_value: Option<String>,
}

impl<'a> Cursor<'a> {
    #[allow(dead_code)]
    fn node(&self) -> &'a CommandNode {
        self.node
    }
    fn positionals_consumed(&self) -> usize {
        self.positionals_consumed
    }
    fn pending_value_flag(&self) -> Option<&'a FlagSpec> {
        self.pending_value
            .as_deref()
            .and_then(|long| self.node.find_flag_by_long(long))
            .map(|idx| &self.node.flags[idx])
    }
}

/// Completes flag names when the token under the cursor starts with `-`.
fn complete_flag(
    node: &CommandNode,
    prefix: &str,
    state: &DynamicState,
    span: (usize, usize),
) -> Vec<Candidate> {
    // Inline value form: `--opt=part` completes the value, not the flag.
    if let Some((long, value_prefix)) = split_inline_value(prefix) {
        if let Some(flag) = node.find_flag_by_long(long) {
            return complete_option_value(&node.flags[flag], value_prefix, state, span);
        }
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for flag in &node.flags {
        if let Some(long) = &flag.long {
            let rendered = format!("--{long}");
            if rendered.starts_with(prefix) {
                candidates.push(Candidate {
                    value: rendered,
                    description: flag.help.clone(),
                    span,
                    append_whitespace: flag.takes_value,
                });
            }
        }
        if let Some(short) = flag.short {
            let rendered = format!("-{short}");
            if rendered.starts_with(prefix) {
                candidates.push(Candidate {
                    value: rendered,
                    description: flag.help.clone(),
                    span,
                    append_whitespace: flag.takes_value,
                });
            }
        }
    }
    dedupe(candidates)
}

/// Returns a human-friendly description for candidates in dynamic domains.
fn domain_description(domain: ValueDomain) -> &'static str {
    match domain {
        ValueDomain::Component => "Component directory",
        ValueDomain::Task => "Task ID",
        ValueDomain::Attempt => "Attempt ID",
        ValueDomain::Model => "Model profile",
        ValueDomain::Branch => "VCS branch",
    }
}

/// Completes the value of a value-taking option.
fn complete_option_value(
    flag: &FlagSpec,
    prefix: &str,
    state: &DynamicState,
    span: (usize, usize),
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for value in &flag.possible_values {
        if value.starts_with(prefix) {
            candidates.push(Candidate {
                value: value.clone(),
                description: flag.help.clone(),
                span,
                append_whitespace: false,
            });
        }
    }
    let empty_scope = Scope::default();
    if let Some(domain) = option_domain(flag.long.as_deref().unwrap_or("")) {
        let desc = domain_description(domain);
        for value in dynamic_values(domain, &empty_scope, prefix, state) {
            candidates.push(Candidate {
                value,
                description: Some(desc.to_owned()),
                span,
                append_whitespace: false,
            });
        }
    }
    dedupe(candidates)
}

/// Completes the value of the positional under the cursor.
fn complete_positional(
    positional: &PositionalSpec,
    scope: &Scope,
    prefix: &str,
    state: &DynamicState,
    span: (usize, usize),
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for value in &positional.possible_values {
        if value.starts_with(prefix) {
            candidates.push(Candidate {
                value: value.clone(),
                description: positional
                    .help
                    .clone()
                    .or_else(|| positional.value_name.clone()),
                span,
                append_whitespace: false,
            });
        }
    }
    if let Some(domain) = positional_domain(positional.value_name.as_deref().unwrap_or("")) {
        let desc = domain_description(domain);
        for value in dynamic_values(domain, scope, prefix, state) {
            candidates.push(Candidate {
                value,
                description: Some(desc.to_owned()),
                span,
                append_whitespace: false,
            });
        }
    }
    dedupe(candidates)
}

/// Completes subcommand names when no positional is expected.
fn complete_subcommands(node: &CommandNode, prefix: &str, span: (usize, usize)) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for sub in &node.subcommands {
        if sub.name.starts_with(prefix) {
            candidates.push(Candidate {
                value: sub.name.clone(),
                description: sub.help.clone(),
                span,
                append_whitespace: true,
            });
        }
    }
    dedupe(candidates)
}

/// Routes an option (by long name) to a dynamic value domain.
fn option_domain(long: &str) -> Option<ValueDomain> {
    match long {
        "model" => Some(ValueDomain::Model),
        "branch" => Some(ValueDomain::Branch),
        _ => None,
    }
}

/// Routes a positional (by value name) to a dynamic value domain.
fn positional_domain(value_name: &str) -> Option<ValueDomain> {
    match value_name {
        "COMPONENT_DIR" => Some(ValueDomain::Component),
        "TASK_ID" => Some(ValueDomain::Task),
        "ATTEMPT_ID" => Some(ValueDomain::Attempt),
        _ => None,
    }
}

/// Resolves the dynamic candidates for one domain under a typed scope.
fn dynamic_values(
    domain: ValueDomain,
    scope: &Scope,
    prefix: &str,
    state: &DynamicState,
) -> Vec<String> {
    let all: Vec<String> = match domain {
        ValueDomain::Component => state.components().to_vec(),
        ValueDomain::Task => match &scope.component {
            Some(component) => state.task_ids_for(component),
            None => state.all_task_ids(),
        },
        ValueDomain::Attempt => match (&scope.component, &scope.task) {
            (Some(component), Some(task)) => state.attempts_for(component, task).to_vec(),
            _ => Vec::new(),
        },
        ValueDomain::Model => state.models().to_vec(),
        ValueDomain::Branch => state
            .branch()
            .into_iter()
            .map(|branch| branch.to_owned())
            .collect(),
    };
    all.into_iter()
        .filter(|value| value.starts_with(prefix))
        .collect()
}

/// Records a typed option value into the scope when it carries domain meaning.
fn apply_option_value(node: &CommandNode, long: &str, value: &str, scope: &mut Scope) {
    let Some(flag) = node.find_flag_by_long(long) else {
        return;
    };
    let Some(domain) = option_domain(long) else {
        return;
    };
    match domain {
        ValueDomain::Component => scope.component = Some(value.to_owned()),
        ValueDomain::Task => scope.task = Some(value.to_owned()),
        _ => {}
    }
    let _ = flag;
}

/// Records a typed positional value into the scope when it carries domain meaning.
fn apply_positional_value(positional: &PositionalSpec, value: &str, scope: &mut Scope) {
    let Some(domain) = positional_domain(positional.value_name.as_deref().unwrap_or("")) else {
        return;
    };
    match domain {
        ValueDomain::Component => {
            scope.component = Some(value.to_owned());
            scope.task = None;
        }
        ValueDomain::Task => scope.task = Some(value.to_owned()),
        _ => {}
    }
}

/// Splits `--long=value` into `(long, value_prefix)` when present.
fn split_inline_value(prefix: &str) -> Option<(&str, &str)> {
    let after_dashes = prefix.strip_prefix("--")?;
    let (long, value) = after_dashes.split_once('=')?;
    Some((long, value))
}

/// The parsed form of a flag token, as needed by the traversal.
struct ParsedFlag {
    /// The long name of the matched flag, if it has one.
    long: Option<String>,
    /// Whether the flag consumes a following value.
    takes_value: bool,
    /// The inline value when the token was `--opt=value`.
    inline_value: Option<String>,
}

/// Parses a token as a flag of `node`, if it names one.
fn parse_flag(node: &CommandNode, text: &str) -> Option<ParsedFlag> {
    if let Some(long_with_eq) = text.strip_prefix("--") {
        let (long, inline) = match long_with_eq.split_once('=') {
            Some((l, v)) => (l, Some(v.to_owned())),
            None => (long_with_eq, None),
        };
        let idx = node.find_flag_by_long(long)?;
        return Some(ParsedFlag {
            long: Some(long.to_owned()),
            takes_value: node.flags[idx].takes_value,
            inline_value: inline,
        });
    }
    if let Some(short) = text.strip_prefix('-').and_then(|rest| rest.chars().next())
        && text.len() == 2
    {
        let idx = node.find_flag_by_short(short)?;
        return Some(ParsedFlag {
            long: node.flags[idx].long.clone(),
            takes_value: node.flags[idx].takes_value,
            inline_value: None,
        });
    }
    None
}

/// A token from the line under the cursor, with its byte span.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    text: String,
    start: usize,
    end: usize,
}

/// Splits `region` into quote-aware tokens with byte spans.
fn tokenize(region: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for (i, c) in region.char_indices() {
        match (quote, c) {
            (None, q @ ('\'' | '"')) => {
                start.get_or_insert(i);
                quote = Some(q);
            }
            (Some(q), c) if c == q => {
                quote = None;
            }
            (Some(_), c) => {
                current.push(c);
            }
            (None, c) if c.is_whitespace() => {
                if let Some(start) = start.take() {
                    tokens.push(Token {
                        text: std::mem::take(&mut current),
                        start,
                        end: i,
                    });
                }
            }
            (None, c) => {
                if start.is_none() {
                    start = Some(i);
                }
                current.push(c);
            }
        }
    }
    if let Some(start) = start {
        tokens.push(Token {
            text: current,
            start,
            end: region.len(),
        });
    }
    tokens
}

/// Removes duplicate candidates, preserving first-occurrence order.
fn dedupe(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut seen = std::collections::BTreeSet::new();
    candidates
        .into_iter()
        .filter(|c| seen.insert(c.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::state::DynamicState;
    use crate::shell::tree::build_root;
    use std::sync::{Arc, Mutex};

    fn completer() -> KvistCompleter {
        KvistCompleter::new(build_root(), Arc::new(Mutex::new(DynamicState::default())))
    }

    fn values(candidates: &[Candidate]) -> Vec<&str> {
        candidates.iter().map(|c| c.value.as_str()).collect()
    }

    fn complete(c: &KvistCompleter, line: &str) -> Vec<Candidate> {
        c.complete(line, line.len())
    }

    // ---- Stage 1: first-order command verbs -------------------------------

    #[test]
    fn empty_line_offers_every_first_order_command() {
        let c = completer();
        let res = complete(&c, "");
        let got = values(&res);
        for name in [
            "shell",
            "init",
            "convert",
            "import",
            "tree",
            "doctor",
            "reverse-discover",
            "prompt",
            "status",
            "task",
            "component",
            "vcs",
            "agent",
            "completions",
        ] {
            assert!(got.contains(&name), "missing {name} in {got:?}");
        }
        assert_eq!(got.len(), 14);
    }

    #[test]
    fn partial_verb_prefix_filters_first_order_commands() {
        let c = completer();
        assert_eq!(values(&complete(&c, "ta")), vec!["task"]);
        assert_eq!(
            values(&complete(&c, "comp")),
            vec!["component", "completions"]
        );
        assert_eq!(values(&complete(&c, "nope")), Vec::<&str>::new());
    }

    #[test]
    fn verb_completion_respects_the_cursor_position() {
        let c = completer();
        // Cursor after "ta" in the middle of a longer line.
        let res = c.complete("task run", 2);
        let got = values(&res);
        assert_eq!(got, vec!["task"]);
    }

    // ---- Stage 2: per-command flags ---------------------------------------

    #[test]
    fn root_dash_offers_global_and_synthesized_flags() {
        let c = completer();
        let res = complete(&c, "-");
        let got = values(&res);
        assert!(got.contains(&"--json"));
        assert!(got.contains(&"--help"));
        assert!(got.contains(&"--version"));
        assert!(got.contains(&"-h"));
    }

    #[test]
    fn long_flag_prefix_filters() {
        let c = completer();
        let res_j = complete(&c, "--j");
        let got = values(&res_j);
        assert_eq!(got, vec!["--json"]);
        let res_h = complete(&c, "--h");
        let got = values(&res_h);
        assert_eq!(got, vec!["--help"]);
    }

    #[test]
    fn command_flags_complete_after_the_verb() {
        let c = completer();
        let res = complete(&c, "status --");
        let got = values(&res);
        for flag in [
            "--format",
            "--only-documents",
            "--only-impls",
            "--unfinished",
            "--json",
            "--help",
        ] {
            assert!(got.contains(&flag), "missing {flag} in {got:?}");
        }
    }

    // ---- Stage 3: subcommand recursion ------------------------------------

    #[test]
    fn task_offers_its_subcommands() {
        let c = completer();
        let res = complete(&c, "task ");
        let got = values(&res);
        for name in [
            "next",
            "transition",
            "run",
            "log",
            "replay",
            "approve-policy",
            "unlock",
            "recover",
            "finalize",
        ] {
            assert!(got.contains(&name), "missing task {name} in {got:?}");
        }
        assert_eq!(got.len(), 9);
    }

    #[test]
    fn nested_subcommand_prefix_filters() {
        let c = completer();
        assert_eq!(values(&complete(&c, "task t")), vec!["transition"]);
        assert_eq!(values(&complete(&c, "task f")), vec!["finalize"]);
        assert_eq!(values(&complete(&c, "component a")), vec!["accept"]);
    }

    // ---- Stage 4: static positional values --------------------------------

    #[test]
    fn transition_status_is_a_closed_set() {
        let c = completer();
        let res = complete(&c, "task transition . write-tests ");
        let got = values(&res);
        assert_eq!(got, vec!["pending", "in-progress", "blocked", "completed"]);
    }

    #[test]
    fn transition_status_prefix_filters() {
        let c = completer();
        assert_eq!(
            values(&complete(&c, "task transition . write-tests c")),
            vec!["completed"]
        );
    }

    #[test]
    fn finalize_disposition_is_a_closed_set() {
        let c = completer();
        let res = complete(&c, "task finalize . write-tests attempt-0001 ");
        let got = values(&res);
        assert_eq!(got, vec!["accept", "block"]);
    }

    #[test]
    fn completions_shell_is_a_closed_set() {
        let c = completer();
        assert_eq!(
            values(&complete(&c, "completions ")),
            vec!["bash", "zsh", "fish", "powershell"]
        );
    }

    #[test]
    fn option_value_completes_from_its_possible_values() {
        let c = completer();
        assert_eq!(
            values(&complete(&c, "status --format ")),
            vec!["text", "json"]
        );
        assert_eq!(values(&complete(&c, "status --format j")), vec!["json"]);
    }

    #[test]
    fn inline_option_value_completes() {
        let c = completer();
        assert_eq!(
            values(&complete(&c, "status --format=")),
            vec!["text", "json"]
        );
        assert_eq!(values(&complete(&c, "status --format=j")), vec!["json"]);
    }

    // ---- Stage 5: dynamic values ------------------------------------------

    use super::super::state::ComponentScope;
    use crate::task_queue::{Task, TaskKind, TaskStatus, TaskTimestamps, Timestamp};
    use std::collections::BTreeMap;

    /// A minimal task record for fixtures.
    fn task(id: &str) -> Task {
        Task {
            id: id.to_owned(),
            title: id.to_owned(),
            description: String::new(),
            context: String::new(),
            purpose: String::new(),
            expected_outcome: String::new(),
            kind: TaskKind::Test,
            status: TaskStatus::Pending,
            depends_on: Vec::new(),
            requirements: Vec::new(),
            timestamps: TaskTimestamps {
                created_at: Timestamp::default(),
                updated_at: Timestamp::default(),
                completed_at: None,
            },
            blocked_reason: None,
            recovery_state: None,
            acceptance_id: None,
            disposition: None,
        }
    }

    /// A snapshot with two components, tasks, attempts, models, and a branch.
    fn fixture_state() -> DynamicState {
        let mut scopes = BTreeMap::new();
        let mut root_attempts = BTreeMap::new();
        root_attempts.insert(
            "write-tests".into(),
            vec!["attempt-0001".into(), "attempt-0002".into()],
        );
        let root = ComponentScope {
            tasks: vec![task("write-tests"), task("implement-code")],
            attempts: root_attempts,
        };
        scopes.insert(".".into(), root);

        let engine = ComponentScope {
            tasks: vec![task("build-tree")],
            attempts: BTreeMap::new(),
        };
        scopes.insert("engine".into(), engine);

        DynamicState::new(
            vec![".".into(), "engine".into()],
            scopes,
            vec!["llama-cli".into(), "ollama".into()],
            Some("feature/shell".into()),
        )
    }

    fn completer_with_state(state: DynamicState) -> KvistCompleter {
        KvistCompleter::new(build_root(), Arc::new(Mutex::new(state)))
    }

    #[test]
    fn component_positional_offers_component_paths() {
        let c = completer_with_state(fixture_state());
        assert_eq!(values(&complete(&c, "task run ")), vec![".", "engine"]);
    }

    #[test]
    fn component_positional_prefix_filters() {
        let c = completer_with_state(fixture_state());
        assert_eq!(values(&complete(&c, "task run e")), vec!["engine"]);
    }

    #[test]
    fn task_positional_is_scoped_to_the_typed_component() {
        let c = completer_with_state(fixture_state());
        assert_eq!(
            values(&complete(&c, "task run . ")),
            vec!["write-tests", "implement-code"]
        );
        assert_eq!(
            values(&complete(&c, "task run engine ")),
            vec!["build-tree"]
        );
    }

    #[test]
    fn task_positional_prefix_filters_within_scope() {
        let c = completer_with_state(fixture_state());
        assert_eq!(
            values(&complete(&c, "task run . i")),
            vec!["implement-code"]
        );
        assert_eq!(values(&complete(&c, "task run . w")), vec!["write-tests"]);
    }

    #[test]
    fn attempt_positional_is_scoped_to_component_and_task() {
        let c = completer_with_state(fixture_state());
        assert_eq!(
            values(&complete(&c, "task finalize . write-tests ")),
            vec!["attempt-0001", "attempt-0002"]
        );
        // A task with no attempts offers nothing.
        assert!(complete(&c, "task finalize . implement-code ").is_empty());
        // A task in another component does not leak its attempts here.
        assert!(complete(&c, "task finalize engine write-tests ").is_empty());
    }

    #[test]
    fn model_option_offers_model_names() {
        let c = completer_with_state(fixture_state());
        assert_eq!(
            values(&complete(&c, "prompt --model ")),
            vec!["llama-cli", "ollama"]
        );
        assert_eq!(values(&complete(&c, "prompt --model o")), vec!["ollama"]);
    }

    #[test]
    fn branch_option_offers_the_active_branch() {
        let c = completer_with_state(fixture_state());
        assert_eq!(
            values(&complete(&c, "import --branch ")),
            vec!["feature/shell"]
        );
        assert_eq!(
            values(&complete(&c, "import --branch feat")),
            vec!["feature/shell"]
        );
        assert!(complete(&c, "import --branch main").is_empty());
    }

    #[test]
    fn dynamic_values_do_not_leak_across_components() {
        let c = completer_with_state(fixture_state());
        // The root's tasks must not appear when completing engine's tasks.
        assert_eq!(
            values(&complete(&c, "task run engine ")),
            vec!["build-tree"]
        );
        // The root's attempts must not appear under engine's tasks.
        assert!(complete(&c, "task finalize engine build-tree ").is_empty());
    }

    #[test]
    fn empty_state_offers_no_dynamic_values() {
        let c = completer();
        assert!(complete(&c, "task run ").is_empty());
        assert!(complete(&c, "task run . ").is_empty());
        assert!(complete(&c, "prompt --model ").is_empty());
        assert!(complete(&c, "import --branch ").is_empty());
    }
}
