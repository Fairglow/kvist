//! Selected-component insertion into the real Reedline edit buffer.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use reedline::{
    AbbrExpandContext, EditCommand, Highlighter, Reedline, SimpleMatchHighlighter, StyledText,
};

use super::completion::component_position;
use super::tree::CommandNode;

#[derive(Clone)]
pub(super) struct ComponentPrefill {
    root: Arc<CommandNode>,
    focus: Arc<Mutex<Option<String>>>,
    inserted: Arc<Mutex<BTreeSet<Vec<String>>>>,
    signal: Arc<AtomicBool>,
    words: Arc<BTreeSet<String>>,
}

impl ComponentPrefill {
    pub(super) fn new(root: CommandNode, focus: Arc<Mutex<Option<String>>>) -> Self {
        let mut words = BTreeSet::from(["--".to_owned()]);
        collect_words(&root, &mut words);
        Self {
            root: Arc::new(root),
            focus,
            inserted: Arc::default(),
            signal: Arc::default(),
            words: Arc::new(words),
        }
    }

    pub(super) fn begin_submission(&self) {
        self.signal.store(false, Ordering::Relaxed);
        if let Ok(mut inserted) = self.inserted.lock() {
            inserted.clear();
        }
    }

    pub(super) fn break_signal(&self) -> Arc<AtomicBool> {
        self.signal.clone()
    }

    pub(super) fn abbreviations(&self) -> HashMap<String, String> {
        let Some(focus) = self.quoted_focus() else {
            return HashMap::new();
        };
        self.words
            .iter()
            .map(|word| (word.clone(), format!("{word} {focus}")))
            .collect()
    }

    fn quoted_focus(&self) -> Option<String> {
        let focus = self.focus.lock().ok()?.clone()?;
        if focus.is_empty() || focus.chars().any(char::is_control) {
            return None;
        }
        // Quoting does not disambiguate options or command-named components.
        let focus = if focus.starts_with('-') || self.words.contains(&focus) {
            format!("./{focus}")
        } else {
            focus
        };
        if focus
            .chars()
            .all(|c| !c.is_whitespace() && !matches!(c, '\'' | '"' | '\\'))
        {
            Some(focus)
        } else {
            Some(format!(
                "\"{}\"",
                focus.replace('\\', "\\\\").replace('"', "\\\"")
            ))
        }
    }

    fn available(&self, line: &str, cursor: usize, consume: bool) -> bool {
        let Some(position) = component_position(&self.root, line, cursor) else {
            return false;
        };
        let Ok(mut inserted) = self.inserted.lock() else {
            return false;
        };
        // A recalled or manually supplied value is also deliberate. Removing
        // it must not activate prefill on the following repaint.
        if position.supplied {
            inserted.insert(position.command);
            return false;
        }
        if !position.vacant || inserted.contains(&position.command) {
            return false;
        }
        if consume {
            inserted.insert(position.command);
        }
        true
    }

    pub(super) fn insertion(&self, line: &str, cursor: usize) -> Option<String> {
        let focus = self.quoted_focus()?;
        // Only a genuinely empty argument is filled. Never complete a partial
        // token or insert within a quoted word.
        if !line.get(..cursor)?.ends_with(char::is_whitespace)
            || line
                .get(cursor..)?
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace() && c != '-')
        {
            self.available(line, cursor, false);
            return None;
        }
        self.available(line, cursor, true)
            .then(|| format!("{focus} "))
    }

    pub(super) fn apply(&self, editor: &mut Reedline) {
        if let Some(text) = self.insertion(
            editor.current_buffer_contents(),
            editor.current_insertion_point(),
        ) {
            editor.run_edit_commands(&[EditCommand::InsertString(text)]);
        }
    }
}

impl Highlighter for ComponentPrefill {
    fn highlight(&self, line: &str, cursor: usize) -> StyledText {
        let available = self.quoted_focus().is_some() && self.available(line, cursor, false);
        if available
            && line
                .get(..cursor)
                .is_some_and(|prefix| prefix.ends_with(char::is_whitespace))
        {
            // Paste, completion acceptance and history can insert whole strings
            // without Reedline's space-abbreviation hook. Suspend in place and
            // let the host edit the authoritative buffer, preserving the cursor.
            // Reedline clears earlier undo history on suspension; insertion
            // itself remains one undoable edit.
            self.signal.store(true, Ordering::Relaxed);
        }
        SimpleMatchHighlighter::default().highlight(line, cursor)
    }

    fn should_expand_abbr(
        &self,
        line: &str,
        word_start: usize,
        _context: AbbrExpandContext,
    ) -> bool {
        let Some(rest) = line.get(word_start..) else {
            return false;
        };
        let length = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..length];
        self.words.contains(word)
            && self.quoted_focus().is_some()
            && self.available(line, word_start + length, true)
    }
}

fn collect_words(node: &CommandNode, words: &mut BTreeSet<String>) {
    for subcommand in &node.subcommands {
        words.insert(subcommand.name.clone());
        collect_words(subcommand, words);
    }
    for flag in &node.flags {
        if let Some(long) = &flag.long {
            words.insert(format!("--{long}"));
        }
        if let Some(short) = flag.short {
            words.insert(format!("-{short}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bark::tree::build_root;

    fn prefill(focus: Option<&str>) -> ComponentPrefill {
        ComponentPrefill::new(build_root(), Arc::new(Mutex::new(focus.map(str::to_owned))))
    }

    fn editor(line: &str) -> Reedline {
        let mut editor = Reedline::create();
        editor.run_edit_commands(&[EditCommand::InsertString(line.to_owned())]);
        editor
    }

    #[test]
    fn prefill_changes_real_buffer_and_cursor() {
        let prefill = prefill(Some("maerg"));
        let mut editor = editor("task run ");
        prefill.apply(&mut editor);
        assert_eq!(editor.current_buffer_contents(), "task run maerg ");
        assert_eq!(editor.current_insertion_point(), "task run maerg ".len());
    }

    #[test]
    fn prefill_command_named_components_remain_real_component_arguments() {
        use clap::Parser;
        for focus in ["show", "options", "discuss", "plan", "apply", "help"] {
            let prefill = prefill(Some(focus));
            let mut editor = editor("decision ");
            prefill.apply(&mut editor);
            let (head, arguments) =
                sav::split_raw_command(editor.current_buffer_contents()).unwrap();
            let parsed = crate::cli::Cli::try_parse_from(
                ["kvist", &head]
                    .into_iter()
                    .chain(arguments.iter().map(String::as_str)),
            );
            assert!(
                matches!(parsed, Ok(crate::cli::Cli {
                command: Some(crate::cli::Command::Decision { component_dir, command: None }), ..
            }) if component_dir.as_path() == std::path::Path::new(&format!("./{focus}"))),
                "{focus}: {}",
                editor.current_buffer_contents()
            );
        }
    }

    #[test]
    fn prefill_uses_component_slots_not_task_or_project_slots() {
        for line in [
            "task run ",
            "task next ",
            "task transition ",
            "task log ",
            "task recover ",
            "task finalize ",
            "component validate ",
            "component accept ",
            "component new ",
            "tasks ",
            "run ",
            "cd ",
        ] {
            assert_eq!(
                prefill(Some("maerg")).insertion(line, line.len()),
                Some("maerg ".into()),
                "{line}"
            );
        }
        for line in [
            "",
            "task ",
            "component ",
            "status ",
            "task approve-policy ",
            "component commit ",
            "prompt ",
            "task run maerg ",
            "task transition maerg write-tests ",
        ] {
            assert_eq!(
                prefill(Some("maerg")).insertion(line, line.len()),
                None,
                "{line}"
            );
        }
    }

    #[test]
    fn prefill_preserves_explicit_and_partial_components() {
        for line in [
            "task run sav ",
            "task run 'other component' ",
            "task run sav",
            "task run s",
            "task run 'unfinished ",
            "bogus task run ",
            "task run \n",
        ] {
            assert_eq!(
                prefill(Some("maerg")).insertion(line, line.len()),
                None,
                "{line:?}"
            );
        }
        let line = "task run  sav write-tests";
        assert_eq!(prefill(Some("maerg")).insertion(line, 9), None);
    }

    #[test]
    fn prefill_honors_flags_values_and_separator() {
        for line in [
            "task run --stream ",
            "task transition --reason chosen ",
            "task transition --reason=chosen ",
            "task transition --reason 'two words' ",
            "task run -- ",
            "component accept --message 'a message' ",
            "component accept --commit ",
            "--json task run ",
        ] {
            assert_eq!(
                prefill(Some("maerg")).insertion(line, line.len()),
                Some("maerg ".into()),
                "{line}"
            );
        }
        for line in [
            "task transition --reason ",
            "component accept --message ",
            "task transition --reason chos",
            "task run --not-a-flag ",
        ] {
            assert_eq!(
                prefill(Some("maerg")).insertion(line, line.len()),
                None,
                "{line}"
            );
        }
    }

    #[test]
    fn prefill_deletion_is_effective_until_next_submission() {
        let prefill = prefill(Some("maerg"));
        assert_eq!(prefill.insertion("task run ", 9), Some("maerg ".into()));
        assert_eq!(prefill.insertion("task run ", 9), None);
        assert_eq!(prefill.insertion("task run --stream ", 18), None);
        prefill.begin_submission();
        assert_eq!(prefill.insertion("task run ", 9), Some("maerg ".into()));
    }

    #[test]
    fn prefill_observes_recalled_explicit_component_before_deletion() {
        let prefill = prefill(Some("maerg"));
        assert_eq!(prefill.insertion("task run sav ", 13), None);
        assert_eq!(prefill.insertion("task run ", 9), None);
    }

    #[test]
    fn prefill_quotes_focus_for_the_shell_parser() {
        for focus in [
            ".",
            "a folder/child",
            "café/子",
            "a'quote",
            "a\"quote",
            "both'\"quotes",
            "a\\backslash",
        ] {
            let prefill = prefill(Some(focus));
            let mut editor = editor("task run ");
            prefill.apply(&mut editor);
            let (_, arguments) = sav::split_raw_command(editor.current_buffer_contents())
                .unwrap_or_else(|error| panic!("shell parse failed: {error}"));
            assert_eq!(arguments, vec!["run", focus], "{focus:?}");
        }
    }

    #[test]
    fn prefill_respects_absent_focus_and_cursor_boundaries() {
        assert_eq!(prefill(None).insertion("task run ", 9), None);
        let line = "task run --stream";
        let mut editor = editor(line);
        editor.run_edit_commands(&[EditCommand::MoveToPosition {
            position: 9,
            select: false,
        }]);
        prefill(Some("maerg")).apply(&mut editor);
        assert_eq!(editor.current_buffer_contents(), "task run maerg --stream");
        assert_eq!(editor.current_insertion_point(), 15);
        assert_eq!(prefill(Some("maerg")).insertion("task run ", 7), None);
        assert_eq!(prefill(Some("maerg")).insertion("task run ", 99), None);
        assert_eq!(prefill(Some("maerg")).insertion("task run café ", 11), None);
    }

    #[test]
    fn prefill_abbreviations_are_gated_by_the_real_component_position() {
        let prefill = prefill(Some("maerg"));
        let abbreviations = prefill.abbreviations();
        assert_eq!(
            abbreviations.get("run").map(String::as_str),
            Some("run maerg")
        );
        assert!(prefill.should_expand_abbr("task run ", 5, AbbrExpandContext::WordAbbreviation));
        assert!(!prefill.should_expand_abbr("task run ", 5, AbbrExpandContext::WordAbbreviation));
        prefill.begin_submission();
        for (line, start) in [
            ("task run sav", 5),
            ("task run maerg run", 15),
            ("component accept --message ", 17),
            ("prompt run", 7),
        ] {
            assert!(
                !prefill.should_expand_abbr(line, start, AbbrExpandContext::WordAbbreviation),
                "{line}"
            );
        }
        prefill.begin_submission();
        assert!(prefill.should_expand_abbr(
            "component validate",
            10,
            AbbrExpandContext::WordAbbreviation
        ));
    }

    #[test]
    fn prefill_paste_requests_a_real_edit_not_highlighted_ghost_text() {
        let prefill = prefill(Some("maerg"));
        let line = "task run --stream ";
        let highlighted = prefill.highlight(line, line.len());
        assert_eq!(
            highlighted
                .buffer
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<String>(),
            line
        );
        assert!(prefill.break_signal().swap(false, Ordering::Relaxed));
        let mut editor = editor(line);
        prefill.apply(&mut editor);
        assert_eq!(editor.current_buffer_contents(), "task run --stream maerg ");
        prefill.highlight(
            editor.current_buffer_contents(),
            editor.current_insertion_point(),
        );
        assert!(!prefill.break_signal().load(Ordering::Relaxed));
        editor.run_edit_commands(&[EditCommand::Undo]);
        prefill.highlight(
            editor.current_buffer_contents(),
            editor.current_insertion_point(),
        );
        assert!(!prefill.break_signal().load(Ordering::Relaxed));
    }

    #[test]
    fn prefill_history_without_trailing_space_is_not_rewritten_after_deletion() {
        let prefill = prefill(Some("maerg"));
        prefill.highlight("task run sav", 12);
        prefill.highlight("task run ", 9);
        assert!(!prefill.break_signal().load(Ordering::Relaxed));
        assert_eq!(prefill.insertion("task run ", 9), None);
    }

    #[test]
    fn prefill_focus_updates_only_between_submissions() {
        let prefill = prefill(Some("maerg"));
        assert_eq!(prefill.insertion("task run ", 9), Some("maerg ".into()));
        *prefill.focus.lock().unwrap() = Some("other folder".into());
        prefill.begin_submission();
        assert_eq!(
            prefill.insertion("task run ", 9),
            Some("\"other folder\" ".into())
        );
        *prefill.focus.lock().unwrap() = None;
        prefill.begin_submission();
        assert!(prefill.abbreviations().is_empty());
        assert!(!prefill.should_expand_abbr("task run", 5, AbbrExpandContext::WordAbbreviation));
    }

    #[test]
    fn prefill_does_not_turn_dash_leading_paths_into_options() {
        assert_eq!(
            prefill(Some("-child")).insertion("task run ", 9),
            Some("./-child ".into())
        );
        assert_eq!(
            prefill(Some("-child")).insertion("task run -- ", 12),
            Some("./-child ".into())
        );
        assert_eq!(
            prefill(Some("-child folder")).insertion("task run ", 9),
            Some("\"./-child folder\" ".into())
        );
        for focus in ["", "line\nbreak", "line\rbreak", "tab\tpath"] {
            assert_eq!(prefill(Some(focus)).insertion("task run ", 9), None);
        }
    }

    #[test]
    fn dash_leading_prefill_is_clap_safe_without_changing_normal_overrides() {
        use clap::Parser;

        let prefill = prefill(Some("-child"));
        let mut buffer = editor("task run ");
        prefill.apply(&mut buffer);
        assert_eq!(buffer.current_buffer_contents(), "task run ./-child ");
        let (program, arguments) =
            sav::split_raw_command(buffer.current_buffer_contents()).unwrap();
        let parsed = crate::cli::Cli::try_parse_from(
            ["kvist".to_owned(), program].into_iter().chain(arguments),
        )
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(crate::cli::Command::Task {
                command: crate::cli::TaskCommand::Run {
                    component_dir: component, ..
                }
            }) if component == std::path::Path::new("./-child")
        ));
        prefill.begin_submission();
        let mut explicit = editor("task run maerg ");
        prefill.apply(&mut explicit);
        assert_eq!(explicit.current_buffer_contents(), "task run maerg ");
        assert_eq!(prefill.insertion("task run ", 9), None);
    }
}
