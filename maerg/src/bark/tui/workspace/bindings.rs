//! Shared descriptions for the command footer and keyboard help.

const BINDINGS: &[(&str, &str, Option<&str>)] = &[
    (
        "Esc / F2",
        "Menu; Esc dismisses completion first or goes back one level",
        Some("menu"),
    ),
    ("F1", "Keyboard help", Some("help")),
    (
        "Tab / Shift+Tab",
        "Complete / select candidate",
        Some("complete"),
    ),
    ("Enter", "Accept candidate or submit command", None),
    ("Up / Down", "Command history; menu navigation", None),
    ("Left / Right, Home / End", "Edit input", None),
    ("Ctrl+Left / Right", "Move by word", None),
    (
        "Ctrl+A / E / U / K / W / Z",
        "Start / end / delete before / after / word / undo",
        None,
    ),
    (
        "PgUp/PgDn",
        "Browse transcript across segments; scroll an overlay",
        Some("scroll"),
    ),
    ("Mouse wheel", "Browse transcript", None),
    (
        "Ctrl+Home / End",
        "Oldest / latest transcript and resume following",
        None,
    ),
    ("Alt+Left / Right", "Horizontal transcript scroll", None),
    (
        "Ctrl+C",
        "Cancel owned command or clear idle input; never quit",
        Some("cancel"),
    ),
    (
        "Ctrl+D (empty input)",
        "Send command EOF; exit when idle",
        None,
    ),
    (
        "Ctrl+Q",
        "Quit; confirm cancellation of active work, then reap",
        Some("quit"),
    ),
    (
        "Ctrl+P (Components menu)",
        "Pin/unpin the highlighted component",
        None,
    ),
];

pub(super) fn footer() -> String {
    BINDINGS
        .iter()
        .filter_map(|(key, _, hint)| hint.map(|hint| format!("{key} {hint}")))
        .collect::<Vec<_>>()
        .join("  ")
}

pub(super) fn help() -> String {
    let mut text = "## Keyboard\n\n| Key | Action |\n| --- | --- |\n".to_owned();
    for (key, action, _) in BINDINGS {
        text.push_str(&format!("| {key} | {action} |\n"));
    }
    text.push_str("\nMenus preserve the command draft. Type to filter component/model lists; Esc returns one level without applying a choice. F2 opens the root menu, cancelling any pending confirmation without approving it. Current values are marked independently of the cursor.\n\nWhile a command runs, input answers its prompts. Answers are masked and not retained. For `prompt --file -`, enter the prompt then Ctrl+D on empty input. Persisted settings, including appearance and pins, require an idle command; inspection and transcript navigation remain available. Settings do not approve policy. Transcript history is local diagnostic state, not canonical evidence.\n");
    text
}
