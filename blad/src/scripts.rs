use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Tag, TagEnd};

#[derive(Clone, Copy)]
pub(crate) enum Position {
    Super,
    Sub,
}

pub(crate) fn events<'a>(
    source: &'a str,
    input: impl Iterator<Item = (Event<'a>, Range<usize>)> + 'a,
) -> impl Iterator<Item = (Event<'a>, Range<usize>)> + 'a {
    let mut input = input
        .map(|(event, mut range)| {
            let marker = match &event {
                Event::Start(Tag::Superscript) | Event::End(TagEnd::Superscript) => Some("^"),
                Event::Start(Tag::Subscript) | Event::End(TagEnd::Subscript) => Some("~"),
                _ => None,
            };
            if let Some(marker) = marker {
                if matches!(event, Event::Start(_)) {
                    range.end = range.start + 1;
                } else {
                    range.start = range.end.saturating_sub(1);
                }
                (Event::Text(marker.into()), range)
            } else {
                (event, range)
            }
        })
        .peekable();
    std::iter::from_fn(move || {
        let (event, mut range) = input.next()?;
        let Event::Text(text) = event else {
            return Some((event, range));
        };
        if source.get(range.clone()) != Some(text.as_ref()) {
            return Some((Event::Text(text), range));
        }
        let mut merged = None;
        while let Some((Event::Text(next), next_range)) = input.peek()
            && range.end == next_range.start
            && source.get(next_range.clone()) == Some(next.as_ref())
        {
            merged
                .get_or_insert_with(|| text.to_string())
                .push_str(next);
            range.end = next_range.end;
            input.next();
        }
        Some((Event::Text(merged.map_or(text, Into::into)), range))
    })
}

pub(crate) fn span(content: &str, position: Position) -> String {
    let mapped: Option<String> = content.chars().map(|c| glyph(c, position)).collect();
    mapped.unwrap_or_else(|| {
        format!(
            "{}({content})",
            match position {
                Position::Super => '^',
                Position::Sub => '_',
            }
        )
    })
}

pub(crate) fn text<'a>(text: &'a str, source: &str, range: Range<usize>) -> Cow<'a, str> {
    if source.get(range.clone()) != Some(text) {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut copied = 0;
    let mut output = String::new();
    while cursor < bytes.len() {
        let marker = bytes[cursor];
        if !matches!(marker, b'^' | b'~')
            || bytes.get(cursor + 1) == Some(&marker)
            || (bytes.get(cursor.wrapping_sub(1)) == Some(&marker) && copied != cursor)
            || escaped(source.as_bytes(), range.start + cursor)
        {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && !(bytes[cursor] == marker && !escaped(source.as_bytes(), range.start + cursor))
        {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        if bytes[cursor] != marker || cursor == start + 1 {
            continue;
        }
        let content = &text[start + 1..cursor];
        if content.chars().any(char::is_whitespace) {
            cursor += 1;
            continue;
        }
        output.push_str(&text[copied..start]);
        output.push_str(&span(
            content,
            if marker == b'^' {
                Position::Super
            } else {
                Position::Sub
            },
        ));
        cursor += 1;
        copied = cursor;
    }
    if copied == 0 {
        Cow::Borrowed(text)
    } else {
        output.push_str(&text[copied..]);
        Cow::Owned(output)
    }
}

fn escaped(source: &[u8], index: usize) -> bool {
    source[..index]
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count()
        % 2
        != 0
}

fn glyph(character: char, position: Position) -> Option<char> {
    let (source, target) = match position {
        Position::Super => ("0123456789+-=()in", "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁱⁿ"),
        Position::Sub => (
            "0123456789+-=()aehijklmnoprstuvx",
            "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ",
        ),
    };
    source
        .chars()
        .zip(target.chars())
        .find_map(|(from, to)| (from == character).then_some(to))
}
