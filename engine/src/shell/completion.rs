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

use reedline::{Completer, CompletionResult, Span, Suggestion};

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
    /// The active subcommand name (e.g. "run"), if any.
    command: Option<String>,
    /// The component path already typed, if any.
    component: Option<String>,
    /// The task ID already typed, if any.
    task: Option<String>,
}

/// A dynamic candidate: the value to insert plus its rich description
/// (e.g. a task's status and title) shown beside the candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValueCandidate {
    /// The text to insert.
    value: String,
    /// A human description of the value, when one can be derived.
    description: Option<String>,
}

/// The tab-completion engine for the Kvist shell.
///
/// The static tree is fixed for the session; the dynamic snapshot is shared
/// behind an `Arc<Mutex>` so the shell can refresh it after each command while
/// the line editor keeps its own copy of the completer.
pub struct KvistCompleter {
    root: CommandNode,
    state: Arc<Mutex<DynamicState>>,
    /// The shell's current component, shared with the REPL so `cd` reorders
    /// completions without rebuilding the editor.
    current_component: Arc<Mutex<Option<String>>>,
}

impl KvistCompleter {
    /// Builds a completer over a static command tree and a shared snapshot.
    pub fn new(
        root: CommandNode,
        state: Arc<Mutex<DynamicState>>,
        current_component: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            root,
            state,
            current_component,
        }
    }

    /// Resolves completions against this completer's current shared state.
    #[allow(dead_code)]
    pub fn complete(&self, line: &str, pos: usize) -> Vec<Candidate> {
        let (Ok(guard), Ok(current)) = (self.state.lock(), self.current_component.lock()) else {
            return Vec::new();
        };
        Self::resolve(&self.root, &guard, current.as_deref(), line, pos)
    }

    /// Resolves completions for the text under the cursor.
    ///
    /// `line` is the full buffer and `pos` the cursor's byte offset. Pure and
    /// deterministic: it performs no I/O and reads no shared state, so it is
    /// the unit under test. `current_component` (from `cd`) is offered first
    /// in component-scoped domains.
    pub fn resolve(
        root: &CommandNode,
        state: &DynamicState,
        current_component: Option<&str>,
        line: &str,
        pos: usize,
    ) -> Vec<Candidate> {
        let pos = pos.min(line.len());
        let region = &line[..pos];
        let tokens = tokenize(region);

        // The token under the cursor is the last one unless the cursor sits on
        // trailing whitespace (in which case a fresh, empty token is starting).
        // A complete `--` separator is consumed rather than treated as a
        // flag prefix: Tab after `--` completes the next positional.
        let (resolved, active_prefix, active_start) = match tokens.last() {
            Some(last) if last.end == pos && last.text != "--" => {
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
        if cursor.no_flags {
            // After `--` only positional values remain; a dash-leading token
            // is a value, not a flag.
            if let Some(positional) = node.positionals.get(cursor.positionals_consumed()) {
                complete_positional(
                    positional,
                    &scope,
                    &active_prefix,
                    state,
                    current_component,
                    span,
                )
            } else {
                Vec::new()
            }
        } else if active_prefix.starts_with('-') {
            complete_flag(node, &active_prefix, state, current_component, span)
        } else if let Some(flag) = cursor.pending_value_flag() {
            // The previous token was a value-taking option; complete its value.
            complete_option_value(flag, &active_prefix, state, current_component, span)
        } else if let Some(positional) = node.positionals.get(cursor.positionals_consumed()) {
            complete_positional(
                positional,
                &scope,
                &active_prefix,
                state,
                current_component,
                span,
            )
        } else {
            complete_subcommands(node, &active_prefix, span)
        }
    }
}

impl Completer for KvistCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        // A poisoned lock means a completer panicked while holding it;
        // degrade to no completions rather than abort the editor.
        let candidates = match (self.state.lock(), self.current_component.lock()) {
            (Ok(guard), Ok(current)) => {
                Self::resolve(&self.root, &guard, current.as_deref(), line, pos)
            }
            _ => Vec::new(),
        };
        // `display_override`/`match_indices` describe fuzzy-match highlighting
        // UX this completer does not compute, so they stay unset. `fresh` marks
        // the result authoritative, so the menu renders immediately without
        // waiting on a follow-up poll (see `poll_completion`).
        let suggestions: Vec<Suggestion> = candidates
            .into_iter()
            .map(|c| Suggestion {
                value: c.value,
                display_override: None,
                description: c.description,
                style: None,
                extra: None,
                span: Span::new(c.span.0, c.span.1),
                append_whitespace: c.append_whitespace,
                match_indices: None,
            })
            .collect();
        CompletionResult::fresh(suggestions)
    }
}

/// Walks the command tree over the resolved (complete) tokens, returning the
/// node under the cursor and recording any typed component/task for scoping.
/// A `--` token ends flag parsing: everything after it is a positional value.
fn walk<'a>(root: &'a CommandNode, resolved: &[Token], scope: &mut Scope) -> Cursor<'a> {
    let mut node: &'a CommandNode = root;
    let mut positionals_consumed = 0usize;
    let mut pending_value: Option<String> = None;
    let mut no_flags = false;

    for token in resolved {
        let text = &token.text;
        if no_flags {
            if let Some(positional) = node.positionals.get(positionals_consumed) {
                positionals_consumed += 1;
                apply_positional_value(positional, text, scope);
            }
            continue;
        }
        if text == "--" {
            no_flags = true;
            continue;
        }
        if let Some(sub) = node.find_subcommand(text) {
            node = sub;
            positionals_consumed = 0;
            *scope = Scope::default();
            scope.command = Some(sub.name.clone());
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
        no_flags,
    }
}

/// The state of the traversal at the cursor.
struct Cursor<'a> {
    node: &'a CommandNode,
    positionals_consumed: usize,
    pending_value: Option<String>,
    no_flags: bool,
}

impl<'a> Cursor<'a> {
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
    current_component: Option<&str>,
    span: (usize, usize),
) -> Vec<Candidate> {
    // Inline value form: `--opt=part` completes the value, not the flag.
    if let Some((long, value_prefix)) = split_inline_value(prefix) {
        if let Some(flag) = node.find_flag_by_long(long) {
            return complete_option_value(
                &node.flags[flag],
                value_prefix,
                state,
                current_component,
                span,
            );
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

/// Returns a human-friendly description for candidates in dynamic domains
/// whose values carry no per-value detail.
fn domain_description(domain: ValueDomain) -> &'static str {
    match domain {
        ValueDomain::Component => "Component directory",
        ValueDomain::Task => "Task ID",
        ValueDomain::Attempt => "Attempt ID",
        ValueDomain::Model => "Model profile",
        ValueDomain::Role => "Agent role",
        ValueDomain::Branch => "VCS branch",
    }
}

/// Builds the rich description for one task candidate: the next-ready marker
/// (in run contexts) plus the task's status and truncated title.
fn task_description(
    state: &DynamicState,
    component: &str,
    task_id: &str,
    next_ready: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if next_ready.is_some_and(|ready| ready == task_id) {
        parts.push("★ next ready".to_owned());
    }
    if let Some(label) = state.task_label(component, task_id) {
        parts.push(label);
    }
    if parts.is_empty() {
        "Task ID".to_owned()
    } else {
        parts.join(" · ")
    }
}

/// Completes the value of a value-taking option.
fn complete_option_value(
    flag: &FlagSpec,
    prefix: &str,
    state: &DynamicState,
    current_component: Option<&str>,
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
        for candidate in dynamic_values(domain, &empty_scope, prefix, state, current_component) {
            candidates.push(Candidate {
                description: candidate.description.or_else(|| Some(desc.to_owned())),
                value: candidate.value,
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
    current_component: Option<&str>,
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
        for candidate in dynamic_values(domain, scope, prefix, state, current_component) {
            candidates.push(Candidate {
                description: candidate.description.or_else(|| Some(desc.to_owned())),
                value: candidate.value,
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
        "MODEL_NAME" | "MODEL" => Some(ValueDomain::Model),
        "ROLE" => Some(ValueDomain::Role),
        _ => None,
    }
}

/// Resolves the dynamic candidates for one domain under a typed scope.
///
/// When the shell has a current component (via `cd`) and the scope does not
/// name one, the current component's values are offered first. Candidates
/// carry a rich description (task status/title, the next-ready marker, the
/// current component) shown beside the value in the completion menu.
fn dynamic_values(
    domain: ValueDomain,
    scope: &Scope,
    prefix: &str,
    state: &DynamicState,
    current_component: Option<&str>,
) -> Vec<ValueCandidate> {
    let candidates: Vec<ValueCandidate> = match domain {
        ValueDomain::Component => {
            let mut components = state.components().to_vec();
            if let Some(current) = current_component
                && let Some(position) = components.iter().position(|c| c == current)
            {
                let promoted = components.remove(position);
                components.insert(0, promoted);
            }
            components
                .into_iter()
                .map(|component| ValueCandidate {
                    description: current_component
                        .is_some_and(|current| current == component)
                        .then(|| "current component".to_owned()),
                    value: component,
                })
                .collect()
        }
        ValueDomain::Task => {
            let runnable_only = scope.command.as_deref() == Some("run");
            let pairs = match &scope.component {
                Some(component) => state
                    .scopes
                    .get(component)
                    .map(|_| {
                        let ids = if runnable_only {
                            state.runnable_task_ids_for(component)
                        } else {
                            state.task_ids_for(component)
                        };
                        ids.into_iter()
                            .map(|id| (component.to_owned(), id))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                None => ordered_task_pairs(state, current_component, runnable_only),
            };
            pairs
                .into_iter()
                .filter(|(_, id)| id.starts_with(prefix))
                .map(|(component, id)| {
                    let next_ready = if runnable_only {
                        state
                            .scopes
                            .get(&component)
                            .and_then(|scope| crate::task_queue::next_ready_task_id(&scope.tasks))
                    } else {
                        None
                    };
                    let description =
                        task_description(state, &component, &id, next_ready.as_deref());
                    ValueCandidate {
                        value: id,
                        description: Some(description),
                    }
                })
                .collect()
        }
        ValueDomain::Attempt => match (&scope.component, &scope.task) {
            (Some(component), Some(task)) => state
                .attempts_for(component, task)
                .iter()
                .map(|attempt| ValueCandidate {
                    value: attempt.clone(),
                    description: None,
                })
                .collect(),
            _ => Vec::new(),
        },
        ValueDomain::Model => state
            .models()
            .iter()
            .map(|model| ValueCandidate {
                value: model.clone(),
                description: None,
            })
            .collect(),
        ValueDomain::Role => ["developer", "architect", "security-reviewer"]
            .into_iter()
            .map(|role| ValueCandidate {
                value: role.to_owned(),
                description: None,
            })
            .collect(),
        ValueDomain::Branch => state
            .branch()
            .into_iter()
            .map(|branch| ValueCandidate {
                value: branch.to_owned(),
                description: None,
            })
            .collect(),
    };
    candidates
        .into_iter()
        .filter(|candidate| candidate.value.starts_with(prefix))
        .collect()
}

/// Every component's task IDs in component-then-queue order, with the
/// current component's tasks promoted to the front when one is set.
fn ordered_task_pairs(
    state: &DynamicState,
    current_component: Option<&str>,
    runnable_only: bool,
) -> Vec<(String, String)> {
    let pick = |component: &str| -> Vec<(String, String)> {
        let ids = if runnable_only {
            state.runnable_task_ids_for(component)
        } else {
            state.task_ids_for(component)
        };
        ids.into_iter()
            .map(|id| (component.to_owned(), id))
            .collect()
    };
    let mut pairs = Vec::new();
    if let Some(current) = current_component
        && state.components().iter().any(|c| c == current)
    {
        pairs.extend(pick(current));
    }
    for component in state.components() {
        if Some(component.as_str()) == current_component {
            continue;
        }
        pairs.extend(pick(component));
    }
    pairs
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
        KvistCompleter::new(
            build_root(),
            Arc::new(Mutex::new(DynamicState::default())),
            Arc::new(Mutex::new(None)),
        )
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
            "overview",
            "task",
            "component",
            "vcs",
            "agent",
            "completions",
            "vendor",
            "toolchain",
            "cd",
            "tasks",
            "run",
            "help",
            "last",
            "history",
            "journal",
            "locks",
            "exit",
            "quit",
        ] {
            assert!(got.contains(&name), "missing {name} in {got:?}");
        }
        assert_eq!(got.len(), 17 + 10);
    }

    #[test]
    fn partial_verb_prefix_filters_first_order_commands() {
        let c = completer();
        assert_eq!(values(&complete(&c, "ta")), vec!["task", "tasks"]);
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
        assert_eq!(got, vec!["task", "tasks"]);
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
        let res = complete(&c, "status -");
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
        // A complete `--` separator ends flag parsing; `status` has no
        // positional value domain, so nothing is offered.
        assert!(complete(&c, "status --").is_empty());
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
        assert_eq!(
            got,
            vec![
                "pending",
                "in-progress",
                "blocked",
                "awaiting-decision",
                "completed"
            ]
        );
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
            vec!["text", "json", "overview"]
        );
        assert_eq!(values(&complete(&c, "status --format j")), vec!["json"]);
        assert_eq!(values(&complete(&c, "status --format o")), vec!["overview"]);
    }

    #[test]
    fn inline_option_value_completes() {
        let c = completer();
        assert_eq!(
            values(&complete(&c, "status --format=")),
            vec!["text", "json", "overview"]
        );
        assert_eq!(values(&complete(&c, "status --format=j")), vec!["json"]);
        assert_eq!(values(&complete(&c, "status --format=o")), vec!["overview"]);
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
        completer_with_state_and_focus(state, None)
    }

    fn completer_with_state_and_focus(state: DynamicState, focus: Option<&str>) -> KvistCompleter {
        KvistCompleter::new(
            build_root(),
            Arc::new(Mutex::new(state)),
            Arc::new(Mutex::new(focus.map(str::to_owned))),
        )
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
    fn task_run_excludes_completed_tasks_and_prioritizes_next_ready_task() {
        let mut state = fixture_state();
        // Mark first task as completed
        state.scopes.get_mut(".").unwrap().tasks[0].status = TaskStatus::Completed;
        let c = completer_with_state(state);
        // Completed "write-tests" must NOT be listed; "implement-code" must be the first option!
        assert_eq!(values(&complete(&c, "task run . ")), vec!["implement-code"]);
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

    // ---- Stage 6: current-component completion ordering -------------------

    #[test]
    fn current_component_is_offered_first() {
        let c = completer_with_state_and_focus(fixture_state(), Some("engine"));
        assert_eq!(values(&complete(&c, "task run ")), vec!["engine", "."]);
    }

    #[test]
    fn current_component_is_offered_first_in_the_run_builtin() {
        let c = completer_with_state_and_focus(fixture_state(), Some("engine"));
        // The bare `run` builtin completes its component from the focus.
        assert_eq!(values(&complete(&c, "run ")), vec!["engine", "."]);
    }

    #[test]
    fn typed_component_still_scopes_tasks_over_the_current_component() {
        let c = completer_with_state_and_focus(fixture_state(), Some("engine"));
        assert_eq!(
            values(&complete(&c, "task run . ")),
            vec!["write-tests", "implement-code"]
        );
    }

    #[test]
    fn unknown_current_component_changes_no_ordering() {
        let c = completer_with_state_and_focus(fixture_state(), Some("ghost"));
        assert_eq!(values(&complete(&c, "task run ")), vec![".", "engine"]);
    }

    // ---- Stage 7: rich descriptions and the `--` separator ----------------

    #[test]
    fn task_candidates_carry_status_and_title_descriptions() {
        let mut state = fixture_state();
        state.scopes.get_mut(".").expect("root scope").tasks[0].title =
            "Write the tests".to_owned();
        let c = completer_with_state(state);
        // Outside run contexts: status and title, no next-ready marker.
        let res = complete(&c, "task transition . ");
        let wt = res
            .iter()
            .find(|candidate| candidate.value == "write-tests")
            .expect("candidate present");
        assert_eq!(wt.description.as_deref(), Some("pending · Write the tests"));
        // The prefix filter keeps the description attached to the value.
        let res = complete(&c, "task transition . w");
        assert_eq!(res.len(), 1);
        assert_eq!(
            res[0].description.as_deref(),
            Some("pending · Write the tests")
        );
    }

    #[test]
    fn run_context_marks_the_next_ready_task_in_its_description() {
        let c = completer_with_state(fixture_state());
        let res = complete(&c, "task run . ");
        let wt = res
            .iter()
            .find(|candidate| candidate.value == "write-tests")
            .expect("candidate present");
        assert_eq!(
            wt.description.as_deref(),
            Some("★ next ready · pending · write-tests")
        );
        let ic = res
            .iter()
            .find(|candidate| candidate.value == "implement-code")
            .expect("candidate present");
        assert!(
            !ic.description
                .as_deref()
                .unwrap_or("")
                .contains("next ready")
        );

        // The bare `run` builtin uses the same run context.
        let res = complete(&c, "run . ");
        let wt = res
            .iter()
            .find(|candidate| candidate.value == "write-tests")
            .expect("candidate");
        assert!(
            wt.description
                .as_deref()
                .unwrap_or("")
                .starts_with("★ next ready")
        );
    }

    #[test]
    fn current_component_candidate_carries_a_description() {
        let c = completer_with_state_and_focus(fixture_state(), Some("engine"));
        let res = complete(&c, "task run ");
        let engine = res
            .iter()
            .find(|candidate| candidate.value == "engine")
            .expect("candidate present");
        assert_eq!(engine.description.as_deref(), Some("current component"));
        let root = res
            .iter()
            .find(|candidate| candidate.value == ".")
            .expect("candidate");
        assert_eq!(root.description.as_deref(), Some("Component directory"));
    }

    #[test]
    fn prompt_positional_offers_the_project_task_ids() {
        let c = completer_with_state(fixture_state());
        // In the shell, `prompt TASK_ID` authors a prompt: the positional is
        // the task-ID domain, ordered by component.
        assert_eq!(
            values(&complete(&c, "prompt ")),
            vec!["write-tests", "implement-code", "build-tree"]
        );
        assert_eq!(values(&complete(&c, "prompt b")), vec!["build-tree"]);
    }

    #[test]
    fn inline_option_value_completes_mid_line_for_dynamic_domains() {
        let c = completer_with_state(fixture_state());
        assert_eq!(values(&complete(&c, "prompt --model=ol")), vec!["ollama"]);
        assert_eq!(values(&complete(&c, "prompt --model=ol")), vec!["ollama"]);
        assert_eq!(
            values(&complete(&c, "prompt --model=")),
            vec!["llama-cli", "ollama"]
        );
    }

    #[test]
    fn double_dash_suppresses_flag_completion_and_keeps_positionals() {
        let c = completer_with_state(fixture_state());
        // Without `--`, a dash-leading token completes flags of `task run`.
        let without = complete(&c, "task run . -");
        assert!(values(&without).contains(&"--stream"));
        // With `--`, the same token is a positional value: no task ID starts
        // with a dash, and no flags are offered.
        let with = complete(&c, "task run . -- -");
        assert!(with.is_empty());
        // A bare value after `--` still completes the positional domain.
        let with_value = complete(&c, "task run . -- w");
        assert_eq!(values(&with_value), vec!["write-tests"]);
    }

    #[test]
    fn double_dash_is_consumed_and_the_next_positional_completes() {
        let c = completer_with_state(fixture_state());
        // Tab on a complete `--` completes the next positional (TASK_ID), not
        // flags: the separator is consumed, not a flag prefix.
        assert_eq!(
            values(&complete(&c, "task run . --")),
            vec!["write-tests", "implement-code"]
        );
        // Subcommand completion after `--` stops (only positionals remain).
        assert!(complete(&c, "task -- r").is_empty());
        // The first `task run` positional is a component: `-- .` offers it.
        assert_eq!(values(&complete(&c, "task run -- .")), vec!["."]);
    }
}
