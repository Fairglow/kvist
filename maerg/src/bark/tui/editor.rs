//! Grapheme-aware command input; completion reuses the existing clap resolver.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

use crate::bark::{completion::Candidate, prefill::ComponentPrefill};

pub(super) const INPUT_BYTES: usize = 8192;

#[derive(Default)]
pub(super) struct Input {
    pub(super) text: String,
    pub(super) cursor: usize,
    undo: Vec<(String, usize)>,
}

impl Input {
    pub(super) fn set(&mut self, text: &str) {
        self.undo.clear();
        self.text = text.to_owned();
        self.cursor = self.text.len();
    }

    fn snapshot(&mut self) {
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push((self.text.clone(), self.cursor));
    }

    pub(super) fn insert(&mut self, text: &str) -> bool {
        let clean: String = text
            .chars()
            .filter_map(|ch| match ch {
                '\n' | '\r' | '\t' => Some(' '),
                ch if ch.is_control() => None,
                ch => Some(ch),
            })
            .collect();
        if self.text.len() + clean.len() > INPUT_BYTES {
            return false;
        }
        self.snapshot();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        true
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.text[self.cursor..]
                .graphemes(true)
                .next()
                .map(str::len)
                .unwrap_or(0)
    }

    pub(super) fn prefill(&mut self, prefill: &ComponentPrefill) {
        if let Some(insertion) = prefill.insertion(&self.text, self.cursor) {
            self.insert(&insertion);
        }
    }

    pub(super) fn accept(&mut self, candidate: &Candidate) {
        let value = quote(&candidate.value);
        let (start, end) = candidate.span;
        if start > end
            || end > self.text.len()
            || !self.text.is_char_boundary(start)
            || !self.text.is_char_boundary(end)
            || self.text.len() - (end - start) + value.len() + 1 > INPUT_BYTES
        {
            return;
        }
        self.snapshot();
        let insertion = format!(
            "{value}{}",
            if candidate.append_whitespace { " " } else { "" }
        );
        self.text.replace_range(start..end, &insertion);
        self.cursor = start + insertion.len();
    }

    pub(super) fn key(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left if control => {
                while self.cursor > 0 && self.text[..self.cursor].ends_with(char::is_whitespace) {
                    self.cursor = self.previous();
                }
                while self.cursor > 0 && !self.text[..self.cursor].ends_with(char::is_whitespace) {
                    self.cursor = self.previous();
                }
            }
            KeyCode::Right if control => {
                while self.cursor < self.text.len()
                    && !self.text[self.cursor..].starts_with(char::is_whitespace)
                {
                    self.cursor = self.next();
                }
                while self.cursor < self.text.len()
                    && self.text[self.cursor..].starts_with(char::is_whitespace)
                {
                    self.cursor = self.next();
                }
            }
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home | KeyCode::Char('a') if control || key.code == KeyCode::Home => {
                self.cursor = 0
            }
            KeyCode::End | KeyCode::Char('e') if control || key.code == KeyCode::End => {
                self.cursor = self.text.len()
            }
            KeyCode::Backspace => {
                self.snapshot();
                let previous = self.previous();
                self.text.drain(previous..self.cursor);
                self.cursor = previous;
            }
            KeyCode::Delete => {
                self.snapshot();
                self.text.drain(self.cursor..self.next());
            }
            KeyCode::Char('u') if control => {
                self.snapshot();
                self.text.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char('k') if control => {
                self.snapshot();
                self.text.truncate(self.cursor);
            }
            KeyCode::Char('w') if control => {
                self.snapshot();
                let end = self.cursor;
                self.key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL));
                self.text.drain(self.cursor..end);
            }
            KeyCode::Char('z') if control => {
                if let Some((text, cursor)) = self.undo.pop() {
                    self.text = text;
                    self.cursor = cursor;
                }
            }
            KeyCode::Char(ch) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&ch.to_string());
            }
            _ => {}
        }
    }
}

pub(super) fn quote(text: &str) -> String {
    if text
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '\'' | '"' | '\\'))
    {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn editor_handles_graphemes_paste_and_real_component_prefill() {
        let mut editor = Input::default();
        editor.insert("a👩‍💻b");
        editor.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        editor.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(editor.text, "ab");
        editor.set("task run ");
        let prefill = crate::bark::prefill::ComponentPrefill::new(
            crate::bark::tree::build_root(),
            std::sync::Arc::new(std::sync::Mutex::new(Some("other folder".into()))),
        );
        editor.prefill(&prefill);
        assert_eq!(editor.text, "task run \"other folder\" ");
        editor.insert("one\n two\r\nexit");
        assert!(!editor.text.contains('\n'));
    }

    #[test]
    fn completion_acceptance_preserves_suffix_and_quotes_paths() {
        let mut input = Input::default();
        input.set("cd ot --flag");
        input.cursor = 5;
        input.accept(&crate::bark::completion::Candidate {
            value: "other folder".into(),
            span: (3, 5),
            description: None,
            append_whitespace: true,
        });
        assert_eq!(input.text, "cd \"other folder\"  --flag");
    }

    #[test]
    fn clearing_a_submission_cannot_recall_an_answer_through_undo() {
        let mut input = Input::default();
        input.insert("private answer");
        input.set("");
        input.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert!(input.text.is_empty());
    }
}
