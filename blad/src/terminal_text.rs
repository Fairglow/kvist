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

    pub fn clip(&self, start: usize, width: usize) -> String {
        self.clip_with_overlay(start, width, "")
    }

    pub fn clip_with_overlay(&self, start: usize, width: usize, overlay: &str) -> String {
        if width == 0 {
            return String::new();
        }
        let end = start.saturating_add(width);
        let mut result = overlay.to_owned();
        let mut styles = self.styles.iter().peekable();
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
                result.push_str(overlay);
                styles.next();
            }
            if column >= start && next <= end {
                let mut part = offset;
                while let Some((position, style)) = styles.peek()
                    && *position < offset + grapheme.len()
                {
                    result.push_str(&self.text[part..*position]);
                    result.push_str(style);
                    result.push_str(overlay);
                    part = *position;
                    styles.next();
                }
                result.push_str(&self.text[part..offset + grapheme.len()]);
            } else {
                while let Some((position, style)) = styles.peek()
                    && *position < offset + grapheme.len()
                {
                    result.push_str(style);
                    result.push_str(overlay);
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
}
