//! Static command tree derived from the CLI's clap definitions.
//!
//! The tree is the single source of truth for which verbs, flags, and enum
//! values are completable: it is built by introspecting the same
//! `clap::Command` that the parser uses, so completions can never drift from
//! the parseable command surface. It is built once per shell session.

use clap::CommandFactory;

use crate::cli;

/// A non-positional argument (flag or option) attached to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagSpec {
    /// Long spelling without leading dashes, when present.
    pub long: Option<String>,
    /// Short spelling, when present.
    pub short: Option<char>,
    /// Whether the flag consumes a following or inline value.
    pub takes_value: bool,
    /// Primary value name shown in help, when present.
    pub value_name: Option<String>,
    /// Closed value set, when the flag accepts only known literals.
    pub possible_values: Vec<String>,
    /// One-line help, when present.
    pub help: Option<String>,
    /// Whether clap propagates this flag to every subcommand.
    pub(crate) global: bool,
}

impl FlagSpec {
    /// The help flag that clap synthesizes for every command.
    fn help_flag() -> Self {
        Self {
            long: Some("help".to_owned()),
            short: Some('h'),
            takes_value: false,
            value_name: None,
            possible_values: Vec::new(),
            help: Some("Print help".to_owned()),
            global: false,
        }
    }

    /// The version flag that clap synthesizes for the root command.
    fn version_flag() -> Self {
        Self {
            long: Some("version".to_owned()),
            short: None,
            takes_value: false,
            value_name: None,
            possible_values: Vec::new(),
            help: Some("Print version".to_owned()),
            global: false,
        }
    }
}

/// A positional argument attached to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionalSpec {
    /// Value name shown in help.
    pub value_name: Option<String>,
    /// Closed value set, when the positional accepts only known literals.
    pub possible_values: Vec<String>,
    /// One-line help, when present.
    pub help: Option<String>,
}

/// One command in the tree: the root command or one of its subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandNode {
    /// Name as parsed on the command line.
    pub name: String,
    /// One-line help, when present.
    pub help: Option<String>,
    /// Subcommands in definition order.
    pub subcommands: Vec<CommandNode>,
    /// Flags in definition order, then inherited globals, then synthesized.
    pub flags: Vec<FlagSpec>,
    /// Positionals in index order.
    pub positionals: Vec<PositionalSpec>,
}

impl CommandNode {
    /// Finds a subcommand by exact name.
    pub fn find_subcommand(&self, name: &str) -> Option<&CommandNode> {
        self.subcommands.iter().find(|sub| sub.name == name)
    }

    /// Finds a flag by long spelling, returning its index in `flags`.
    pub fn find_flag_by_long(&self, long: &str) -> Option<usize> {
        self.flags
            .iter()
            .position(|flag| flag.long.as_deref() == Some(long))
    }

    /// Finds a flag by short spelling, returning its index in `flags`.
    pub fn find_flag_by_short(&self, short: char) -> Option<usize> {
        self.flags.iter().position(|flag| flag.short == Some(short))
    }

    /// Every flag spelling accepted at this node, in a stable order.
    #[allow(dead_code)]
    pub fn flag_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();
        for flag in &self.flags {
            if let Some(long) = &flag.long {
                candidates.push(format!("--{long}"));
            }
            if let Some(short) = flag.short {
                candidates.push(format!("-{short}"));
            }
        }
        candidates
    }
}

/// Builds the static tree for the public CLI surface.
pub fn build_root() -> CommandNode {
    let command = cli::Cli::command();
    let mut root = node_from_command(&command);
    let globals = root
        .flags
        .iter()
        .filter(|flag| flag.global)
        .cloned()
        .collect::<Vec<_>>();
    propagate_globals(&mut root, &globals);
    annotate_synthesized(&mut root, true);
    root
}

fn propagate_globals(node: &mut CommandNode, globals: &[FlagSpec]) {
    for global in globals {
        if node
            .flags
            .iter()
            .all(|flag| flag.long.as_deref() != global.long.as_deref())
        {
            node.flags.push(global.clone());
        }
    }
    for subcommand in &mut node.subcommands {
        propagate_globals(subcommand, globals);
    }
}

fn annotate_synthesized(node: &mut CommandNode, is_root: bool) {
    node.flags.push(FlagSpec::help_flag());
    if is_root {
        node.flags.push(FlagSpec::version_flag());
    }
    for subcommand in &mut node.subcommands {
        annotate_synthesized(subcommand, false);
    }
}

fn node_from_command(command: &clap::Command) -> CommandNode {
    let flags = command
        .get_arguments()
        .filter(|arg| !arg.is_positional())
        .map(flag_from_arg)
        .collect();
    let positionals = command
        .get_positionals()
        .map(|arg| PositionalSpec {
            value_name: value_name_of(arg),
            possible_values: possible_values_of(arg),
            help: arg.get_help().map(|help| help.to_string()),
        })
        .collect();
    let subcommands = command.get_subcommands().map(node_from_command).collect();

    CommandNode {
        name: command.get_name().to_owned(),
        help: command.get_about().map(|about| about.to_string()),
        subcommands,
        flags,
        positionals,
    }
}

fn flag_from_arg(arg: &clap::Arg) -> FlagSpec {
    FlagSpec {
        long: arg.get_long().map(str::to_owned),
        short: arg.get_short(),
        takes_value: arg.get_action().takes_values(),
        value_name: value_name_of(arg),
        possible_values: possible_values_of(arg),
        help: arg.get_help().map(|help| help.to_string()),
        global: arg.is_global_set(),
    }
}

fn value_name_of(arg: &clap::Arg) -> Option<String> {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(|name| name.as_str().to_owned())
}

fn possible_values_of(arg: &clap::Arg) -> Vec<String> {
    arg.get_possible_values()
        .into_iter()
        .map(|value| value.get_name().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> CommandNode {
        build_root()
    }

    fn node<'a>(root: &'a CommandNode, name: &str) -> &'a CommandNode {
        root.find_subcommand(name)
            .unwrap_or_else(|| panic!("missing subcommand {name}"))
    }

    #[test]
    fn root_command_is_named_kvist() {
        assert_eq!(root().name, "kvist");
    }

    #[test]
    fn root_lists_every_first_order_command() {
        let root = root();
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
            assert!(root.find_subcommand(name).is_some(), "missing {name}");
        }
        assert_eq!(root.subcommands.len(), 14);
    }

    #[test]
    fn root_flags_include_global_json_and_synthesized_help() {
        let root = root();
        let json = root.find_flag_by_long("json");
        assert!(json.is_some());
        assert!(root.flags[json.unwrap()].global);
        assert!(root.find_flag_by_long("help").is_some());
        assert!(root.find_flag_by_long("version").is_some());
    }

    #[test]
    fn every_node_offers_json_help_and_its_flags_in_stable_order() {
        let root = root();
        let mut nodes: Vec<&CommandNode> = vec![&root];
        while let Some(current) = nodes.pop() {
            for flag in ["json", "help"] {
                assert!(
                    current.find_flag_by_long(flag).is_some(),
                    "`--{flag}` missing on `{}`",
                    current.name
                );
            }
            let candidates = current.flag_candidates();
            assert!(candidates.iter().any(|c| c == "--json"));
            assert!(candidates.iter().any(|c| c == "--help"));
            for sub in &current.subcommands {
                nodes.push(sub);
            }
        }
    }

    #[test]
    fn task_node_exposes_its_subcommands() {
        let root = root();
        let task = node(&root, "task");
        for name in [
            "next",
            "transition",
            "run",
            "log",
            "approve-policy",
            "unlock",
            "recover",
            "finalize",
        ] {
            assert!(task.find_subcommand(name).is_some(), "missing task {name}");
        }
        assert_eq!(task.subcommands.len(), 8);
    }

    #[test]
    fn component_node_exposes_its_subcommands() {
        let root = root();
        let component = node(&root, "component");
        for name in ["new", "validate", "accept"] {
            assert!(
                component.find_subcommand(name).is_some(),
                "missing component {name}"
            );
        }
        assert_eq!(component.subcommands.len(), 3);
    }

    #[test]
    fn vcs_and_agent_nodes_expose_their_subcommands() {
        let root = root();
        assert!(
            node(&root, "vcs")
                .find_subcommand("commit-accepted")
                .is_some()
        );
        assert!(node(&root, "agent").find_subcommand("setup").is_some());
    }

    #[test]
    fn run_node_positionals_and_flags() {
        let root = root();
        let run = node(node(&root, "task"), "run");
        let value_names: Vec<_> = run
            .positionals
            .iter()
            .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
            .collect();
        assert_eq!(value_names, vec!["COMPONENT_DIR", "TASK_ID"]);
        assert!(run.find_flag_by_long("stream").is_some());
        assert!(!run.flags[run.find_flag_by_long("stream").unwrap()].takes_value);
        assert_eq!(run.positionals.len(), 2);
    }

    #[test]
    fn transition_node_positionals_include_closed_status_set() {
        let root = root();
        let transition = node(node(&root, "task"), "transition");
        assert_eq!(transition.positionals.len(), 3);
        let status = &transition.positionals[2];
        assert_eq!(status.value_name.as_deref(), Some("STATUS"));
        assert_eq!(
            status.possible_values,
            vec![
                "pending".to_owned(),
                "in-progress".to_owned(),
                "blocked".to_owned(),
                "completed".to_owned()
            ]
        );
        assert!(transition.find_flag_by_long("reason").is_some());
        assert!(transition.flags[transition.find_flag_by_long("reason").unwrap()].takes_value);
    }

    #[test]
    fn finalize_node_disposition_is_a_closed_set() {
        let root = root();
        let finalize = node(node(&root, "task"), "finalize");
        let disposition = &finalize.positionals[3];
        assert_eq!(disposition.value_name.as_deref(), Some("DISPOSITION"));
        assert_eq!(
            disposition.possible_values,
            vec!["accept".to_owned(), "block".to_owned()]
        );
        let recover = node(node(&root, "task"), "recover");
        let flag = recover
            .find_flag_by_long("disposition")
            .expect("disposition flag");
        assert_eq!(
            recover.flags[flag].possible_values,
            vec!["execution-did-not-start".to_owned()]
        );
    }

    #[test]
    fn prompt_node_flags_carry_value_metadata() {
        let root = root();
        let prompt = node(&root, "prompt");
        let model = prompt.find_flag_by_long("model").expect("model flag");
        assert!(prompt.flags[model].takes_value);
        assert_eq!(prompt.flags[model].value_name.as_deref(), Some("NAME"));
        let role = prompt.find_flag_by_long("role").expect("role flag");
        assert!(prompt.flags[role].takes_value);
        let effort = prompt
            .find_flag_by_long("reasoning-effort")
            .expect("reasoning effort flag");
        assert_eq!(
            prompt.flags[effort].possible_values,
            vec!["none", "minimal", "low", "medium", "high", "xhigh", "max"]
        );
        assert!(prompt.find_flag_by_long("detect-loops").is_some());
        assert!(!prompt.flags[prompt.find_flag_by_long("detect-loops").unwrap()].takes_value);
    }

    #[test]
    fn status_node_flags_carry_value_metadata() {
        let root = root();
        let status = node(&root, "status");
        let format = status.find_flag_by_long("format").expect("format flag");
        assert_eq!(status.flags[format].possible_values, vec!["text", "json"]);
        for flag in ["only-documents", "only-impls", "unfinished"] {
            assert!(!status.flags[status.find_flag_by_long(flag).unwrap()].takes_value);
        }
    }

    #[test]
    fn import_node_positionals_and_flags() {
        let root = root();
        let import = node(&root, "import");
        let value_names: Vec<_> = import
            .positionals
            .iter()
            .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
            .collect();
        assert_eq!(value_names, vec!["REPO_URL", "DEST_DIR"]);
        let branch = import.find_flag_by_long("branch").expect("branch flag");
        assert!(import.flags[branch].takes_value);
        let component = import
            .find_flag_by_long("component")
            .expect("component flag");
        assert!(import.flags[component].takes_value);
    }

    #[test]
    fn leaf_command_positionals() {
        let root = root();
        assert_eq!(
            node(&root, "init")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["PROJECT_DIR"]
        );
        assert_eq!(
            node(&root, "convert")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["PROJECT_DIR"]
        );
        assert_eq!(
            node(&root, "tree")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["PROJECT_DIR"]
        );
        assert_eq!(
            node(&root, "doctor")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["PROJECT_DIR"]
        );
        assert_eq!(
            node(&root, "reverse-discover")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["PATH"]
        );
        assert_eq!(
            node(&root, "completions")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["SHELL"]
        );
        assert_eq!(
            node(&root, "prompt")
                .positionals
                .iter()
                .map(|pos| pos.value_name.as_deref().unwrap_or("<none>"))
                .collect::<Vec<_>>(),
            vec!["PROMPT"]
        );
    }

    #[test]
    fn completions_shell_positional_is_a_closed_set() {
        let root = root();
        let completions = node(&root, "completions");
        assert_eq!(
            completions.positionals[0].possible_values,
            vec!["bash", "zsh", "fish", "powershell"]
        );
    }

    #[test]
    fn approve_policy_and_accept_positionals_and_flags() {
        let root = root();
        let approve = node(node(&root, "task"), "approve-policy");
        assert_eq!(
            approve.positionals[0].value_name.as_deref(),
            Some("PROJECT_DIR")
        );
        let accept = node(node(&root, "component"), "accept");
        assert_eq!(
            accept.positionals[0].value_name.as_deref(),
            Some("COMPONENT_DIR")
        );
        let commit = accept.find_flag_by_long("commit").expect("commit flag");
        assert!(!accept.flags[commit].takes_value);
        let message = accept.find_flag_by_long("message").expect("message flag");
        assert!(accept.flags[message].takes_value);
    }

    #[test]
    fn flag_candidates_are_stable_and_deduplicated_per_spelling() {
        let root = root();
        let run = node(node(&root, "task"), "run");
        let candidates = run.flag_candidates();
        assert_eq!(
            candidates,
            vec![
                "--stream".to_owned(),
                "--json".to_owned(),
                "--help".to_owned(),
                "-h".to_owned()
            ]
        );
    }

    #[test]
    fn short_flags_are_reported() {
        let root = root();
        let prompt = node(&root, "prompt");
        let file = prompt.find_flag_by_long("file").expect("file flag");
        assert_eq!(prompt.flags[file].short, Some('f'));
    }
}
