use regex::RegexBuilder;

use crate::render_writer::LinkSpan;
use crate::terminal_text::TerminalLine;

const MAX_QUERY: usize = 4096;

#[derive(Default)]
pub(crate) struct Search {
    pub editing: bool,
    pub query: String,
    pub regex: bool,
    pub backward: bool,
    pub matches: Vec<LinkSpan>,
    pub index: usize,
    pub error: Option<String>,
}

impl Search {
    pub fn active(&self) -> bool {
        self.editing || !self.query.is_empty()
    }

    pub fn begin(&mut self, backward: bool) {
        *self = Self {
            editing: true,
            backward,
            ..Self::default()
        };
    }

    pub fn push(&mut self, character: char) -> bool {
        if self.query.len() + character.len_utf8() > MAX_QUERY {
            self.error = Some(format!("Search input limit: {MAX_QUERY} bytes"));
            false
        } else {
            self.query.push(character);
            true
        }
    }

    pub fn update(&mut self, lines: &[TerminalLine], row: usize) {
        self.matches.clear();
        self.index = 0;
        self.error = None;
        if self.query.is_empty() {
            return;
        }
        let pattern = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let expression = match RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .size_limit(2 * 1024 * 1024)
            .dfa_size_limit(2 * 1024 * 1024)
            .build()
        {
            Ok(expression) => expression,
            Err(error) => {
                self.error = Some(format!("Invalid regex: {error}"));
                return;
            }
        };
        for (row, line) in lines.iter().enumerate() {
            let ranges: Vec<_> = expression
                .find_iter(line.plain())
                .filter(|found| !found.is_empty())
                .map(|found| found.range())
                .collect();
            self.matches.extend(
                line.column_ranges(&ranges)
                    .into_iter()
                    .map(|range| LinkSpan {
                        row,
                        start: range.start,
                        end: range.end,
                    }),
            );
        }
        self.index = if self.backward {
            self.matches
                .iter()
                .rposition(|found| found.row <= row)
                .unwrap_or(self.matches.len().saturating_sub(1))
        } else {
            self.matches
                .iter()
                .position(|found| found.row >= row)
                .unwrap_or(0)
        };
    }

    pub fn advance(&mut self, reverse: bool) {
        if !self.matches.is_empty() {
            let count = self.matches.len();
            self.index = if self.backward ^ reverse {
                (self.index + count - 1) % count
            } else {
                (self.index + 1) % count
            };
        }
    }

    pub fn current(&self) -> Option<&LinkSpan> {
        self.matches.get(self.index)
    }

    pub fn footer(&self) -> String {
        let direction = if self.backward { '?' } else { '/' };
        let mode = if self.regex { "regex" } else { "literal" };
        let result = if let Some(error) = &self.error {
            error.clone()
        } else if self.query.is_empty() {
            "type query; Ctrl+R toggles regex".into()
        } else if self.matches.is_empty() {
            "No matches".into()
        } else {
            String::new()
        };
        let (phase, hints) = if self.editing {
            ("FIND", "Esc:cancel Enter:search")
        } else {
            ("SEARCH", "Esc/q:exit n/N:move Enter:next")
        };
        let position = format!(
            "{}/{}",
            if self.matches.is_empty() {
                0
            } else {
                self.index + 1
            },
            self.matches.len()
        );
        let detail = if result.is_empty() {
            String::new()
        } else {
            format!("{result}  ")
        };
        format!(
            "{phase} [{mode}] {position} {hints} {detail}{direction}{}",
            self.query
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_ranges_case_folding_and_zero_width_patterns() {
        let lines = [TerminalLine::new("界 e\u{301} Éé HIT hit")];
        let mut search = Search {
            query: "é".into(),
            ..Search::default()
        };
        search.update(&lines, 0);
        assert_eq!(search.matches.len(), 2);
        assert_eq!((search.matches[0].start, search.matches[1].end), (5, 7));
        search.regex = true;
        search.query = "^|$".into();
        search.update(&lines, 0);
        assert!(search.matches.is_empty());
        search.query = "[".into();
        search.update(&lines, 0);
        assert!(search.error.as_deref().unwrap().contains("Invalid regex"));
    }

    #[test]
    fn footer_distinguishes_query_entry_from_confirmed_search_and_prioritizes_exit_hints() {
        let mut search = Search {
            editing: true,
            query: "x".repeat(4096),
            ..Search::default()
        };
        let editing = search.footer();
        assert!(editing.starts_with("FIND "));
        assert!(editing.find("Esc:cancel").unwrap() < editing.find(&search.query).unwrap());
        assert!(editing.contains("Enter:search"));
        search.editing = false;
        let confirmed = search.footer();
        assert!(confirmed.starts_with("SEARCH "));
        assert!(confirmed.find("Esc/q:exit").unwrap() < confirmed.find(&search.query).unwrap());
        assert!(confirmed.contains("n/N"));
    }

    #[test]
    fn input_and_compiled_patterns_are_bounded() {
        let mut search = Search {
            query: "x".repeat(MAX_QUERY),
            ..Search::default()
        };
        search.push('é');
        assert_eq!(search.query.len(), MAX_QUERY);
        assert!(search.error.is_some());
        search.query = "(a{1000}){1000}".into();
        search.regex = true;
        search.update(&[], 0);
        assert!(search.error.is_some());
    }
}
