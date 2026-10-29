use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::Event;

use crate::inline::Style;

pub(crate) enum Piece<'a> {
    Text(Cow<'a, str>, Style),
    Boundary,
    Break,
    Rule,
}

#[derive(Default)]
pub(crate) struct Html {
    stack: Vec<String>,
    raw: Option<&'static str>,
}

impl Html {
    pub fn style(&self) -> Style {
        let mut style = Style::default();
        for tag in &self.stack {
            match tag.as_str() {
                "b" | "strong" | "summary" => style.bold = true,
                "i" | "em" => style.italic = true,
                "s" | "strike" | "del" => style.strike = true,
                "code" | "kbd" | "samp" => style.reverse = true,
                _ => {}
            }
        }
        style
    }

    pub fn project<'a>(&mut self, input: &'a str, strip: bool) -> Vec<Piece<'a>> {
        let mut pieces = Vec::new();
        let mut cursor = 0;
        while cursor < input.len() {
            if let Some(raw) = self.raw {
                let close = input.as_bytes()[cursor..]
                    .windows(raw.len() + 2)
                    .position(|bytes| {
                        bytes[..2] == *b"</" && bytes[2..].eq_ignore_ascii_case(raw.as_bytes())
                    });
                let Some(close) = close else {
                    pieces.push(Piece::Text(input[cursor..].into(), self.style()));
                    break;
                };
                let end = cursor + close;
                if end > cursor {
                    pieces.push(Piece::Text(input[cursor..end].into(), self.style()));
                }
                cursor = end;
                let suffix = &input[cursor + raw.len() + 2..];
                let whitespace = suffix.len()
                    - suffix
                        .trim_start_matches(|ch: char| ch.is_ascii_whitespace())
                        .len();
                if suffix.as_bytes().get(whitespace) == Some(&b'>') {
                    let end = raw.len() + 3 + whitespace;
                    if !strip {
                        pieces.push(Piece::Text(
                            input[cursor..cursor + end].into(),
                            self.style(),
                        ));
                    }
                    self.raw = None;
                    cursor += end;
                } else {
                    pieces.push(Piece::Text("<".into(), self.style()));
                    cursor += 1;
                }
                continue;
            }
            let next = input[cursor..]
                .find('<')
                .map_or(input.len(), |offset| cursor + offset);
            if next > cursor {
                pieces.push(Piece::Text(entities(&input[cursor..next]), self.style()));
                cursor = next;
                continue;
            }
            let Some(length) = token_end(&input[cursor..]) else {
                pieces.push(Piece::Text(input[cursor..].into(), self.style()));
                break;
            };
            let token = &input[cursor..cursor + length];
            cursor += length;
            let Some((name, closing, empty)) = tag(token) else {
                if !strip
                    || !(token.starts_with("<!--")
                        || token.starts_with("<!")
                        || token.starts_with("<?"))
                {
                    pieces.push(Piece::Text(token.into(), self.style()));
                }
                continue;
            };
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "br" if !closing => pieces.push(Piece::Break),
                "hr" if !closing => pieces.push(Piece::Rule),
                "details" | "p" | "div" => pieces.push(Piece::Boundary),
                "b" | "strong" | "i" | "em" | "s" | "strike" | "del" | "code" | "kbd" | "samp"
                | "summary" => {
                    let admitted = if closing {
                        if self.stack.last() == Some(&name) {
                            self.stack.pop();
                            true
                        } else {
                            false
                        }
                    } else if !empty && self.stack.len() < 64 {
                        if name == "summary" {
                            pieces.push(Piece::Boundary);
                        }
                        self.stack.push(name.clone());
                        true
                    } else {
                        false
                    };
                    if admitted && closing && name == "summary" {
                        pieces.push(Piece::Boundary);
                    }
                    if !admitted && !strip {
                        pieces.push(Piece::Text(token.into(), self.style()));
                    }
                }
                _ => {
                    if !strip {
                        pieces.push(Piece::Text(token.into(), self.style()));
                    }
                    if !closing && !empty {
                        self.raw = match name.as_str() {
                            "script" => Some("script"),
                            "style" => Some("style"),
                            "textarea" => Some("textarea"),
                            _ => None,
                        };
                    }
                }
            }
        }
        pieces
    }
}

fn token_end(input: &str) -> Option<usize> {
    if input.starts_with("<!--") {
        return input.find("-->").map(|end| end + 3);
    }
    let mut quote = None;
    for (offset, byte) in input.bytes().enumerate().skip(1) {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'>' if quote.is_none() => return Some(offset + 1),
            b'<' if quote.is_none() => return None,
            _ => {}
        }
    }
    None
}

fn tag(token: &str) -> Option<(&str, bool, bool)> {
    let body = token.strip_prefix('<')?.strip_suffix('>')?;
    let (body, closing) = body
        .strip_prefix('/')
        .map_or((body, false), |body| (body, true));
    if !body.as_bytes().first()?.is_ascii_alphabetic() {
        return None;
    }
    let end = body
        .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .unwrap_or(body.len());
    let rest = &body[end..];
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) && rest != "/" {
        return None;
    }
    if closing && !rest.trim().is_empty() {
        return None;
    }
    if !closing && !attributes(rest) {
        return None;
    }
    Some((&body[..end], closing, rest.trim_end().ends_with('/')))
}

fn attributes(mut rest: &str) -> bool {
    while !rest.trim_start().is_empty() {
        rest = rest.trim_start();
        if rest == "/" {
            return true;
        }
        let length = rest
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '>' | '=' | '\'' | '"' | '<'))
            .unwrap_or(rest.len());
        if length == 0 {
            return false;
        }
        rest = rest[length..].trim_start();
        if let Some(value) = rest.strip_prefix('=') {
            rest = value.trim_start();
            if let Some(quote @ ('\'' | '"')) = rest.chars().next() {
                let Some(end) = rest[1..].find(quote) else {
                    return false;
                };
                rest = &rest[end + 2..];
            } else {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                if end == 0 || rest[..end].contains(['"', '\'', '`', '=', '<', '>']) {
                    return false;
                }
                rest = &rest[end..];
            }
            if !rest.is_empty() && !rest.starts_with(char::is_whitespace) && rest != "/" {
                return false;
            }
        }
    }
    true
}

fn entities(text: &str) -> Cow<'_, str> {
    let mut output = String::new();
    let mut copied = 0;
    for (start, _) in text.match_indices('&') {
        let tail = &text[start + 1..];
        let Some(end) = tail.find([';', '&']) else {
            continue;
        };
        if tail.as_bytes()[end] != b';' {
            continue;
        }
        let entity = &tail[..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .filter(|value| value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .and_then(|value| u32::from_str_radix(value, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
                        .and_then(|value| value.parse().ok())
                })
                .and_then(char::from_u32)
                .filter(|ch| *ch != '\0'),
        };
        if let Some(decoded) = decoded {
            output.push_str(&text[copied..start]);
            output.push(decoded);
            copied = start + end + 2;
        }
    }
    if copied == 0 {
        Cow::Borrowed(text)
    } else {
        output.push_str(&text[copied..]);
        Cow::Owned(output)
    }
}

// pulldown-cmark emits HTML blocks one line at a time. Join adjacent payloads
// so comments, quoted attributes and raw-text elements can span lines.
pub(crate) fn events<'a>(
    input: impl Iterator<Item = (Event<'a>, Range<usize>)>,
) -> impl Iterator<Item = (Event<'a>, Range<usize>)> {
    let mut input = input.peekable();
    std::iter::from_fn(move || {
        let (event, mut range) = input.next()?;
        let Event::Html(first) = event else {
            return Some((event, range));
        };
        let mut joined = None;
        while let Some((Event::Html(_), next)) = input.peek()
            && next.start == range.end
        {
            let Some((Event::Html(text), next)) = input.next() else {
                break;
            };
            joined
                .get_or_insert_with(|| first.to_string())
                .push_str(&text);
            range.end = next.end;
        }
        Some((Event::Html(joined.map_or(first, Into::into)), range))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(html: &mut Html, input: &str, strip: bool) -> String {
        html.project(input, strip)
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Text(text, _) => Some(text.into_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn style_stack_is_bounded_and_unmatched_closes_do_not_cancel_other_styles() {
        let mut html = Html::default();
        let source = format!("{}overflow{}", "<b>".repeat(65), "</b>".repeat(65));
        assert_eq!(text(&mut html, &source, false), "<b>overflow</b>");
        assert_eq!(html.stack.len(), 0);
        assert!(!html.style().bold);
        assert_eq!(text(&mut html, "<i>x</b>y</i>", false), "x</b>y");
        assert!(html.style() == Style::default());
    }

    #[test]
    fn entities_preserve_invalid_values_and_make_decoded_controls_inert_later() {
        assert_eq!(
            entities("&amp;&lt;&gt;&quot;&apos;&nbsp;&#65;&#x41;&#X41;"),
            "&<>\"'\u{a0}AAA"
        );
        assert_eq!(
            entities("&#0; &#xD800; &#1114112; &#+65; &#x+41; &unknown; &amp"),
            "&#0; &#xD800; &#1114112; &#+65; &#x+41; &unknown; &amp"
        );
        assert_eq!(entities("&#27;"), "\x1b");
    }

    #[test]
    fn raw_elements_keep_fake_tags_and_survive_chunk_boundaries() {
        let mut html = Html::default();
        assert_eq!(text(&mut html, "<script>one<br>", true), "one<br>");
        assert_eq!(
            text(&mut html, "</scripted>two</SCRIPT><b>three</b>", true),
            "</scripted>twothree"
        );
        assert_eq!(text(&mut html, "<!-- incomplete", true), "<!-- incomplete");
        assert_eq!(text(&mut html, "<x title='a > b'>text</x>", true), "text");
        assert_eq!(text(&mut html, "a < b", true), "a < b");
        assert_eq!(text(&mut html, "<b =bad>x</b>", true), "<b =bad>x");
    }
}
