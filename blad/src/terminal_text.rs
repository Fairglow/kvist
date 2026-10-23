use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn inert_source(source: &str) -> std::borrow::Cow<'_, str> {
    let control =
        |c: char| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    if !source
        .chars()
        .any(|c| control(c) && !matches!(c, '\n' | '\t'))
    {
        return std::borrow::Cow::Borrowed(source);
    }
    let mut output = String::with_capacity(source.len());
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' && characters.peek() == Some(&'\n') {
            continue;
        }
        if control(character) && !matches!(character, '\n' | '\t') {
            use std::fmt::Write;
            let _ = write!(output, "\\u{{{:x}}}", u32::from(character));
        } else {
            output.push(character);
        }
    }
    std::borrow::Cow::Owned(output)
}

pub(crate) fn width_with_tabs(source: &str) -> usize {
    source
        .split('\t')
        .enumerate()
        .fold(0, |column, (index, part)| {
            let column = if index == 0 {
                column
            } else {
                column.saturating_add(tab_spaces(column))
            };
            column.saturating_add(part.width())
        })
}

fn tab_spaces(column: usize) -> usize {
    8 - column % 8
}

pub(crate) struct TerminalLine {
    text: String,
    styles: Vec<(usize, String)>,
    width: usize,
}

impl TerminalLine {
    pub fn new(ansi: &str) -> Self {
        let mut text = String::new();
        let mut styles = Vec::new();
        let mut remaining = ansi;
        let mut column = 0;
        let mut measured_until = 0;
        while !remaining.is_empty() {
            if let Some(sequence) = remaining.strip_prefix("\x1b[")
                && let Some(end) =
                    sequence.find(|c: char| !c.is_ascii_digit() && c != ';' && c != ':')
                && sequence.as_bytes()[end] == b'm'
            {
                let length = end + 3;
                styles.push((text.len(), remaining[..length].to_owned()));
                remaining = &remaining[length..];
                continue;
            }
            let Some(c) = remaining.chars().next() else {
                break;
            };
            match c {
                '\t' => {
                    column += text[measured_until..].width();
                    let spaces = tab_spaces(column);
                    text.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                    measured_until = text.len();
                }
                c if c.is_control() => {
                    text.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
                }
                _ => text.push(c),
            }
            remaining = &remaining[c.len_utf8()..];
        }
        let width = text.width();
        Self {
            text,
            styles,
            width,
        }
    }

    pub fn plain(&self) -> &str {
        &self.text
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn column_ranges(&self, ranges: &[std::ops::Range<usize>]) -> Vec<std::ops::Range<usize>> {
        if ranges.is_empty() {
            return Vec::new();
        }
        let mut column = 0;
        let graphemes: Vec<_> = self
            .text
            .grapheme_indices(true)
            .map(|(offset, text)| {
                let start = column;
                column += text.width();
                (offset, offset + text.len(), start, column)
            })
            .collect();
        ranges
            .iter()
            .filter_map(|range| {
                let first = graphemes.partition_point(|(_, end, _, _)| *end <= range.start);
                let last = graphemes.partition_point(|(start, _, _, _)| *start < range.end);
                (first < last).then(|| graphemes[first].2..graphemes[last - 1].3)
            })
            .collect()
    }

    pub fn clip(&self, start: usize, width: usize) -> String {
        self.clip_with_spans(start, width, &[], "")
    }

    pub fn clip_with_spans(
        &self,
        start: usize,
        width: usize,
        ranges: &[std::ops::Range<usize>],
        overlay: &str,
    ) -> String {
        if width == 0 {
            return String::new();
        }
        let end = start.saturating_add(width);
        let mut result = String::new();
        let mut styles = self.styles.iter().peekable();
        let mut source = SgrState::default();
        let mut active = false;
        let mut range_index = 0;
        let mut column = 0;
        for (offset, grapheme) in self.text.grapheme_indices(true) {
            let next = column + grapheme.width();
            if column >= end {
                break;
            }
            while let Some((position, style)) = styles.peek()
                && *position <= offset
            {
                result.push_str(style);
                source.apply(style);
                if active {
                    result.push_str(overlay);
                }
                styles.next();
            }
            while ranges
                .get(range_index)
                .is_some_and(|range| range.end <= column)
            {
                range_index += 1;
            }
            let highlight = ranges
                .get(range_index)
                .is_some_and(|range| range.start < next);
            if active != highlight {
                source.restore(&mut result);
                if highlight {
                    result.push_str(overlay);
                }
                active = highlight;
            }
            if column >= start && next <= end {
                let mut part = offset;
                while let Some((position, style)) = styles.peek()
                    && *position < offset + grapheme.len()
                {
                    result.push_str(&self.text[part..*position]);
                    result.push_str(style);
                    source.apply(style);
                    if active {
                        result.push_str(overlay);
                    }
                    part = *position;
                    styles.next();
                }
                result.push_str(&self.text[part..offset + grapheme.len()]);
            } else {
                while let Some((position, style)) = styles.peek()
                    && *position < offset + grapheme.len()
                {
                    result.push_str(style);
                    source.apply(style);
                    if active {
                        result.push_str(overlay);
                    }
                    styles.next();
                }
                if next > start {
                    result.extend(std::iter::repeat_n(' ', next.min(end) - column.max(start)));
                }
            }
            column = next;
        }
        result.push_str("\x1b[0m");
        result
    }
}

#[derive(Default)]
struct SgrState(std::collections::BTreeMap<u16, String>);

impl SgrState {
    fn apply(&mut self, sequence: &str) {
        let parameters = &sequence[2..sequence.len() - 1];
        let mut parts = parameters.split(';').peekable();
        while let Some(part) = parts.next() {
            let Ok(code) = part
                .parse::<u16>()
                .or_else(|error| if part.is_empty() { Ok(0) } else { Err(error) })
            else {
                continue;
            };
            let key = match code {
                0 => {
                    self.0.clear();
                    continue;
                }
                22 => {
                    self.0.remove(&1);
                    self.0.remove(&2);
                    continue;
                }
                23 => {
                    self.0.remove(&3);
                    continue;
                }
                24 => {
                    self.0.remove(&4);
                    continue;
                }
                27 => {
                    self.0.remove(&7);
                    continue;
                }
                29 => {
                    self.0.remove(&9);
                    continue;
                }
                30..=39 | 90..=97 => 38,
                40..=49 | 100..=107 => 48,
                _ => code,
            };
            let mut value = part.to_string();
            if matches!(code, 38 | 48)
                && let Some(mode) = parts.next()
            {
                value.push(';');
                value.push_str(mode);
                for _ in 0..if mode == "2" { 3 } else { 1 } {
                    if let Some(channel) = parts.next() {
                        value.push(';');
                        value.push_str(channel);
                    }
                }
            }
            self.0.insert(key, format!("\x1b[{value}m"));
        }
    }

    fn restore(&self, output: &mut String) {
        output.push_str("\x1b[0m");
        for sequence in self.0.values() {
            output.push_str(sequence);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clipped(text: &str, start: usize, width: usize) -> String {
        TerminalLine::new(&TerminalLine::new(text).clip(start, width)).text
    }

    #[test]
    fn clipping_preserves_styles_without_counting_them() {
        let line = TerminalLine::new("\x1b[31mabcd\x1b[0mef");
        assert_eq!(line.width(), 6);
        assert_eq!(clipped("\x1b[31mabcd\x1b[0mef", 2, 3), "cde");
        assert!(line.clip(2, 3).starts_with("\x1b[31m"));
        assert_eq!(clipped("\x1b[31mabcd", 20, 3), "");
    }

    #[test]
    fn clipping_never_splits_graphemes() {
        assert_eq!(clipped("世界abc", 1, 3), " 界");
        assert_eq!(clipped("世界abc", 0, 3), "世 ");
        assert_eq!(clipped("e\u{301}👩‍💻z", 1, 2), "👩‍💻");
        assert_eq!(clipped("e\u{301}👩‍💻z", 2, 2), " z");
    }

    #[test]
    fn controls_are_inert_and_tabs_use_fixed_stops() {
        assert_eq!(clipped("a\tb", 0, 20), "a       b");
        assert_eq!(clipped("\x1b[2Jx\x07", 0, 30), "\\u{1b}[2Jx\\u{7}");
        assert_eq!(clipped("abc", 0, 0), "");
        for source in ["\tX", "\tX\tY", "世界\tX", "e\u{301}\tX", "👩‍💻\tX"] {
            assert_eq!(width_with_tabs(source), TerminalLine::new(source).width());
        }
    }

    #[test]
    fn styles_inside_a_grapheme_do_not_split_it() {
        assert_eq!(clipped("e\x1b[31m\u{301}x", 0, 1), "e\u{301}");
        assert_eq!(clipped("e\x1b[31m\u{301}x", 1, 1), "x");
    }

    #[test]
    fn overlays_are_exact_restore_source_styles_and_preserve_clipped_graphemes() {
        let line = TerminalLine::new("\x1b[31ma\x1b[1mbc\x1b[0md界e\u{301}z");
        let output = line.clip_with_spans(0, 20, &[1..2, 4..7], "\x1b[7m");
        assert!(output.contains("\x1b[7mb"));
        assert!(output.contains("\x1b[0m\x1b[1m\x1b[31mc"), "{output:?}");
        assert_eq!(TerminalLine::new(&output).plain(), line.plain());
        assert_eq!(
            line.column_ranges(std::slice::from_ref(&(8..10)))
                .as_slice(),
            std::slice::from_ref(&(6..7))
        );
        let clipped = line.clip_with_spans(5, 3, std::slice::from_ref(&(4..7)), "\x1b[7m");
        assert_eq!(TerminalLine::new(&clipped).plain(), " e\u{301}z");
    }
}
