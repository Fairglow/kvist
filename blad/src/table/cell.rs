use unicode_width::UnicodeWidthStr;

use super::COLUMNS_PER_LINE;
use crate::terminal_text::TerminalLine;

struct Word {
    text: String,
    width: usize,
}

pub(crate) struct Cell {
    lines: Vec<Vec<Word>>,
    pub minimum: usize,
    pub natural: usize,
}

impl Cell {
    pub fn new(text: &str) -> Self {
        let mut minimum = 0;
        let mut natural = 0;
        let lines = text
            .split('\n')
            .map(|line| {
                let normalized = TerminalLine::new(line);
                let words: Vec<_> = normalized
                    .plain()
                    .split_whitespace()
                    .map(|text| {
                        let width = text.width();
                        minimum = minimum.max(width);
                        Word {
                            text: text.to_owned(),
                            width,
                        }
                    })
                    .collect();
                let width = words
                    .iter()
                    .fold(0usize, |sum, word| sum.saturating_add(word.width))
                    .saturating_add(words.len().saturating_sub(1));
                natural = natural.max(width);
                words
            })
            .collect();
        let mut cell = Self {
            lines,
            minimum,
            natural,
        };
        cell.minimum = cell.aspect_minimum();
        cell
    }

    pub fn explicit_height(&self) -> usize {
        self.lines.len()
    }

    pub fn fits_height(&self, width: usize, cap: usize) -> bool {
        self.fragments(width).take(cap.saturating_add(1)).count() <= cap
    }

    pub fn height(&self, width: usize) -> usize {
        self.fragments(width).count()
    }

    pub fn wrap(&self, width: usize) -> Vec<String> {
        self.fragments(width)
            .map(|words| {
                let mut line = String::new();
                for (index, word) in words.iter().enumerate() {
                    if index > 0 {
                        line.push(' ');
                    }
                    line.push_str(&word.text);
                }
                line
            })
            .collect()
    }

    fn aspect_minimum(&self) -> usize {
        let mut low = self.minimum;
        let mut high = self.natural;
        // Natural width is always admissible: geometry must not invent width
        // for short values or text with unavoidable explicit line breaks.
        while low < high {
            let mid = low + (high - low) / 2;
            let cap = mid / COLUMNS_PER_LINE;
            let occupied = self
                .fragments(mid)
                .filter(|words| !words.is_empty())
                .take(cap.saturating_add(1))
                .count();
            if occupied <= cap {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        low
    }

    fn fragments(&self, width: usize) -> impl Iterator<Item = &[Word]> {
        self.lines.iter().flat_map(move |words| {
            let mut start = 0;
            let mut empty = words.is_empty();
            std::iter::from_fn(move || {
                if empty {
                    empty = false;
                    return Some(&words[..]);
                }
                if start == words.len() {
                    return None;
                }
                let mut end = start + 1;
                let mut used = words[start].width;
                while end < words.len() {
                    let next = used.saturating_add(1).saturating_add(words[end].width);
                    if next > width {
                        break;
                    }
                    used = next;
                    end += 1;
                }
                let fragment = &words[start..end];
                start = end;
                Some(fragment)
            })
        })
    }
}
